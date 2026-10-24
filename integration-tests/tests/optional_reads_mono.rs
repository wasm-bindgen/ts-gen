//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

// `optional_reads.rs` against the per-monomorphization bindings.
use js_sys::Nullable;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::optional_reads_mono::*;

#[wasm_bindgen_test]
fn absent_fields_read_as_none() {
    let settings = settings("empty");
    assert_eq!(settings.data(), None);
    assert_eq!(settings.cleared(), None);
    assert_eq!(settings.maybe(), None);
    assert_eq!(settings.value(), JsValue::from(1));
}

#[wasm_bindgen_test]
fn written_null_reads_as_nullable() {
    assert_eq!(settings("null").cleared(), Some(Nullable::Null));
    assert_eq!(Registry::missing(), None);
    assert_eq!(Registry::cleared(), Some(Nullable::Null));
}
