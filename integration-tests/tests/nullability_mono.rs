//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Nullable};

use ts_gen_integration_tests::nullability_mono::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage of each read carrier and each write expansion under
// per-monomorphization.
#[allow(dead_code)]
fn nullability_mono_signatures_compile(settings: &Settings, js_string: &JsString) {
    let _: Option<Nullable<String>> = settings.both();
    let _: Option<Nullable<JsString>> = settings.both_js_string();
    settings.set_both("x");
    settings.set_both(js_string);
    settings.set_both_with_null();
    settings.set_explicit(Some(String::from("x")));
    settings.set_key("k");
    settings.set_key(1.0);
    settings.set_key_with_null();
    let _: Settings = Settings::builder(js_string, None)
        .key(2.0)
        .key_with_null()
        .both_with_null()
        .build();

    optional(Some("x"));
    nullable(String::from("x"));
    nullable_with_null();
    let _: Option<Nullable<String>> = both(Some(js_string));
    let _: Option<Nullable<JsString>> = both_js_string(None::<&str>);
    let _: Option<Nullable<JsString>> = both_with_null_js_string();
    let _: Option<Nullable<String>> = opt_aliased_null_with_null();
    scalar(Some("x"));
    scalar(Some(1.0));
    scalar(None::<f64>);
    scalar_with_null();
    scalar_null(1.0);
    scalar_null("x");
    scalar_optional_with_x(js_string);
    scalar_optional_with_null();
    let _: Nullable<JsString> = generic(js_string.clone());
    let _: Nullable<f64> = generic(1.0);
    let _: Nullable<f64> = generic_with_null();
    let _: Option<Nullable<String>> = generic_both(Some(String::from("x")));
    let _: Option<Nullable<String>> = generic_both_with_null();

    let _: Upload = Upload::new_file("path");
    let _: Cover = Cover::new_image(js_string);
    let _: Cover = Cover::new_blank();
    let _: Attachment = Attachment::new_inline(js_string, "name");
}
