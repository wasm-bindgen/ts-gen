//! @ts-gen --experimental-generic-mono

// Under per-monomorphization a `null`-admitting argument is an `Option`
// like an undefined-only one, keeping strings and numbers on their native
// ABI; `None` crosses as `undefined`. A union that fans out into overloads
// takes each arm bare and gives `null` its own arm.

declare class Signal {}
declare class Body<T = unknown> {
  constructor(body: string | null, length?: number | null);
  static from(signal: Signal | null, tags: string[] | null): Body;
  put<V>(value: V | null): void;
}
interface ListOptions {
  prefix?: string | null;
  limit?: number | null;
  signal?: Signal | null;
  body?: string | Signal | null;
}
declare function clearTimeout(id: number | null): void;

type BodyInit = string | ArrayBuffer | Signal;
interface ResponseInit {
  status?: number;
}
declare class Reply {
  constructor(body?: BodyInit | null, init?: ResponseInit);
}
declare class Bucket {
  put(key: string, value: ArrayBuffer | string | null): Promise<void>;
}
