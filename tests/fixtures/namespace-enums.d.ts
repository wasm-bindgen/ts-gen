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

  // A synthesised enum is emitted in the module of its first use; uses in
  // other modules name it by path. A set of literals is shared by every use
  // of the same values; any other union belongs to the member declaring it.
  // Names are `<Anchor>Kind`, numbered when taken.
  namespace Clock {
    interface ToStringOptions {
      // `OffsetKind`; `FromOptions.offset` is another set, `OffsetKind2`.
      offset?: "auto" | "never";
      // Emitted in `clock`; `Calendar.Options`, a sibling namespace, names
      // it by path.
      unit?: "day" | "week";
      // Also `FromOptions.display`, under this name.
      style?: "short" | "long";
      // Not a literal set: one enum per owner and member, so
      // `FromOptions.precision` is `PrecisionKind2`.
      precision?: number | "auto";
    }

    interface FromOptions {
      display?: "short" | "long";
      precision?: number | "auto";
      offset?: "use" | "ignore" | "prefer" | "reject";
      // Shared with `Clock.Zone.Options`.
      overflow?: "constrain" | "reject";
    }

    namespace Zone {
      interface Options {
        overflow?: "constrain" | "reject";
        // Only used here: emitted in `clock::zone`.
        mode?: "fast" | "slow";
        // The same union as `precision`, but its own enum.
        limit?: number | "auto";
      }
    }
  }

  namespace Calendar {
    interface Options {
      unit?: "day" | "week";
    }

    // One set in two orders, whose values give the same variant name: the
    // enum numbers them in its first use's order, which every setter,
    // builder and literal factory names them by.
    interface Separator {
      sep?: "foo-bar" | "foo_bar";
    }

    interface ReversedSeparator {
      sep: "foo_bar" | "foo-bar";
    }
  }
}
