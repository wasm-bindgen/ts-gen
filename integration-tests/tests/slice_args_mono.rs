//! @ts-gen --experimental-generic-mono

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
    let bag = Bag::<f64>::new_typed().unwrap();
    assert_eq!(bag.take(&[1.5]), "Array(1.5)");
}

#[wasm_bindgen_test]
fn mixed_slice_modes_share_a_binding() {
    assert_eq!(
        mixed_generic::<String>(&["a".to_string()], &[1.0], &[2.0]),
        "Array(a) Array(1) Float64Array(2)"
    );
}

#[wasm_bindgen_test]
fn a_buffer_takes_bytes_or_either_class() {
    use js_sys::{Float32Array, Uint8Array};
    let bytes = Uint8Array::new_from_slice(&[1, 2]);
    assert_eq!(hash(&[1u8, 2][..]), "Uint8Array(1,2)");
    assert_eq!(hash(vec![3u8]), "Uint8Array(3)");
    assert_eq!(hash(&bytes), "Uint8Array(1,2)");
    assert_eq!(hash(bytes.buffer()), "ArrayBuffer(1,2)");
    assert_eq!(
        hash(Float32Array::new_with_length(1)),
        "Float32Array(0,0,0,0)"
    );
    assert_eq!(store("k", "v"), "k=v");
    assert_eq!(store_with_buffer("k", &[4u8][..]), "k=Uint8Array(4)");
    assert_eq!(
        store_with_buffer("k", &bytes.buffer()),
        "k=ArrayBuffer(1,2)"
    );
    assert_eq!(
        concat(&[&[7u8][..], &[8u8, 9][..]]),
        "Uint8Array(7)+Uint8Array(8,9)"
    );
    assert_eq!(concat(&[bytes.buffer()]), "ArrayBuffer(1,2)");
    assert!(matches!(
        echo(&bytes.buffer()),
        EchoReturnKind::ArrayBuffer(b) if b.byte_length() == 2
    ));
}

#[wasm_bindgen_test]
fn buffer_bytes_are_a_copy_unless_viewed() {
    assert!(owned(&[1u8, 2][..]));
    assert!(!owned_view(&[1u8, 2][..]));
    assert_eq!(peek(&[5u8, 6][..]), "Uint8Array(5,6)");
}
