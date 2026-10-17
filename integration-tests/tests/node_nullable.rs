#![cfg(target_arch = "wasm32")]

// `null` and `undefined` reach a Node-style module as distinct values: a
// default initializer fires on `undefined`, `=== null` is checked. Reads
// keep `Nullable`; writes send `null` through their own `_with_null` variant.
use js_sys::Nullable;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::node_nullable::*;

#[wasm_bindgen_test]
fn nullable_argument_sends_null_or_a_value() {
    assert_eq!(describe_with_null(), "no name");
    assert_eq!(describe("g"), "name:g");
}

#[wasm_bindgen_test]
fn optional_parameter_is_omitted_or_present() {
    assert_eq!(repeat(), "x");
    assert_eq!(repeat_with_count(3.0), "xxx");
}

#[wasm_bindgen_test]
fn optional_nullable_parameter_has_three_inputs() {
    assert_eq!(greet(), "hello world");
    assert_eq!(greet_with_null(), "hello nobody");
    assert_eq!(greet_with_name("g"), "hello g");
}

#[wasm_bindgen_test]
fn optional_nullable_argument_sends_each_state() {
    assert_eq!(classify(None), "undefined");
    assert_eq!(classify_with_null(), "null");
    assert_eq!(classify(Some("v")), "value:v");
}

#[wasm_bindgen_test]
fn optional_nullable_return_keeps_each_state() {
    assert_eq!(lookup("known"), Some(Nullable::Value("value".to_string())));
    assert_eq!(lookup("other"), Some(Nullable::Null));
    assert_eq!(lookup("empty"), None);
    assert_eq!(
        lookup("known").and_then(Nullable::value).as_deref(),
        Some("value")
    );
    assert_eq!(lookup("other").and_then(Nullable::value), None);
}

#[wasm_bindgen_test]
fn optional_nullable_field_reads_each_state() {
    assert_eq!(options("missing").label(), None);
    assert_eq!(options("undefined").label(), None);
    assert_eq!(options("null").label(), Some(Nullable::Null));
    assert_eq!(options("x").label(), Some(Nullable::Value("x".to_string())));
}

#[wasm_bindgen_test]
fn optional_nullable_field_setter_assigns_null_or_a_value() {
    assert_eq!(configure(&Options::new()), "no label key");
    assert_eq!(
        configure(&Options::builder().label_with_null().build()),
        "unlabelled"
    );
    assert_eq!(configure(&Options::builder().label("x").build()), "label:x");
    let options = Options::new();
    options.set_label_with_null();
    assert_eq!(configure(&options), "unlabelled");
    assert_eq!(options.label(), Some(Nullable::Null));
}
