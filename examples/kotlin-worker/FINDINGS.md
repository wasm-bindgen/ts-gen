# Kotlin/Wasm facts for the ts-gen IR

Measured with Kotlin 2.5.0-Beta2 (Gradle 9.0.0, JDK 21), `wasmJs` target,
production executable. Paths below are relative to this directory;
`OUT=build/compileSync/wasmJs/main/productionExecutable/kotlin`.

```sh
./gradlew compileProductionExecutableKotlinWasmJs
wasm-tools print $OUT/worker.wasm | grep -E '^\s*\((import|export|start)'
```

## Import names

A top-level `@JsFun("...") external fun` imports from module `js_code`
under its fully qualified name, so the package is the class binding name:

| Kotlin | Import |
| --- | --- |
| `package Headers; external fun get(...)` | `"js_code" "Headers.get"` |
| `package WebAssembly.Module; external fun exports(...)` | `"js_code" "WebAssembly.Module.exports"` |
| root package `external fun fetch(...)` | `"js_code" "fetch"` |
| ``package `in`.`is`; external fun `fun`(...)`` | `"js_code" "in.is.fun"` (backticks dropped) |

Unused externals are not imported (Kotlin's DCE drops them). The stdlib's
own snippets share the module (`"js_code" "kotlin.wasm.internal.isNullish"`,
`kotlin.io.printlnImpl`, `kotlin.js.JsError_$external_class_instanceof`, ...),
all under `kotlin.`/`kotlinx.` packages.

A package named like a class is fine from another package through an
import alias: `import Number.value as Number__value` binds the root
package `Number`, not `kotlin.Number`, and `import Headers.get as
Headers__get` works beside a class `workers.Headers`. Unqualified
`Headers.get(...)` in a package that has a class `Headers` would resolve
to the class, so the typed layer imports every flat function by alias.

## Wasm types of `@JsFun` externals

| Kotlin | Param | Result |
| --- | --- | --- |
| `JsAny?`, `JsAny`, `JsString` | `externref` | `externref` |
| `Double` | `f64` | `f64` |
| `Long` | `i64` | `i64` |
| `Int` | `i32` | `i32` |
| `Boolean` | **`externref`** (converted with `kotlinBooleanToExternRefAdapter`, a JS `true`/`false`) | `i32` |

So a `bool` (`i32`) *parameter* must be `Int` in the flat layer;
a `Boolean` parameter would change the import's type to `externref` and
not link. Results may be `Boolean`.

Non-null `JsAny` is still `externref` at the wasm level (never `(ref
extern)`). Kotlin wraps every reference result in an adapter:

* `JsAny?` result: `jsCheckIsNullOrUndefinedAdapter`, which calls the
  `js_code` import `kotlin.wasm.internal.isNullish` and maps **both
  `null` and `undefined` to wasm null**. Kotlin cannot see the
  difference between the two empties in a result.
* `JsAny` result: the same, then `THROW_NPE` on null.

```sh
wasm-tools print $OUT/worker.wasm | sed -n '/func $Headers.get__externalAdapter/,/^  )/p'
```

## `@JsExport`

`package app; @JsExport fun fetch(request: JsAny, env: JsAny, ctx: JsAny): JsAny`
exports as `(export "fetch" (func $app.fetch__JsExportAdapter))` of type
`(func (param externref externref externref) (result externref))`. The
export name is the simple name (the package is dropped); the adapter
checks each non-null parameter with `jsCheckIsNullOrUndefinedAdapter`
and throws NPE. A Kotlin `fetch` export and a root-package `fetch`
external coexist (`"js_code" "fetch"` import, `"fetch"` export).

## Initialisation and the import object

* The module has a start function, `(start $_initializeModule)`, that
  runs top-level initialisers. Nothing has to be called by the host.
* `fun main()` adds an `_start` export that the generated `worker.mjs`
  calls after instantiation. Without `main` there is no `_start` and no
  `_initialize`.
* `worker.import-object.mjs` exports `importObject`:
  * `js_code`: one arrow function per `@JsFun`/external, keyed by FQN,
    including the binding stubs (harmless extra keys once embed has linked
    them).
  * `intrinsics`: `{ memory: new WebAssembly.Memory({initial: 0}), tag:
    WebAssembly.JSTag ?? new Tag }`. The module imports `intrinsics.tag`
    as a tag, and `intrinsics.memory` both as a memory and as an
    `externref` global.
  * `"'"`: a `Proxy` whose every property is its own name, i.e. the
    string constants, for hosts that compile without
    `importedStringConstants: "'"`.
  * `wasm:js-string`: a JS polyfill from `worker.js-builtins.mjs`, for
    hosts that compile without `builtins: ['js-string']`.
* `worker.mjs` compiles with `{ builtins: ['js-string'],
  importedStringConstants: "'" }` and does
  `new WebAssembly.Instance(module, importObject)`. A hand-written
  loader does the same with `import { importObject } from
  './worker.import-object.mjs'`, adding `'./builtins.js'`.

## wasm-merge

binaryen 132 (both copies) links `js_code` imports by export name from
a second module passed under the name `js_code`, and leaves the
unmatched `js_code` imports in place:

```sh
wasm-tools parse b.wat -o b.wasm   # exports "Headers.get", "fetch"
wasm-merge --all-features --disable-compact-imports --disable-custom-descriptors \
  --rename-export-conflicts $OUT/worker.wasm kotlin b.wasm js_code -o m.wasm
node imports.mjs m.wasm   # lists WebAssembly.Module.imports/exports
```

`Headers.get` and `fetch` disappear from the imports; the
`kotlin.*` ones and the other binding stubs remain. (The bindings' own
`fetch` export clashes with Kotlin's `fetch` handler export, hence
`--rename-export-conflicts` here; embed's `ts-gen:bindings.` prefix
avoids that.) Without `--disable-compact-imports`, `--all-features`
writes the compact import encoding, which node 22 rejects
(`unknown import kind 0x7e`).

## Coroutines

`kotlinx-coroutines-core:1.10.2` works on `wasmJs`: `GlobalScope.promise {}`
returns a JS promise from a `@JsExport`, run in node 24 and workerd. Its
`await` is `suspend fun <T> Promise<JsAny?>.await(): T`, so `T` is not
inferred from a `Promise<JsString>` ("Cannot infer type for type parameter
'T'"); the bindings define `suspend fun <T : JsAny?> Promise<T>.await(): T`,
and `request.text().await()` is a `JsString`.

## `undefined` cannot be held in Kotlin

Every reference result of an external, whatever its declared type
(`JsAny?`, an `external interface X : JsAny`, an `external class`,
`JsReference<Any>?`, `JsString`), goes through
`jsCheckIsNullOrUndefinedAdapter`, so `@JsFun("() => undefined")`
returns Kotlin `null`, and `x === undefined` on it from JS is `false`
(a JS `null` arrives):

```kotlin
@JsFun("() => undefined") external fun u1(): JsAny?
@JsFun("(x) => x === undefined") external fun isU(x: JsAny?): Boolean
isU(u1())   // false
```

The stdlib has no public `undefined` value. So the bindings' sys functions
`JsValue.undefined` and `JsValue.is_undefined` are of no use from
Kotlin, and a parameter that admits `undefined` but not `null` can
only be given `null`.

## The typed layer: external interfaces

Type arguments (`Promise<T>`, `JsArray<T>`) must be `JsAny?` subtypes, so a
class must be one to appear in one, and only external declarations may be:

```kotlin
interface Kind : JsAny            // e: Non-external type extends external type 'JsAny'.
external interface A : JsAny { companion object }
// e: Accessing a companion object of an external interface currently produces an
//    empty JavaScript object. [...] annotate the interface with '@JsName("Object")'.
```

(the companion error stays with `@JsName("Object")`). So a class is an
`external interface` and its members are extensions, which also makes
inheritance free: an extension on a base applies to every derived interface,
and a redeclaration on the derived one is more specific, never a clash.
Constructors are factory functions (`fun Response(...)`: a function may share
an interface's name), statics live in a separate `object ResponseStatic`. A
top-level property may not share it (`val Cloudflare` beside `external
interface Cloudflare`: "Conflicting declarations").

A member declared by two unrelated bases is ambiguous on a class extending
both (neither receiver is more specific), so the bindings forward it to the
first base: `fun C.foo() = (this as A).foo()`.

`unsafeCast` is `fun <T : JsAny> JsAny.unsafeCast(): T`: a nullable value is
cast with `x?.unsafeCast<T>()`, and to a type parameter (`T : JsAny?`) with an
unchecked `x as T`.

Overloads differing only in type arguments (`f(JsArray<JsString>)`,
`f(JsArray<JsNumber>)`) or only in nullability compile. `JsBigInt`, with
`toLong()`, exists.

## Callbacks

A function-typed parameter of a `@JsFun` external (`(JsAny?, Double) ->
Boolean`, nullable too) stays `externref` in the import; Kotlin converts the
lambda through one more `js_code` import per arity in the external's package,
`"js_code" "Headers.__convertKotlinClosureToJsClosure_1"`, which its import
object provides and embed leaves alone (no binding has that name). Only
parameters convert: a callback result, or one inside a type argument, is
`JsAny`.

## What the IR leaves to the generator

* Module-scoped bindings: `@JsFun` only imports from `js_code`, and
  `embed --bindings-module js_code` links global bindings only, so a
  `module "spec"` block (and a member `from "spec"`) is not bound. A
  `js_code` name for them (`"spec".Class.member`) would do.
* Variadics: the rest array is `(ref null $anyrefs)`, a wasm GC array an
  external cannot declare; skipped.
* Literal parameters (`static of(kind: "a")`) are still on the wire: the
  bindings fill them in (`CounterStatic.ofA()`); the cohort's shared binding
  (`cohort __of`) is not used, since IR.md does not say whether it is
  imported.
* `Null`-only parameters (`new(body: Null, init: ResponseInit)`) are dropped
  the same way, which leaves `new(body: Null)` clashing with `new()`; it
  becomes `Response_newWithNull()`.
* Kinds: no binding tests an arm, so a result is the kind's interface, not
  matched. Class arms are subtypes of the kind; other arms are factory
  overloads (`BodyInit("text")`).
* `throws`: the IR does not say how a caught exception crosses, so it is
  ignored; a JS exception reaches Kotlin as a `JsException`.
* A getter and setter typed apart (`get status?: Number | Undefined`, `set
  status(value: f64)`) cannot be one `var`, so setters are `setX(value)`.

## `@JsExport` scalar types

`@JsExport fun exB(x: Boolean): Boolean` is `(param i32) (result
externref)`: the reverse of imports (a `Boolean` export *result* is a JS
boolean). `kotlin.String` parameters of externals are `externref`, passed
without conversion.

## Exception handling

Kotlin's default output uses the legacy exception handling proposal
(`try`/`catch`), which `ts-gen embed` validates with wasmparser's
`legacy_exceptions` feature. `-Xwasm-use-new-exception-proposal` (exnref,
`try_table`) works too.

## workerd

The workerd at `workers-rs/node_modules/@cloudflare/workerd-linux-64`
(2026-09-23) does not know `wasm_esm_integration_builtins` ("No such
compatibility flag"), and none is needed: `loader.js` imports the `.wasm`
as a `wasm` module (a `WebAssembly.Module`) and instantiates it with Kotlin's
import object, whose `'` Proxy and `wasm:js-string` polyfill cover a host that
compiles without them. `worker.js-builtins.mjs` compiles a tiny wasm module
from bytes at module evaluation, which workerd allows at startup.

## Export names

`@JsExport @JsName("api.math.add") fun api_math_add(...)` exports as
`"api.math.add"`, so dotted handler export names are expressible; the
generated entry always sets `@JsName` to the provide's export name.
