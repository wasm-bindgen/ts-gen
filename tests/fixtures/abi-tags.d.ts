// `@tw-abi <type>` gives the ABI scalar of a property or a callable's
// return; `@tw-abi <param>: <type>` of one parameter. The scalar replaces
// the leaf through `?`, `| null`, `T[]` and an async return's `Promise<…>`.
// u64/i64 on a `bigint` cross as BigInt; on a `number` they are `u53`/`i53`.
// Any other type takes no tag (an error diagnostic).

interface Response {
  /** @tw-abi u16 */
  readonly status: number;
  /** @tw-abi u32 */
  readonly contentLength: number | null;
  /**
   * @tw-abi status: u16
   * @tw-abi u32
   */
  withStatus(status: number, reason?: string): number;
  /** @tw-abi u32 */
  count(): Promise<number>;
  /** @tw-abi u32 */
  maybeCount(): Promise<number | null>;
  /** @tw-abi partNumber: u16 */
  uploadPart(partNumber: number): void;
  /** @tw-abi u8 */
  bytes(): number[];
  /** @tw-abi ports: u16 */
  listen(ports: number[]): void;
  /** @tw-abi u64 */
  readonly offset: bigint;
  /** @tw-abi u64 */
  readonly size: number;
  // A `u53`/`i53` is a `SafeInteger`, through `| null`, arrays and an async
  // return like any scalar.
  /** @tw-abi i53 */
  readonly delta: number;
  /** @tw-abi u64 */
  readonly remaining: number | null;
  /** @tw-abi offset: i64 */
  seek(offset: number): void;
  /** @tw-abi u64 */
  sizes(): number[];
  /** @tw-abi u64 */
  totalSize(): Promise<number>;
  /** @tw-abi u16 */
  readonly statusText: string;
  // Inside a generic handle the leaf must stay a JS type, so the tag
  // narrows nothing there: a `Promise<number>` that is not an async
  // return, a nested array, an array of nullables.
  /** @tw-abi u32 */
  readonly closed: Promise<number>;
  /** @tw-abi u64 */
  readonly total: Promise<bigint>;
  /** @tw-abi ready: u32 */
  whenReady(ready: Promise<number>): void;
  /** @tw-abi u8 */
  matrix(): number[][];
  /** @tw-abi rows: u8 */
  setMatrix(rows: number[][]): void;
  /** @tw-abi u16 */
  codes(): (number | null)[];
  /** @tw-abi codes: u16 */
  setCodes(codes: (number | null)[]): void;
}

interface ResponseInit {
  /** @tw-abi u16 */
  status?: number;
}

/**
 * @tw-abi ms: u32
 * @tw-abi i32
 */
declare function setTimeout(callback: () => void, ms?: number): number;
