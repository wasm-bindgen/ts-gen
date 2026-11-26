//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::error_enum_node_mono::*;
use ts_gen_integration_tests::CaughtError;

#[wasm_bindgen_test]
fn iteration_does_not_ask_the_error_type_to_convert_back() {
    let kinds = Kinds::new().unwrap();
    let read: Vec<JsString> = (&kinds).into_iter().collect();
    assert_eq!(read, ["sub", "fault"]);
    let opened: Result<_, CaughtError> = kinds.iterator();
    assert!(opened.is_ok());
}
