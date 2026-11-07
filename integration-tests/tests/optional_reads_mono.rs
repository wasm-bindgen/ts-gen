//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

// `optional_reads.rs` against the per-monomorphization bindings.
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::optional_reads_mono::*;

#[wasm_bindgen_test]
fn absent_fields_read_as_none() {
    let settings = settings("empty");
    assert_eq!(settings.stamp(), None);
    assert_eq!(settings.cleared(), None);
    assert_eq!(settings.maybe(), None);
    assert!(settings.data().is_undefined());
}

#[wasm_bindgen_test]
fn null_reads_as_none_off_an_erased_field() {
    let settings = settings("null");
    assert_eq!(settings.data(), JsValue::NULL);
    assert_eq!(settings.cleared(), None);
    assert_eq!(Registry::missing(), None);
    assert_eq!(Registry::cleared(), None);
    assert_eq!(Registry::anything(), JsValue::NULL);
}
