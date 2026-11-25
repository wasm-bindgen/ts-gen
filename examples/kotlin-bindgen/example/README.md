# Kotlin Worker

A tour of the Workers API from Kotlin/Wasm: a Cloudflare Worker over
`tests/fixtures/workers-types.d.ts` and the JS standard library
(`std/std.tw`), bound through the v2 IR under the profile
`all-host, results = single, throws = js`. Each route does one thing and
replies in plain text; `/` lists them.

```sh
./run.sh serve   # the tour: workerd on 127.0.0.1:8789, every route curled (WORKERD or PATH)
./run.sh node    # the routes node can serve
```

`tour.txt` is what `serve` prints, and `tour.node.txt` what `node` does.

In an editor, run `./run.sh node` once, so the generated `std` and
`workers` packages exist under `build/`, then open `examples/kotlin-bindgen/example`
itself as the project: its `./gradlew` is the generator's Gradle wrapper.

## The tour

The app is `src/wasmJsMain/kotlin/app/`, one file per topic. `App.kt` is
the router, and `Reply.kt` what the routes share.

| Route | File | From the platform | From the IR |
|---|---|---|---|
| `/` | `App.kt` | the default export's `fetch` | an exported function (`@provides`), a `suspend fun` whose future the export makes |
| `/echo` | `Http.kt` | `Request`, `Headers`, a `Response` with a status and headers | `str?` (`headers.get`); a callback called repeatedly (`forEach`); an iterator (`entries()`); `future<str>`; a dictionary (`ResponseInit`); a `u16` from `@tw-abi` (`status`) |
| `/url` | `Http.kt` | `URL`, `URLSearchParams` | getters and setters; a `u32` (`size`); `arr<str>` as a `List`; a `union` parameter (`str \| URL`); a constructor that `throws`, caught as `ErrException` |
| `/form` | `Http.kt` | `FormData`, `File` | a payload enum (`File \| str`), nullable; a std class (`Uint8Array`) |
| `/json` | `Http.kt` | `request.json()`, `JSON`, `Response.json` | an erased generic (`json<T>`); an interface of fields, some optional; `Any`; an error enum from `@throws {SyntaxError}`; a static (`ResponseStatic`); a rejected future |
| `/crypto` | `Crypto.kt` | `crypto.subtle.digest`, `getRandomValues`, `randomUUID`, `TextEncoder`, `btoa`/`atob` | a `union` of buffer types; a generic that returns its argument's type; `@throws {DOMException}` as its own exception |
| `/stream` | `Streams.kt` | `ReadableStream`, `TransformStream`, `pipeTo`, `tee`, a streamed body | callbacks held in a dictionary, one returning a future (a suspend lambda); a tuple (`tee()`); a host class's type arguments, which are boxes (`ReadableStream<JsString>`, `toJsString()`) |
| `/fetch` | `Fetch.kt` | a service binding, the global `fetch` with a `RequestInit`, `AbortSignal.timeout` | a literal enum (`cache`); overloads of a `union` parameter; cancellation: `Future.drop()` runs the member's `cancel`, whose signal joins the caller's own |
| `/timers` | `Timers.kt` | `scheduler.wait`, `setTimeout`, `queueMicrotask`, `ctx.waitUntil` | `future<unit>`; one-shot callbacks (`awaitCallback`); a future Kotlin provides (`tw:future new`) |
| `/rewrite` | `Rewrite.kt` | `HTMLRewriter` | callbacks in a dictionary; `str?` (`getAttribute`) |
| `/counter/*` | `Durable.kt` | a Durable Object: an id from a name, `stub.fetch`, KV `get`/`put`/`delete`/`list`, a transaction, SQL with a cursor, an alarm | an exported class, constructed by JS; an export that ignores an argument it doesn't declare (`alarm()`); a member's own `T`, named at the call (`get<JsNumber>`, `exec<Visit>`) and boxed (`toJsNumber()`); a callback returning `future<T>`; overloads by argument type (`delete`); a variadic (`sql.exec`); an iterator (`cursor.next()`); a std `Map<JsString, T>` |
| `/ws`, `/counter/ws` | `Durable.kt` | `WebSocketPair`, `accept`, a message listener; hibernation with `acceptWebSocket` and `webSocketMessage` | an enum with a function arm (`addEventListener`); an export overloaded on `str \| ArrayBuffer`; a `u16` status of 101 |
| `/rpc` | `Rpc.kt` | a `WorkerEntrypoint` called through its service binding | an exported class with an imported base, constructed through `tw:new`; a literal enum parameter, optional; `u8[]` as a `ByteArray`; `bigint` as a `JsBigInt`; a tuple as a `Pair`; `u32` parameters |
| `/errors` | `Errors.kt` | errors of `URL`, `structuredClone`, `fetch`, and the Durable Object | `throws`: the arms of `err`, `TypeError` and the fallback `Error`; a rejected future throwing from `await`; a Kotlin exception crossing JS and back |
| `/fail`, `/fail/plain` | `Errors.kt` | an exception out of the handler | an `ErrException` leaves as its arm's JS error, any other exception as an `Error`; a `Str` payload (`toJsString()`) |
| `/scheduled`, `/queue` | `Events.kt` | the `scheduled` and `queue` handlers, run through a binding to the worker itself | more exported functions; dictionaries built in Kotlin; `arr<T>` of an interface; a std `Date` |

Left out, for want of something to run them on in plain workerd: the Cache
API, KV, R2 and D1. No member of workers-types that lowers returns `never`.

The declarations beside the app:

* `env.d.ts`: the bindings of `config.capnp` as `Env`, the RPC stubs, and
  the shapes the routes read (`Order`, `Visit`).
* `annotate.d.ts`: ABI choices layered onto workers-types by name: scalar
  widths, and the errors a member throws.
* `provides.d.ts`: what the module provides: the default export's
  `fetch`, `scheduled` and `queue`, the `Counter` Durable Object, and the
  `Greeter` entrypoint, whose base it imports from `cloudflare:workers`.

`config.capnp` has two services: the worker, and `backend.js`, which
`/fetch` reaches through a binding and as the worker's outbound.

## The pipeline

`run.sh`:

1. `ts-gen --ir`, with `--annotate annotate.d.ts`, `--with std.tw` and
   `--provides provides.d.ts`: the IR.
2. `ts-gen-abi lower --profile "all-host, results = single, throws = js"`,
   then `kotlin-bindgen`, for `std.tw` and for the worker: the bindings
   (`build/std/` with `tsgen.runtime`, and `build/bindings/` over them
   with `--lib std=std`), each with a `links.tsv`.
3. Gradle: the Kotlin/Wasm module, from the bindings and the app.
4. `kotlin-bindgen link`: Kotlin imports every `@JsFun` from `js_code` under
   its qualified name; the link step renames the ones the `links.tsv`
   files list to their tw keys. `ts-gen-bridge generate` then writes the
   bridge for the linked module.

`loader.js` instantiates the linked module through the bridge's
`instantiate`, with Kotlin's own import object for its `js_code` snippets,
runs `_start`, and re-exports the bridge's `default`, `Counter` and
`Greeter`. In node, `node.mjs` loads the `.wasm` files as
`WebAssembly.Module`s, as workerd does, and shims what node lacks.

## The profile

Kotlin functions return one value and Kotlin can't declare Wasm tags, hence
the profile:

* `results = single`: a call returns slot 0, and slots 1.. go through the
  bridge's `tw:ret` spill, popped right after an import returns and pushed
  before an export or trampoline returns, in slot order.
* `throws = js`: a JS exception reaches Kotlin as a `JsException`; the
  bindings read its arm and message through `("tw:err", E)` and throw `E`'s
  typed exception (`ErrException` for `err`). An export that throws reports
  its error through `("tw:err", "set:E")`, and the bridge throws the JS error.

Async: an exported `suspend` function makes its future with
`tw:future new`, runs as a task on `tsgen.runtime.Loop.main`, and settles
it with `resolve<T>`, or `reject<E>` with the task's exception as `E`.
`Future.await` polls with a waker and parks the task; `tw:future/wake`
reposts it, and the bridge ticks the loop through `tw:loop/tick` and
`next_sleep`. A rejected JS future throws its typed exception, through
`error<E>`.
