//! @ts-gen --lib-name namespace-enums --export namespace-enums --export tests/fixtures/namespace-enums.d.ts

// An enum inside a namespace carries its `js_namespace`, so it does not
// clash with a same-named enum at the top level, nor with a top-level
// `TimingDurationUnit` that spells out the namespace.

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
