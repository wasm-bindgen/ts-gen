#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::provides_worker::*;
use ts_gen_integration_tests::provides_worker_provides;

// The fixture is ambient, so its runtime lives on `globalThis`.
#[wasm_bindgen(inline_js = r#"
    globalThis.Request = class Request { constructor(url) { this.url = url; } };
    globalThis.Response = class Response { constructor(body) { this.body = body; } };
    globalThis.ExecutionContext = class ExecutionContext { waitUntil() {} };
    export function installed() { return true; }
"#)]
extern "C" {
    fn installed() -> bool;
}

// The wrapper's `#[wasm_bindgen]` exports compile against the real macro
// (`js_namespace = default` on an export) and delegate to the app with
// by-value arguments.
#[wasm_bindgen_test]
async fn fetch_delegates_to_the_app() {
    assert!(installed());
    let request = Request::new("http://x/y").unwrap();
    let ctx = ExecutionContext::new().unwrap();
    let response = provides_worker_provides::fetch(
        request,
        JsValue::from(js_sys::Object::new()).unchecked_into::<Env>(),
        ctx,
    )
    .await
    .unwrap();
    assert_eq!(response.body(), "echo http://x/y");
}

#[wasm_bindgen_test]
async fn tail_returns_unit() {
    assert!(installed());
    let ctx = ExecutionContext::new().unwrap();
    provides_worker_provides::tail(
        vec!["event".to_string()],
        JsValue::from(js_sys::Object::new()).unchecked_into::<Env>(),
        ctx,
    )
    .await
    .unwrap();
}

#[wasm_bindgen_test]
fn optional_parameter_is_an_option() {
    assert_eq!(provides_worker_provides::health(None), "ok");
    assert_eq!(provides_worker_provides::health(Some(true)), "ok (verbose)");
}
