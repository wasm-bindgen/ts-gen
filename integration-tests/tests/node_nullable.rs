#![cfg(target_arch = "wasm32")]

// `null` and `undefined` reach a Node-style module as distinct values: a
// default initializer fires on `undefined`, `=== null` is checked.
use js_sys::{JsNullable, JsString};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::node_nullable::*;

#[wasm_bindgen_test]
fn nullable_argument_crosses_null() {
    assert_eq!(describe(&JsNullable::new()), "no name");
    assert_eq!(describe(&JsNullable::wrap(JsString::from("g"))), "name:g");
}

#[wasm_bindgen_test]
fn optional_parameter_is_omitted_or_present() {
    assert_eq!(repeat(), "x");
    assert_eq!(repeat_with_count(3.0), "xxx");
}

#[wasm_bindgen_test]
fn optional_nullable_parameter_has_three_inputs() {
    assert_eq!(greet(), "hello world");
    assert_eq!(greet_with_name(&JsNullable::new()), "hello nobody");
    assert_eq!(greet_with_name(&JsNullable::wrap("g".into())), "hello g");
}

#[wasm_bindgen_test]
fn returns_read_both_empties_as_none() {
    assert_eq!(lookup("known"), Some("value".to_string()));
    assert_eq!(lookup("other"), None);
    assert_eq!(lookup("empty"), None);
}

#[wasm_bindgen_test]
fn optional_nullable_field_setter_assigns_null() {
    assert_eq!(configure(&Options::new()), "no label key");
    assert_eq!(
        configure(&Options::builder().label(&JsNullable::new()).build()),
        "unlabelled"
    );
    assert_eq!(
        configure(
            &Options::builder()
                .label(&JsNullable::wrap("x".into()))
                .build()
        ),
        "label:x"
    );
}
