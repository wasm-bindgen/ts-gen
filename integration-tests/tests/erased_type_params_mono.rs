#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;

use ts_gen_integration_tests::erased_type_params_mono::{ListResult, Store};

// Compile-only: `list` has no `_js_string` variant, since the erased `Key`
// leaves no string leaf in its return.
#[allow(dead_code)]
async fn list_compiles(store: &Store) -> Result<ListResult<JsValue>, JsValue> {
    store.list().await
}
