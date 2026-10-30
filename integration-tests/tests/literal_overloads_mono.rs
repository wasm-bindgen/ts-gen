//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsNullable, JsString, Map, Object, Promise, Uint8Array};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads_mono::{
    pick_free_a, pick_free_b_with_null, Boxed, Clash, Headers, KVNamespace, KVStore, Opener,
    Picker, Repeats, SubtleCrypto,
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
    let _: Option<String> = kv.get("k").await?;
    let _: Option<JsString> = kv.get_js_string("k").await?;
    let _: Option<String> = kv.get_text("k").await?;
    let _: Option<JsString> = kv.get_text_js_string("k").await?;
    let _: JsValue = kv.get_json("k").await?;
    let _: Option<ArrayBuffer> = kv.get_array_buffer("k").await?;
    let _: Option<Uint8Array> = kv.get_stream("k").await?;
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
    let _: Option<String> = store.get_text(JsString::from("k")).await?;
    let _: Option<JsString> = store.get_text_js_string(JsString::from("k")).await?;
    let _: Option<Object> = store.get_json::<Object>(JsString::from("k")).await?;
    Ok(())
}

// A literal cohort with a `null` arm under mono: the extern's `T` and
// `impl JsStringLike` parameters stay on the typed arms, and the `null`
// arms take neither.
#[allow(dead_code)]
fn null_arms_compile(picker: &Picker, buf: &ArrayBuffer) -> Result<(), JsValue> {
    picker.pick_a(buf);
    picker.pick_b_with_null();
    picker.try_pick_a_with_null()?;
    let _: Picker = Picker::make_a("x");
    let _: Picker = Picker::make_a(JsString::from("x"));
    let _: Picker = Picker::make_b_with_null();
    picker.put_a(JsString::from("x"));
    picker.put_b_with_null();
    Ok(())
}

#[allow(dead_code)]
fn repeated_literal_compiles(repeats: &Repeats) {
    let _: String = repeats.tag_a();
    let _: String = repeats.tag_b();
    let _: JsString = repeats.tag_b_js_string();
    let _: String = repeats.tag_c();
}

// A generic class: a bare static binds the type's defaults, a static
// returning the class hoists its type arguments.
#[allow(dead_code)]
fn generic_class_arms_compile(obj: &Object) -> Result<(), JsValue> {
    let _: String = Boxed::label_a();
    let _: JsString = Boxed::label_b_js_string();
    let _: Boxed<Object> = Boxed::of_b(obj.clone());
    let _: Boxed<Object> = Boxed::of_a(obj.clone());
    let _: Boxed = Boxed::new_a()?;
    Ok(())
}

// A `null` arm's `_js_string` and `_promise` forms, on a generic receiver
// and on a static returning its class; and a plain `null` arm beside a
// cohort, whose private bindings don't collide.
#[allow(dead_code)]
async fn null_arm_forms_compile(
    picker: &Picker,
    store: &KVStore<JsString>,
    clash: &Clash,
    buf: &ArrayBuffer,
    obj: &Object,
) -> Result<(), JsValue> {
    let _: String = picker.name_a_with_null();
    let _: JsString = picker.name_b_with_null_js_string();
    let _: Option<String> = picker.load_b_with_null().await?;
    let _: Option<JsString> = picker.load_a_with_null_js_string().await?;
    let _: Promise<Option<JsString>> = picker.load_a_with_null_promise();
    store.put_a(JsString::from("k"), "v");
    store.put_b_with_null(JsString::from("k"));
    let _: Boxed<Object> = Boxed::from_a(obj.clone());
    let _: Boxed<Object> = Boxed::from_b_with_null();
    clash.mode_with_null();
    clash.mode_x();
    clash.f(buf);
    clash.f_with_null();
    clash.f_a(buf);
    clash.f_b_with_null();
    Ok(())
}

#[allow(dead_code)]
fn free_and_constructor_null_arms_compile(buf: &ArrayBuffer, obj: &Object) -> Result<(), JsValue> {
    pick_free_a(obj.clone());
    pick_free_b_with_null();
    let _: Opener = Opener::new_a(buf)?;
    let _: Opener = Opener::new_b_with_null()?;
    Ok(())
}
