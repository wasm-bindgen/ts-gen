//! @ts-gen --lib-name prelude-collision --export prelude-collision

// Types named like the Rust prelude are renamed with a trailing `_` like
// `js_sys` collisions, but not re-exported under the original name: a
// `pub use Result_ as Result;` would shadow the prelude for the module.
// `js_name` keeps the JS binding; references use the renamed ident.

export declare class Result {
  constructor(ok: boolean);
  readonly ok: boolean;
}
export interface Option {
  value: string;
}
export interface Default {
  limit?: number;
}
export declare class Vec {
  length: number;
}
export declare function take(r: Result, o: Option, d?: Default): Vec;
export declare function label(s: string): string;
