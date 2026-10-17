#![cfg(target_arch = "wasm32")]

use js_sys::Object;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::generic_type_level::{Bag, Labelled, PingTarget, TransformStream};

fn assert_eq<T: Eq>() {}

// `Eq` holds for a generic type at its `JsValue` default, for a subclass of
// a generic parent written with arguments, and for a child of
// `Record<string, unknown>` (`Object<JsValue>`).
#[allow(dead_code)]
fn every_type_is_eq() {
    assert_eq::<TransformStream>();
    assert_eq::<Labelled<JsValue>>();
    assert_eq::<PingTarget>();
    assert_eq::<Bag>();
}

#[wasm_bindgen_test]
fn generic_handles_compare_by_identity() {
    let object = Object::new();
    let a: TransformStream = JsValue::from(&object).unchecked_into();
    let b: TransformStream = JsValue::from(&object).unchecked_into();
    let other: TransformStream = JsValue::from(Object::new()).unchecked_into();
    assert!(a == b);
    assert!(a != other);
}
