#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::error_enum_node::*;
use ts_gen_integration_tests::CaughtError;

#[wasm_bindgen_test]
fn subclass_lands_before_its_parent() {
    let err = try_raise("sub").unwrap_err();
    assert!(matches!(err, JsException::SubFault(_)), "{err:?}");
    assert_eq!(err.to_string(), "sub fault");

    let err = try_raise("fault").unwrap_err();
    assert!(matches!(err, JsException::Fault(_)), "{err:?}");
    assert_eq!(err.to_string(), "plain fault");
}

#[wasm_bindgen_test]
fn builtin_class_has_its_own_variant() {
    let err = try_raise("type").unwrap_err();
    let JsException::TypeError(error) = &err else {
        panic!("expected TypeError, got {err:?}");
    };
    assert_eq!(String::from(error.message()), "bad type");
    assert_eq!(err.to_string(), "bad type");
}

#[wasm_bindgen_test]
fn non_error_values_land_in_other() {
    let err = try_raise("string").unwrap_err();
    let JsException::Other(value) = &err else {
        panic!("expected Other, got {err:?}");
    };
    assert_eq!(value.as_string().as_deref(), Some("just a string"));
    assert_eq!(err.to_string(), "just a string");

    let err = try_raise("number").unwrap_err();
    assert!(matches!(err, JsException::Other(_)), "{err:?}");
    assert_eq!(err.to_string(), "42");

    // `String(value)` throws for a null-prototype object; `Display` falls
    // back to the `JsValue` debug form.
    let err = try_raise("opaque").unwrap_err();
    assert!(matches!(err, JsException::Other(_)), "{err:?}");
    assert!(err.to_string().starts_with("JsValue("), "{err}");
}

#[wasm_bindgen_test]
fn enum_round_trips_and_boxes() {
    assert_eq!(try_raise("ok").unwrap(), "ok");
    let err = try_raise("sub").unwrap_err();
    let value: JsValue = err.clone().into();
    assert!(matches!(JsException::from(value), JsException::SubFault(_)));
    let boxed: Box<dyn std::error::Error> = Box::new(err);
    assert_eq!(boxed.to_string(), "sub fault");
}

#[wasm_bindgen_test]
fn getter_throws_through_the_enum() {
    assert_eq!(Gate::new(true).unwrap().value().unwrap(), 1.0);
    let err = Gate::new(false).unwrap().value().unwrap_err();
    assert!(matches!(err, JsException::SubFault(_)), "{err:?}");
    assert_eq!(err.to_string(), "gate closed");
}

#[wasm_bindgen_test]
fn repeated_variant_idents_are_numbered() {
    let err = try_raise_named("other").unwrap_err();
    assert!(matches!(err, JsException::Other2(_)), "{err:?}");
    assert_eq!(err.to_string(), "declared other");

    let err = try_raise_named("codes-fault").unwrap_err();
    assert!(matches!(err, JsException::CodesFault(_)), "{err:?}");
    assert_eq!(err.to_string(), "flat fault");

    let err = try_raise_named("codes.fault").unwrap_err();
    assert!(matches!(err, JsException::CodesFault2(_)), "{err:?}");
    assert_eq!(err.to_string(), "namespaced fault");
}

#[wasm_bindgen_test]
fn untyped_bindings_keep_the_error_type() {
    let CaughtError(value) = try_raise_untyped("fault").unwrap_err();
    assert!(matches!(JsException::from(value), JsException::Fault(_)));
}
