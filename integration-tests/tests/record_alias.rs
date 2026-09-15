#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

use ts_gen_integration_tests::record_alias::{
    AliasedKnownLabels, EvaluationContext, FlexibleRecord, GenericKnown, KnownLabels,
    KnownLiteralValues, KnownValues, Labels, MixedValues, NumericLabels, ReservedKeyLabels,
    SingleLabel, Values,
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
    let string_key = JsValue::from_str("name");
    let number_key = JsValue::from_f64(1.0);
    let _: String = flexible.get_string(&string_key);
    let _: bool = flexible.get_bool(&number_key);
    flexible.set_bool(&string_key, true);
    flexible.set_string(&number_key, "first");

    let known_labels = KnownLabels::new("worker", "us-east");
    let _: String = known_labels.get_display_name();
    known_labels.set_region("us-east");

    let name = JsValue::from_str("worker");
    let enabled = JsValue::from_bool(true);
    let known_values = KnownValues::new(&name, &enabled);
    let _: String = known_values.get_string_with_name();
    let _: bool = known_values.get_bool_with_enabled();
    known_values.set_string_with_name("worker");
    known_values.set_bool_with_enabled(true);

    let literal_values = KnownLiteralValues::new("red", "blue");
    let _: String = literal_values.get_string_with_primary();

    let single = SingleLabel::new("worker");
    single.set_name("worker");
    let aliased = AliasedKnownLabels::new("one", "two");
    aliased.set_primary("one");
    let reserved = ReservedKeyLabels::new(true, false);
    reserved.set_string_string(true);
    let numeric = NumericLabels::new("one", "two");
    let _: String = numeric.get_number(1.0);
    let _: GenericKnown<JsValue> = GenericKnown::new(JsValue::NULL, JsValue::UNDEFINED);
    Ok(())
}

#[wasm_bindgen_test]
fn record_accessors_round_trip() {
    let labels = Labels::new();
    labels.set("region", "us-east");
    assert_eq!(labels.get("region"), "us-east");
    assert_eq!(labels.try_get("region").unwrap(), "us-east");

    let flexible = FlexibleRecord::new();
    let string_key = JsValue::from_str("name");
    let number_key = JsValue::from_f64(1.0);
    flexible.set_string(&string_key, "worker");
    flexible.set_bool(&number_key, true);
    assert_eq!(flexible.get_string(&string_key), "worker");
    assert!(flexible.get_bool(&number_key));

    let known = KnownLabels::new("worker", "us-east");
    assert_eq!(known.get_display_name(), "worker");
    assert_eq!(known.get_region(), "us-east");

    let name = JsValue::from_str("worker");
    let enabled = JsValue::from_bool(true);
    let known_values = KnownValues::new(&name, &enabled);
    assert_eq!(known_values.get_string_with_name(), "worker");
    assert!(known_values.get_bool_with_enabled());

    let numeric = NumericLabels::new("one", "two");
    assert_eq!(numeric.get_number(1.0), "one");
    assert_eq!(numeric.get_number(2.0), "two");

    let aliased = AliasedKnownLabels::new("one", "two");
    assert_eq!(aliased.get_primary(), "one");
    assert_eq!(aliased.get_secondary(), "two");

    let reserved = ReservedKeyLabels::new(true, false);
    assert!(reserved.get_string_string());
    assert!(!reserved.get_string_number());

    let generic: GenericKnown<JsValue> =
        GenericKnown::new(JsValue::from_f64(1.0), JsValue::from_f64(2.0));
    assert_eq!(generic.get_first().as_f64(), Some(1.0));
    assert_eq!(generic.get_second().as_f64(), Some(2.0));
}
