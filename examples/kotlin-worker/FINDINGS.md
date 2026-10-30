# Kotlin/Wasm facts for the ts-gen ABI

Measured with Kotlin 2.5.0-Beta2 (Gradle 9.0.0, JDK 21), `wasmJs` target,
production executable. Paths below are relative to this directory;
`OUT=build/compileSync/wasmJs/main/productionExecutable/kotlin`.

```sh
./gradlew compileProductionExecutableKotlinWasmJs
wasm-tools print $OUT/worker.wasm | grep -E '^\s*\((import|export|start)'
```

## Import names

A top-level `@JsFun("...") external fun` imports from module `js_code`
under its fully qualified name, so the package is the ABI owner path:

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

So an ABI `i32` (boolean) *parameter* must be `Int` in the flat layer;
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
    including the ABI stubs (harmless extra keys once embed has linked
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
`kotlin.*` ones and the other ABI stubs remain. (The bindings' own
`fetch` export clashes with Kotlin's `fetch` handler export, hence
`--rename-export-conflicts` here; embed's `ts-gen:bindings.` prefix
avoids that.) Without `--disable-compact-imports`, `--all-features`
writes the compact import encoding, which node 22 rejects
(`unknown import kind 0x7e`).

## Coroutines

`kotlinx-coroutines-core:1.10.2` works on `wasmJs`:
`GlobalScope.promise { p.unsafeCast<Promise<JsString>>().await<JsString>() }`
returns a JS promise from a `@JsExport`, run in node 24.

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

The stdlib has no public `undefined` value. So the ABI's sys functions
`JsValue.undefined` and `JsValue.is_undefined` are of no use from
Kotlin, and a parameter that admits `undefined` but not `null` can
only be given `null`.

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
generated entry always sets `@JsName` to the template's export name.
