#![cfg(target_arch = "wasm32")]

use js_sys::{BigInt, Boolean, JsNullable, JsString, Uint8Array, Uint8ClampedArray};
use wasm_bindgen::Clamped;
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
fn typed_array_unions_pass_a_typed_copy() {
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
    assert_eq!(clamped(Clamped(&[5, 255])), "Uint8ClampedArray(5,255)");
    assert_eq!(
        clamped_with_uint8_clamped_array(&Uint8ClampedArray::new_from_slice(&[6])),
        "Uint8ClampedArray(6)"
    );
    assert_eq!(pixels(&[7]), "Uint8Array(7)");
    assert_eq!(floats(), "none");
    assert_eq!(floats_with_slice(&[0.5]), "Float64Array(0.5)");
    assert_eq!(floats_with_float64_array(&JsNullable::new()), "null");
    assert_eq!(maybe_bytes(Some(&[5])), "Uint8Array(5)");
    assert_eq!(maybe_bytes(None), "none");
    assert_eq!(maybe_clamped(Clamped(&[6])), "Uint8ClampedArray(6)");
    assert_eq!(maybe_clamped_with_uint8_clamped_array(None), "none");
}

#[wasm_bindgen_test]
fn slice_modes_are_per_argument() {
    assert_eq!(mixed(&[1.0], &[2]), "Array(1)|Uint8Array(2)");
}

#[wasm_bindgen_test]
fn generic_array_spellings_lower_like_arrays() {
    assert_eq!(spelled(&["a".to_string()]), "Array(a)");
    assert_eq!(spelled_readonly(&[1.0]), "Array(1)");
    let handle: js_sys::Array<JsString> =
        wasm_bindgen::JsCast::unchecked_into(js_sys::Array::from_iter([JsString::from("h")]));
    assert_eq!(array_like(&handle), "Array(h)");
}
