#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;

use ts_gen_integration_tests::record_alias::{
    EvaluationContext, FlexibleRecord, KnownLabels, KnownValues, Labels, MixedValues, Values,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage for the generated record constructors and indexing setters.
#[allow(dead_code)]
fn record_alias_signatures_compile() -> Result<(), JsValue> {
    let context = EvaluationContext::new();
    let _: String = context.get_string("country")?;
    let _: f64 = context.get_number("age")?;
    let _: bool = context.get_bool("enabled")?;
    context.set_string("country", "US")?;
    context.set_number("age", 42.0)?;
    context.set_bool("enabled", true)?;

    let labels = Labels::new();
    labels.set("region", "us-east")?;

    let values = Values::<JsValue>::new();
    values.set("anything", JsValue::NULL)?;

    let mixed = MixedValues::new();
    mixed.set_string("one", "value")?;
    mixed.set_slice("many", &[String::from("value")])?;

    let flexible = FlexibleRecord::new();
    let _: String = flexible.get_string_as_string("name")?;
    let _: bool = flexible.get_number_as_bool(1.0)?;
    flexible.set_string_with_bool("enabled", true)?;
    flexible.set_number_with_string(1.0, "first")?;

    let known_labels = KnownLabels::new();
    let _: String = known_labels.get_display_name()?;
    known_labels.set_region("us-east")?;

    let known_values = KnownValues::new();
    let _: String = known_values.get_name_as_string()?;
    let _: bool = known_values.get_enabled_as_bool()?;
    known_values.set_name_with_string("worker")?;
    known_values.set_enabled_with_bool(true)?;
    Ok(())
}
