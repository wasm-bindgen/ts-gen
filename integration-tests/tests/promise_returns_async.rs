//! @ts-gen --promise-returns async

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, RangeError, TypeError};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::promise_returns_async::*;

// `--promise-returns async`: the `async fn name()` alone, which calls JS
// when it is first polled.

fn message(err: &JsValue) -> String {
    err.unchecked_ref::<js_sys::Error>().message().into()
}

#[wasm_bindgen_test]
async fn the_async_form_awaits_rust_values() {
    let value: Result<String, JsValue> = text().await;
    assert_eq!(value.unwrap(), "hello");
    assert_eq!(count().await.unwrap(), 3.0);
    let (): () = done().await.unwrap();
    assert_eq!(mode().await.unwrap(), Mode::Slow);
    let echoed: JsString = identity(&JsString::from("same")).await.unwrap();
    assert_eq!(String::from(echoed), "same");
    assert_eq!(tags().await.unwrap(), ["a", "b"]);
    assert_eq!(lookup("x").await.unwrap(), None);
    let ready: bool = ready().await;
    assert!(ready);
}

#[wasm_bindgen_test]
async fn rejections_come_out_of_the_await() {
    assert_eq!(message(&reject("later").await.unwrap_err()), "later");
    let err: RangeError = sqrt(-1.0).await.unwrap_err();
    assert_eq!(err.message(), "negative: -1");
    let parsed: Result<f64, TypeError> = parse("2").await;
    assert_eq!(parsed.unwrap(), 2.0);
}

#[wasm_bindgen_test]
async fn classes_null_arms_and_globals() {
    let item = Item::new("a").unwrap();
    assert_eq!(item.rename("b").await.unwrap().name(), "b");
    assert_eq!(Item::load("c").await.unwrap().name(), "c");
    assert_eq!(item.describe_with_prefix("> ").await.unwrap(), "> a");
    // A `Promise`-typed property is still a getter of the promise.
    assert!(item.loaded().await.unwrap().value_of());
    assert_eq!(store_with_null("k").await.unwrap(), "k=null");
    assert_eq!(promise_returns_tick().await.unwrap(), "tick");
}
