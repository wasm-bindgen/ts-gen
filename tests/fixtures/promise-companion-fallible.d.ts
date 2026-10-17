//! @ts-gen --fallible --error-type crate::Error

// Fixture: the `<name>_promise` form under `--fallible`. The policy says
// no binding aborts, so the sync companion catches a synchronous throw
// (`Result<Promise<T>, E>`); rejection still travels in the promise.
// `@throws {X}` types the sync throw; `@throws {never}` makes it plain.

declare class Store {
  put(key: string, value: string): Promise<void>;
  /** @throws {TypeError} on an invalid key */
  get(key: string): Promise<string | null>;
  /** @throws {never} */
  ready(): Promise<boolean>;
}

declare function sleep(ms: number): Promise<void>;
