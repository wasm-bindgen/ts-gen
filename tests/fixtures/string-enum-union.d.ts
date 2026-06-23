//! @ts-gen --lib-name string-enum-union --export string-enum-union

// Regression fixture: a string enum used as a member of a synthesised
// dynamic union must lower to its `JsString` wrapper, not be emitted as a
// `Variant(TheEnum)` payload. String enums are `Copy` value enums that don't
// implement `JsCast`, so a `#[wasm_bindgen]` union enum carrying one directly
// fails to compile (`the trait bound `Color: TryFromJsValue` is not
// satisfied`). The wrapper carries the same FFI value (a JS string).
//
// These declarations are `export`ed, so they land in module scope. The first
// pass at this fix only resolved union members in `root_scope`, which can't
// see module-scoped enums, so the lowering silently didn't fire here and the
// broken `Variant(TheEnum)` form was emitted. The matching
// `integration-tests/tests/string_enum_union.rs` compiles the output against
// real wasm-bindgen so this can't regress to text-only coverage again.
//
// Mirrors the two shapes that surfaced in the Cloudflare Workers types:
//   * `Color | "transparent"`  ~ `CountryKind`        (enum + string literal)
//   * `boolean | Color`        ~ `ReturnMetadataKind` (boolean + enum)

export declare type Color = "red" | "green" | "blue";

export interface Style {
  readonly tint: Color | "transparent";
  readonly fill: boolean | Color;
}
