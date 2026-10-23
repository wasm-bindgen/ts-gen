//! @ts-gen --lib-name slice-args --export slice-args

// Every `T[]` argument reaches JS as a plain `Array`. A slice of numbers
// needs `slice = "array"` for that, however the element is spelled (an alias,
// a literal union, a numeric enum); every other slice (strings, JS handles, a
// type parameter) is already an `Array` and takes no attribute.

export type Count = number;
export declare enum Level {
  Low = 1,
  High = 2,
}
export declare class Item {
  constructor(name: string);
  readonly name: string;
}

export declare function numbers(values: number[]): string;
export declare function counts(values: Count[]): string;
export declare function literals(values: (1 | 2)[]): string;
export declare function levels(values: Level[]): string;
export declare function strings(values: string[]): string;
export declare function flags(values: boolean[]): string;
export declare function bigints(values: bigint[]): string;
export declare function items(values: Item[]): string;
export declare function generic<T>(values: T[]): string;

// `TypedArray | number[]` (or `bigint[]` for the 64-bit arrays) is how
// wasm-bindgen declares a numeric slice argument, and how libraries accept
// either form. The slice of the typed array's element comes first and takes
// `slice = "copy"`, so JS receives a fresh typed array rather than a view of
// linear memory; the typed array keeps a handle overload. A
// `Uint8ClampedArray` slice is wrapped in `Clamped`, and beside a `Uint8Array`
// the slice is the `Uint8Array`'s.
export declare function bytes(values: Uint8Array | number[]): string;
export declare function words(values: BigInt64Array | bigint[]): string;
export declare function unsigned(values: BigUint64Array | bigint[]): string;
export declare function buffered(values: Uint8Array<ArrayBuffer> | number[]): string;
export declare function clamped(values: Uint8ClampedArray | number[]): string;
export declare function pixels(values: Uint8Array | Uint8ClampedArray | number[]): string;
export declare function floats(values?: number[] | Float64Array | null): string;
export declare function maybeBytes(values: Uint8Array | number[] | undefined): string;
export declare function maybeClamped(values: Uint8ClampedArray | number[] | undefined): string;

// Slice modes are per-argument, so a `number[]` beside a typed-array
// union doesn't turn the typed slice's copy into a plain `Array`.
export declare function mixed(nums: number[], bytes: Uint8Array | number[]): string;

// `Array<T>` / `ReadonlyArray<T>` are the same TS type as `T[]` and lower
// identically. `ArrayLike<T>` is the distinct TS type that keeps the JS
// array-handle form.
export declare function spelled(values: Array<string>): string;
export declare function spelled_readonly(values: ReadonlyArray<number>): string;
export declare function array_like(values: ArrayLike<string>): string;

// Per-monomorphization keeps a type-parameter element as a bare `T`, which
// wasm-bindgen lowers per instantiation: `T = f64` would be a `Float64Array`
// view. So a `T[]` argument takes `slice = "array"`, which reaches JS as a
// plain `Array` for numeric `T` and is a no-op for every other `T`.
export declare class Bag<T> {
  constructor();
  take(values: T[]): string;
}
export declare function mixedGeneric<T>(values: T[], weights: number[], data: Float64Array | number[]): string;
