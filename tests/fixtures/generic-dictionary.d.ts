// Generic dictionary interface — exercises type-parameter propagation
// through every binding site (extern getters/setters, factory `impl`,
// builder struct + impl) on a property-only interface that classifies
// as `Dictionary`.
//
// Mirrors the shape that motivated this support:
// `FlagshipEvaluationDetails<T>` from `@cloudflare/workers-types`.
//
// Covers:
//   * Required + optional fields mixed (builder is emitted).
//   * A required field typed as the type parameter (`value: T`) —
//     forces the setter and the builder's `new`/`builder` factory
//     to thread `<T: JsGeneric>` through their signatures.
//   * Optional `string | undefined` fields — fluent builder setters
//     don't reference `T`, so they live inside `impl<T> Builder<T>`
//     without redeclaring it.

interface EvaluationDetails<T> {
  flagKey: string;
  value: T;
  variant?: string | undefined;
  reason?: string | undefined;
}
