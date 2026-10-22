//! @ts-gen --export x:uses --external x:foo=::other::foo

// `x:foo` is redirected to a separately-generated crate via
// `--external x:foo=::other::foo`. The `mod foo` block is suppressed
// entirely (Rule 3), and `UsesBar`'s imported `Bar` resolves through the
// external map to `::other::foo::Bar`.

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
