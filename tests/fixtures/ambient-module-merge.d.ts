//! @ts-gen --input tests/fixtures/ambient-module-merge/b.d.ts --input tests/fixtures/ambient-module-merge/aug.d.ts --lib-name ambient-module-merge-aug --export ovi --export loc --export dflt --export ia --export ib --export ic --export mm --export mm2

// As in TypeScript, an ambient module is one symbol whose exports merge
// across files and module augmentations: `I`'s members, `k`'s overloads
// and `NS`'s members come from this file, b.d.ts and aug.d.ts. Each block
// keeps its own locals: its imports ("ib"'s `T` is "ia"'s here and "ic"'s
// in b.d.ts), and in a block with an export declaration its unexported
// declarations ("loc"'s `N` here is exported as `M` only, and "dflt"'s as
// `default` only, and neither is b.d.ts's `N`). Such a block's
// `export`-modified declarations are still the module's ("loc"'s `E`).
// A constructor var and the module's interface of its name in another
// file are one class in either file order ("mm"'s and "mm2"'s `W`).

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
  export interface E {
    a: number;
  }
  export { N as M };
}

declare module "ia" {
  export interface T {
    a: number;
  }
}

declare module "ic" {
  export interface T {
    c: number;
  }
}

declare module "ib" {
  import { T } from "ia";
  export function fa(t: T): void;
}

declare module "dflt" {
  namespace N {
    function f(): void;
  }
  export default N;
}

interface WCtor {
  new (): W;
  readonly prototype: W;
  make(): W;
}
interface W {
  glob(): void;
}

declare module "mm" {
  const W: WCtor;
}

declare module "mm2" {
  interface W {
    w(): void;
  }
}
