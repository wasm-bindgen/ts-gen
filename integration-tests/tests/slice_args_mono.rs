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
    // Where `undefined` can't be omitted the argument still takes a class.
    assert_eq!(
        decode_with_input_and_label(Some(&[1u8][..]), "x"),
        "x:Uint8Array(1)"
    );
    assert_eq!(
        decode_with_input_and_label(Some(bytes.buffer()), "x"),
        "x:ArrayBuffer(1,2)"
    );
    assert_eq!(decode_with_input_and_label(None::<&[u8]>, "x"), "x:none");
}

#[wasm_bindgen_test]
async fn a_buffer_reads_as_its_bytes_or_its_handle() {
    use js_sys::Uint8Array;
    let bytes = Uint8Array::new_from_slice(&[1, 2]);
    assert_eq!(echo(&bytes.buffer()), [1, 2]);
    assert_eq!(echo(&[5u8, 6][..]), [5, 6]);
    assert!(matches!(
        echo_js_buffer(&bytes.buffer()),
        EchoReturnKind::ArrayBuffer(b) if b.byte_length() == 2
    ));
    // A view is its own window of its buffer, whatever its element.
    assert_eq!(slab("buffer"), [1, 2, 3]);
    assert_eq!(slab("bytes"), [2, 3]);
    assert_eq!(slab("floats"), [0, 0, 128, 63, 0, 0, 0, 64]);
    assert_eq!(slab("view"), [8, 7]);
    assert_eq!(maybe_slab(), None);
    assert_eq!(maybe_slab_with_kind("view"), Some(vec![8, 7]));
    assert_eq!(slab_later("floats").await.unwrap().len(), 8);
    let slab = Slab::new("bytes").unwrap();
    assert_eq!(slab.data(), [2, 3]);
    assert_eq!(slab.bytes(), [2, 3]);
    assert_eq!(Slab::of("view"), [8, 7]);
}

#[wasm_bindgen_test]
fn buffer_bytes_are_a_copy_unless_viewed() {
    assert!(owned(&[1u8, 2][..]));
    assert!(!owned_view(&[1u8, 2][..]));
    assert_eq!(peek(&[5u8, 6][..]), "Uint8Array(5,6)");
}
