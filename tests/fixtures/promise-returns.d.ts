// Fixture: a `Promise`-returning callable is one catching binding
// returning `Promise<T, E>`. The call starts eagerly, a synchronous throw
// rejects the promise, and awaiting it yields `Result<T, E>`, with `T`
// lowered like a sync return and `E` typed by `@throws`. It is the same
// under every policy.

declare class State {
  /** `block_concurrency_while() -> Promise<()>`. */
  blockConcurrencyWhile(callback: () => Promise<void>): Promise<void>;
  /** Overloads: each expansion is its own promise. */
  fetch(url: string): Promise<Response>;
  fetch(url: string, init: RequestInit): Promise<Response>;
  /** Awaited: `Promise<Nullable<String>>`. */
  get(key: string): Promise<string | null>;
  /** A `null` argument arm is its own promise too. */
  put(key: string, value: string | null): Promise<void>;
  /** @throws {TypeError} on an invalid key */
  delete(key: string): Promise<boolean>;
  /**
   * `@throws {never}` drops `catch`: a plain `Promise<bool>`.
   * @throws {never}
   */
  ready(): Promise<boolean>;
  /** Sync methods are untouched. */
  id(): string;
  /** A `Promise`-typed property is a getter of the promise. */
  readonly done: Promise<void>;
}

declare class Response {}
interface RequestInit {
  method?: string;
}

declare function sleep(ms: number): Promise<void>;
