//! @ts-gen --promise-returns promise

#![cfg(target_arch = "wasm32")]

use js_sys::{Promise, RangeError, TypeError, Undefined};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::promise_returns_promise::*;

// `--promise-returns promise`: one sync `name()` returning `Promise<T, E>`,
// started at the call. Awaiting it yields `Result<T, E>`, with `T` lowered
// like a sync return.

fn message(err: &JsValue) -> String {
    err.unchecked_ref::<js_sys::Error>().message().into()
}

#[wasm_bindgen_test]
async fn promises_await_rust_values() {
    let promise: Promise<String> = text();
    assert_eq!(promise.await.unwrap(), "hello");
    assert_eq!(count().await.unwrap(), 3.0);
    assert!(flag().await.unwrap());
    let _: Undefined = done().await.unwrap();
    assert_eq!(mode().await.unwrap(), Mode::Slow);
    assert_eq!(tags().await.unwrap(), ["a", "b"]);
    assert_eq!(maybe_tags().await.unwrap(), None);
    assert_eq!(lookup("k").await.unwrap().as_deref(), Some("v"));
    assert_eq!(find("x").await.unwrap(), None);
    let ready: Promise<bool> = ready();
    assert!(ready.await.unwrap());
}

#[wasm_bindgen_test]
async fn the_call_starts_before_the_await() {
    let a = text();
    let b = count();
    assert_eq!(b.await.unwrap(), 3.0);
    assert_eq!(a.await.unwrap(), "hello");
}

#[wasm_bindgen_test]
async fn a_synchronous_throw_is_a_rejection() {
    assert_eq!(message(&reject("later").await.unwrap_err()), "later");
    let thrown: Promise<String> = throw_now("now");
    assert_eq!(message(&thrown.await.unwrap_err()), "now");
    let typed: Promise<f64, TypeError> = parse("x");
    assert_eq!(typed.await.unwrap_err().message(), "malformed: x");
    assert_eq!(parse("2").await.unwrap(), 2.0);
    let typed: Promise<f64, RangeError> = sqrt(-1.0);
    assert_eq!(typed.await.unwrap_err().message(), "negative: -1");
}

#[wasm_bindgen_test]
async fn classes_null_arms_and_globals() {
    let item = Item::new("a").unwrap();
    let renamed: Promise<Item> = item.rename("b");
    assert_eq!(renamed.await.unwrap().name(), "b");
    assert_eq!(Item::load("c").await.unwrap().name(), "c");
    assert_eq!(item.describe_with_prefix("> ").await.unwrap(), "> a");
    assert!(item.loaded().await.unwrap().value_of());
    assert_eq!(items().await.unwrap().len(), 2);
    assert_eq!(store_with_null("k").await.unwrap(), "k=null");
    assert_eq!(promise_returns_tick().await.unwrap(), "tick");
}
