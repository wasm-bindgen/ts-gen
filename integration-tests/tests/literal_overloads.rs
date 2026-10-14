#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsNullable, JsString, Map, Object, Promise};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads::{Headers, KVNamespace, SubtleCrypto};

// No backing JavaScript module; compile-only coverage that each literal arm
// is a wrapper with the arm's own return type and without the literal
// parameter, that a lone literal keeps the plain `&str` binding, and that
// static and `_promise` / `try_` forms follow.
#[allow(dead_code)]
async fn arms_compile(
    kv: &KVNamespace,
    headers: &Headers,
    crypto: &SubtleCrypto,
) -> Result<(), JsValue> {
    let _: Option<String> = kv.get("k").await?;
    let _: Option<String> = kv.get_text("k").await?;
    let _: JsValue = kv.get_json("k").await?;
    let _: Option<ArrayBuffer> = kv.get_array_buffer("k").await?;
    let _: Promise<JsNullable<JsString>> = kv.get_text_promise("k");
    let _: Map<JsString, JsValue> = kv.get_json_with_slice(&["a".to_string()]).await?;
    let _: bool = kv.mode_fast();
    let _: Result<bool, JsValue> = kv.try_mode_safe();
    let _: Vec<String> = headers.get_all("Set-Cookie");
    let _: Object = crypto.export_key_jwk("k").await?;
    let _: ArrayBuffer = crypto.export_key_spki("k").await?;
    let _: String = SubtleCrypto::describe_raw();
    Ok(())
}
