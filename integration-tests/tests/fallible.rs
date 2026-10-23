//! @ts-gen --fallible

#![cfg(target_arch = "wasm32")]

use js_sys::{Number, RangeError};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::fallible::*;

#[wasm_bindgen_test]
fn primary_name_returns_result() {
    let ok: Result<f64, JsValue> = checked_sqrt(9.0);
    assert_eq!(ok.unwrap(), 3.0);
    let err = checked_sqrt(-1.0).unwrap_err();
    assert!(err.is_instance_of::<js_sys::Error>());
}

#[wasm_bindgen_test]
fn throws_never_stays_infallible() {
    let doubled: f64 = double(2.0);
    assert_eq!(doubled, 4.0);
}

#[wasm_bindgen_test]
fn typed_throws_downcasts() {
    let ok: Result<f64, RangeError> = clamp_index(1.0, 3.0);
    assert_eq!(ok.unwrap(), 1.0);
    let err: RangeError = clamp_index(5.0, 3.0).unwrap_err();
    assert_eq!(err.message(), "index 5 out of range");
}

#[wasm_bindgen_test]
async fn async_and_constructor_are_unchanged() {
    let parsed: Result<Number, JsValue> = parse_later("7").await;
    assert_eq!(parsed.unwrap().value_of(), 7.0);
    let counter = Counter::new(8.0).unwrap();
    assert_eq!(counter.increment().unwrap(), 9.0);
    assert_eq!(counter.increment_with_by(1.0).unwrap(), 10.0);
    assert!(counter.increment().is_err());
    assert_eq!(counter.value(), 10.0);
    counter.set_max(20.0);
    assert_eq!(counter.increment_with_by(5.0).unwrap(), 15.0);
}
