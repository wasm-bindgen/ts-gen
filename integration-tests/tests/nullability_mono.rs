//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{JsNullable, JsOption, JsString, Nullable, Number, Object, Promise};

use ts_gen_integration_tests::nullability_mono::*;

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage of each read carrier and each write expansion under
// per-monomorphization.
#[allow(dead_code)]
fn nullability_mono_signatures_compile(
    settings: &Settings,
    js_string: &JsString,
    registry_value: &Registry,
    holder_value: &Holder<Nullable<String>>,
) {
    let _: Option<Nullable<String>> = settings.both();
    let _: Option<Nullable<JsString>> = settings.both_js_string();
    settings.set_both("x");
    settings.set_both(js_string);
    settings.set_both_with_null();
    settings.set_explicit(String::from("x"));
    settings.set_required(js_string);
    settings.set_empties_with_null();
    settings.set_key("k");
    settings.set_key_with_f64(1.0);
    settings.set_key_with_null();
    let _: Settings = Settings::new(Some("r"), "x", None);
    let _: Settings = Settings::builder(None::<&str>, js_string, None)
        .key_with_f64(2.0)
        .key_with_null()
        .both_with_null()
        .build();

    optional_with_x("x");
    nullable(String::from("x"));
    nullable_with_null();
    let _: Option<Nullable<String>> = both_with_x(js_string);
    let _: Option<Nullable<JsString>> = both_js_string();
    let _: Option<Nullable<JsString>> = both_with_null_js_string();
    let _: Option<Nullable<String>> = opt_aliased_null_with_null();
    scalar();
    scalar_with_str("x");
    scalar_with_str(js_string);
    scalar_with_f64(1.0);
    scalar_with_null();
    scalar_null_with_f64(1.0);
    scalar_null("x");
    scalar_optional_with_str(js_string);
    scalar_optional_with_null();
    let _: Nullable<JsString> = generic(js_string.clone());
    let _: Nullable<f64> = generic(1.0);
    let _: Nullable<f64> = generic_with_null();
    let _: Option<Nullable<String>> = generic_both_with_x(String::from("x"));
    let _: Option<Nullable<String>> = generic_both_with_null();

    pair(js_string);
    pair_with_f64(1.0);
    leading(Some(js_string), 1.0);
    leading(None::<&str>, 1.0);
    chain_with_a_and_b(js_string, 1.0);
    chain_with_a_and_null(String::from("a"));

    let _: Upload = Upload::new_file("path");
    let _: Cover = Cover::new_image(js_string);
    let _: Cover = Cover::new_blank();
    let _: Attachment = Attachment::new_inline(js_string, "name");

    // Inner positions: a received `Promise` and a generic user type take the
    // carriers; `js_sys` containers and callbacks keep the `JsGeneric`
    // handles their `T: JsGeneric` APIs need.
    let _: Promise<Nullable<JsString>> = registry_value.pending();
    let _: Promise<Option<Number>> = registry_value.settled();
    let _: Nullable<String> = holder_value.get();
    let _: Holder<Option<f64>> = holder(holder_value);
    let _: Option<f64> = holder(holder_value).get();
    let _: Object<JsOption<Number>> = record(&Object::new_typed());
    let _: Nullable<String> = Nullable::Null;
    let _: MaybeStr = Nullable::Value(String::from("x"));
    let _: JsNullable<JsString> = JsNullable::new();
}

// The awaited value of an async binding, and of a received `Promise`, takes
// its read carrier.
#[allow(dead_code)]
async fn nullability_mono_async_compile(registry_value: &Registry) {
    let _: Result<Nullable<Registry>, _> = fetch_null().await;
    let _: Result<Option<f64>, _> = fetch_optional().await;
    let _: Result<Option<Nullable<String>>, _> = fetch_both().await;
    let _: Result<Nullable<f64>, _> = fetch_generic::<f64>().await;
    let _: Result<Nullable<JsString>, _> =
        wasm_bindgen_futures::JsFuture::from(registry_value.pending()).await;
}
