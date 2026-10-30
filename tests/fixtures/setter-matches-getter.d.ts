//! @ts-gen --lib-name setter-matches-getter --export tests/fixtures/setter-matches-getter.d.ts

// A setter, builder method and literal factory take the enum the getter
// reads, so the getter's value passes straight back.

declare global {
  // Ten number literals widen into one `F64(f64)` payload, and share one
  // `_with_f64` overload rather than overflowing the fan-out to `&JsValue`.
  interface DigitsOptions {
    fractionalSecondDigits?: "auto" | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | undefined;
    // Literals alone: a plain value the getter's read compares and prints.
    roundingMode?: "ceil" | "floor";
  }

  // A required field: each literal is a factory.
  interface RequiredDigits {
    digits: "auto" | 0 | 1 | 2;
  }

  // The enum is the unsuffixed setter even when a widened literal comes
  // first.
  interface Toggle {
    mode: true | "x" | "y";
  }
}

export {};
