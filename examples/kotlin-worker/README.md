# Kotlin Worker

A Cloudflare Worker in Kotlin/Wasm, built against bindings that a
generator written in Kotlin (`generator/`) derives from the ABI template
alone (`ts-gen --target wat`, format in `ABI.md`), and linked with `ts-gen
embed`. It replies with the request's method, URL, `x-kotlin` header and
body, and the `GREETING` binding from `config.capnp`.

Files:

- `src/wasmJsMain/kotlin/app/App.kt`: the worker, `suspend fun fetch(request,
  env, ctx): Response` against the typed bindings.
- `generator/`: the bindings generator (Kotlin/JVM, its own S-expression
  reader, no dependency on ts-gen). `generator/testdata/sample.abi.wat` is a
  hand-written template covering the whole grammar.
- `build.gradle.kts`: the Kotlin/Wasm build; its `generateBindings` task runs
  the generator on `build/worker.abi.wat` into `build/generated/bindings/`.
- `env.d.ts`: the worker's bindings, merged into the `Env` of
  `tests/fixtures/cloudflare-worker.d.ts`.
- `loader.js`: instantiates the merged module with Kotlin's import object
  plus `builtins.js`, and publishes its `default` export. App-independent.
- `config.capnp`: workerd config, serving on `127.0.0.1:8789`.
- `run.sh`: the whole pipeline. `FINDINGS.md`: Kotlin/Wasm facts it rests on.
- `check-templates.sh [template...]`: generates and compiles bindings (with an
  app scaffold) for each template, by default every
  `tests/snapshots/*/default.wat` (all 124 compile).

## Generated bindings

- `flat/<Owner>.kt`: per ABI function, a `@JsFun` external in the package that
  makes its fully qualified name the import name (`package Headers; external
  fun get(...)` imports as `"js_code" "Headers.get"`). The JS bodies only
  throw: `embed --bindings-module js_code` replaces them with the bindings.
- `workers/Bindings.kt`: per owner, an interface over the JS value
  (`ref: JsAny`) with members forwarding to the flat layer, inheriting its
  `extends` owners; constructors and statics on its companion
  (`Response(body)`, `Response.redirect(url)`), `wrap(ref)` to type a value.
  Getters are properties, setters `setX(value)`. `string` is `String`, an owner
  its interface, an `i32` `Boolean`, a position that admits `null`/`undefined`
  nullable; a `Promise` head is `kotlin.js.Promise<JsAny?>`; anything else is
  `JsAny`. Overloads share the JS name unless their Kotlin signatures clash,
  then take the ABI name (`jsonWithInit`). Module-scoped, alias and variadic
  imports are skipped (listed in a comment).
- `workers/provided/Entry.kt`: per handler, a `@JsExport` that wraps the wire
  values and calls `app.<name>`; for a promise handler inside
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
  --target wat --provides ExportedHandler.fetch -o build/worker.abi.wat
./gradlew compileProductionExecutableKotlinWasmJs   # generator + Kotlin
K=build/compileSync/wasmJs/main/productionExecutable/kotlin
ts-gen embed --abi build/worker.abi.wat --bindings-module js_code \
  --imported-string-constants "'" -m $K/worker.wasm -o out
cp loader.js config.capnp $K/worker.import-object.mjs $K/worker.js-builtins.mjs out/
(cd out && workerd serve config.capnp --experimental)
```

The generator alone: `./gradlew :generator:run --args="<template> <out-dir>
[--package workers] [--app-package app] [--scaffold <App.kt>]"`.

## Notes

- Kotlin cannot hold `undefined` (every reference an external returns
  passes through a null-or-undefined check), so a position that admits only
  `undefined` is passed `null`.
- workerd needs no compatibility flag: `loader.js` instantiates the wasm
  module itself with Kotlin's import object, which provides the `'` string
  constants and a `wasm:js-string` polyfill for hosts that do not compile
  them in.
