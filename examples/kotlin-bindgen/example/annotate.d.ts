// ABI choices layered onto workers-types by name (`--annotate`): scalar
// widths, and the errors a member throws.

interface Response {
  /** @tw-abi u16 */
  readonly status: number;
}

interface ResponseInit {
  /** @tw-abi u16 */
  status?: number;
}

interface URLSearchParams {
  /** @tw-abi u32 */
  readonly size: number;
}

interface Blob {
  /** @tw-abi u32 */
  readonly size: number;
}

/** @throws {DOMException} if the data is not base64 */
declare function atob(data: string): string;
