//! @ts-gen --experimental-generic-mono --fallible --promise-returns promise

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Promise, TypeError};
use wasm_bindgen::convert::Upcast;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::promise_returns_promise_mono::*;

// `--promise-returns promise` is the same under `--fallible`: a
// synchronous throw is the promise's rejection, so no call returns a
// `Result`.

#[wasm_bindgen_test]
async fn promises_await_rust_values() {
    let promise: Promise<String> = text();
    assert_eq!(promise.await.unwrap(), "hello");
    let handle: Promise<JsString> = text_js_string();
    assert_eq!(String::from(handle.await.unwrap()), "hello");
    let (): () = done().await.unwrap();
    assert_eq!(identity(5.0).await.unwrap(), 5.0);
    assert_eq!(identity("same".to_string()).await.unwrap(), "same");
    let boxed: Wrapper<String> = boxed().await.unwrap();
    assert_eq!(boxed.value(), "boxed");
}

#[wasm_bindgen_test]
async fn promises_upcast_to_js_handles() {
    let handle: Promise<JsString> = text().upcast_into();
    assert_eq!(String::from(handle.await.unwrap()), "hello");
    let any: Promise = count().upcast_into();
    assert_eq!(any.await.unwrap().as_f64(), Some(3.0));
}

#[wasm_bindgen_test]
async fn a_synchronous_throw_is_a_rejection() {
    assert!(throw_now("now").await.is_err());
    let typed: Promise<f64, TypeError> = parse("x");
    assert_eq!(typed.await.unwrap_err().message(), "malformed: x");
}
