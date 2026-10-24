//! @ts-gen --lib-name slice-args-mono --export slice-args-mono

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
