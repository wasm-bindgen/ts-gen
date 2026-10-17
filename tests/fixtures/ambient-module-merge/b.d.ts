declare module "ovi" {
  export interface I {
    b: number;
  }
  export function k(x: number): void;
  export namespace NS {
    function h(x: number): void;
  }
}

declare module "loc" {
  namespace N {
    function g(): void;
  }
  interface E {
    b: number;
  }
}

declare module "ib" {
  import { T } from "ic";
  export function fb(t: T): void;
}

declare module "dflt" {
  namespace N {
    function g(): void;
  }
}

declare module "mm" {
  interface W {
    w(): void;
  }
}
