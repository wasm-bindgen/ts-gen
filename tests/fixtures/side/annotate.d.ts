// Side file for `annotate.d.ts`: re-declares only the members it tags.

// Only the `## Errors` section of a side doc is carried over: `clone`,
// copied from upstream, keeps one description, and `ok`, with no tag,
// is ignored.
interface Response {
  /** @rust u16 */
  readonly status: number;
  /** A note that is not carried over. */
  readonly ok: boolean;
  /**
   * Creates a clone of the response.
   * @throws {TypeError} if the body has already been used
   */
  clone(): Response;
  // `@retype`: the side member's type replaces the input's.
  /** @retype */
  readonly body: ReadableStream<Uint8Array> | null;
  /**
   * The body as bytes.
   * @retype
   */
  bytes(): Promise<Uint8Array>;
  // A property with no setter here is read-only in the input too.
  readonly url: string;
  get type(): string;
}

interface ResponsePair {
  readonly 0: Response;
  readonly 1: Response;
}

interface ResponseInit {
  /** @rust u16 */
  status?: number;
}

declare abstract class Cache {
  /** @rust ttl: u32 */
  put(url: string, ttl?: number): Promise<void>;
}

// A return tag applies to the overloads whose return can carry it.
interface Storage {
  /** @rust u32 */
  delete(keys: string[]): Promise<number>;
}

interface Console {
  /** @throws {never} */
  log(...data: any[]): void;
}

/** @rust i32 */
declare function setTimeout(callback: () => void, ms?: number): number;

// A side function applies to every input overload, and each side
// overload applies.
/**
 * @rust i32
 * @throws {never}
 */
declare function setInterval(callback: () => void, ms?: number): number;

/** @rust u32 */
declare function size(key: string): number;
/** @rust n: u32 */
declare function size(keys: string[], n: number): number;

interface Request<Meta = unknown, Cf = Incoming> {}
type RequestInfo<Meta = unknown, Cf = Incoming> = Request<Meta, Cf> | string;
