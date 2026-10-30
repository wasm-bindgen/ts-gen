# Zena Worker

A Cloudflare Worker in [Zena](https://github.com/elematic/zena), built against
bindings `ts-gen-zena` writes from the ABI template, and embedded with
`ts-gen embed`. It
replies with the request's method, URL, `x-zena` header and body, and the
`GREETING` binding from `config.capnp`.

Files:

- `app.zena`: the worker, against the typed classes in `bindings.zena`.
- `env.d.ts`: the worker's bindings, merged into the `Env` of
  `tests/fixtures/cloudflare-worker.d.ts`.
- `worker.abi.wat`, `bindings.zena`, `main.zena` (generated, not
  committed): the ABI template, the bindings, and the compile entry,
  whose raw `fetch` export wraps `app.fetch`.
- `loader.js`: instantiates the merged module and publishes its `default`
  export. App-independent.
- `config.capnp`: workerd config loading `loader.js`, `builtins.js` and
  `main.wasm` from `out/`.

## Prerequisites

- The `zena` CLI, as `$ZENA`, built with `declare let` value imports and
  `ref.null` emission for `externref | null`. No Zena release has these
  yet.
- binaryen's `wasm-merge` with GC support, on `PATH` or in `$WASM_MERGE`.
- `workerd`.

## Build

From the repository root:

```sh
EX=examples/zena-worker

# The ABI template: `--provides` names the handler the default export
# implements. Everything after this step reads only the template.
cargo run -- -i tests/fixtures/cloudflare-worker.d.ts -i $EX/env.d.ts \
  -o $EX/worker.abi.wat --target wat --provides ExportedHandler.fetch

# Bindings plus main.zena, the compile entry. app.zena is committed;
# ts-gen-zena would scaffold it if it were absent.
cargo run -p ts-gen-zena -- $EX/worker.abi.wat -o $EX/bindings.zena

# Compile. String literals become `wasm:js/string-constants` imports.
(cd $EX && $ZENA build main.zena -t js \
  --imported-string-constants wasm:js/string-constants -o main.wasm)

# Embed against the same template: writes out/main.wasm and out/builtins.js.
cargo run -- embed --abi $EX/worker.abi.wat -m $EX/main.wasm -o $EX/out
cp $EX/loader.js $EX/config.capnp $EX/out/
```

## Run

```sh
(cd $EX/out && workerd serve config.capnp --experimental) &
curl -H 'x-zena: hi' -d 'Hello!' http://127.0.0.1:8788/echo
# POST http://127.0.0.1:8788/echo
# x-zena: hi
# GREETING: Hello from workerd
# body: Hello!
```

## Test

`cargo test -p ts-gen-zena zena_worker` runs the same build and calls the
worker's `fetch` in node (`generators/zena/tests/worker.mjs`). It runs when `ZENA`
names the `zena` CLI and is skipped otherwise; with `ZENA` set it needs
`wasm-merge` and node 24+ as the embed tests do.

## Notes

- TS strings are Zena `String`s, which on `-t js` wrap the JS string, so
  `request.method + " "` concatenates JS strings with no copy. A union
  such as `BodyInit` stays `externref`; pass a `String`'s `.handle`.
- Positions TS allows to be `null` are `externref | null`; `undefined`
  is a value an `externref` holds. An owner or string result TS allows
  to be empty is `T | null`, `null` for both empties:
  `request.headers.get("x-zena")` is `String | null`.
- Importing `await_` puts `embed` in JSPI mode, so the wrapper exports
  `fetch` through `WebAssembly.promising`.
- Zena's `Error` imports `captureStackTrace` and `formatStackTrace` from
  `env`; `loader.js` supplies both.
- The bindings export the global `fetch` function under the same name as
  the `fetch` handler this module exports. A worker that calls global
  `fetch` should import it under another name:
  `import { fetch as jsFetch } from './bindings.zena'`.
