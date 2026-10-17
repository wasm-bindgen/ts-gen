//! @ts-gen --export pc --export ns

// An export emits the declaration its local name binds, under its public
// name. A local declaration sharing a public name is not that export, and a
// namespace exported under other names binds by the first in JS.

declare module "pc" {
  interface Foo {
    internal: number;
  }
  interface _Bar {
    b: number;
  }
  export { _Bar as Foo };
}

declare module "ns" {
  namespace _N {
    function f(): void;
  }
  export { _N as N1, _N as N2 };
}
