//! @ts-gen --experimental-generic-mono

// Under per-monomorphization a `null`-admitting argument is an `Option`
// like an undefined-only one, keeping strings and numbers on their native
// ABI; `None` crosses as `undefined`.

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
}
declare function clearTimeout(id: number | null): void;
