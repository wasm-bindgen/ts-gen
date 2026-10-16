//! @ts-gen --export dep --export main

// A module's export map is built up front, so an import names an export
// rather than a local: `import { Foo }` and `D.Foo` both reach the class
// `_Foo` that `dep` exports as `Foo`, through every indirection, and the
// Rust reference is the declaration's own name.

declare module "dep" {
  interface _Foo {
    bar(): void;
  }
  var _Foo: { new (): _Foo };
  export { _Foo as Foo };
  export type Alias = Foo;
  export function makeFoo(): Foo;
  export interface Holder {
    foo: Foo;
    alias: Alias;
  }
  export namespace Foo {
    type Mode = "a" | "b";
    function helper(): void;
  }
  function hidden(): void;
}

declare module "main" {
  import { Foo, Alias, Holder } from "dep";
  import * as D from "dep";
  export function take(f: Foo, a: Alias, h: Holder, d: D.Foo, da: D.Alias, m: Foo.Mode): Foo;
  export class Sub extends Foo {}
}
