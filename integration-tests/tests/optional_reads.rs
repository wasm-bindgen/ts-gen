#![cfg(target_arch = "wasm32")]

// A property that can be absent reads as `Option` when its type erases to
// `JsValue`: an absent field is `None`, and a written `null` is the
// `Nullable` under it. `any` and `unknown` hold both empties in-band and
// stay a bare `JsValue`, so a stored `null` is not lost.
use js_sys::Nullable;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::optional_reads::*;

#[wasm_bindgen_test]
fn absent_fields_read_as_none() {
    let settings = settings("empty");
    assert_eq!(settings.stamp(), None);
    assert_eq!(settings.cleared(), None);
    assert_eq!(settings.maybe(), None);
    assert_eq!(settings.value(), JsValue::from(1));
}

#[wasm_bindgen_test]
fn top_type_keeps_both_empties_in_band() {
    let empty = settings("empty");
    assert!(empty.data().is_undefined());
    assert!(empty.detail().is_undefined());
    let null = settings("null");
    assert_eq!(null.data(), JsValue::NULL);
    assert_eq!(null.detail(), JsValue::NULL);
}

#[wasm_bindgen_test]
fn written_null_reads_as_nullable() {
    let settings = settings("null");
    assert_eq!(settings.cleared(), Some(Nullable::Null));
    assert_eq!(settings.value(), JsValue::NULL);
}

#[wasm_bindgen_test]
fn set_fields_read_as_some() {
    let settings = settings("full");
    assert_eq!(settings.data(), JsValue::from(1));
    assert_eq!(settings.detail(), JsValue::from("detail"));
    assert_eq!(settings.stamp(), Some(JsValue::from(2)));
    assert_eq!(settings.cleared(), Some(Nullable::Value(JsValue::from(3))));
    assert_eq!(settings.maybe(), Some(JsValue::TRUE));
}

#[wasm_bindgen_test]
fn built_fields_read_back() {
    let built = Settings::builder(None, &JsValue::from(2))
        .stamp(&JsValue::from("stamp"))
        .build();
    assert_eq!(built.stamp(), Some(JsValue::from("stamp")));
    assert_eq!(built.cleared(), None);
    assert_eq!(built.maybe(), None);
}

#[wasm_bindgen_test]
fn static_getters_read_as_option() {
    assert_eq!(Registry::missing(), None);
    assert_eq!(Registry::cleared(), Some(Nullable::Null));
    assert_eq!(Registry::anything(), JsValue::NULL);
}
