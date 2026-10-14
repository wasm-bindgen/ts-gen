// `@rust <type>` forces the Rust scalar for a property or a callable's
// return; `@rust <param>: <type>` for one parameter. The scalar
// replaces the leaf through `?`, `| null`, `T[]` and `Promise<…>`;
// wasm-bindgen converts the JS number at the boundary. u64/i64 cross
// as BigInt, so they apply to `bigint` only; on a `number` (or any
// other type) the tag is an error diagnostic.

interface Response {
  /** @rust u16 */
  readonly status: number;
  /** @rust u32 */
  readonly contentLength: number | null;
  /**
   * @rust status: u16
   * @rust u32
   */
  withStatus(status: number, reason?: string): number;
  /** @rust u32 */
  count(): Promise<number>;
  /** @rust partNumber: u16 */
  uploadPart(partNumber: number): void;
  /** @rust u8 */
  bytes(): number[];
  /** @rust ports: u16 */
  listen(ports: number[]): void;
  /** @rust u64 */
  readonly offset: bigint;
  /** @rust u64 */
  readonly size: number;
  /** @rust u16 */
  readonly statusText: string;
}

interface ResponseInit {
  /** @rust u16 */
  status?: number;
}

/**
 * @rust ms: u32
 * @rust i32
 */
declare function setTimeout(callback: () => void, ms?: number): number;
