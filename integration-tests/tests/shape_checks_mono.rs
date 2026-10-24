//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::Object;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::shape_checks_mono::{shapes, Choice, GlobalInit, ModuleOptions};

// As in `shape_checks.rs`, under per-monomorphization.
#[wasm_bindgen_test]
fn shapes_accept_any_object() {
    let plain = || JsValue::from(Object::new());
    assert!(plain().dyn_into::<shapes::RoundingOptions>().is_ok());
    assert!(plain().dyn_into::<shapes::Handle>().is_ok());
    assert!(plain().dyn_into::<GlobalInit>().is_ok());
    assert!(plain().dyn_into::<ModuleOptions>().is_ok());
    assert!(plain().dyn_into::<Choice>().is_ok());
    for value in [
        JsValue::from(1),
        JsValue::from("x"),
        JsValue::NULL,
        JsValue::UNDEFINED,
    ] {
        assert!(value.dyn_into::<shapes::RoundingOptions>().is_err());
    }
}

#[wasm_bindgen_test]
fn classes_keep_instanceof() {
    js_sys::eval(
        "globalThis.Shapes = { Span: class {
            constructor(hours, unit) { this.hours = hours; this.unit = unit ?? 'hour'; }
            round(options) { return new globalThis.Shapes.Span(this.hours, options.smallestUnit); }
            handle() { return { close() {} }; }
        } };",
    )
    .unwrap();

    let span = shapes::Span::new_with_unit(2.0, shapes::Unit::Minute).unwrap();
    assert_eq!(span.unit(), shapes::Unit::Minute);
    let options = shapes::RoundingOptions::builder()
        .smallest_unit(shapes::Unit::Hour)
        .build();
    let rounded = span.round(&options);
    assert_eq!(rounded.unit(), shapes::Unit::Hour);

    assert!(JsValue::from(rounded).dyn_into::<shapes::Span>().is_ok());
    assert!(JsValue::from(Object::new())
        .dyn_into::<shapes::Span>()
        .is_err());
}
