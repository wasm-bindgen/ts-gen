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
declare module "ix" {
  import { T } from "ia";
  export interface U {
    ua: T;
    ma(): T;
  }
  export class K {
    ka: T;
  }
}
declare module "ib2" {
  import { T } from "ia";
  export namespace NS {
    function ga(t: T): void;
  }
}

interface Ev {
  e: number;
}
interface Win {
  a: Ev;
  l(): Ev;
}
declare namespace GN {
  function gl(e: Ev): void;
}
interface X {
  x(): void;
}
interface XCtor {
  new (e: Ev): X;
  readonly prototype: X;
  sl(e: Ev): void;
}
declare var X: XCtor;
