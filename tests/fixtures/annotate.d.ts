//! @ts-gen --annotate tests/fixtures/side/annotate.d.ts

// `--annotate` layers the `@rust`, `@throws` and `@retype` tags of a side
// file onto these declarations by name; the side file's own declarations
// are not emitted and untagged members there are ignored.

declare abstract class Response {
  readonly status: number;
  readonly ok: boolean;
  readonly body: ReadableStream | null;
  // Writable here, read-only at runtime: the side file's `readonly` drops
  // the setter.
  url: string;
  type: string;
  /** Creates a clone of the response. */
  clone(): Response;
  bytes(): Promise<any>;
}

declare const ResponsePair: {
  new (): {
    0: Response;
    1: Response;
  };
};

declare abstract class ReadableStream<R = any> {
  readonly locked: boolean;
}

interface ResponseInit {
  status?: number;
  statusText?: string;
}

declare abstract class Cache {
  put(url: string, ttl?: number): Promise<void>;
  put(request: Response, ttl?: number): Promise<void>;
}

interface Storage {
  delete(key: string): Promise<boolean>;
  delete(keys: string[]): Promise<number>;
}

interface Console {
  log(...data: any[]): void;
}

declare const console: Console;

declare function setTimeout(callback: () => void, ms?: number): number;

declare function setInterval(callback: () => void): number;
declare function setInterval(callback: () => void, ms: number): number;

declare function size(key: string): number;
declare function size(keys: string[], n: number): number;

// A side declaration's type-parameter defaults replace the input's. The
// side interface's default makes the emitted `Request<Meta = JsValue, Cf =
// Incoming>`, so a bare `Request` reads `cf()` as `Incoming` rather than
// the erased union; the side alias's default expands `fetch`'s argument to
// `Request<JsValue, Incoming>`, which collapses to `&Request`.
interface Incoming {
  readonly colo: string;
}
interface Outgoing {
  readonly cacheTtl: number;
}
type CfProperties = Incoming | Outgoing;
interface Request<Meta = unknown, Cf = CfProperties> {
  readonly cf?: Cf;
}
type RequestInfo<Meta = unknown, Cf = CfProperties> = Request<Meta, Cf> | string;
declare function fetch(input: RequestInfo): Promise<Request>;
