//! @ts-gen --lib-name enum-args --export enum-args

// String enums lower to `Copy` Rust enums, which wasm-bindgen only
// converts by value: argument positions take `Kind`, not `&Kind`.
// Optional `ArrayBufferView` dictionary properties must expose the same
// `<T: TypedArray>` generic on the fluent builder method as on the setter.

export type Kind = "text" | "bytes";

export interface SendOptions {
  kind?: Kind;
  payload?: ArrayBufferView;
}

export declare class Sender {
  send(kind: Kind, options?: SendOptions): void;
  readonly kind: Kind;
}
