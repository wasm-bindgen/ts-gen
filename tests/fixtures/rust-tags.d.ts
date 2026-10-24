// `@rust <type>` forces the Rust scalar for a property or a callable's
// return; `@rust <param>: <type>` for one parameter. The scalar
// replaces the leaf through `?`, `| null`, `T[]` and an async return's
// `Promise<…>`; wasm-bindgen converts the JS number at the boundary. u64/i64 cross
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
  /** @rust u32 */
  maybeCount(): Promise<number | null>;
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
  // Inside a generic handle the leaf must stay a JS type, so the tag
  // narrows nothing there: a `Promise<number>` that is not an async
  // return, a nested array, an array of nullables.
  /** @rust u32 */
  readonly closed: Promise<number>;
  /** @rust u64 */
  readonly total: Promise<bigint>;
  /** @rust ready: u32 */
  whenReady(ready: Promise<number>): void;
  /** @rust u8 */
  matrix(): number[][];
  /** @rust rows: u8 */
  setMatrix(rows: number[][]): void;
  /** @rust u16 */
  codes(): (number | null)[];
  /** @rust codes: u16 */
  setCodes(codes: (number | null)[]): void;
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
