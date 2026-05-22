//! @ts-gen --export tests/fixtures/module_exported.d.ts --export x:foo

// Same shape as `module_private.d.ts`, but the module is lifted to global
// scope via `--export x:foo`. The `mod foo` block disappears; `Bar`'s
// extern block keeps its `#[wasm_bindgen(module = "x:foo")]` attribute
// (the module association is carried per-decl), and `UsesBar` references
// the bare `Bar` ident.

declare module "x:foo" {
  export class Bar {
    constructor(name: string);
    name(): string;
  }
}

interface UsesBar {
  method(b: Bar): void;
}
