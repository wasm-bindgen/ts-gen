#![cfg(target_arch = "wasm32")]

//! Regression test for string enums used as members of a synthesised dynamic
//! union (fixture `tests/fixtures/string-enum-union.d.ts`).
//!
//! A string enum is a `Copy` value enum that doesn't implement `JsCast`, so it
//! can't be a `#[wasm_bindgen]` union-enum payload directly — the binding must
//! lower it to its `JsString` wrapper, which carries the same FFI value. The
//! snapshot test only compares text; this one compiles the generated bindings
//! against real wasm-bindgen. If `TintKind` / `FillKind` ever regress to a
//! `Variant(Color)` payload, this crate fails to build with `the trait bound
//! `Color: TryFromJsValue` is not satisfied`.
//!
//! The getters call into the (non-existent) `string-enum-union` JS module, so
//! we don't invoke them at runtime — we only construct the generated types,
//! which is enough to force the union enums through macro expansion.

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::string_enum_union::*;

#[wasm_bindgen_test]
fn string_enum_union_members_lower_to_jsstring() {
    // `Color | "transparent"` — the enum member lowered to `JsString`,
    // the string literal stayed a discriminant variant.
    let _tint = TintKind::JsString(JsString::from("red"));

    // `boolean | Color` — bool stayed `bool`, the enum member lowered to
    // `JsString`.
    let _fill_bool = FillKind::Bool(true);
    let _fill_str = FillKind::JsString(JsString::from("blue"));

    // The string enum itself is still generated and usable.
    let _color = Color::Red;
}
