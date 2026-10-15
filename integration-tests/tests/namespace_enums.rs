#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::namespace_enums::{
    timing, DurationUnit, Precision, TimingDurationUnit,
};

// Same-named enums at the top level and inside a namespace only clash when
// wasm-bindgen processes the linked module, so this has to run, not just
// compile.
#[wasm_bindgen_test]
fn namespace_enums_coexist() {
    assert_eq!(
        wasm_bindgen::JsValue::from(DurationUnit::Minute).as_string(),
        Some("minute".to_string())
    );
    assert_eq!(
        wasm_bindgen::JsValue::from(timing::DurationUnit::Hour).as_string(),
        Some("hour".to_string())
    );
    assert_eq!(
        wasm_bindgen::JsValue::from(TimingDurationUnit::Week).as_string(),
        Some("week".to_string())
    );
    assert_ne!(Precision::High as u32, timing::Precision::Coarse as u32);
}
