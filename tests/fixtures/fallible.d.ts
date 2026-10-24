//! @ts-gen --lib-name fallible --export fallible

// Fixture: `--fallible` folds the trapping primary + `try_<name>` pair into
// one catching `Result` form under the primary name. `@throws {never}` keeps
// a binding infallible; `@throws {T}` types the error; constructors and
// async callables are unchanged.

/** Throws when `n` is negative. */
export declare function checkedSqrt(n: number): number;

/**
 * Pure arithmetic — stays a plain `f64` return under `--fallible`.
 * @throws {never}
 */
export declare function double(n: number): number;

/** @throws {RangeError} when `n` is out of range */
export declare function clampIndex(n: number, len: number): number;

export declare function parseLater(s: string): Promise<number>;

export declare class Counter {
  constructor(start: number);
  /** Throws when the counter would overflow `max`. */
  increment(by?: number): number;
  readonly value: number;
  max: number;
}
