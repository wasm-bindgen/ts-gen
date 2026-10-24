//! @ts-gen --lib-name literal-discriminants --export literal-discriminants

// Dictionary factories pass an integer literal as the getter's enum
// variant, read through the alias, and bake any other number literal in as
// an `f64` literal.
export type KeepAlive = 0 | 1 | 2;

export interface Options {
  keepAlive: KeepAlive;
  label?: string;
}

export interface Scaled {
  scale: -1 | 0.5;
}

// Return-position unions of 32-bit integer literals become C-style
// enums, signed when any literal is negative.
export declare function level(): 0 | 1 | 2;
export declare function direction(): -1 | 0 | 1;

export interface Feedback {
  readonly value: -1 | 1 | null;
}

// Any other number literal widens to `number`, and boolean literals to
// `boolean`, since wasm-bindgen cannot mix them with other discriminants.
export declare function ratio(): 0.5 | 1;
export declare function count(): number | 0;
export declare function wide(): 0 | 4294967296;
export declare function size(): "auto" | 0;
export declare function repeat(): true | "x" | "y";
export declare function flag(): false | ArrayBuffer;
