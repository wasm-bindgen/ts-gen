//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsNullable, JsString, Map, Nullable, Object, Uint8Array};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads_mono::{
    Headers, KVNamespace, KVStore, SubtleCrypto,
};

// No backing JavaScript module; compile-only coverage that under
// per-monomorphization each literal arm's `_js_string` sibling is a wrapper
// of its own name beside the arm's native-string one, and that a generic
// arm carries its extern's ABI bounds.
#[allow(dead_code)]
async fn mono_arms_compile(
    kv: &KVNamespace,
    headers: &Headers,
    crypto: &SubtleCrypto,
) -> Result<(), JsValue> {
    let _: Nullable<String> = kv.get("k").await?;
    let _: Nullable<JsString> = kv.get_js_string("k").await?;
    let _: Nullable<String> = kv.get_text("k").await?;
    let _: Nullable<JsString> = kv.get_text_js_string("k").await?;
    let _: JsValue = kv.get_json("k").await?;
    let _: Nullable<ArrayBuffer> = kv.get_array_buffer("k").await?;
    let _: Nullable<Uint8Array> = kv.get_stream("k").await?;
    let _: Map<JsString, JsNullable<JsString>> = kv.get_text_with_slice(&["a"]).await?;
    let _: Map<JsString, JsValue> = kv.get_json_with_slice(&["a"]).await?;
    let _: bool = kv.mode_fast();
    let _: bool = kv.mode_safe();
    let _: Vec<String> = headers.get_all("Set-Cookie");
    let _: Object = crypto.export_key_jwk("k").await?;
    let _: ArrayBuffer = crypto.export_key_spki("k").await?;
    let _: String = SubtleCrypto::describe_jwk();
    let _: String = SubtleCrypto::describe_raw();
    let _: JsString = SubtleCrypto::describe_jwk_js_string();
    let _: JsString = SubtleCrypto::describe_raw_js_string();
    Ok(())
}

#[allow(dead_code)]
async fn generic_arms_compile(store: &KVStore<JsString>) -> Result<(), JsValue> {
    let _: Nullable<String> = store.get_text(JsString::from("k")).await?;
    let _: Nullable<JsString> = store.get_text_js_string(JsString::from("k")).await?;
    let _: Nullable<Object> = store.get_json::<Object>(JsString::from("k")).await?;
    Ok(())
}
