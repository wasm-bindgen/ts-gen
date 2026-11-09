//! @ts-gen --strict-nullable

#![cfg(target_arch = "wasm32")]

// Under `--strict-nullable` the IR keeps `T | null | undefined` as `T??`:
// a write sends each state, and a read tells a received `null` from a
// received `undefined`.
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::node_nullable_strict::*;

#[wasm_bindgen_test]
fn optional_nullable_argument_sends_each_state() {
    assert_eq!(classify(), "undefined");
    assert_eq!(classify_with_null(), "null");
    assert_eq!(classify_with_value("v"), "value:v");
}

#[wasm_bindgen_test]
fn optional_nullable_return_tells_null_from_undefined() {
    assert!(matches!(lookup("known"), Some(Nullable::Value(v)) if v == "value"));
    assert!(matches!(lookup("other"), Some(Nullable::Null)));
    assert!(lookup("empty").is_none());
}

#[wasm_bindgen_test]
async fn awaited_value_tells_null_from_undefined() {
    assert!(matches!(
        lookup_async("other").await.unwrap(),
        Some(Nullable::Null)
    ));
    assert!(lookup_async("empty").await.unwrap().is_none());
}
