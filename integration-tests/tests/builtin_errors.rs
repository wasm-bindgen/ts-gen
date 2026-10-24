#![cfg(target_arch = "wasm32")]

use js_sys::Intl::{DateTimeFormatOptions, NumberFormatOptions, NumberFormatStyle, YearFormat};
use js_sys::{AggregateError, Error, JsString, Object, UriError};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::builtin_errors::*;

// The JavaScript module has no `Codec`, so this is compile-only coverage
// that js_sys error classes resolve as `@throws` types.
#[allow(dead_code)]
async fn builtin_errors_compile(codec: &Codec) {
    let _: Result<String, UriError> = codec.try_decode("%");
    let _: Result<String, AggregateError> = codec.try_decode_all("%");
    let _: Result<String, Error> = codec.parse("%").await;
}

fn date_options() -> DateTimeFormatOptions {
    let options = DateTimeFormatOptions::new();
    options.set_time_zone("UTC");
    options.set_year(YearFormat::Numeric);
    options
}

const DATE_OPTIONS: &str = r#"Object{"timeZone":"UTC","year":"numeric"}"#;

#[wasm_bindgen_test]
fn functions_take_a_locale_or_a_list() {
    assert_eq!(describe(), "none none");
    assert_eq!(describe_with_str("en-GB"), "string(en-GB) none");
    let locales = [JsString::from("en-GB"), JsString::from("fr")];
    assert_eq!(describe_with_slice(&locales), "list(en-GB,fr) none");
    assert_eq!(
        describe_with_str_and_options("de", &date_options()),
        format!("string(de) {DATE_OPTIONS}")
    );
    assert_eq!(
        describe_with_slice_and_options(&locales, &date_options()),
        format!("list(en-GB,fr) {DATE_OPTIONS}")
    );
}

#[wasm_bindgen_test]
fn constructors_take_intl_options() {
    assert_eq!(Formatter::new().unwrap().received(), "none none");
    assert_eq!(
        Formatter::new_with_str("ja").unwrap().received(),
        "string(ja) none"
    );
    let formatter =
        Formatter::new_with_slice_and_options(&[JsString::from("es")], &date_options()).unwrap();
    assert_eq!(formatter.received(), format!("list(es) {DATE_OPTIONS}"));
}

#[wasm_bindgen_test]
fn methods_take_intl_options_and_return_intl_types() {
    let formatter = Formatter::new().unwrap();
    let options = NumberFormatOptions::new();
    options.set_style(NumberFormatStyle::Currency);
    options.set_currency("EUR");
    // `--external Intl.NumberFormatOptions=::js_sys::Object` beats the
    // `::js_sys::Intl` default.
    let mapped: &Object = &options;
    assert_eq!(
        formatter.format_with_str_and_options("fr", mapped),
        r#"string(fr) Object{"style":"currency","currency":"EUR"}"#
    );
    assert_eq!(
        formatter.format_with_slice(&[JsString::from("it")]),
        "list(it) none"
    );
    let locale: js_sys::Intl::Locale = formatter.locale("en-Latn-US");
    assert_eq!(locale.base_name(), "en-Latn-US");
}

#[wasm_bindgen_test]
fn dictionaries_set_locales_either_way() {
    let single = FormatSettings::builder()
        .locales("pt")
        .date_time(&date_options())
        .build();
    assert_eq!(settings(&single), format!("string(pt) {DATE_OPTIONS}"));
    let list = FormatSettings::builder()
        .locales_with_slice(&[JsString::from("pt"), JsString::from("en")])
        .build();
    assert_eq!(settings(&list), "list(pt,en) none");
}
