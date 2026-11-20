// ABI choices layered onto workers-types by name (`--annotate`).

interface Response {
  /** @tw-abi u16 */
  readonly status: number;
}

interface ResponseInit {
  /** @tw-abi u16 */
  status?: number;
}

/** @throws {DOMException} */
declare function atob(data: string): string;
