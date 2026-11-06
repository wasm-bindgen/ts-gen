# Kotlin Worker

A Cloudflare Worker in Kotlin/Wasm, built against bindings that a
generator written in Kotlin (`generator/`) derives from the ts-gen IR
alone (`ts-gen --target ir`, format in `IR.md`), and linked with `ts-gen
embed`. It replies with the request's method, URL, `x-kotlin` header and
body, and the `GREETING` binding from `config.capnp`.

Files:

- `src/wasmJsMain/kotlin/app/App.kt`: the worker, `suspend fun fetch(request,
  env, ctx): Response` against the typed bindings.
- `generator/`: the bindings generator (Kotlin/JVM, its own IR reader in
  `Ir.kt`, no dependency on ts-gen). `generator/testdata/sample.ir` is a copy
  of `abi/testdata/sample.ir`, which covers the whole grammar.
- `build.gradle.kts`: the Kotlin/Wasm build; its `generateBindings` task runs
  the generator on `build/worker.ir` into `build/generated/bindings/`.
- `env.d.ts`: the worker's bindings, merged into the `Env` of
  `tests/fixtures/cloudflare-worker.d.ts`.
- `loader.js`: instantiates the merged module with Kotlin's import object
  plus `builtins.js`, and publishes its `default` export. App-independent.
- `config.capnp`: workerd config, serving on `127.0.0.1:8789`.
- `run.sh`: the whole pipeline. `FINDINGS.md`: Kotlin/Wasm facts it rests on.
- `check-templates.sh [file.ir...]`: generates and compiles bindings (with an
  app scaffold) for each IR file, by default every
  `tests/snapshots/*/default.ir` (all compile).

## Generated bindings

- `flat/<Class>.kt`: per binding, a `@JsFun` external in the package that
  makes its fully qualified name the import name (`package Headers; external
  fun get(...)` imports as `"js_code" "Headers.get"`). The JS bodies only
  throw: `embed --bindings-module js_code` replaces them with the bindings.
- `workers/Bindings.kt`: per class and kind, an `external interface` (a JS
  reference, so it can be a type argument) inheriting its `extends` classes
  and the kinds it is an arm of. Members are extensions forwarding to the flat
  layer: getters are properties, setters `setX(value)`. Constructors are
  factory functions named like the class (`Response(body, init)`), statics
  live in `object <Class>Static`, a `static class` is an `object`. Types:
  `String` is `String`, `Number` `Double`, `bool` `Boolean`; a class or kind
  its interface; `Promise<T>` `kotlin.js.Promise<T>`, `Array<T>` `JsArray<T>`
  with type arguments as JS types (`Promise<JsString>`; a typed `await()` is
  generated beside them); type parameters are `<R : JsAny?>`; a tuple a
  `JsArray`; a callback parameter a Kotlin function type; anything else
  `JsAny`. A position admitting `null`/`undefined` is nullable. Literal and
  `Null` parameters are filled in, not taken (`static of(kind: "a")` is
  `CounterStatic.ofA()`).
- Kinds: a class arm is a subtype; another arm has a factory overload named
  like the kind (`BodyInit("text")`), and literal arms are constants in
  `object <Kind>Static` (`MethodKindStatic.Get` in the sample). Results are not matched on arms.
- Names: by default the JS name, falling back to the IR binding name,
  camel-cased, when Kotlin signatures clash (`redirectWithStatus`).
  `--verbatim-names` takes the binding names as they are, for IR written with
  `--naming kotlin`, which is how this worker is built. Module-scoped and variadic bindings are skipped
  (listed in a comment). `throws` is ignored: a JS exception propagates as a
  Kotlin `JsException`.
- `workers/provided/Entry.kt`: per provide, a `@JsExport` that takes the wire
  values and calls `app.<name>`; for an `async` provide inside
  `GlobalScope.promise {}`, so the app function is `suspend`.

## Run

Needs JDK 21 (only a JRE: Gradle provisions a JDK for the generator through
the foojay resolver), binaryen's `wasm-merge` (`$WASM_MERGE` or `PATH`) and
`workerd` (`$WORKERD` or `PATH`).

```sh
examples/kotlin-worker/run.sh serve
# POST http://127.0.0.1:8789/echo
# x-kotlin: hi
# GREETING: Hello from workerd
# body: Hello!
```

Step by step (from this directory):

```sh
ts-gen -i ../../tests/fixtures/cloudflare-worker.d.ts -i env.d.ts \
  --target ir --naming kotlin --provides ExportedHandler.fetch -o build/worker.ir
./gradlew compileProductionExecutableKotlinWasmJs   # generator + Kotlin
K=build/compileSync/wasmJs/main/productionExecutable/kotlin
ts-gen embed --ir build/worker.ir --bindings-module js_code \
  --imported-string-constants "'" -m $K/worker.wasm -o out
cp loader.js config.capnp $K/worker.import-object.mjs $K/worker.js-builtins.mjs out/
(cd out && workerd serve config.capnp --experimental)
```

The generator alone: `./gradlew :generator:run --args="<file.ir> <out-dir>
[--package workers] [--app-package app] [--scaffold <App.kt>] [--verbatim-names]"`.

## Notes

- Kotlin cannot hold `undefined` (every reference an external returns
  passes through a null-or-undefined check), so a position that admits only
  `undefined` is passed `null`.
- workerd needs no compatibility flag: `loader.js` instantiates the wasm
  module itself with Kotlin's import object, which provides the `'` string
  constants and a `wasm:js-string` polyfill for hosts that do not compile
  them in.
