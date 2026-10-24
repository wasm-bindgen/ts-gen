//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Nullable, Object};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::erased_type_params_mono::{ListKey, ListResult, Store};

// Compile-only: the fixture has no backing JavaScript module. An argument
// given for an erased parameter (`ListResult<Metadata, Key>`) is dropped
// before per-monomorphization lowering looks for string leaves, so `list`
// returns the one-parameter `ListResult` and has no `_js_string` variant.
#[allow(dead_code)]
async fn erased_type_params_mono_compile(store: &Store) -> Result<(), JsValue> {
    let _: Nullable<String> = store.get("k").await?;
    let _: Nullable<JsString> = store.get_js_string("k").await?;
    let result: ListResult<Object> = store.list::<Object>().await?;
    let keys: Vec<ListKey<Object>> = result.keys();
    for key in &keys {
        let _: String = key.name();
        let _: Option<Object> = key.metadata();
    }
    let _: String = result.cursor();
    store.tag("info", 1.0);
    let _: f64 = store.bump(2.0);
    Ok(())
}
