//! @ts-gen --export tests/fixtures/module_exported.d.ts --export x:foo

// Same shape as `module_private.d.ts`, but the module is emitted via
// `--export x:foo`, in `pub mod x { pub mod foo }` with its
// `#[wasm_bindgen(module = "x:foo")]` attribute, and `UsesBar` references
// it as `x::foo::Bar`.

declare module "x:foo" {
  export class Bar {
    constructor(name: string);
    name(): string;
  }
}

interface UsesBar {
  method(b: Bar): void;
}
