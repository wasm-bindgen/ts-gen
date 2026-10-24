//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use js_sys::Intl::{DateTimeFormatOptions, YearFormat};
use js_sys::JsString;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::intl_types_mono::*;

fn date_options() -> DateTimeFormatOptions {
    let options = DateTimeFormatOptions::new();
    options.set_time_zone("UTC");
    options.set_year(YearFormat::Numeric);
    options
}

const DATE_OPTIONS: &str = r#"Object{"timeZone":"UTC","year":"numeric"}"#;

#[wasm_bindgen_test]
fn a_single_locale_takes_any_string() {
    assert_eq!(describe_with_str("en-GB").unwrap(), "string(en-GB) none");
    assert_eq!(
        describe_with_str(String::from("fr")).unwrap(),
        "string(fr) none"
    );
    assert_eq!(
        describe_with_str_and_options(JsString::from("de"), &date_options()).unwrap(),
        format!("string(de) {DATE_OPTIONS}")
    );
}

#[wasm_bindgen_test]
fn a_locale_list_takes_js_strings() {
    let locales = [JsString::from("en-GB"), JsString::from("fr")];
    assert_eq!(
        describe_with_slice(&locales).unwrap(),
        "list(en-GB,fr) none"
    );
    let formatter = Formatter::new_with_slice_and_options(&locales, &date_options()).unwrap();
    assert_eq!(
        formatter.received(),
        format!("list(en-GB,fr) {DATE_OPTIONS}")
    );
    assert_eq!(
        formatter.locale("en-Latn-US").unwrap().base_name(),
        "en-Latn-US"
    );
}
