//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{ArrayBuffer, JsNullable, JsString, Map, Object, Promise, Uint8Array};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::literal_overloads_mono::{
    pick_free, pick_free_with_null, store, Boxed, BoxedKind, Clash, ClashFKind, ClashKind,
    GetOptions, Headers, KVNamespace, KVNamespaceKind, KVStore, KVStoreKind, Opener, OpenerKind,
    PickFreeKind, Picker, PickerKind, Repeats, StoreReader, SubtleCrypto,
};

// Compile-only coverage that under
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
    let _: Promise<Option<JsString>> = kv.get_text_promise("k");
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
    let _: Option<Object> = store.get_json_typed::<Object>(JsString::from("k")).await?;
    Ok(())
}

// An enum beside a `null` arm under mono: the extern's `T` and
// `impl JsStringLike` parameters stay on the typed arms, and the `null`
// arms take neither.
#[allow(dead_code)]
fn null_arms_compile(picker: &Picker, buf: &ArrayBuffer) -> Result<(), JsValue> {
    picker.pick(PickerKind::A, buf);
    picker.pick_with_null(PickerKind::B);
    picker.try_pick_with_null(PickerKind::A)?;
    let _: Picker = Picker::make(PickerKind::A, "x");
    let _: Picker = Picker::make(PickerKind::A, JsString::from("x"));
    let _: Picker = Picker::make_with_null(PickerKind::B);
    picker.put(PickerKind::A, JsString::from("x"));
    picker.put_with_null(PickerKind::B);
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
    let _: String = Boxed::label(BoxedKind::A);
    let _: JsString = Boxed::label_js_string(BoxedKind::B);
    let _: Boxed<Object> = Boxed::of(BoxedKind::B, obj.clone());
    let _: Boxed<Object> = Boxed::of(BoxedKind::A, obj.clone());
    let _: Boxed = Boxed::new(BoxedKind::A)?;
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
    let _: String = picker.name_with_null(PickerKind::A);
    let _: JsString = picker.name_with_null_js_string(PickerKind::B);
    let _: Option<String> = picker.load_with_null(PickerKind::B).await?;
    let _: Option<JsString> = picker.load_with_null_js_string(PickerKind::A).await?;
    store.put(JsString::from("k"), KVStoreKind::A, "v");
    store.put_with_null(JsString::from("k"), KVStoreKind::B);
    let _: Boxed<Object> = Boxed::from(BoxedKind::A, obj.clone());
    let _: Boxed<Object> = Boxed::from_with_null(BoxedKind::B);
    clash.mode_with_null();
    clash.mode(ClashKind::X);
    clash.f(buf);
    clash.f_with_x_null();
    clash.f_with_kind_and_x(ClashFKind::A, buf);
    clash.f_with_kind_and_x_null(ClashFKind::B);
    Ok(())
}

#[allow(dead_code)]
fn free_and_constructor_null_arms_compile(buf: &ArrayBuffer, obj: &Object) -> Result<(), JsValue> {
    pick_free(PickFreeKind::A, obj.clone());
    pick_free_with_null(PickFreeKind::B);
    let _: Opener = Opener::new(OpenerKind::A, buf)?;
    let _: Opener = Opener::new_with_null(OpenerKind::B)?;
    Ok(())
}

// A literal a parameter's type argument fixes is set by the binding, on a
// copy of the caller's options; one a field's own type fixes, by the
// dictionary's factory.
#[wasm_bindgen_test]
async fn literal_fields_are_filled_by_the_binding() {
    let store = store();
    let options = GetOptions::new();
    options.set_ttl(60.0);
    assert_eq!(store.read("k").await.unwrap().unwrap(), "k");
    let text = store.read_text_with_options("k", &options).await.unwrap();
    assert_eq!(text.unwrap(), "k:ttl=60,type=text");
    let json = store.read_json_with_options("k", &options).await.unwrap();
    assert_eq!(json.as_string().unwrap(), "k:ttl=60,type=json");
    let bytes = store
        .read_array_buffer_with_options("k", &options)
        .await
        .unwrap();
    let bytes = bytes.unwrap().byte_length() as usize;
    assert_eq!(bytes, "k:ttl=60,type=arrayBuffer".len());
    assert_eq!(store.tail(&options), "ttl=60,type=lines");
    // The caller's own value doesn't reach the call, and stays on its options.
    options.set_type(JsValue::from_str("json"));
    let text = store.read_text_with_options("k", &options).await.unwrap();
    assert_eq!(text.unwrap(), "k:ttl=60,type=text");
    assert_eq!(options.type_(), Some(JsValue::from_str("json")));
    assert_eq!(store.open(&StoreReader::new()), "mode=byob");
    // A dictionary of constants alone is the call's to make.
    assert_eq!(store.get_reader(), "default");
    assert_eq!(store.get_reader_byob(), "mode=byob".len() as f64);
    assert_eq!(store.lock(), "mode=exclusive");
}
