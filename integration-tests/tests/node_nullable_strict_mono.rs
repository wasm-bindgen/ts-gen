//! @ts-gen --strict-nullable --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

// Under `--strict-nullable` the IR keeps `T | null | undefined` as `T??`:
// a write sends each state, and a read tells a received `null` from a
// received `undefined`. The pinned wasm-bindgen crosses only
// per-monomorphization's `Nullable<Option<T>>` for it, and the released one
// only the default's `Option<Nullable<T>>`, so this runs under
// per-monomorphization until the nesting is released.
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::node_nullable_strict_mono::*;

#[wasm_bindgen_test]
fn optional_nullable_argument_sends_each_state() {
    assert_eq!(classify(), "undefined");
    assert_eq!(classify_with_null(), "null");
    assert_eq!(classify_with_value("v"), "value:v");
}

#[wasm_bindgen_test]
fn optional_nullable_return_tells_null_from_undefined() {
    assert!(matches!(lookup("known"), Nullable::Value(Some(v)) if v == "value"));
    assert!(matches!(lookup("other"), Nullable::Null));
    assert!(matches!(lookup("empty"), Nullable::Value(None)));
}

#[wasm_bindgen_test]
async fn awaited_value_tells_null_from_undefined() {
    assert!(matches!(
        lookup_async("other").await.unwrap(),
        Nullable::Null
    ));
    assert!(matches!(
        lookup_async("empty").await.unwrap(),
        Nullable::Value(None)
    ));
}
