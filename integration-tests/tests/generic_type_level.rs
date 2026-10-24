#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Number, Object};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::generic_type_level::{
    load_value, Bag, Chained, Labelled, PingTarget, TransformStream,
};

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

// An async static and an async free function resolving to a bare type
// parameter bind it `FromWasmAbi`, which `JsFuture<T>` needs to convert
// the settled value, and resolve to the caller's choice of `T`.
#[allow(dead_code)]
async fn async_bare_param_returns() -> Result<(), JsValue> {
    let _: JsString = Labelled::<JsValue>::load::<JsString>().await?;
    let _: Number = load_value::<Number>().await?;
    Ok(())
}

// A constructor fills an unmentioned parameter whose default names an
// earlier unmentioned one (`B = A`) with that one's default, so `new`
// declares only the parameter its argument mentions.
#[allow(dead_code)]
fn chained_defaults_resolve(c: &Number) -> Result<Chained<JsString, JsString, Number>, JsValue> {
    Chained::new(c)
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
