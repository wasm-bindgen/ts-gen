//! @ts-gen --export pc --export ns --export plain

// An export emits the declaration its local name binds, under its public
// name. A local declaration sharing a public name is not that export, and a
// namespace exported under other names binds by the first in JS, with a
// module aliasing it for each. A function exported under its own name and
// another binds under both.

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

declare module "plain" {
  function make(): void;
  export { make, make as create };
}
