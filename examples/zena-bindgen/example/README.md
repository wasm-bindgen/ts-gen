# Zena Worker

A Cloudflare Worker in Zena over `tests/fixtures/workers-types.d.ts`, bound
through the v2 IR under the GC profile with host strings (`all-gc, str =
host`). It is a tour of the Workers API: each route does one thing and
replies in plain text with what happened.

```sh
./run.sh serve   # workerd on 127.0.0.1:8788, then tour.sh curls every route
./run.sh node    # the routes node can serve, through node.mjs
```

What they print is kept beside them, in `tour.workerd.txt` and
`tour.node.txt`.

It needs a zena-cli from the fork with templated `@external` names,
`@export` and `@spelling` (`ZENA`, and `ZENA_REPO_ROOT` for its checkout),
and for `serve` a workerd (`WORKERD`, else `PATH`).

## The tour

`app.zena` is the router, and exports what the module provides. Each
topic is a file in `app/`.

| route | file | from the platform | from the IR |
|---|---|---|---|
| `/` | `app.zena` | a text binding | exported function, `future<Response>` |
| `/echo` | `http.zena` | method, URL, headers, body; a `Response` built from parts | `@js(null) str?` (`Headers#get`), a repeating callback (`forEach`), an iterator of `Str` (`keys`), an options object, `u16` from `@tw-abi` (`status`), a payload enum to write (`BodyInit`, `HeadersInit`) |
| `/url` | `http.zena` | `URL`, `URLSearchParams` | an iterator of `Tuple<str, str>` unboxed to a tuple, `err`'s `TypeError` arm caught, `@js(null) URL?` (`URL.parse`), statics |
| `/form` | `http.zena` | `FormData`, `File` | a payload enum to read (`File \| string`), inside a tuple box; `future<Uint8Array>` |
| `/json` | `http.zena` | `Request#json`, `JSON`, `Response.json` | a generic member at a declared interface (`json<Order>`), `Any`, optional properties (`f64?`, `arr<str>?`), a global from `env.d.ts` |
| `/crypto` | `crypto.zena` | `crypto.subtle.digest`, `getRandomValues`, `TextEncoder`, `btoa` | a `union` parameter's arm overloads, a typed array `u8[]`, `future<ArrayBuffer>`, a generic `T -> T`, a callback per byte |
| `/stream` | `streams.zena` | `ReadableStream`, `TransformStream`, `pipeTo`, `pipeThrough`, a streamed body | mono generics (`ReadableStream<str>`, imports per instantiation), callbacks returning `future<unit>?`, `T?` in `read()`'s result |
| `/timers` | `timers.zena` | `setTimeout`, `queueMicrotask`, `scheduler.wait`, `ctx.waitUntil` | one-shot callbacks, a variadic (`setTimeout`'s arguments), a Zena future given to JS |
| `/fetch` | `fetch.zena` | a service binding, the global `fetch` with `RequestInit`, `AbortSignal.timeout`, `AbortController` | a literal enum (`cache`), `@js(null)` setters (`body`, `signal`), a caller's signal joined with the bridge's, cancellation (`TaskGroup.race` cancels the loser, which requests its JS future's cancellation; it had no init to carry a signal), a function in a payload enum (the event listener) |
| `/rewrite` | `rewrite.zena` | `HTMLRewriter` | a callback property of an options object, a `union` parameter (`append`) |
| `/cache` | `cache.zena` | `caches.default`: `put`, `match`, `delete` | `future<Response?>`, `future<bool>` |
| `/counter/kv` | `durable.zena` | an id from a name, `stub.fetch`; storage `get`, `put`, `list`, `delete` | an exported class with an imported base, generic members at scalars (`get<f64>`: `f64?`), `Map<Str, T>` and its key iterator |
| `/counter/transaction` | `durable.zena` | `storage.transaction` | a generic callback, `fn(Txn) -> future<T>` |
| `/counter/sql` | `durable.zena` | `storage.sql.exec`, a cursor | a variadic of `Any`, `arr<str>`, an iterator of a declared row type, a symbol member (`[Symbol.iterator]`) |
| `/counter/alarm` | `durable.zena` | `setAlarm`, the `alarm` handler | a `union` parameter (`number \| Date`), an exported method JS calls |
| `/ws` | `ws.zena` | `WebSocketPair`, `accept`, message events; `acceptWebSocket` and `webSocketMessage` in a Durable Object | a tuple-like class (`pair[0]`), a base-less exported class, exported overloads (`string \| ArrayBuffer`), `arr<WebSocket>` |
| `/rpc` | `durable.zena` | a Durable Object method and a `WorkerEntrypoint` method over RPC | exported classes with imported bases, constructed through JS (`tw:new`); stubs declared in `env.d.ts` |
| `/fail` | `fail.zena` | errors in both directions | a `@throws` enum (`DOMExceptionErr`), `err`'s arms, a rejected future; Zena's `ErrException` and plain `Error` leaving as a JS `TypeError` and `Error` |
| `/scheduled`, `/queue` | `app.zena` | the `scheduled` and `queue` handlers | exported functions, a generic class to read (`MessageBatch<Any?>`) |

`helper.js` is a second service: the target of the service binding and of
the global `fetch`, the backing of the Cache API, and the caller of the
worker's `scheduled` and `queue` handlers and of its throwing routes. It
counts the requests it sees abort, which is how `/fetch` shows a
cancellation reaching it.

The declarations beside workers-types:

* `env.d.ts`: the bindings, the JSON and SQL row shapes, `JSON`, the RPC
  stubs, and two additions to workers-types: `digest` over a
  `Uint8Array`, which gives the `u8[]` copy, and the rewriter's element
  handler as a function property.
* `annotate.d.ts`: `@tw-abi u16` on a status, and `@throws
  {DOMException}` on `atob`.
* `provides.d.ts`: the default export's `fetch`, `scheduled` and `queue`,
  and the exported classes `Counter`, `Room` and `Greeter`.

## The pipeline

`run.sh`:

1. `ts-gen --provides provides.d.ts --with std.tw --ir`: the IR, against
   the JS standard library's.
2. `zena-bindgen --entry`: the bindings (`build/bindings.zena`, and
   `build/std.zena` for the library, whose `ErrException` and tw loop
   both use), and the entry module
   (`build/worker.zena`), which exports the functions and the classes'
   members under their IR keys with `@export`, over `app.zena`.
3. `zena build -t js`: the module. Generic members import per
   instantiation under their templated keys.
4. `ts-gen --bridge`: the bridge for the module. No link step is
   needed, as Zena names every import and export itself.

`loader.js` instantiates the module through the bridge's `instantiate`,
supplying Zena's own imports for the JS target: its `_` string constants
(each named by its text), its `env` stack trace hooks, and JS's number
formatting and string methods. It re-exports the bridge's exports.
workerd compiles wasm with the js-string builtins under
`wasm_esm_integration_builtins`; in node, `node.mjs` loads the `.wasm`
files as `WebAssembly.Module`s the same way.

Async: a `future<T>` is a Zena `Future<T>`, so a handler is an `async`
function. The bindings run one tw loop over Zena's microtask queue:
awaiting a JS future reads its value with `get<T>`, which has one once the
future is fulfilled, and validates it: a value that isn't a `T` fails the
await with the error a rejection would. With no value it reads the
`state`, and while that's pending leaves a waker on the future (`add_listener`);
`tw:future/wake` reads it again. A completion (or a callback's return)
asks the bridge for a tick (`work_available`), which drains the queue in
`tw:loop/tick`. Cancelling an awaiter requests the JS future's
cancellation (`cancel`) and stops reading it. A Zena future handed to JS
settles a `tw:future` `new` promise through `resolve<T>`, or `reject<err>`
when it fails.

Errors leave Zena the way they come in, as `ErrException`s thrown as the
`tw:tag` `err`: one with the `TypeError` arm rejects a JS call with a
`TypeError`, and any other Zena error leaves as the fallback arm, a JS
`Error`.

In node the helper stands behind the binding and the global `fetch`
directly, rejecting a call whose signal aborts, and `node.mjs` provides
`scheduler`, numeric timer ids and the `cloudflare:workers` base classes.
