#![cfg(target_arch = "wasm32")]

use js_sys::{
    Array, Boolean, JsNullable, JsOption, JsString, Number, Promise, RangeError, TypeError,
    Undefined,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::promise_returns::*;

// `--promise-returns both`, the default: an `async fn name()` beside a sync
// `name_promise()` returning the promise as a handle over the `js_sys`
// types.

fn message(err: &JsValue) -> String {
    err.unchecked_ref::<js_sys::Error>().message().into()
}

#[wasm_bindgen_test]
async fn the_async_form_awaits_rust_values() {
    let value: String = text().await.unwrap();
    assert_eq!(value, "hello");
    assert_eq!(count().await.unwrap(), 3.0);
    assert!(flag().await.unwrap());
    let (): () = done().await.unwrap();
    assert_eq!(mode().await.unwrap(), Mode::Slow);
    assert_eq!(anything().await.unwrap().as_f64(), Some(42.0));
    let echoed: JsString = identity(&JsString::from("same")).await.unwrap();
    assert_eq!(String::from(echoed), "same");
    assert_eq!(tags().await.unwrap(), ["a", "b"]);
    assert_eq!(maybe_tags().await.unwrap(), None);
    assert_eq!(rows().await.unwrap().len(), 2);
    assert_eq!(lookup("k").await.unwrap().as_deref(), Some("v"));
    assert_eq!(lookup("x").await.unwrap(), None);
    assert_eq!(find("x").await.unwrap(), None);
    assert_eq!(peek("null").await.unwrap(), None);
    // `@throws {never}`: no `Result`.
    let ready: bool = ready().await;
    assert!(ready);
    assert_eq!(size().await.unwrap(), 7.0);
}

#[wasm_bindgen_test]
async fn the_promise_form_is_a_handle() {
    let promise: Promise<JsString> = text_promise();
    assert_eq!(String::from(promise.await.unwrap()), "hello");
    let n: Number = count_promise().await.unwrap();
    assert_eq!(n.value_of(), 3.0);
    let b: Boolean = flag_promise().await.unwrap();
    assert!(b.value_of());
    let _: Undefined = done_promise().await.unwrap();
    let m: JsString = mode_promise().await.unwrap();
    assert_eq!(String::from(m), "slow");
    let list: Array<JsString> = tags_promise().await.unwrap();
    assert_eq!(list.length(), 2);
    let maybe: JsNullable<Array<JsString>> = maybe_tags_promise().await.unwrap();
    assert!(maybe.is_empty());
    let hit: JsNullable<JsString> = lookup_promise("k").await.unwrap();
    assert_eq!(hit.as_option().map(String::from).as_deref(), Some("v"));
    let found: JsOption<JsString> = find_promise("k").await.unwrap();
    assert!(!found.is_empty());
    let ready: Promise<Boolean> = ready_promise();
    assert!(ready.await.unwrap().value_of());
    // It is a JS promise, to hand on as one.
    let any: Promise = text_promise().unchecked_into();
    assert_eq!(any.await.unwrap().as_string().as_deref(), Some("hello"));
}

#[wasm_bindgen_test]
async fn the_promise_form_starts_the_call() {
    let a = text_promise();
    let b = count_promise();
    assert_eq!(b.await.unwrap().value_of(), 3.0);
    assert_eq!(String::from(a.await.unwrap()), "hello");
}

#[wasm_bindgen_test]
async fn a_typed_throws_types_both_forms() {
    let err: RangeError = sqrt(-1.0).await.unwrap_err();
    assert_eq!(err.message(), "negative: -1");
    assert_eq!(sqrt(4.0).await.unwrap(), 2.0);
    let promise: Promise<Number, RangeError> = sqrt_promise(-1.0);
    let rejected: Result<Number, RangeError> = promise.await;
    assert_eq!(rejected.unwrap_err().message(), "negative: -1");
    let parsed: Result<f64, TypeError> = parse("2").await;
    assert_eq!(parsed.unwrap(), 2.0);
    let parsed: Promise<Number, TypeError> = parse_promise("2");
    assert_eq!(parsed.await.unwrap().value_of(), 2.0);
}

#[wasm_bindgen_test]
async fn rejections_come_out_of_the_await() {
    assert_eq!(message(&reject("later").await.unwrap_err()), "later");
    assert_eq!(
        message(&reject_promise("later").await.unwrap_err()),
        "later"
    );
}

#[wasm_bindgen_test]
async fn classes_null_arms_and_globals() {
    let item = Item::new("a").unwrap();
    let renamed: Item = item.rename("b").await.unwrap();
    assert_eq!(renamed.name(), "b");
    let renamed: Promise<Item> = item.rename_promise("c");
    assert_eq!(renamed.await.unwrap().name(), "c");
    assert_eq!(Item::load("c").await.unwrap().name(), "c");
    assert_eq!(Item::load_promise("d").await.unwrap().name(), "d");
    assert_eq!(item.describe().await.unwrap(), "a");
    assert_eq!(item.describe_with_prefix("> ").await.unwrap(), "> a");
    let described: Promise<JsString> = item.describe_with_prefix_promise("> ");
    assert_eq!(String::from(described.await.unwrap()), "> a");
    assert!(item.loaded().await.unwrap().value_of());
    assert_eq!(items().await.unwrap().len(), 2);
    assert!(maybe_item("").await.unwrap().is_none());
    assert_eq!(store("k", "v").await.unwrap(), "k=v");
    assert_eq!(store_with_null("k").await.unwrap(), "k=null");
    let stored: Promise<JsString> = store_with_null_promise("k");
    assert_eq!(String::from(stored.await.unwrap()), "k=null");
    assert_eq!(String::from(boxed().await.unwrap().value()), "boxed");
    assert_eq!(promise_returns_tick().await.unwrap(), "tick");
    let tick: Promise<JsString> = promise_returns_tick_promise();
    assert_eq!(String::from(tick.await.unwrap()), "tick");
}
