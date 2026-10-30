//! @ts-gen --lib-name inline-wrappers --export inline-wrappers
// `@rust u64` / `i64` on a `number`: the extern ABI crosses 64-bit
// integers as BigInt, so the binding keeps `f64` under a private
// `__<name>` name and an inline `pub fn` of the public name converts
// with `as`, through `?` / `| null`, `T[]`, `Promise<…>` and `Result`.
// The `_promise` companion hands its `Promise<T>` to JS, so it stays
// `Promise<Number>`.

export interface R2Object {
  /**
   * Size in bytes.
   * @rust u64
   */
  readonly size: number;
  /** @rust u64 */
  readonly uploadedAt: number | null;
  /** @rust limit: u64 */
  list(limit: number, prefix?: string): Promise<R2Object[]>;
  /** @rust u64 */
  count(): Promise<number>;
  /** @rust u64 */
  sizes(): number[];
  /** @rust parts: u64 */
  upload(parts: number[]): void;
  // `| null` splits into a slice form and a `_with_null` one; a
  // non-trailing `| undefined` stays one `Option<&[u64]>` parameter,
  // collected inside the `map` and borrowed outside it.
  /** @rust parts: u64 */
  uploadMaybe(parts: number[] | null): void;
  /** @rust parts: u64 */
  uploadUnset(parts: number[] | undefined, tag: string): void;
  /**
   * @rust offset: i64
   * @rust i64
   */
  seek(offset: number): number;
  // Inside a generic handle the leaf stays a JS `Number`, as any
  // `@rust` scalar does there, and needs no wrapper.
  /** @rust u64 */
  readonly settled: Promise<number>;
  /** @rust u64 */
  matrix(): number[][];
}

export declare class R2Bucket {
  /** @rust u64 */
  static maxSize(): number;
  /** @rust size: u64 */
  constructor(size: number);
}

export interface R2PutOptions {
  /**
   * Size in bytes.
   * @rust u64
   */
  size?: number;
}

/** @rust u64 */
export declare function now(): number;

export interface Box<T> {
  /** @rust u64 */
  readonly length: number;
  /** @rust n: u64 */
  take(n: number): T;
}
