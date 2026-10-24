// Fixture: every `Promise`-returning callable also gets a sync
// `<name>_promise` form returning the `Promise<T>` itself, for callers
// that must start the call now rather than when the future is polled.
// Rejection travels in the promise. Under the default policy the form is
// plain (a sync throw aborts, as the primary of any sync callable does).
// Under `--fallible` it catches a synchronous throw as
// `Result<Promise<T>, E>`, typed by `@throws`, except for a builtin or
// global callable, which rejects rather than throws: these are all
// globals, so their `_promise` forms are plain in the `mono` snapshot too
// (`promise-builtins` covers the module cases).

declare class State {
  /** `block_concurrency_while` + `block_concurrency_while_promise`. */
  blockConcurrencyWhile(callback: () => Promise<void>): Promise<void>;
  /** Overloads: each expansion gets its own `_promise`. */
  fetch(url: string): Promise<Response>;
  fetch(url: string, init: RequestInit): Promise<Response>;
  /** Awaited: `Nullable<String>`; the promise form's `string | null` is an inner position: `Promise<JsNullable<JsString>>`. */
  get(key: string): Promise<string | null>;
  /** A `null` argument arm gets its own `_promise` form too. */
  put(key: string, value: string | null): Promise<void>;
  /** @throws {TypeError} on an invalid key */
  delete(key: string): Promise<boolean>;
  /**
   * `@throws {never}` drops `catch` from the async form; the promise
   * form is plain under every policy.
   * @throws {never}
   */
  ready(): Promise<boolean>;
  /** Sync methods are untouched. */
  id(): string;
  /** A `Promise`-typed property is already sync; no companion. */
  readonly done: Promise<void>;
}

declare class Response {}
interface RequestInit {
  method?: string;
}

declare function sleep(ms: number): Promise<void>;
