// Fixture: top-level return-position unions that would otherwise
// erase to `JsValue` get synthesised into `#[wasm_bindgen]` enums
// (dynamic unions) and used as the typed return / getter type.
//
// Synthesising cases (get a `<Anchor>Kind` enum):
//   * Mixed-kind unions with no LUB: `string | ArrayBuffer | ArrayBufferView`
//   * Property-position unions: each getter that returns one
//
// Non-synthesising cases (regular lowering):
//   * Single types — no union to synthesise from
//   * Literal-widened unions: `"a" | "b"` lowers to `string`
//   * Named-LUB unions: `TypeError | RangeError` lowers to `Error`
//   * Inner-erased generics: `Array<32 | "foo">`, `Promise<32 | "foo">`
//     stay as their inner-position lowering — the synthesised-enum
//     path doesn't reach inside generics because wasm-bindgen
//     requires `T: JsGeneric` for `Promise<T>` / `Map<K, V>` / etc.

interface Erased {
  /** Mixed primitives + objects — no LUB, becomes a dynamic union. */
  readonly content: string | ArrayBuffer | ArrayBufferView;

  /** Inner-erased generic — outer Array stays as `Vec<JsValue>`. */
  readonly tags: Array<32 | "foo">;
}

interface NoErasure {
  /** Literal widening — lowers to `string`, no enum synthesised. */
  readonly variant: "a" | "b";

  /** Plain type — no enum synthesised. */
  readonly name: string;
}

/** Async function with an inner-erased Promise — no enum synthesis. */
declare function fetchValue(): Promise<32 | "foo">;
