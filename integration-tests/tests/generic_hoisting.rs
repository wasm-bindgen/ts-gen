#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Number};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::generic_hoisting::{post, post_async, relay, Courier, Envelope};

/// An instantiation other than the defaults, which the hoisted parameters
/// accept as it is.
fn typed(label: &str) -> Envelope<JsString, Number> {
    Envelope::new(label).unwrap().unchecked_into()
}

#[wasm_bindgen_test]
fn free_functions_take_any_instantiation() {
    let plain: Envelope = Envelope::new("plain").unwrap();
    assert_eq!(post(&plain), "post:plain");
    assert_eq!(post(&typed("typed")), "post:typed");
    assert_eq!(relay(&typed("r"), &JsString::from("m")), "relay:r:m");
}

#[wasm_bindgen_test]
async fn async_functions_take_any_instantiation() {
    let plain: Envelope = Envelope::new("plain").unwrap();
    assert_eq!(post_async(&plain).await.unwrap(), "post-async:plain");
    assert_eq!(
        post_async(&typed("typed")).await.unwrap(),
        "post-async:typed"
    );
}

#[wasm_bindgen_test]
async fn methods_take_any_instantiation() {
    let plain: Envelope = Envelope::new("plain").unwrap();
    let typed = typed("typed");
    let courier = Courier::new(&typed).unwrap();
    assert_eq!(
        Courier::new(&plain).unwrap().send(&typed),
        "send:plain:typed"
    );
    assert_eq!(courier.send(&plain), "send:typed:plain");
    assert_eq!(courier.send(&typed), "send:typed:typed");
    assert_eq!(courier.send_labelled(&typed), "labelled:typed");
    assert_eq!(courier.send_either(&typed), "env:typed");
    assert_eq!(courier.send_either_with_str("s"), "str:s");
    assert_eq!(courier.send_async(&typed).await.unwrap(), "async:typed");
    let promised: JsString = JsFuture::from(courier.send_async_promise(&plain))
        .await
        .unwrap()
        .unchecked_into();
    assert_eq!(promised, "async:plain");
    assert_eq!(Courier::check(&typed), "check:typed");
    // The receiver's parameters and the argument's are independent.
    assert_eq!(typed.forward(&plain), "typed->plain");
    assert_eq!(plain.forward(&typed), "plain->typed");
}

#[wasm_bindgen_test]
fn wrapped_and_returned_types_keep_their_defaults() {
    let plain: Envelope = Envelope::new("plain").unwrap();
    let courier = Courier::new(&plain).unwrap();
    // `Option<&Envelope>` takes the default instantiation, so `None` needs
    // no annotation.
    assert_eq!(courier.send_maybe(None, "t"), "t:none");
    assert_eq!(courier.send_maybe(Some(&plain), "t"), "t:plain");
    let copy: Envelope<wasm_bindgen::JsValue, wasm_bindgen::JsValue> = plain.copy();
    assert_eq!(copy.label(), "plain");
}
