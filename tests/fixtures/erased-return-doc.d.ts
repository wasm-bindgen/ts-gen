// Fixture: when a return-position type contains a union that erases to
// `JsValue`, codegen appends a `Returns: <ts-shape>` doc line so the
// caller can still see the original TypeScript shape.
//
// Erasing cases (get the doc):
//   * Mixed-kind unions with no LUB: `string | ArrayBuffer | ArrayBufferView`
//   * Inner-erased generics: `Array<32 | "foo">`, `Promise<32 | "foo">`
//
// Non-erasing cases (no doc):
//   * Single types
//   * Literal-widened unions: `"a" | "b"` lowers to `string`
//   * Named-LUB unions: `TypeError | RangeError` lowers to `Error`

interface Erased {
  /** Mixed primitives + objects — no LUB, erases to JsValue. */
  readonly content: string | ArrayBuffer | ArrayBufferView;

  /** Inner-erased generic. */
  readonly tags: Array<32 | "foo">;
}

interface NoErasure {
  /** Literal widening — lowers to `string`, no Returns: line. */
  readonly variant: "a" | "b";

  /** Plain type — no Returns: line. */
  readonly name: string;
}

/** Async function returning erased inner union. */
declare function fetchValue(): Promise<32 | "foo">;
