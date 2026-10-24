//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use js_sys::{Promise, RangeError};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::never_returns_mono::*;

// Under `--fallible` the bare name is the catching `never` form, as by default.

fn message(err: &JsValue) -> String {
    err.unchecked_ref::<js_sys::Error>().message().into()
}

#[wasm_bindgen_test]
fn functions_catch_under_the_bare_name() {
    let err: JsValue = fail("boom").unwrap_err();
    assert_eq!(message(&err), "boom");
    let err: RangeError = fail_range(3.0).unwrap_err();
    assert_eq!(err.message(), "out of range: 3");
    let failed: Result<(), JsValue> = fail_anyway();
    assert_eq!(message(&failed.unwrap_err()), "anyway");
    assert_eq!(message(&abort(2.0).unwrap_err()), "abort 2");
    assert_eq!(check("ok").unwrap(), "ok");
    assert!(check_with_f64(1.0)
        .unwrap_err()
        .is_instance_of::<js_sys::TypeError>());
}

#[wasm_bindgen_test]
fn literal_arms_catch_under_the_bare_name() {
    assert!(raise_type("t")
        .unwrap_err()
        .is_instance_of::<js_sys::TypeError>());
    assert!(raise_range("r")
        .unwrap_err()
        .is_instance_of::<js_sys::RangeError>());
    assert_eq!(read_text().unwrap(), "text");
    let failed: Result<(), JsValue> = read_fail();
    assert_eq!(message(&failed.unwrap_err()), "read failed");
    let guard = Guard::new().unwrap();
    assert_eq!(message(&guard.reject_soft().unwrap_err()), "rejected soft");
    assert_eq!(message(&guard.reject_hard().unwrap_err()), "rejected hard");
}

#[wasm_bindgen_test]
fn methods_and_statics_catch_under_the_bare_name() {
    let guard = Guard::new().unwrap();
    let failed: Result<(), JsValue> = guard.value_of();
    assert_eq!(
        message(&failed.unwrap_err()),
        "a guard has no primitive value"
    );
    assert_eq!(
        message(&Guard::unreachable("here").unwrap_err()),
        "unreachable: here"
    );
    assert_eq!(message(&Halt::stop(7.0).unwrap_err()), "stop 7");
}

#[wasm_bindgen_test]
async fn promise_never_awaits_unit() {
    let rejected: Result<(), JsValue> = reject_later("nope").await;
    assert_eq!(message(&rejected.unwrap_err()), "nope");
    // Per-mono the async member is its `Promise<(), JsValue>`; there is no
    // `_promise` twin.
    let promise: Promise<()> = reject_later("later");
    assert_eq!(message(&promise.await.unwrap_err()), "later");
    let rejected: Result<(), JsValue> = Guard::new().unwrap().fail_later().await;
    assert_eq!(message(&rejected.unwrap_err()), "later");
}
