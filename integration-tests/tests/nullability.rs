#![cfg(target_arch = "wasm32")]

use js_sys::{JsNullable, JsOption, JsString, Number, Object, Promise, Uint8Array};
use wasm_bindgen::JsCast;

use ts_gen_integration_tests::nullability::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage of each read carrier (`Option<T>` whichever empties a read admits)
// and each write expansion, which keeps `null` as an arm of its own.
#[allow(dead_code)]
fn nullability_signatures_compile(
    settings: &Settings,
    registry_value: &Registry,
    body: &Body,
    holder_value: &Holder<JsNullable<JsString>>,
) {
    let _: Option<String> = settings.plain();
    let _: Option<String> = settings.nullable();
    let _: Option<String> = settings.both();
    let _: Option<Mode> = settings.mode();
    let _: Option<f64> = settings.limit();
    let _: Option<Vec<f64>> = settings.values();
    settings.set_both("x");
    settings.set_both_with_null();
    settings.set_mode(Mode::Fast);
    settings.set_mode_with_null();
    settings.set_limit(1.0);
    settings.set_limit_with_null();
    settings.set_owner(registry_value);
    settings.set_owner_with_null();
    settings.set_values(&[1.0]);
    settings.set_values_with_null();
    settings.set_key("k");
    settings.set_key_with_f64(1.0);
    settings.set_key_with_null();
    settings.set_explicit("x");
    settings.set_required("x");
    settings.set_empties_with_null();
    let _: Option<String> = settings.required();
    let _: Settings = Settings::new_with_null(None, None);
    let _: Settings = Settings::builder(Some("r"), "x", None)
        .explicit("e")
        .both_with_null()
        .limit(1.0)
        .owner_with_null()
        .key_with_f64(2.0)
        .build();

    let _: Option<Registry> = Registry::last();
    let _: Option<Registry> = Registry::current();
    let _: Promise<JsNullable<JsString>> = registry_value.pending();
    let _: Promise<JsOption<Number>> = registry_value.settled();
    Registry::set_fallback(registry_value);
    Registry::set_fallback_with_null();

    let _: Result<Body, _> = Body::new("x");
    let _: Result<Body, _> = Body::new_with_body_null();
    let _: Result<Body, _> = Body::new_with_length("x", 1.0);
    let _: Result<Body, _> = Body::new_with_length_null("x");
    let _: Result<Body, _> = Body::new_with_body_null_and_length(1.0);
    let _: Result<Body, _> = Body::new_with_body_null_and_length_null();
    body.put("x");
    body.put_with_null();
    let _: Body = Body::from(registry_value);
    let _: Body = Body::from_with_null();

    optional();
    optional_with_x("x");
    nullable("x");
    nullable_with_null();
    let _: Option<String> = both();
    let _: Option<String> = both_with_x("x");
    let _: Option<String> = both_with_null();
    optional_both();
    optional_both_with_x("x");
    optional_both_with_null();
    let _: Option<String> = aliased_null("x");
    let _: Option<String> = opt_aliased_null();
    let _: Option<f64> = count_with_x(1.0);
    let _: Option<f64> = count_with_null();
    count_null(1.0);
    count_optional_with_null();
    let _: Option<bool> = flag(true);
    let _: Option<Registry> = registry_with_r(registry_value);
    let _: Option<Registry> = registry_with_null();
    registry_null_with_null();
    registry_optional_with_null();
    scalar();
    scalar_with_str("x");
    scalar_with_f64(1.0);
    scalar_with_null();
    scalar_null_with_f64(1.0);
    scalar_optional_with_str("x");
    scalar_optional_with_null();
    let _: Option<Mode> = mode_both_with_m(Mode::Slow);
    mode_optional_with_null();
    let _: Option<JsString> = generic(&JsString::from("x"));
    let _: Option<JsString> = generic_with_null();
    let _: Option<JsString> = generic_both_with_x(&JsString::from("x"));
    let _: Option<JsString> = generic_both_with_null();
    let _: Option<Vec<f64>> = numbers_both_with_xs(&[1.0]);
    let _: Option<Vec<f64>> = numbers_both_with_null();
    numbers_optional_with_xs(&[1.0]);
    numbers_optional_with_null();
    bytes(&[1]);
    bytes_with_null();
    bytes_both_with_slice(&[1]);
    bytes_both_with_uint8_array(&Uint8Array::new_with_length(1));
    bytes_both_with_null();
    jsdoc_with_null();
    pair("x");
    pair_with_f64(1.0);
    leading(Some("a"), 1.0);
    leading(None, 1.0);
    leading_null_with_null(1.0);
    trailing(1.0);
    trailing_with_b(1.0, "b");
    chain();
    chain_with_a("a");
    chain_with_a_and_b("a", 1.0);
    chain_with_a_and_null("a");
    chain_optional_with_a_and_b("a", 1.0);
    chain_optional_with_a_and_null("a");
    chain_three();
    chain_three_with_a_and_null_and_c("a", true);
    chain_three_with_a_and_b_and_c("a", 1.0, false);
    let _: Object<JsOption<Number>> = record(&Object::new().unchecked_into());

    let _: Upload = Upload::new_file("path");
    let _: Upload = Upload::builder_skip().path("p").build();
    let _: Cover = Cover::new_image("src");
    let _: Cover = Cover::new_blank();
    let _: Option<JsNullable<JsString>> = holder_value.value();
    let _: Holder<JsOption<Number>> = holder(holder_value);
}

// The awaited value of an async binding takes its read carrier around
// the sync return form; the `_promise` form's is an inner position.
#[allow(dead_code)]
async fn nullability_async_compile() {
    let _: Result<Option<Registry>, _> = fetch_null().await;
    let _: Result<Option<f64>, _> = fetch_optional().await;
    let _: Result<Option<String>, _> = fetch_both().await;
    let _: Result<Option<JsString>, _> = fetch_generic::<JsString>().await;
    let _: Result<JsNullable<Registry>, _> = fetch_null_promise().await;
    let _: Result<JsOption<Number>, _> = fetch_optional_promise().await;
    let _: Result<JsNullable<JsString>, _> = fetch_both_promise().await;
}
