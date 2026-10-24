//! @ts-gen --promise-only

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, Boolean, JsNullable, JsOption, JsString, Number, Promise, Undefined};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::promise_only::*;

// Without `--experimental-generic-mono`, a promise-only binding returns the
// `Promise<T>` over the `js_sys` handle, so awaiting yields the handle.

#[wasm_bindgen_test]
async fn primitives_await_to_handles() {
    let promise: Promise<JsString> = text();
    let value: JsString = promise.await.unwrap();
    assert_eq!(String::from(value), "hello");
    let n: Number = count().await.unwrap();
    assert_eq!(n.value_of(), 3.0);
    let b: Boolean = flag().await.unwrap();
    assert!(b.value_of());
    let _: Undefined = done().await.unwrap();
    let m: JsString = mode().await.unwrap();
    assert_eq!(String::from(m), "slow");
    let any: JsValue = anything().await.unwrap();
    assert_eq!(any.as_f64(), Some(42.0));
    let echoed: JsString = identity(&JsString::from("same")).await.unwrap();
    assert_eq!(String::from(echoed), "same");
}

#[wasm_bindgen_test]
async fn lists_and_nullables_await_to_handles() {
    let list: Array<JsString> = tags().await.unwrap();
    assert_eq!(list.length(), 2);
    let maybe: JsNullable<Array<JsString>> = maybe_tags().await.unwrap();
    assert!(maybe.is_empty());
    let nested: Array<Array<JsString>> = rows().await.unwrap();
    assert_eq!(nested.length(), 2);
    let hit: JsNullable<JsString> = lookup("k").await.unwrap();
    assert_eq!(hit.as_option().map(String::from).as_deref(), Some("v"));
    assert!(lookup("x").await.unwrap().is_empty());
    let found: JsOption<JsString> = find("k").await.unwrap();
    assert!(!found.is_empty());
    assert!(find("x").await.unwrap().is_empty());
    assert!(peek("null").await.unwrap().is_empty());
}

#[wasm_bindgen_test]
async fn rejection_comes_out_of_the_await() {
    let err = reject("boom").await.unwrap_err();
    assert!(err.is_instance_of::<js_sys::Error>());
}

#[wasm_bindgen_test]
async fn the_call_starts_before_the_await() {
    // Both calls happen now; awaiting in either order works.
    let a = text();
    let b = count();
    assert_eq!(b.await.unwrap().value_of(), 3.0);
    assert_eq!(String::from(a.await.unwrap()), "hello");
}

#[wasm_bindgen_test]
async fn classes_null_arms_and_globals() {
    let item = Item::new("a").unwrap();
    let renamed: Item = item.rename("b").await.unwrap();
    assert_eq!(renamed.name(), "b");
    assert_eq!(Item::load("c").await.unwrap().name(), "c");
    assert_eq!(String::from(item.describe().await.unwrap()), "a");
    assert_eq!(
        String::from(item.describe_with_prefix("> ").await.unwrap()),
        "> a"
    );
    assert!(item.loaded().await.unwrap().value_of());
    let list: Array<Item> = items().await.unwrap();
    assert_eq!(list.length(), 2);
    assert!(maybe_item("").await.unwrap().is_empty());
    assert_eq!(String::from(store("k", "v").await.unwrap()), "k=v");
    assert_eq!(String::from(store_with_null("k").await.unwrap()), "k=null");
    assert_eq!(String::from(boxed().await.unwrap().value()), "boxed");
    assert_eq!(String::from(promise_only_tick().await.unwrap()), "tick");
    assert_eq!(
        String::from(ready().await.unwrap().value_of().to_string()),
        "true"
    );
    assert_eq!(size().await.unwrap().value_of(), 7.0);
    assert_eq!(parse("2").await.unwrap().value_of(), 2.0);
}
