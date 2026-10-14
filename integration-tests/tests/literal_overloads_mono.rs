#![cfg(target_arch = "wasm32")]

use js_sys::{JsNullable, JsString};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads_mono::{Cell, Store};

// No backing JavaScript module; compile-only.
#[allow(dead_code)]
async fn arms_compile(store: &Store, cell: &Cell<JsString>) -> Result<(), JsValue> {
    let _: Option<String> = store.get_text("k").await?;
    let _: Option<JsString> = store.get_text_js_string("k").await?;
    let _: JsNullable<JsValue> = store.get_json("k").await?;
    let _: String = store.read_text("k");
    let _: JsString = store.read_text_js_string("k");
    let _: JsValue = store.read_json("k");
    let _: Result<JsValue, JsValue> = store.try_read_json("k");
    let _: JsString = cell.take_now();
    let _: JsString = cell.take_later().await?;
    Ok(())
}
