#![cfg(target_arch = "wasm32")]

use js_sys::{Promise, RangeError, Undefined};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::never_returns::*;

// A `never` return binds only its catching form, under the bare name and
// typed `Result<(), E>`; `try_` stays only where a plain twin exists.

fn message(err: &JsValue) -> String {
    err.unchecked_ref::<js_sys::Error>().message().into()
}

#[wasm_bindgen_test]
fn functions_bind_only_the_catching_form() {
    let err: JsValue = fail("boom").unwrap_err();
    assert_eq!(message(&err), "boom");
    let err: RangeError = fail_range(3.0).unwrap_err();
    assert_eq!(err.message(), "out of range: 3");
    // `@throws {never}` loses to the `never` return.
    let failed: Result<(), JsValue> = fail_anyway();
    assert_eq!(message(&failed.unwrap_err()), "anyway");
    let err = abort(2.0).unwrap_err();
    assert_eq!(message(&err), "abort 2");
}

#[wasm_bindgen_test]
fn overloads_apply_the_rule_one_by_one() {
    assert_eq!(check("ok"), "ok");
    assert_eq!(try_check("ok").unwrap(), "ok");
    let err = check_with_f64(1.0).unwrap_err();
    assert!(err.is_instance_of::<js_sys::TypeError>());
}

#[wasm_bindgen_test]
fn literal_arms_get_catch_only_wrappers() {
    assert!(raise_type("t")
        .unwrap_err()
        .is_instance_of::<js_sys::TypeError>());
    assert!(raise_range("r")
        .unwrap_err()
        .is_instance_of::<js_sys::RangeError>());
    assert_eq!(read_text(), "text");
    assert_eq!(try_read_text().unwrap(), "text");
    let failed: Result<(), JsValue> = read_fail();
    assert_eq!(message(&failed.unwrap_err()), "read failed");
    let guard = Guard::new().unwrap();
    assert_eq!(message(&guard.reject_soft().unwrap_err()), "rejected soft");
    assert_eq!(message(&guard.reject_hard().unwrap_err()), "rejected hard");
}

#[wasm_bindgen_test]
fn methods_and_statics_bind_only_the_catching_form() {
    let guard = Guard::new().unwrap();
    let err = guard.value_of().unwrap_err();
    assert_eq!(message(&err), "a guard has no primitive value");
    let err = Guard::unreachable("here").unwrap_err();
    assert_eq!(message(&err), "unreachable: here");
    let halt = Halt::new().unwrap();
    assert!(halt.ok());
    let err = Halt::stop(7.0).unwrap_err();
    assert_eq!(message(&err), "stop 7");
}

#[wasm_bindgen_test]
async fn promise_never_awaits_unit() {
    let rejected: Result<(), JsValue> = reject_later("nope").await;
    assert_eq!(message(&rejected.unwrap_err()), "nope");
    let promise: Promise<Undefined> = reject_later_promise("later");
    assert_eq!(message(&promise.await.unwrap_err()), "later");
    let guard = Guard::new().unwrap();
    let rejected: Result<(), JsValue> = guard.fail_later().await;
    assert_eq!(message(&rejected.unwrap_err()), "later");
    let _: Promise<Undefined> = guard.fail_later_promise();
}
