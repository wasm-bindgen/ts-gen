#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsString, Map, Object};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::literal_overloads::{
    pick_free, pick_free_with_null, AOrB, Boxed, Clash, ClashKind, Headers, KVNamespace,
    KVNamespaceKind, KVStore, Opener, Picker, Repeats, SubtleCrypto,
};

// No backing JavaScript module; compile-only coverage that a literal union
// takes the IR's enum, that each literal arm of separate overloads is a
// wrapper with the arm's own return type and without the literal
// parameter, that a lone literal keeps the plain `&str` binding, that
// static and `try_` forms follow, and that a generic arm carries its
// extern's ABI bounds.
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
    let _: Map<JsString, JsValue> = kv.get_json_with_slice(&["a".to_string()]).await?;
    let _: bool = kv.mode(KVNamespaceKind::Fast);
    let _: Result<bool, JsValue> = kv.try_mode(KVNamespaceKind::Safe);
    let _: Vec<String> = headers.get_all("Set-Cookie");
    let _: Object = crypto.export_key_jwk("k").await?;
    let _: ArrayBuffer = crypto.export_key_spki("k").await?;
    let _: String = SubtleCrypto::describe_raw();
    Ok(())
}

#[allow(dead_code)]
async fn generic_arms_compile(store: &KVStore<JsString>, key: &JsString) -> Result<(), JsValue> {
    let _: Option<String> = store.get_text(key).await?;
    let _: Option<Object> = store.get_json::<Object>(key).await?;
    Ok(())
}

// An enum beside a `null` arm: its wrappers drop the `null` parameter and
// pass `null` themselves, under `_with_null` names.
#[allow(dead_code)]
fn null_arms_compile(picker: &Picker, buf: &ArrayBuffer, obj: &Object) -> Result<(), JsValue> {
    picker.pick(AOrB::A, buf);
    picker.pick_with_null(AOrB::B);
    picker.try_pick_with_null(AOrB::A)?;
    let _: Picker = Picker::make(AOrB::A, "x");
    let _: Picker = Picker::make_with_null(AOrB::B);
    let _: Result<Picker, JsValue> = Picker::try_make_with_null(AOrB::A);
    picker.put(AOrB::A, obj);
    picker.put_with_null(AOrB::B);
    Ok(())
}

// A literal repeated across overloads is one arm with one wrapper.
#[allow(dead_code)]
fn repeated_literal_compiles(repeats: &Repeats) {
    let _: String = repeats.tag_a();
    let _: String = repeats.tag_b();
    let _: String = repeats.tag_c();
}

// A generic class's statics taking an enum.
#[allow(dead_code)]
fn generic_class_arms_compile(obj: &Object) -> Result<(), JsValue> {
    let _: String = Boxed::label(AOrB::A);
    let _: Boxed<Object> = Boxed::of(AOrB::B, obj);
    let _: Boxed<Object> = Boxed::of(AOrB::A, obj);
    let _: Boxed = Boxed::new(AOrB::A)?;
    Ok(())
}

// A `null` arm's promise and `try_` forms, on a generic receiver and on a
// static returning its class; and a plain `null` arm beside an enum,
// whose private bindings don't collide.
#[allow(dead_code)]
async fn null_arm_forms_compile(
    picker: &Picker,
    store: &KVStore<JsString>,
    key: &JsString,
    clash: &Clash,
    buf: &ArrayBuffer,
    obj: &Object,
) -> Result<(), JsValue> {
    let _: String = picker.name_with_null(AOrB::A);
    let _: Result<String, JsValue> = picker.try_name_with_null(AOrB::B);
    let _: Option<String> = picker.load_with_null(AOrB::B).await?;
    store.put(key, AOrB::A, "v");
    store.put_with_null(key, AOrB::B);
    let _: Boxed<Object> = Boxed::from(AOrB::A, obj);
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
    pick_free(AOrB::A, obj);
    pick_free_with_null(AOrB::B);
    let _: Opener = Opener::new(AOrB::A, buf)?;
    let _: Opener = Opener::new_with_null(AOrB::B)?;
    Ok(())
}
