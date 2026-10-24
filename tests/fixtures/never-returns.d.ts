//! @ts-gen --lib-name never-returns --export never-returns

// Fixture: a callable returning `never` always throws, rejects or never
// returns, so only its catching form is emitted, with a `()` success type:
// `try_<name>` by default, the primary under `--fallible`. `Promise<never>`
// keeps its `async fn` and `_promise` forms awaiting `()`. A `never` return
// overrides `@throws {never}`, and overloads apply the rule one by one,
// literal-keyed arms included: each arm's wrapper is catch-only.
// A `never`-typed property is unaffected.

/** Always throws an `Error` carrying `message`. */
export declare function fail(message: string): never;

/** @throws {RangeError} always */
export declare function failRange(n: number): never;

/**
 * Contradicts its return type: the `never` return wins.
 * @throws {never}
 */
export declare function failAnyway(): never;

/** Always rejects. */
export declare function rejectLater(message: string): Promise<never>;

/** The string overload returns; the number overload throws. */
export declare function check(value: string): string;
export declare function check(value: number): never;

/** Every literal arm throws. */
export declare function raise(kind: "type", message: string): never;
export declare function raise(kind: "range", message: string): never;

/** The `"text"` arm returns; the `"fail"` arm throws. */
export declare function read(kind: "text"): string;
export declare function read(kind: "fail"): never;

/** A variable typed as a function binds as one. */
export declare const abort: (code: number) => never;

export declare class Guard {
  constructor();
  /** A guard has no primitive value. */
  valueOf(): never;
  /** Always rejects. */
  failLater(): Promise<never>;
  static unreachable(reason: string): never;
  /** Every literal arm throws. */
  reject(kind: "soft"): never;
  reject(kind: "hard"): never;
  readonly impossible: never;
}

export interface Halt {
  ok(): boolean;
}
export declare var Halt: {
  prototype: Halt;
  new (): Halt;
  stop(code: number): never;
};
