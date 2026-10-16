#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::slice_args_mono::*;

#[wasm_bindgen_test]
fn generic_slices_arrive_as_arrays() {
    assert_eq!(generic::<f64>(&[1.0, 2.5]), "Array(1,2.5)");
    assert_eq!(generic::<u8>(&[3]), "Array(3)");
    assert_eq!(generic::<String>(&["a".to_string()]), "Array(a)");
    assert_eq!(generic::<JsString>(&[JsString::from("j")]), "Array(j)");
}

#[wasm_bindgen_test]
fn class_generic_slices_arrive_as_arrays() {
    let bag = Bag::<f64>::new().unwrap();
    assert_eq!(bag.take(&[1.5]), "Array(1.5)");
}
