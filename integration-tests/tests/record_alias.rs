#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

use ts_gen_integration_tests::record_alias::{
    AutoKeyed, Colors, EvaluationContext, FlexibleRecord, KnownLabels, Labels, MixedValues,
    NumericLabels, OpenKeyFlags, Values,
};

// Records are plain JS objects, so the accessors run without a backing
// module. `evaluate` / `labelAll` have no JS implementation and are only
// type-checked through the record types they accept.
#[allow(dead_code)]
fn record_alias_signatures_compile() {
    let context = EvaluationContext::new();
    let _: JsValue = context.get("country");
    let _: Option<String> = context.get_string("country");
    let _: Option<f64> = context.get_number("age");
    let _: Option<bool> = context.get_bool("enabled");
    context.set_string("country", "US");
    context.set_number("age", 42.0);
    context.set_bool("enabled", true);

    let values = Values::<JsValue>::new();
    values.set("anything", JsValue::NULL);
    let numbers = Values::<f64>::new();
    numbers.set("one", 1.0);

    let mixed = MixedValues::new();
    mixed.set_string("one", "value");
    mixed.set_slice_of_string("many", &[String::from("value")]);

    let colors = Colors::default();
    colors.set("primary", "red");

    let flags = OpenKeyFlags::new();
    flags.set(&JsValue::from_f64(1.0), true);
}

#[wasm_bindgen_test]
fn record_accessors_round_trip() {
    let labels = Labels::new();
    labels.set("region", "us-east");
    assert_eq!(labels.get("region").as_deref(), Some("us-east"));

    let numeric = NumericLabels::new();
    numeric.set(1.0, "one");
    assert_eq!(numeric.get(1.0).as_deref(), Some("one"));

    let flexible = FlexibleRecord::new();
    let string_key = JsValue::from_str("name");
    let number_key = JsValue::from_f64(1.0);
    flexible.set_string(&string_key, "worker");
    flexible.set_bool(&number_key, true);
    assert_eq!(flexible.get_string(&string_key).as_deref(), Some("worker"));
    assert_eq!(flexible.get_bool(&number_key), Some(true));
    assert_eq!(
        flexible.get(&string_key).as_string().as_deref(),
        Some("worker")
    );

    let known = KnownLabels::new();
    known.set("region", "us-east");
    assert_eq!(known.get("region").as_deref(), Some("us-east"));

    let auto = AutoKeyed::new();
    auto.set("auto", 1.0);
    assert_eq!(auto.get("auto"), Some(1.0));
}

#[wasm_bindgen_test]
fn missing_keys_read_as_none() {
    assert_eq!(Labels::new().get("missing"), None);
    assert_eq!(NumericLabels::new().get(1.0), None);

    let context = EvaluationContext::new();
    assert!(context.get("missing").is_undefined());
    assert_eq!(context.get_string("missing"), None);
    assert_eq!(context.get_number("missing"), None);
    assert_eq!(context.get_bool("missing"), None);
}
