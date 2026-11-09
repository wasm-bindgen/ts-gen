// Side file for `annotate.d.ts`: re-declares only the members it tags.

// Only the `## Errors` section of a side doc is carried over: `clone`,
// copied from upstream, keeps one description, and `ok`, with no tag,
// is ignored.
interface Response {
  /** @tw-abi u16 */
  readonly status: number;
  /** A note that is not carried over. */
  readonly ok: boolean;
  /**
   * Creates a clone of the response.
   * @throws {TypeError} if the body has already been used
   */
  clone(): Response;
}

interface ResponseInit {
  /** @tw-abi u16 */
  status?: number;
}

declare abstract class Cache {
  /** @tw-abi ttl: u32 */
  put(url: string, ttl?: number): Promise<void>;
}

// A return tag applies to the overloads whose return can carry it.
interface Storage {
  /** @tw-abi u32 */
  delete(keys: string[]): Promise<number>;
}

interface Console {
  /** @throws {never} */
  log(...data: any[]): void;
}

/** @tw-abi i32 */
declare function setTimeout(callback: () => void, ms?: number): number;

// A side function applies to every input overload, and each side
// overload applies.
/**
 * @tw-abi i32
 * @throws {never}
 */
declare function setInterval(callback: () => void, ms?: number): number;

/** @tw-abi u32 */
declare function size(key: string): number;
/** @tw-abi n: u32 */
declare function size(keys: string[], n: number): number;

interface Request<Meta = unknown, Cf = Incoming> {}
type RequestInfo<Meta = unknown, Cf = Incoming> = Request<Meta, Cf> | string;
