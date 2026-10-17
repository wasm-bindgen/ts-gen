//! @ts-gen --input tests/fixtures/ambient-module-merge/b.d.ts --export ovi --export loc

// As in TypeScript, an ambient module is one symbol whose declarations
// merge across files: `I`'s members, `k`'s overloads and `NS`'s members
// come from both this file and b.d.ts. A block with an export declaration
// has locals of its own: "loc"'s `N` here is exported as `M` only, and is
// not b.d.ts's `N`.

declare module "ovi" {
  export interface I {
    a: number;
  }
  export function k(x: string): void;
  export namespace NS {
    function h(x: string): void;
  }
}

declare module "loc" {
  namespace N {
    function f(): void;
  }
  export { N as M };
}
