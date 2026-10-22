//! @ts-gen --export x:uses --export x:foo

// `x:foo` is emitted via `--export x:foo`, in `pub mod x { pub mod foo }`
// with its `#[wasm_bindgen(module = "x:foo")]` attribute, and `UsesBar`,
// which imports `Bar`, references it relative to its own `pub mod` as
// `super::foo::Bar`.

declare module "x:foo" {
  export class Bar {
    constructor(name: string);
    name(): string;
  }
}

declare module "x:uses" {
  import { Bar } from "x:foo";
  export interface UsesBar {
    method(b: Bar): void;
  }
}
