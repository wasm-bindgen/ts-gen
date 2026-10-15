//! @ts-gen --lib-name nested-arrays --export nested-arrays

// A top-level `T[]` keeps the Rust-native `&[String]` / `Vec<f64>` form only
// when its element resolves (through aliases and unions) to a string or
// number. wasm-bindgen has no slice / `Vec` ABI for other Rust-native
// elements, so they take the JS handle: `string[][]` is `&[Array<JsString>]`
// in argument position and `Vec<Array<JsString>>` in return position.

export type Row = string[];
export type Id = string;

export declare class Grid {
  rows(): Promise<string[][]>;
  putRows(rows: number[][]): void;
  names(): string[];
  tags: boolean[][];
}

export declare function deep(): string[][][];
export declare function putDeep(rows: string[][][]): void;
export declare function readonlyRows(rows: readonly string[][]): string[][];
export declare function genericRows(rows: Array<string>[]): Array<string>[];
export declare function aliasRows(rows: Row[]): Row[];
export declare function maybeRows(rows: string[][] | null): string[][] | null;
export declare function forEachRow(callback: (rows: string[][]) => void): void;
export declare function nullableStrings(values: (string | null)[]): (string | null)[];
export declare function nullableRows(rows: (string[] | null)[]): (string[] | null)[];
export declare function flags(values: boolean[]): boolean[];
export declare function ids(values: Id[]): Id[];
export declare function modes(values: ("a" | "b")[]): ("a" | "b")[];
export declare function counts(values: (1 | 2)[]): (1 | 2)[];
export declare function bigints(values: bigint[]): bigint[];
export declare function grids(values: Grid[]): Grid[];
