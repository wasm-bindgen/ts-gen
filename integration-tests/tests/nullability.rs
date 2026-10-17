#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Nullable, Uint8Array};

use ts_gen_integration_tests::nullability::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage of each read carrier and each write expansion.
#[allow(dead_code)]
fn nullability_signatures_compile(settings: &Settings, registry_value: &Registry, body: &Body) {
    let _: Option<String> = settings.plain();
    let _: Nullable<String> = settings.nullable();
    let _: Option<Nullable<String>> = settings.both();
    let _: Option<Nullable<Mode>> = settings.mode();
    let _: Option<Nullable<f64>> = settings.limit();
    let _: Option<Nullable<Vec<f64>>> = settings.values();
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
    let _: Settings = Settings::new_with_null(None);
    let _: Settings = Settings::builder("x", None)
        .both_with_null()
        .limit(1.0)
        .owner_with_null()
        .key_with_f64(2.0)
        .build();

    let _: Option<Nullable<Registry>> = Registry::last();
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

    optional(None);
    nullable("x");
    nullable_with_null();
    let _: Option<Nullable<String>> = both(Some("x"));
    let _: Option<Nullable<String>> = both_with_null();
    optional_both();
    optional_both_with_x("x");
    optional_both_with_null();
    let _: Nullable<String> = aliased_null("x");
    let _: Option<Nullable<String>> = opt_aliased_null(None);
    let _: Option<Nullable<f64>> = count(Some(1.0));
    let _: Option<Nullable<f64>> = count_with_null();
    count_null(1.0);
    count_optional_with_null();
    let _: Nullable<bool> = flag(true);
    let _: Option<Nullable<Registry>> = registry(Some(registry_value));
    let _: Option<Nullable<Registry>> = registry_with_null();
    registry_null_with_null();
    registry_optional_with_null();
    scalar(Some("x"));
    scalar_with_f64(None);
    scalar_with_null();
    scalar_null_with_f64(1.0);
    scalar_optional_with_str("x");
    scalar_optional_with_null();
    let _: Option<Nullable<Mode>> = mode_both(Some(Mode::Slow));
    mode_optional_with_null();
    let _: Nullable<JsString> = generic(&JsString::from("x"));
    let _: Nullable<JsString> = generic_with_null();
    let _: Option<Nullable<JsString>> = generic_both(Some(JsString::from("x")));
    let _: Option<Nullable<JsString>> = generic_both_with_null();
    let _: Option<Nullable<Vec<f64>>> = numbers_both(Some(&[1.0]));
    let _: Option<Nullable<Vec<f64>>> = numbers_both_with_null();
    numbers_optional_with_xs(&[1.0]);
    numbers_optional_with_null();
    bytes(&[1]);
    bytes_with_null();
    bytes_both(Some(&[1]));
    bytes_both_with_uint8_array(Some(&Uint8Array::new_with_length(1)));
    bytes_both_with_null();
    jsdoc_with_null();

    let _: Upload = Upload::new_file("path");
    let _: Upload = Upload::builder_skip().path(None).build();
    let _: Cover = Cover::new_image("src");
    let _: Cover = Cover::new_blank();
}
