#![cfg(target_arch = "wasm32")]

// A property that can be absent reads as `Option` even when its value is a
// `JsValue`: an absent field is `None`, and a written `null` is the
// `Nullable` under it.
use js_sys::Nullable;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::optional_reads::*;

#[wasm_bindgen_test]
fn absent_fields_read_as_none() {
    let settings = settings("empty");
    assert_eq!(settings.data(), None);
    assert_eq!(settings.detail(), None);
    assert_eq!(settings.cleared(), None);
    assert_eq!(settings.maybe(), None);
    assert_eq!(settings.value(), JsValue::from(1));
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
    assert_eq!(settings.data(), Some(JsValue::from(1)));
    assert_eq!(settings.detail(), Some(JsValue::from("detail")));
    assert_eq!(settings.cleared(), Some(Nullable::Value(JsValue::from(2))));
    assert_eq!(settings.maybe(), Some(JsValue::TRUE));
}

#[wasm_bindgen_test]
fn built_fields_read_back() {
    let built = Settings::builder(None, &JsValue::from(2))
        .data(&JsValue::from("data"))
        .build();
    assert_eq!(built.data(), Some(JsValue::from("data")));
    assert_eq!(built.detail(), None);
    assert_eq!(built.maybe(), None);
}

#[wasm_bindgen_test]
fn static_getters_read_as_option() {
    assert_eq!(Registry::missing(), None);
    assert_eq!(Registry::present(), Some(JsValue::from("present")));
    assert_eq!(Registry::cleared(), Some(Nullable::Null));
}
