//! @ts-gen --error-enum --error-type crate::Error --external AbortError=::web_sys::AbortError

// Fixture: `--error-enum` collects every type named in a `@throws` into one
// generated enum (`JsException` by default). Bindings with a `@throws` type —
// union or single — return `Result<T, JsException>`; untyped bindings keep the
// `--error-type` default. Variants are ordered most derived first so the
// `From<JsValue>` dispatch tests `TypeError` before `Error`. An `--external`
// class with no visible ancestry sorts before the builtin-rooted ones.

declare class DOMException extends Error {
  readonly code: number;
}

declare class QuotaError extends DOMException {
  readonly quota: number;
}

declare namespace Lock {
  class HeldError extends Error {
    readonly holder: string;
  }
}

declare class Store {
  /**
   * A union keeps both classes instead of collapsing to `Error`.
   * @throws {TypeError | RangeError} on bad input
   */
  put(key: string, value: string): void;

  /**
   * A single type routes through the enum too.
   * @throws {DOMException} when the store is closed
   */
  get(key: string): string | null;

  /**
   * Multiple `@throws` lines aggregate; a user subclass sorts before its
   * parents.
   * @throws {QuotaError} when over quota
   * @throws {Error} otherwise
   */
  grow(bytes: number): Promise<void>;

  /**
   * A qualified name resolves through its namespace; the variant is named
   * by its Rust path.
   * @throws {Lock.HeldError} when another writer holds the lock
   */
  lock(): void;

  /** Untyped — `crate::Error`, not the enum. */
  size(): number;

  /**
   * Typed getter.
   * @throws {TypeError}
   */
  readonly name: string;

  /**
   * An external class: ancestry unknown to ts-gen, so it is tested first.
   * @throws {AbortError} when the signal fires
   */
  abortable(): void;
}
