//! @ts-gen --lib-name namespace-enums --export namespace-enums --export tests/fixtures/namespace-enums.d.ts

// wasm-bindgen exports enums by name across the whole crate, so an enum
// inside a namespace exports under a namespace-prefixed name, sparing it
// a clash with a same-named enum elsewhere.

export type DurationUnit = "second" | "minute";
export declare enum Precision {
  Low = 1,
  High = 2,
}

declare global {
  namespace Timing {
    type DurationUnit = "second" | "minute" | "hour";
    enum Precision {
      Coarse = 1,
      Fine = 2,
    }
  }
}
