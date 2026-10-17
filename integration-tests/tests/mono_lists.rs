#![cfg(target_arch = "wasm32")]

use js_sys::{Array, JsString};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::mono_lists::*;

#[wasm_bindgen_test]
fn string_lists_accept_any_string_element() {
    assert_eq!(tags(&["a".to_string()]), "Array(string:a)");
    assert_eq!(tags(&["a", "b"]), "Array(string:a,string:b)");
    assert_eq!(tags(&[JsString::from("j")]), "Array(string:j)");
    assert_eq!(tagged(&["t"]), "Array(string:t)");
    let handle: Array<JsString> = Array::new_typed();
    handle.push(&JsString::from("h"));
    assert_eq!(tags_like(&handle), "Array(string:h)");
}

#[cfg(js_sys_unstable_apis)]
#[wasm_bindgen_test]
fn string_list_handles_hold_rust_strings() {
    let handle: Array<String> = ["h", "i"].into_iter().map(String::from).collect();
    assert_eq!(tags_like(&handle), "Array(string:h,string:i)");
    let empty: &[&str] = &[];
    assert_eq!(tags(empty), "Array()");
}

#[wasm_bindgen_test]
fn string_list_returns_have_a_handle_variant() {
    assert_eq!(get_tags(), ["a", "b"]);
    let handle: Array<JsString> = get_tags_js_string();
    assert_eq!(handle.length(), 2);
    assert_eq!(get_maybe_tags(), None);
    assert!(get_maybe_tags_js_string().is_none());
}

#[wasm_bindgen_test]
async fn awaited_string_lists_are_handles() {
    let handle: Array<JsString> = fetch_tags().await.unwrap();
    assert_eq!(handle.length(), 1);
}

#[wasm_bindgen_test]
fn boolean_lists_cross_as_bool() {
    assert_eq!(flags(&[true, false]), "Array(boolean:true,boolean:false)");
    assert_eq!(get_flags(), [true, false]);
    assert!(matches!(get_value(), GetValueReturnKind::VecOfBool(v) if v == [true, false]));
}

#[wasm_bindgen_test]
fn properties_and_builders_take_string_lists() {
    let labels = Labels::new().unwrap();
    labels.set_values(&["a", "b"]);
    assert_eq!(labels.values(), ["a", "b"]);
    assert_eq!(labels.values_js_string().length(), 2);
    labels.set_flags(&[true]);
    assert_eq!(labels.flags(), [true]);

    let options = LabelOptions::builder()
        .values(&[JsString::from("x")])
        .flags(&[false])
        .build();
    assert_eq!(
        describe_options(&options),
        "Array(string:x) Array(boolean:false)"
    );
}
