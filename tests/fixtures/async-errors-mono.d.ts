//! @ts-gen --experimental-generic-mono --fallible --error-type crate::Error

// A per-mono async member's `Promise<T, E>` awaits as `Result<T, E>`, with
// the binding's error type as `E`: the configured one, or the `@throws`
// type. `@throws {never}` keeps `JsValue`, `Promise<T>`'s default.

export declare function fetchText(url: string): Promise<string>;
/** @throws {TypeError} if the body was already used */
export declare function json(): Promise<unknown>;
/** @throws {never} */
export declare function ready(): Promise<void>;
export declare abstract class Store {
  get(key: string): Promise<string | null>;
}
