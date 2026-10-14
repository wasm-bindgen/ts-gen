//! @ts-gen --lib-name enum-args --export enum-args

// String enums lower to `Copy` Rust enums, which wasm-bindgen only
// converts by value: argument positions take `Kind`, not `&Kind`.

export type Kind = "text" | "bytes";

export interface SendOptions {
  kind?: Kind;
}

export declare class Sender {
  send(kind: Kind, options?: SendOptions): void;
  readonly kind: Kind;
}
