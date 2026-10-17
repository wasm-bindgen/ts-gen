//! @ts-gen --export pc --export ns --export plain --export pure --export own

// An export emits the declaration its local name binds, under its public
// name. A local declaration sharing a public name is not that export. A
// namespace's module takes its own name if it is exported under it, else
// the first public name, and each other public name aliases it; a local
// name the module does not export is not emitted. A function exported under
// its own name and another binds under both.

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
    interface T {}
  }
  function g(t: _N.T): void;
  export { _N as N1, _N as N2, g };
}

declare module "plain" {
  function make(): void;
  export { make, make as create };
}

declare module "pure" {
  namespace N {
    function f(): void;
  }
  export { N as M };
}

declare module "own" {
  function make(): void;
  namespace make {
    function helper(): void;
  }
  export { make, make as create };
}
