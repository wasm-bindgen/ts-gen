// Resolution-only module reference: `x:foo` is in the type universe so
// `Bar` resolves at parse time, but it's neither `--export`ed nor
// `--external`ised. The reference from the global `UsesBar` is a
// codegen error — ts-gen emits the diagnostic at the next CLI run and
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
