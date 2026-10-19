//! @ts-gen --experimental-generic-mono --lib-name slice-args-mono --export slice-args-mono

// Per-monomorphization keeps a type-parameter element as a bare `T`, which
// wasm-bindgen lowers per instantiation: `T = f64` would be a `Float64Array`
// view. So a `T[]` argument takes `slice = "array"`, which reaches JS as a
// plain `Array` for numeric `T` and is a no-op for every other `T`.

export declare function generic<T>(values: T[]): string;
export declare class Bag<T> {
  constructor();
  take(values: T[]): string;
}
export declare function mixed<T>(values: T[], weights: number[], data: Float64Array | number[]): string;

// An `Iterable<T>` argument is taken like `T[]`.
export declare class Queue<T> {
  sendBatch(messages: Iterable<T>): void;
  tag(names: Iterable<string>, scores: Iterable<number> | null): void;
}

// A written list of a union takes each element arm with a typed slice
// (strings, numbers, buffer sources, handles) as its own binding, named for
// the element, before the erased mixed list.
export declare class Blob {
  constructor(bits?: (ArrayBuffer | ArrayBufferView | Blob | string)[]);
}
export interface DataPoint {
  blobs?: ((ArrayBuffer | string) | null)[];
}
export declare function write(point: DataPoint): void;
