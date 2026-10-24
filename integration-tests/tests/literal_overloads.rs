#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsString, Map, Nullable, Object};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads::{Headers, KVNamespace, KVStore, SubtleCrypto};

// No backing JavaScript module; compile-only coverage that each literal arm
// is a wrapper with the arm's own return type and without the literal
// parameter, that a lone literal keeps the plain `&str` binding, and that
// static and `try_` forms follow, and that a generic arm carries its
// extern's ABI bounds.
#[allow(dead_code)]
async fn arms_compile(
    kv: &KVNamespace,
    headers: &Headers,
    crypto: &SubtleCrypto,
) -> Result<(), JsValue> {
    let _: Nullable<JsString> = kv.get("k").await?;
    let _: Nullable<JsString> = kv.get_text("k").await?;
    let _: JsValue = kv.get_json("k").await?;
    let _: Nullable<ArrayBuffer> = kv.get_array_buffer("k").await?;
    let _: Map<JsString, JsValue> = kv.get_json_with_slice(&["a".to_string()]).await?;
    let _: bool = kv.mode_fast();
    let _: Result<bool, JsValue> = kv.try_mode_safe();
    let _: Vec<String> = headers.get_all("Set-Cookie");
    let _: Object = crypto.export_key_jwk("k").await?;
    let _: ArrayBuffer = crypto.export_key_spki("k").await?;
    let _: String = SubtleCrypto::describe_raw();
    Ok(())
}

#[allow(dead_code)]
async fn generic_arms_compile(store: &KVStore<JsString>, key: &JsString) -> Result<(), JsValue> {
    let _: Nullable<JsString> = store.get_text(key).await?;
    let _: Nullable<Object> = store.get_json::<Object>(key).await?;
    Ok(())
}
