#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;

use ts_gen_integration_tests::record_alias::{EvaluationContext, Labels, MixedValues, Values};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage for the generated record constructors and indexing setters.
#[allow(dead_code)]
fn record_alias_signatures_compile() -> Result<(), JsValue> {
    let context = EvaluationContext::new();
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
    Ok(())
}
