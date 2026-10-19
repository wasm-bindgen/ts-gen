//! @ts-gen --experimental-generic-mono --fallible --error-type crate::Error

// A per-mono async member's `Promise<T, E>` awaits as `Result<T, E>`, with
// the binding's error type as `E`: the configured one, or the `@throws`
// type. `@throws {never}` keeps `JsValue`, `Promise<T>`'s default. A
// `Promise`-typed property awaits the same way.

export declare function fetchText(url: string): Promise<string>;
/** @throws {TypeError} if the body was already used */
export declare function json(): Promise<unknown>;
/** @throws {never} */
export declare function ready(): Promise<void>;
export declare abstract class Store {
  get(key: string): Promise<string | null>;
  readonly closed: Promise<void>;
  get opened(): Promise<string | null>;
  /** @throws {never} */
  readonly settled: Promise<number>;
  static readonly ready: Promise<boolean>;
}
