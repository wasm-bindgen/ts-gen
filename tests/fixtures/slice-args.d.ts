//! @ts-gen --lib-name slice-args --export slice-args

// Every `T[]` argument reaches JS as a plain `Array`. A slice of Rust values
// (`&[f64]`, `&[String]`) needs `slice_to_array` for that, however the
// element is spelled (an alias, a literal union, an enum); a slice of JS
// handles is already an `Array` and takes no attribute, which is what lets a
// type-parameter element compile.

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
// either form. Accepting `number[]` means the callee reads the values during
// the call, so the slice of the typed array's element is passed as a
// zero-copy view and comes first; the typed array keeps a handle overload.
export declare function bytes(values: Uint8Array | number[]): string;
export declare function words(values: BigInt64Array | bigint[]): string;
export declare function unsigned(values: BigUint64Array | bigint[]): string;
export declare function buffered(values: Uint8Array<ArrayBuffer> | number[]): string;
export declare function floats(values?: number[] | Float64Array | null): string;

// `slice_to_array` is per-argument, so a `number[]` beside a typed-array
// union doesn't turn the typed slice's view into a plain `Array`.
export declare function mixed(nums: number[], bytes: Uint8Array | number[]): string;
