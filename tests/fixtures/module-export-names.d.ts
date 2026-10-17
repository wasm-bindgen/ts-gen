//! @ts-gen --export pc --export ns --export plain --export pure --export own --export col --export user

// An export emits the declaration its local name binds, under its public
// name. A local declaration sharing a public name is not that export. A
// namespace's module takes its own name if it is exported under it, else
// the first public name, and each other public name aliases it; a local
// name the module does not export is not emitted, whether or not the module
// is exported ("user" reaches unexported "reached"). A function exported
// under its own name and another binds under both. A namespace the module
// doesn't export takes a suffix where an exported one has its name, as
// TypeScript keeps a module's exports and locals apart ("col").

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

declare module "col" {
  namespace N {
    interface T {
      n: number;
    }
    function f(t: T): void;
  }
  namespace M {
    interface T {
      m: string;
    }
  }
  function g(t: M.T): void;
  export { N as M, g };
}

declare module "reached" {
  namespace N {
    interface T {
      t: number;
    }
  }
  export { N as M };
}

declare module "user" {
  import { M } from "reached";
  export function use(x: M.T): void;
}
