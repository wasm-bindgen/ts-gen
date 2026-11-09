# Kotlin Worker

A Cloudflare Worker in Kotlin/Wasm over `tests/fixtures/workers-types.d.ts`,
bound through the v2 IR under the profile
`all-host, results = single, throws = js`. It replies with the request's
method, URL, `x-kotlin` header and body, and the `GREETING` binding, after
awaiting the body and a 1 ms timer on the runtime's loop. It also reads a
`FormData` entry (a payload enum: two values) and catches the `TypeError` a
bad `URL` throws, as `ErrException`.

```sh
./run.sh node    # one request in node
./run.sh serve   # workerd on 127.0.0.1:8789, then curls it (WORKERD or PATH)
```

The pipeline (`run.sh`):

1. `ts-gen --provides ExportedHandler.fetch --ir`: the IR, with the
   default export's `fetch`.
2. `ts-gen-abi lower --profile "all-host, results = single, throws = js"`,
   then `ts-gen-kotlin --runtime`:
   the bindings (`build/bindings/`), `links.tsv`, and `tsgen.runtime`.
3. Gradle: the Kotlin/Wasm module, from the bindings and
   `src/wasmJsMain/kotlin/app/App.kt`.
4. `ts-gen-bridge link`: Kotlin imports every `@JsFun` from `js_code` under
   its qualified name; the link step renames the ones `links.tsv` lists to
   their tw keys. `ts-gen-bridge generate` then writes the bridge for the
   linked module.

`loader.js` instantiates the linked module through the bridge's
`instantiate`, with Kotlin's own import object for its `js_code` snippets,
runs `_start`, and re-exports the bridge's `default`. In node, `node.mjs`
loads the `.wasm` files as `WebAssembly.Module`s, as workerd does.

Kotlin functions return one value and Kotlin can't declare Wasm tags, hence
the profile:

* `results = single`: a call returns slot 0, and slots 1.. go through the
  bridge's `tw:ret` spill, popped right after an import returns and pushed
  before an export or trampoline returns, in slot order.
* `throws = js`: a JS exception reaches Kotlin as a `JsException`; the
  bindings read its arm and message through `("tw:err", E)` and throw `E`'s
  typed exception (`ErrException` for `err`). An export that throws reports
  its error through `("tw:err", "set:E")`, and the bridge throws the JS error.

Async: `App.fetch` is a `suspend` function. The export makes its future with
`tw:future new`, runs `fetch` as a task on `tsgen.runtime.Loop.main`, and
settles it with `resolve<T>`, or `reject<E>` with the task's exception as
`E`. `Future.await` polls with a waker and parks the task; `tw:future/wake`
reposts it, and the bridge ticks the loop through `tw:loop/tick` and
`next_sleep`. A rejected JS future throws its typed exception, through
`error<E>`.
