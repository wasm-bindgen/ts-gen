// Fixture: every `Promise`-returning callable also gets a sync
// `<name>_promise` form returning the `Promise<T>` itself, for callers
// that must start the call now rather than when the future is polled.
// Under the default policy it is plain (a sync throw aborts, as the
// primary of any sync callable does); see `promise-companion-fallible`.

declare class State {
  /** `block_concurrency_while` + `block_concurrency_while_promise`. */
  blockConcurrencyWhile(callback: () => Promise<void>): Promise<void>;
  /** Overloads: each expansion gets its own `_promise`. */
  fetch(url: string): Promise<Response>;
  fetch(url: string, init: RequestInit): Promise<Response>;
  /** Nullable result keeps its inner wrapper: `Promise<JsNullable<JsString>>`. */
  get(key: string): Promise<string | null>;
  /**
   * `@throws {never}` drops `catch` from the async form; the promise
   * form is plain either way.
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
