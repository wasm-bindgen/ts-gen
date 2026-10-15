//! @ts-gen --lib-name namespace-enums --export namespace-enums --export tests/fixtures/namespace-enums.d.ts

// wasm-bindgen exports enums by name across the whole crate, so an enum
// inside a namespace exports under a namespace-prefixed name, sparing it
// a clash with a same-named enum elsewhere. The prefix is separated, so it
// does not collide with `TimingDurationUnit` either.

export type DurationUnit = "second" | "minute";
export type TimingDurationUnit = "day" | "week";
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
