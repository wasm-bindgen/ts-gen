//! @ts-gen --lib-name inline-wrappers --export inline-wrappers --errors-as-error
// `@rust u64` / `i64` on a `number`: the extern ABI crosses 64-bit
// integers as BigInt, so the binding keeps `f64` under a private
// `<name>_js` name and an inline `pub fn` of the public name converts
// with `as`, through `?` / `| null`, `T[]`, `Promise<…>` and `Result`.
// The `_promise` companion hands its `Promise<T>` to JS, so it stays
// `Promise<f64>`.

export interface R2Object {
  /** @rust u64 */
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
  /**
   * @rust offset: i64
   * @rust i64
   */
  seek(offset: number): number;
}

export declare class R2Bucket {
  /** @rust u64 */
  static maxSize(): number;
  /** @rust size: u64 */
  constructor(size: number);
}

export interface R2PutOptions {
  /** @rust u64 */
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
