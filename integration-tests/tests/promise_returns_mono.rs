//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Number, Promise, RangeError, TypeError};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::promise_returns_mono::*;

// `--promise-returns both` under `--fallible`: the sync `name_promise()`
// catches a synchronous throw as `Result<Promise<T>, E>`, unless the
// callable is a global, declared `async` or `@throws {never}`.

#[wasm_bindgen_test]
async fn the_async_form_awaits_rust_values() {
    assert_eq!(text().await.unwrap(), "hello");
    let js: JsString = text_js_string().await.unwrap();
    assert_eq!(String::from(js), "hello");
    let (): () = done().await.unwrap();
    assert_eq!(identity(5.0).await.unwrap(), 5.0);
    assert_eq!(lookup("k").await.unwrap().as_deref(), Some("v"));
    let ready: bool = ready().await;
    assert!(ready);
}

#[wasm_bindgen_test]
async fn sync_throws_and_rejections_stay_apart() {
    // A synchronous throw is the call's `Err`; a rejection the await's.
    let thrown: JsValue = throw_now_promise("now").unwrap_err();
    assert!(thrown.is_instance_of::<js_sys::Error>());
    let rejected: JsValue = reject_promise("later").unwrap().await.unwrap_err();
    assert!(rejected.is_instance_of::<js_sys::Error>());
    let typed: TypeError = parse_promise("x").unwrap_err();
    assert_eq!(typed.message(), "malformed: x");
    let parsed: Result<Promise<Number, TypeError>, TypeError> = parse_promise("2");
    assert_eq!(parsed.unwrap().await.unwrap().value_of(), 2.0);
    // The `async fn` reports a rejection from the await.
    assert!(reject("later").await.is_err());
    let typed: RangeError = sqrt(-1.0).await.unwrap_err();
    assert_eq!(typed.message(), "negative: -1");
    let typed: Promise<Number, RangeError> = sqrt_promise(-1.0).unwrap();
    assert_eq!(typed.await.unwrap_err().message(), "negative: -1");
}

#[wasm_bindgen_test]
async fn plain_forms_never_catch() {
    // `@throws {never}`, declared `async`, and a global.
    let ready: Promise<js_sys::Boolean> = ready_promise();
    assert!(ready.await.unwrap().value_of());
    let size: Promise<Number> = size_promise();
    assert_eq!(size.await.unwrap().value_of(), 7.0);
    let tick: Promise<JsString> = promise_returns_tick_promise();
    assert_eq!(String::from(tick.await.unwrap()), "tick");
}

#[wasm_bindgen_test]
async fn classes_and_null_arms() {
    let item = Item::new("a").unwrap();
    assert_eq!(item.rename("b").await.unwrap().name(), "b");
    let renamed: Promise<Item> = item.rename_promise("c").unwrap();
    assert_eq!(renamed.await.unwrap().name(), "c");
    assert_eq!(item.describe_with_prefix("> ").await.unwrap(), "> a");
    assert_eq!(store_with_null("k").await.unwrap(), "k=null");
    let stored: Promise<JsString> = store_with_null_promise("k").unwrap();
    assert_eq!(String::from(stored.await.unwrap()), "k=null");
    let boxed: Wrapper<String> = boxed().await.unwrap();
    assert_eq!(boxed.value(), "boxed");
}
