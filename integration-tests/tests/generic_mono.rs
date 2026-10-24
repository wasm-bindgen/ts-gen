//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{Array, ArrayTuple, Function, JsString, Map, Uint8Array};
use wasm_bindgen::convert::{FromWasmAbi, RefIntoWasmAbi};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::generic_mono::{
    identity, parse_string, parse_string_js_string, roundtrip_boolean, roundtrip_number,
    try_parse_string, try_parse_string_js_string, EvaluationDetails, Flags, Holder, Pair, Store,
    TypedArrayOptions,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// signature coverage rather than runtime `wasm_bindgen_test` cases.
#[allow(dead_code)]
async fn generic_mono_signatures_compile(flags: &Flags, js_string: &JsString) {
    let holder: Holder<f64> = Holder::new(&1.0).unwrap();
    let _: f64 = holder.get();
    holder.set(&2.0);
    let holder: Holder<String> = Holder::new(&String::from("value")).unwrap();
    let _: String = holder.get();

    let _: f64 = identity(&1.0);
    let _: JsString = identity(js_string);
    let _: JsValue = identity(&JsValue::NULL);

    let _: Result<String, JsValue> = flags.get_string_value("borrowed").await;
    let _: Result<String, JsValue> = flags.get_string_value(String::from("owned")).await;
    let _: Result<String, JsValue> = flags.get_string_value(js_string).await;
    let _: Result<String, JsValue> = flags.get_string_value(js_string.clone()).await;

    let _: Result<JsString, JsValue> = flags.get_string_value_js_string("default").await;
    let _: Result<EvaluationDetails<String>, JsValue> = flags.get_string_details("default").await;
    let _: Result<EvaluationDetails<JsString>, JsValue> =
        flags.get_string_details_js_string("default").await;
    let _: Result<EvaluationDetails<f64>, JsValue> = flags.get_number_details(0.0).await;
    let _: Result<Option<String>, JsValue> = flags.get_nullable_string("default").await;
    let _: Result<Option<JsString>, JsValue> = flags.get_nullable_string_js_string("default").await;
    let _: Result<Pair<JsString, JsString>, JsValue> =
        flags.get_string_pair_js_string("default").await;

    // `EvaluationDetails` and `Pair` are only ever read, so they have getters
    // and no factory.
    let details: EvaluationDetails<bool> = flags.get_boolean_details(false).await.unwrap();
    let _: String = details.flag_key();
    let _: JsString = details.flag_key_js_string();
    let _: bool = details.value();
    let _: Option<String> = details.variant();
    let _: Option<JsString> = details.variant_js_string();
    let pair: Pair<String, String> = flags.get_string_pair("default").await.unwrap();
    let _: String = pair.first();
    let _: String = pair.second();

    let _: Result<Vec<String>, JsValue> = flags.get_string_array().await;
    let _: Result<Array<JsString>, JsValue> = flags.get_string_array_js_string().await;
    let _: Result<Map<String, String>, JsValue> = flags.get_string_map().await;
    let _: Result<ArrayTuple<(String, String)>, JsValue> = flags.get_string_tuple().await;
    let _: Result<Function<fn(String) -> String>, JsValue> = flags.get_string_callback().await;
    let _: bool = flags.compare_strings("left", js_string.clone());

    let _: String = parse_string("borrowed");
    let _: String = parse_string(String::from("owned"));
    let _: String = parse_string(js_string);
    let _: String = parse_string(js_string.clone());
    let _: JsString = parse_string_js_string("borrowed");
    let _: JsString = parse_string_js_string(String::from("owned"));
    let _: JsString = parse_string_js_string(js_string);
    let _: JsString = parse_string_js_string(js_string.clone());
    let _: Result<String, js_sys::TypeError> = try_parse_string("fallible");
    let _: Result<JsString, js_sys::TypeError> = try_parse_string_js_string("fallible");

    let _: bool = roundtrip_boolean(false);
    let _: f64 = roundtrip_number(0.0);

    let bytes = Uint8Array::new_with_length(1);
    let _ = TypedArrayOptions::new(&bytes);
    let _ = TypedArrayOptions::builder(&bytes).extra(&bytes).build();
}

// A generic caller needs exactly the bounds the declarations state.
#[allow(dead_code)]
async fn generic_mono_bounds_are_declared<T>(holder: &Holder<T>, store: &Store<T>, value: &T)
where
    T: FromWasmAbi + RefIntoWasmAbi + 'static,
    Option<T>: FromWasmAbi,
{
    holder.set(value);
    let _: T = holder.get();
    let _: Option<T> = store.find("key");
    let _: Result<T, JsValue> = store.load().await;
    let _: Result<Holder<T>, JsValue> = store.load_holder().await;
}
