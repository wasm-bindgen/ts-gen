//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsNullable, JsString, Map, Object, Uint8Array};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads_mono::{
    pick_free, pick_free_with_null, AOrB, Boxed, Clash, ClashKind, Headers, KVNamespace,
    KVNamespaceKind, KVStore, Opener, Picker, Repeats, SubtleCrypto,
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
    let _: bool = kv.mode(KVNamespaceKind::Fast);
    let _: bool = kv.mode(KVNamespaceKind::Safe);
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

// An enum beside a `null` arm under mono: the extern's `T` and
// `impl JsStringLike` parameters stay on the typed arms, and the `null`
// arms take neither.
#[allow(dead_code)]
fn null_arms_compile(picker: &Picker, buf: &ArrayBuffer) -> Result<(), JsValue> {
    picker.pick(AOrB::A, buf);
    picker.pick_with_null(AOrB::B);
    picker.try_pick_with_null(AOrB::A)?;
    let _: Picker = Picker::make(AOrB::A, "x");
    let _: Picker = Picker::make(AOrB::A, JsString::from("x"));
    let _: Picker = Picker::make_with_null(AOrB::B);
    picker.put(AOrB::A, JsString::from("x"));
    picker.put_with_null(AOrB::B);
    Ok(())
}

#[allow(dead_code)]
fn repeated_literal_compiles(repeats: &Repeats) {
    let _: String = repeats.tag_a();
    let _: String = repeats.tag_b();
    let _: JsString = repeats.tag_b_js_string();
    let _: String = repeats.tag_c();
}

// A generic class's statics taking an enum.
#[allow(dead_code)]
fn generic_class_arms_compile(obj: &Object) -> Result<(), JsValue> {
    let _: String = Boxed::label(AOrB::A);
    let _: JsString = Boxed::label_js_string(AOrB::B);
    let _: Boxed<Object> = Boxed::of(AOrB::B, obj.clone());
    let _: Boxed<Object> = Boxed::of(AOrB::A, obj.clone());
    let _: Boxed = Boxed::new(AOrB::A)?;
    Ok(())
}

// A `null` arm's `_js_string` and promise forms, on a generic receiver
// and on a static returning its class; and a plain `null` arm beside an
// enum, whose private bindings don't collide.
#[allow(dead_code)]
async fn null_arm_forms_compile(
    picker: &Picker,
    store: &KVStore<JsString>,
    clash: &Clash,
    buf: &ArrayBuffer,
    obj: &Object,
) -> Result<(), JsValue> {
    let _: String = picker.name_with_null(AOrB::A);
    let _: JsString = picker.name_with_null_js_string(AOrB::B);
    let _: Option<String> = picker.load_with_null(AOrB::B).await?;
    let _: Option<JsString> = picker.load_with_null_js_string(AOrB::A).await?;
    store.put(JsString::from("k"), AOrB::A, "v");
    store.put_with_null(JsString::from("k"), AOrB::B);
    let _: Boxed<Object> = Boxed::from(AOrB::A, obj.clone());
    let _: Boxed<Object> = Boxed::from_with_null(AOrB::B);
    clash.mode_with_null();
    clash.mode(ClashKind::X);
    clash.f(buf);
    clash.f_with_x_null();
    clash.f_with_kind_and_x(AOrB::A, buf);
    clash.f_with_kind_and_x_null(AOrB::B);
    Ok(())
}

#[allow(dead_code)]
fn free_and_constructor_null_arms_compile(buf: &ArrayBuffer, obj: &Object) -> Result<(), JsValue> {
    pick_free(AOrB::A, obj.clone());
    pick_free_with_null(AOrB::B);
    let _: Opener = Opener::new(AOrB::A, buf)?;
    let _: Opener = Opener::new_with_null(AOrB::B)?;
    Ok(())
}
