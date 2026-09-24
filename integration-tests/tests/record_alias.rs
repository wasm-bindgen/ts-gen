#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

use ts_gen_integration_tests::record_alias::{
    Colors, EvaluationContext, FlexibleRecord, KnownLabels, Labels, MixedValues, NumericLabels,
    OpenKeyFlags, Values,
};

// Records are plain JS objects, so the accessors run without a backing
// module. `evaluate` / `labelAll` have no JS implementation and are only
// type-checked through the record types they accept.
#[allow(dead_code)]
fn record_alias_signatures_compile() {
    let context = EvaluationContext::new();
    let _: String = context.get_string("country");
    let _: f64 = context.get_number("age");
    let _: bool = context.get_bool("enabled");
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
    assert_eq!(labels.get("region"), "us-east");
    assert_eq!(labels.try_get("region").unwrap(), "us-east");

    let numeric = NumericLabels::new();
    numeric.set(1.0, "one");
    assert_eq!(numeric.get(1.0), "one");

    let flexible = FlexibleRecord::new();
    let string_key = JsValue::from_str("name");
    let number_key = JsValue::from_f64(1.0);
    flexible.set_string(&string_key, "worker");
    flexible.set_bool(&number_key, true);
    assert_eq!(flexible.get_string(&string_key), "worker");
    assert!(flexible.get_bool(&number_key));

    let known = KnownLabels::new();
    known.set(&JsValue::from_str("region"), "us-east");
    assert_eq!(known.get(&JsValue::from_str("region")), "us-east");
}
