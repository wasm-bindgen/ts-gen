#![cfg(target_arch = "wasm32")]

use js_sys::Object;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

use ts_gen_integration_tests::record_alias::{
    AliasedKnownLabels, EvaluationContext, FlexibleRecord, KnownLabels, KnownValues, Labels,
    MixedValues, ReservedKeyLabels, SingleLabel, Values,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage for the generated record constructors and indexing setters.
#[allow(dead_code)]
fn record_alias_signatures_compile() -> Result<(), JsValue> {
    let context = EvaluationContext::new();
    let _: String = context.get_string("country");
    let _: f64 = context.get_number("age");
    let _: bool = context.get_bool("enabled");
    context.set_string("country", "US");
    context.set_number("age", 42.0);
    context.set_bool("enabled", true);

    let labels = Labels::new();
    labels.set("region", "us-east");

    let values = Values::<JsValue>::new();
    values.set("anything", JsValue::NULL);

    let mixed = MixedValues::new();
    mixed.set_string("one", "value");
    mixed.set_slice_of_string("many", &[String::from("value")]);

    let flexible = FlexibleRecord::new();
    let _: String = flexible.get_string_with_string("name");
    let _: bool = flexible.get_bool_with_number(1.0);
    flexible.set_bool_with_string("enabled", true);
    flexible.set_string_with_number(1.0, "first");

    let known_labels: KnownLabels = Object::new().unchecked_into();
    let _: String = known_labels.get_display_name();
    known_labels.set_region("us-east");

    let known_values: KnownValues = Object::new().unchecked_into();
    let _: String = known_values.get_string_with_name();
    let _: bool = known_values.get_bool_with_enabled();
    known_values.set_string_with_name("worker");
    known_values.set_bool_with_enabled(true);

    let single: SingleLabel = Object::new().unchecked_into();
    single.set_name("worker");
    let aliased: AliasedKnownLabels = Object::new().unchecked_into();
    aliased.set_primary("one");
    let reserved: ReservedKeyLabels = Object::new().unchecked_into();
    reserved.set_string_string(true);
    Ok(())
}

#[wasm_bindgen_test]
fn record_accessors_round_trip() {
    let labels = Labels::new();
    labels.set("region", "us-east");
    assert_eq!(labels.get("region"), "us-east");
    assert_eq!(labels.try_get("region").unwrap(), "us-east");

    let known: KnownLabels = Object::new().unchecked_into();
    known.set_display_name("worker");
    assert_eq!(known.get_display_name(), "worker");
}
