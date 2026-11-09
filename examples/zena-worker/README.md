# Zena Worker

A Cloudflare Worker in Zena over `tests/fixtures/workers-types.d.ts`, bound
through the v2 IR under the GC profile with host strings (`all-gc, str =
host`). It replies with the request's method, URL, `x-zena` header and
body, and the `GREETING` binding. Along the way it:

* lists the `x-` headers through a `Headers.forEach` callback (a Zena
  closure JS calls through a `tw:call:F` trampoline);
* waits for a `queueMicrotask` callback that completes a Zena `Completer`;
* catches the `TypeError` of `new URL("not a url")` as an `ErrException`
  with `arm == ErrArm.TypeError`, and the rejection of `fetch("not a url")`;
* given an `x-hang` URL, races a fetch of it against `scheduler.wait` with
  `TaskGroup.race`. The timeout wins, which cancels the fetch: Zena drops
  the JS future, and the bridge aborts it through its `AbortController`.

`/count` goes to the `Counter` Durable Object through the `COUNTER`
binding. `Counter` is the app's Zena class, exported as a JS class whose
instances hold a Zena handle; it counts the requests it serves.

Errors leave Zena the way they come in, as `ErrException`s thrown as the
`tw:tag` `err`: `/fail` throws one with the `TypeError` arm, which rejects
the JS fetch with a `TypeError`, and any other Zena error, like the plain
`Error` the `Counter` constructor throws without a `GREETING`, leaves as
the fallback arm, a JS `Error`.

```sh
./run.sh node    # in node, with a hanging server for x-hang, then /count twice
./run.sh serve   # workerd on 127.0.0.1:8788, then curls /echo, /count twice and /fail
```

It needs a zena-cli from the fork with templated `@external` names,
`@export` and `@spelling` (`ZENA`, and `ZENA_REPO_ROOT` for its checkout).

The pipeline (`run.sh`):

1. `ts-gen --provides provides.d.ts --ir`: the IR, with the default
   export's `fetch` and the `Counter` export class.
2. `ts-gen-zena --entry`: the bindings (`build/bindings.zena`), and the
   entry module (`build/worker.zena`), which exports `fetch` and
   `Counter`'s members under their IR keys with `@export`, over `fetch` and
   `Counter` in `app.zena`.
3. `zena build -t js`: the module. Generic members import per
   instantiation under their templated keys.
4. `ts-gen-bridge generate`: the bridge for the module. No link step is
   needed, as Zena names every import and export itself.

`loader.js` instantiates the module through the bridge's `instantiate`,
supplying Zena's `_` string constants (each named by its text) and its
`env` stack trace hooks, and re-exports the bridge's `default` and
`Counter`. workerd
compiles wasm with the js-string builtins under
`wasm_esm_integration_builtins`; in node, `node.mjs` loads the `.wasm`
files as `WebAssembly.Module`s the same way.

Async: a `future<T>` is a Zena `Future<T>`, so `fetch` is an `async`
function. The bindings run one tw loop over Zena's microtask queue:
awaiting a JS future polls it with a waker, `tw:future/wake` polls again,
and a completion (or a callback's return) asks the bridge for a tick
(`work_available`), which drains the queue in `tw:loop/tick`. A Zena
future handed to JS settles a `tw:future` `new` promise through
`resolve<T>`, or `reject<err>` when it fails.

Node lacks `scheduler`, so `node.mjs` provides one, and stands in a
`COUNTER` namespace over a `Counter` it constructs. `env.d.ts` types
`COUNTER` as an interface: workerd's namespace binding isn't an instance
of a global `DurableObjectNamespace`, which the bridge checks for the class
workers-types declares.
