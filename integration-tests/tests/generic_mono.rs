#![cfg(target_arch = "wasm32")]

use js_sys::{Array, ArrayTuple, Function, JsString, Map, Uint8Array};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::generic_mono::{
    identity, parse_string, parse_string_js_string, roundtrip_boolean, roundtrip_number,
    try_parse_string, try_parse_string_js_string, EvaluationDetails, Flags, Holder, Pair,
    TypedArrayOptions,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// signature coverage rather than runtime `wasm_bindgen_test` cases.
#[allow(dead_code)]
async fn generic_mono_signatures_compile(flags: &Flags, js_string: &JsString) {
    let holder: Holder<f64> = Holder::new(1.0).unwrap();
    let _: f64 = holder.get();
    holder.set(2.0);
    let holder: Holder<String> = Holder::new(String::from("value")).unwrap();
    let _: String = holder.get();

    let _: f64 = identity(1.0);
    let _: JsString = identity(js_string.clone());
    let _: JsValue = identity(JsValue::NULL);
    let _: Pair<f64, bool> = Pair::new(1.0, true);

    let _: Result<String, JsValue> = flags.get_string_value("borrowed").await;
    let _: Result<String, JsValue> = flags.get_string_value(String::from("owned")).await;
    let _: Result<String, JsValue> = flags.get_string_value(js_string).await;
    let _: Result<String, JsValue> = flags.get_string_value(js_string.clone()).await;

    let _: Result<JsString, JsValue> = flags.get_string_value_js_string("default").await;
    let _: Result<EvaluationDetails<String>, JsValue> = flags.get_string_details("default").await;
    let _: Result<EvaluationDetails<JsString>, JsValue> =
        flags.get_string_details_js_string("default").await;
    let _: Result<EvaluationDetails<bool>, JsValue> = flags.get_boolean_details(false).await;
    let _: Result<EvaluationDetails<f64>, JsValue> = flags.get_number_details(0.0).await;
    let _: Result<Option<String>, JsValue> = flags.get_nullable_string("default").await;
    let _: Result<Option<JsString>, JsValue> = flags.get_nullable_string_js_string("default").await;
    let _: Result<Pair<String, String>, JsValue> = flags.get_string_pair("default").await;
    let _: Result<Pair<JsString, JsString>, JsValue> =
        flags.get_string_pair_js_string("default").await;

    let _: Result<Array<JsString>, JsValue> = flags.get_string_array().await;
    let _: Result<Map<JsString, JsString>, JsValue> = flags.get_string_map().await;
    let _: Result<ArrayTuple<(JsString, JsString)>, JsValue> = flags.get_string_tuple().await;
    let _: Result<Function<fn(JsString) -> JsString>, JsValue> = flags.get_string_callback().await;
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

    let _ = EvaluationDetails::<bool>::new("flag", false);
    let _ = EvaluationDetails::<bool>::new(String::from("flag"), false);
    let _ = EvaluationDetails::<bool>::new(js_string, false);
    let _ = EvaluationDetails::<bool>::new(js_string.clone(), false);

    let bytes = Uint8Array::new_with_length(1);
    let _ = TypedArrayOptions::new(&bytes);
}
