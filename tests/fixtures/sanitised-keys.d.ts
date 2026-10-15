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
