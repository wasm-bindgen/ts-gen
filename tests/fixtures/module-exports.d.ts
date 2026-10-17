//! @ts-gen --export dep --export main

// A module's export map is built up front, so an import names an export
// rather than a local: `import { Foo }`, `D.Foo`, `Foo.Mode` and `extends Foo`
// all reach the class `_Foo`, and the namespace merged with it, that `dep`
// exports as `Foo`. The Rust reference is the declaration's own name, and the
// namespace binds by `Foo` in JS.

declare module "dep" {
  class _Foo {
    constructor();
    bar(): void;
  }
  namespace _Foo {
    type Mode = "a" | "b";
    function helper(): void;
  }
  export { _Foo as Foo };
  export type Alias = _Foo;
  export function makeFoo(): _Foo;
  export interface Holder {
    foo: _Foo;
    alias: Alias;
  }
  function hidden(): void;
}

declare module "main" {
  import { Foo, Alias, Holder } from "dep";
  import * as D from "dep";
  export function take(f: Foo, a: Alias, h: Holder, d: D.Foo, da: D.Alias, m: Foo.Mode): Foo;
  export class Sub extends Foo {}
}
