// Names that sanitise (`0` → `_0`, `$public` → `public`) must still address
// the original JS name from every binding wasm-bindgen infers a name for.
declare class Pair {
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
declare class Socket {
  accept(): void;
}
declare class $Store {
  constructor();
}
declare function $(selector: string): Pair;
declare const $version: string;
