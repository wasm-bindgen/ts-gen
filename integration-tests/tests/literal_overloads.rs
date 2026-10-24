#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsNullable, JsString, Map, Nullable, Object, Promise};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads::{
    Boxed, Headers, KVNamespace, KVStore, Picker, Repeats, SubtleCrypto,
};

// No backing JavaScript module; compile-only coverage that each literal arm
// is a wrapper with the arm's own return type and without the literal
// parameter, that a lone literal keeps the plain `&str` binding, and that
// static and `_promise` / `try_` forms follow, and that a generic arm
// carries its extern's ABI bounds.
#[allow(dead_code)]
async fn arms_compile(
    kv: &KVNamespace,
    headers: &Headers,
    crypto: &SubtleCrypto,
) -> Result<(), JsValue> {
    let _: Nullable<String> = kv.get("k").await?;
    let _: Nullable<String> = kv.get_text("k").await?;
    let _: JsValue = kv.get_json("k").await?;
    let _: Nullable<ArrayBuffer> = kv.get_array_buffer("k").await?;
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

#[allow(dead_code)]
async fn generic_arms_compile(store: &KVStore<JsString>, key: &JsString) -> Result<(), JsValue> {
    let _: Nullable<String> = store.get_text(key).await?;
    let _: Nullable<Object> = store.get_json::<Object>(key).await?;
    Ok(())
}

// A literal cohort with a `null` arm: its wrappers drop the `null`
// parameter and pass `null` themselves, under `_with_null` names.
#[allow(dead_code)]
fn null_arms_compile(picker: &Picker, buf: &ArrayBuffer, obj: &Object) -> Result<(), JsValue> {
    picker.pick_a(buf);
    picker.pick_b_with_null();
    picker.try_pick_a_with_null()?;
    let _: Picker = Picker::make_a("x");
    let _: Picker = Picker::make_b_with_null();
    let _: Result<Picker, JsValue> = Picker::try_make_a_with_null();
    picker.put_a(obj);
    picker.put_b_with_null();
    Ok(())
}

// A literal repeated across overloads is one arm with one wrapper.
#[allow(dead_code)]
fn repeated_literal_compiles(repeats: &Repeats) {
    let _: String = repeats.tag_a();
    let _: String = repeats.tag_b();
    let _: String = repeats.tag_c();
}

// A generic class: a bare static binds the type's defaults, a static
// returning the class hoists its type arguments.
#[allow(dead_code)]
fn generic_class_arms_compile(obj: &Object) -> Result<(), JsValue> {
    let _: String = Boxed::label_a();
    let _: Boxed<Object> = Boxed::of_b(obj);
    let _: Boxed<Object> = Boxed::of_a(obj);
    let _: Boxed = Boxed::new_a()?;
    Ok(())
}
