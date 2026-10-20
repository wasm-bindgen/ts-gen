//! @ts-gen --lib-name dynamic-union-externals --export dynamic-union-externals

// Externals reachable only through a synthesised `<Name>Kind` enum arm
// still get their `use`: `Worker` from the default `web_sys` map, and the
// unresolved `Widget` as a `JsValue` alias.

export interface Foo {
  worker(): Worker | string;
  widget(): Widget | number;
}
