//! @ts-gen --lib-name display-to-string --export display-to-string --export tests/fixtures/display-to-string.d.ts

// A parameterless `toString(): string` is the type's `Display`, so
// `.to_string()` comes from `ToString` instead of an inherent method that
// shadows it. The catching `try_to_string`, the `_js_string` variant and
// the overloads with parameters keep their bindings.

declare global {
  interface StampOptions {
    upper?: boolean;
  }

  // An overload with a parameter beside the bare one.
  class Stamp {
    constructor(label: string);
    toString(): string;
    toString(options: StampOptions): string;
  }

  // Inherits `toString` without a `Display` of its own.
  class SubStamp extends Stamp {}

  // An optional parameter, as Temporal declares it.
  class Moment {
    constructor(label: string);
    toString(options?: StampOptions): string;
  }

  // No catching form, so `Display` calls a private extern.
  class Pure {
    constructor(label: string);
    /** @throws {never} */
    toString(): string;
  }

  // A typed error on the catching form.
  class Strict {
    constructor(label: string);
    /** @throws {RangeError} when the label is empty */
    toString(): string;
  }

  class Boxed<T> {
    constructor(value: T);
    toString(): string;
  }

  interface Labelled {
    toString(): string;
  }

  // Not a `Display`: a required parameter, or a return other than `string`.
  class Radix {
    constructor();
    toString(radix: number): string;
  }
  class Opaque {
    constructor();
    toString(): number;
  }

  function labelled(label: string): Labelled;
}
