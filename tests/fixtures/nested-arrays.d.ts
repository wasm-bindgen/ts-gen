// wasm-bindgen has no `Vec<Vec<_>>` ABI, so the inner array of `T[][]`
// is the `Array<U>` handle: `string[][]` is `&[Array<JsString>]` in
// argument position and `Vec<Array<JsString>>` in return position.
// A single level stays a Rust slice / Vec.

declare class Grid {
  rows(): Promise<string[][]>;
  putRows(rows: number[][]): void;
  names(): string[];
  tags: boolean[][];
}
