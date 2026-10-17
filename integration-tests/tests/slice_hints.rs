#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::slice_hints::*;

#[wasm_bindgen_test]
fn view_aliases_wasm_memory_and_copy_detaches() {
    let mut buf = vec![1.0, 2.0];
    keep_view(&buf);
    buf[0] = 9.0;
    // The retained view reads through to the caller's buffer.
    assert_eq!(first(), 9.0);

    keep_copy(&buf);
    buf[0] = 5.0;
    // The retained copy is detached from it.
    assert_eq!(first(), 9.0);
}

#[wasm_bindgen_test]
fn unshared_view_copies_only_under_atomics() {
    let mut buf = vec![1, 2];
    keep_unshared_view(&buf);
    buf[0] = 9;
    // An atomics build's memory is shared, so the callee gets a copy there.
    let expected = if cfg!(target_feature = "atomics") {
        1.0
    } else {
        9.0
    };
    assert_eq!(first(), expected);

    keep_shared_view(&buf);
    buf[0] = 4;
    assert_eq!(first(), 4.0);
}

#[wasm_bindgen_test]
fn hints_select_the_wire_shape() {
    assert_eq!(entries(&[-1]), "Array(-1)");
    assert_eq!(digest(&[7, 8]), "Uint8Array(7,8)");
    assert_eq!(
        digest_with_uint8_array(&js_sys::Uint8Array::new_from_slice(&[3])),
        "Uint8Array(3)"
    );
    let target = Target::builder().buffer(&[0.5]).build();
    assert_eq!(describe_target(&target), "Float64Array(0.5)");
}
