//! @ts-gen --lib-name tags

// `@tw-abi` tags: a tag replaces the leaf type through `?`, `T[]` and
// `future`; `number` lifts to `i53`/`u53` under `i64`/`u64`; and
// `@throws {never}` drops the error channel, except on a `never` return.

declare class Pixels {
  /** @tw-abi u16 */
  readonly width: number;
  /** @tw-abi u64 */
  readonly count: number;
  /** @tw-abi i64 */
  readonly big: bigint;
  /**
   * @tw-abi x: u8
   * @tw-abi f32
   */
  sample(x: number, y: number): number;
  /** @tw-abi u32 */
  sizes(): number[];
  /** @tw-abi u32 */
  first(): number | null;
  /** @tw-abi u32 */
  load(): Promise<number>;
  /** @throws {never} */
  clear(): void;
  /** @throws {never} */
  fail(): never;
}
