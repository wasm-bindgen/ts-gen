// Fixture: dictionary builder is suppressed when there are no optional
// fields to chain.
//
// When every property is required, the generated `FooBuilder` would only
// have `pub fn build(self) -> Foo` — pure dead weight. ts-gen detects
// this and emits only `new(reqs) -> Foo` with the construction inlined.

/** All fields required → no builder, only `new`. */
interface AllRequired {
  /** The thing's name. */
  name: string;
  /** The thing's count. */
  count: number;
}

/** Has at least one optional field → builder still emitted. */
interface HasOptional {
  /** Required. */
  name: string;
  /** Optional. */
  count?: number;
}

/** All optional → zero-arg `new()` and `builder()` with chainable setters. */
interface AllOptional {
  /** Optional. */
  name?: string;
  /** Optional. */
  count?: number;
}

/** Single required field, no optionals → no builder, just `new`. */
interface SingleRequired {
  /** The id. */
  id: string;
}
