//! @ts-gen --experimental-generic-mono --fallible --promise-only

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, JsString, Nullable, Promise, TypeError};
use wasm_bindgen::convert::Upcast;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::promise_only_mono::*;

// Under `--experimental-generic-mono`, a promise-only binding's `Promise<T>`
// is over the awaited Rust value. Under `--fallible` the call returns
// `Result<Promise<T>, E>`, catching a synchronous throw, unless the callable
// is a global, declared `async` or `@throws {never}`.

#[wasm_bindgen_test]
async fn primitives_await_to_rust_values() {
    let promise: Promise<String> = text().unwrap();
    let value: String = promise.await.unwrap();
    assert_eq!(value, "hello");
    let n: f64 = count().unwrap().await.unwrap();
    assert_eq!(n, 3.0);
    let b: bool = flag().unwrap().await.unwrap();
    assert!(b);
    let (): () = done().unwrap().await.unwrap();
    let m: Mode = mode().unwrap().await.unwrap();
    assert_eq!(m, Mode::Slow);
    let any: JsValue = anything().unwrap().await.unwrap();
    assert_eq!(any.as_f64(), Some(42.0));
    let echoed: f64 = identity(5.0).unwrap().await.unwrap();
    assert_eq!(echoed, 5.0);
    let echoed: String = identity("same".to_string()).unwrap().await.unwrap();
    assert_eq!(echoed, "same");
}

#[wasm_bindgen_test]
async fn promises_upcast_to_js_handles() {
    // A `Promise<String>` is still a JS promise: it upcasts for free to the
    // handle forms that JS APIs (`Promise.race`, `waitUntil`) take.
    let handle: Promise<JsString> = text().unwrap().upcast_into();
    assert_eq!(String::from(handle.await.unwrap()), "hello");
    // `JsValue` has no `UpcastFrom<String>` at the pin, so a string promise
    // reaches `Promise<JsValue>` through `Promise<JsString>`.
    let handle: Promise<JsString> = text().unwrap().upcast_into();
    let any: Promise = handle.upcast_into();
    assert_eq!(any.await.unwrap().as_string().as_deref(), Some("hello"));
    let n: Promise = count().unwrap().upcast_into();
    assert_eq!(n.await.unwrap().as_f64(), Some(3.0));
    let flag: Promise = flag().unwrap().upcast_into();
    assert_eq!(flag.await.unwrap().as_bool(), Some(true));
    let done: Promise = done().unwrap().upcast_into();
    assert!(done.await.unwrap().is_undefined());
}

#[wasm_bindgen_test]
async fn lists_and_nullables_keep_their_carriers() {
    let list: Array<String> = tags().unwrap().await.unwrap();
    assert_eq!(list.length(), 2);
    // `Promise<Array<String>>` upcasts to the `JsString` handle for free.
    let handle: Promise<Array<JsString>> = tags().unwrap().upcast_into();
    assert_eq!(handle.await.unwrap().length(), 2);
    let maybe: Nullable<Array<String>> = maybe_tags().unwrap().await.unwrap();
    assert!(maybe.value().is_none());
    let nested: Array<Array<String>> = rows().unwrap().await.unwrap();
    assert_eq!(nested.length(), 2);
    let hit: Nullable<String> = lookup("k").unwrap().await.unwrap();
    assert_eq!(hit.value().as_deref(), Some("v"));
    assert!(lookup("x").unwrap().await.unwrap().value().is_none());
    let found: Option<String> = find("k").unwrap().await.unwrap();
    assert_eq!(found.as_deref(), Some("v"));
    assert_eq!(find("x").unwrap().await.unwrap(), None);
    let peeked: Option<Nullable<String>> = peek("null").unwrap().await.unwrap();
    assert!(peeked.is_some_and(|n| n.value().is_none()));
    assert_eq!(peek("x").unwrap().await.unwrap().map(|n| n.value()), None);
    let peeked = peek("k").unwrap().await.unwrap().and_then(|n| n.value());
    assert_eq!(peeked.as_deref(), Some("v"));
}

#[wasm_bindgen_test]
async fn sync_throws_and_rejections_stay_apart() {
    // A synchronous throw is the call's `Err`; a rejection the await's.
    let thrown: JsValue = throw_now("now").unwrap_err();
    assert!(thrown.is_instance_of::<js_sys::Error>());
    let rejected: JsValue = reject("later").unwrap().await.unwrap_err();
    assert!(rejected.is_instance_of::<js_sys::Error>());
    let typed: TypeError = parse("x").unwrap_err();
    assert_eq!(typed.message(), "malformed: x");
    let parsed: Result<Promise<f64>, TypeError> = parse("2");
    assert_eq!(parsed.unwrap().await.unwrap(), 2.0);
}

#[wasm_bindgen_test]
async fn the_call_starts_before_the_await() {
    let a = text().unwrap();
    let b = count().unwrap();
    assert_eq!(b.await.unwrap(), 3.0);
    assert_eq!(a.await.unwrap(), "hello");
}

#[wasm_bindgen_test]
async fn plain_forms_never_catch() {
    // `@throws {never}`, declared `async`, a global, and a property.
    let ready: Promise<bool> = ready();
    assert!(ready.await.unwrap());
    let size: Promise<f64> = size();
    assert_eq!(size.await.unwrap(), 7.0);
    let tick: Promise<String> = promise_only_tick();
    assert_eq!(tick.await.unwrap(), "tick");
    let item = Item::new("a").unwrap();
    let loaded: Promise<bool> = item.loaded();
    assert!(loaded.await.unwrap());
}

#[wasm_bindgen_test]
async fn classes_null_arms_and_generics() {
    let item = Item::new("a").unwrap();
    let renamed: Item = item.rename("b").unwrap().await.unwrap();
    assert_eq!(renamed.name(), "b");
    assert_eq!(Item::load("c").unwrap().await.unwrap().name(), "c");
    assert_eq!(item.describe().unwrap().await.unwrap(), "a");
    assert_eq!(
        item.describe_with_prefix("> ").unwrap().await.unwrap(),
        "> a"
    );
    let list: Array<Item> = items().unwrap().await.unwrap();
    assert_eq!(list.length(), 2);
    let none: Nullable<Item> = maybe_item("").unwrap().await.unwrap();
    assert!(none.value().is_none());
    assert_eq!(store("k", "v").unwrap().await.unwrap(), "k=v");
    assert_eq!(store_with_null("k").unwrap().await.unwrap(), "k=null");
    let boxed: Wrapper<String> = boxed().unwrap().await.unwrap();
    assert_eq!(boxed.value(), "boxed");
}
