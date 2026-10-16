#![cfg(target_arch = "wasm32")]

use js_sys::{BigInt, Boolean, JsString, Uint8Array};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::slice_args::*;

#[wasm_bindgen_test]
fn slice_arguments_arrive_as_arrays() {
    assert_eq!(numbers(&[1.0, 2.5]), "Array(1,2.5)");
    assert_eq!(counts(&[3.0]), "Array(3)");
    assert_eq!(literals(&[1.0, 2.0]), "Array(1,2)");
    assert_eq!(levels(&[2.0]), "Array(2)");
    assert_eq!(strings(&["a".to_string(), "b".to_string()]), "Array(a,b)");
    assert_eq!(
        flags(&[Boolean::from(true), Boolean::from(false)]),
        "Array(true,false)"
    );
    assert_eq!(bigints(&[BigInt::from(7)]), "Array(7)");
    assert_eq!(
        items(&[Item::new("x").unwrap(), Item::new("y").unwrap()]),
        "Array(x,y)"
    );
    assert_eq!(generic(&[JsString::from("g")]), "Array(g)");
}

#[wasm_bindgen_test]
fn typed_array_unions_pass_a_typed_view() {
    assert_eq!(bytes(&[1, 2]), "Uint8Array(1,2)");
    assert_eq!(
        bytes_with_uint8_array(&Uint8Array::new_from_slice(&[3])),
        "Uint8Array(3)"
    );
    assert_eq!(words(&[-1]), "BigInt64Array(-1)");
    assert_eq!(
        unsigned(&[u64::MAX]),
        "BigUint64Array(18446744073709551615)"
    );
    assert_eq!(buffered(&[4]), "Uint8Array(4)");
    assert_eq!(floats(), "none");
    assert_eq!(floats_with_slice(&[0.5]), "Float64Array(0.5)");
}
