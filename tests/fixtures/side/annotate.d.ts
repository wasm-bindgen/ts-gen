// Side file for `annotate.d.ts`: re-declares only the members it tags.

interface Response {
  /** @rust u16 */
  readonly status: number;
  /** @throws {TypeError} if the body has already been used */
  clone(): Response;
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

interface Request<Meta = unknown, Cf = Incoming> {}
type RequestInfo<Meta = unknown, Cf = Incoming> = Request<Meta, Cf> | string;
