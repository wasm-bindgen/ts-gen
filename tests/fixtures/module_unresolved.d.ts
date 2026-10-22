// A module's exports are reachable only through an import: the global
// `UsesBar` doesn't import `Bar`, so as in TypeScript ("Cannot find name")
// it doesn't see `x:foo`'s `Bar`. The reference is a codegen error and
// falls back to a `use JsValue as Bar;` alias so the output still
// compiles. No `mod foo` block is ever generated.

declare module "x:foo" {
  export class Bar {
    constructor(name: string);
    name(): string;
  }
}

interface UsesBar {
  method(b: Bar): void;
}
