//! @ts-gen --lib-name sanitised-keys --export sanitised-keys

// Names that sanitise (`0` → `_0`, `$public` → `public`) must still address
// the original JS name from every binding wasm-bindgen infers a name for.
export declare class Pair {
  constructor();
  0: Socket;
  1: Socket;
  static 2: Socket;
  $public: string;
  static "@cf/model": string;
  $emit(event: string): void;
  "@cf/run"(): void;
  static $create(): Pair;
}
export declare class Socket {
  accept(): void;
}
// Constructors and statics resolve their JS class from the Rust ident, so
// they need `js_class` too.
export declare class $Store {
  constructor();
  static create(): $Store;
  static count: number;
}
export declare function $(selector: string): Pair;
export declare const $version: string;
// Names with nothing left after sanitising spell out their symbols.
export declare function $$(selector: string): Pair[];
export declare const __: number;
export declare function each(_: number, $: string): void;
export type Op = "+" | "-" | "" | "<=";
export declare function compare(op: Op): void;
export declare class $_ {
  constructor();
  _: string;
}
export declare enum Sigil {
  "$" = 1,
  "_" = 2,
}

// Names that sanitise to the same ident are numbered in declaration order.
export declare class Collide {
  constructor();
  data: string;
  $data: string;
}
export interface CollideInit {
  fooBar: number;
  foo_bar: number;
  data?: string;
  $data?: string;
}
export interface MethodCollide {
  data(): void;
  $data: string;
  setFoo(): void;
  foo: string;
  call(): void;
  call(a: string): void;
  call($a: number): void;
}
export interface SetterCollide {
  setFoo: string;
  foo: string;
}
export declare function ajax(url: string): void;
export declare function $ajax(url: string): void;
export declare function dollar(): void;
export declare function collideArgs(a: string, $a: number, fooBar: string, foo_bar: number): void;
