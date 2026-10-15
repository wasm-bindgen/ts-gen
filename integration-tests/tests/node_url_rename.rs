#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::node_url_rename::{file_url_to_path, URL};

#[wasm_bindgen_test]
fn test_renamed_class() {
    let url = URL::new("file:///tmp/a%20b").unwrap();
    assert_eq!(url.href(), "file:///tmp/a%20b");
    assert!(JsValue::from(url).is_instance_of::<URL>());
}

#[wasm_bindgen_test]
fn test_renamed_class_static() {
    assert!(URL::can_parse("https://example.com"));
    assert!(!URL::can_parse("not a url"));
}

#[wasm_bindgen_test]
fn test_renamed_function() {
    assert_eq!(file_url_to_path("file:///tmp/a%20b"), "/tmp/a b");
}
