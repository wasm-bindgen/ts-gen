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
