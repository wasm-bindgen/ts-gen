#![cfg(target_arch = "wasm32")]

// `null` and `undefined` reach a Node-style module as distinct values: a
// default initializer fires on `undefined`, `=== null` is checked. Writes
// send `null` through their own `_with_null` variant, and omission sends no
// argument. Reads, an awaited value included, are `Option<T>`: a received
// `null` is `None` like a received `undefined`. `T | null | undefined` is
// the IR's `T?`, writing `undefined`; `node_nullable_strict` keeps its
// three states.
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
fn optional_nullable_parameter_is_omitted_or_present() {
    assert_eq!(greet(), "hello world");
    assert_eq!(greet_with_name("g"), "hello g");
}

#[wasm_bindgen_test]
fn optional_nullable_argument_sends_undefined_or_a_value() {
    assert_eq!(classify(), "undefined");
    assert_eq!(classify_with_value("v"), "value:v");
}

#[wasm_bindgen_test]
fn optional_nullable_return_reads_either_empty_as_none() {
    assert_eq!(lookup("known").as_deref(), Some("value"));
    assert_eq!(lookup("other"), None);
    assert_eq!(lookup("empty"), None);
}

#[wasm_bindgen_test]
async fn awaited_value_reads_either_empty_as_none() {
    let value = |s: &str| Some(s.to_string());
    assert_eq!(lookup_async("known").await.unwrap(), value("value"));
    assert_eq!(lookup_async("other").await.unwrap(), None);
    assert_eq!(lookup_async("empty").await.unwrap(), None);
    assert!(find("other").await.unwrap().is_none());
    let found = find("known").await.unwrap().unwrap();
    assert_eq!(found.label(), value("found"));
}

#[wasm_bindgen_test]
async fn inner_option_reads_null_as_empty() {
    let known = pending("known").value().await.unwrap();
    assert_eq!(known.into_option().map(String::from), Some("value".into()));
    assert!(pending("other").value().await.unwrap().is_empty());
    assert!(pending("empty").value().await.unwrap().is_empty());
}

#[wasm_bindgen_test]
fn optional_nullable_field_reads_either_empty_as_none() {
    assert_eq!(options("missing").label(), None);
    assert_eq!(options("undefined").label(), None);
    assert_eq!(options("null").label(), None);
    assert_eq!(options("x").label(), Some("x".to_string()));
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
    assert_eq!(options.label(), None);
}

#[wasm_bindgen_test]
fn trailing_undefined_is_omitted() {
    assert_eq!(arity(), "0:-");
    assert_eq!(arity_with_a("x"), "1:x");
    assert_eq!(arity_with_a_and_b(Some("x"), 2.0), "2:2");
}

#[wasm_bindgen_test]
fn non_trailing_undefined_is_passed() {
    assert_eq!(pick(None, 1.0), "2:undefined:1");
    assert_eq!(pick(Some("a"), 1.0), "2:a:1");
}

#[wasm_bindgen_test]
fn null_only_parameter_is_named_with_null_and_sends_null() {
    assert_eq!(only_null_with_null(), "1:null");
}
