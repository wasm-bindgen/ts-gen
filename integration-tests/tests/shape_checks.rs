#![cfg(target_arch = "wasm32")]

use js_sys::Object;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::shape_checks::{shapes, Choice, GlobalInit, ModuleOptions};

// No JS constructor exists for an interface or object type, so `dyn_into`
// checks for an object rather than `instanceof`; the module-scoped ones
// would otherwise import a class `shape-checks` doesn't export.
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
    let built: JsValue = shapes::RoundingOptions::builder()
        .smallest_unit(shapes::Unit::Hour)
        .build()
        .into();
    let options = built.dyn_into::<shapes::RoundingOptions>().unwrap();
    assert_eq!(options.smallest_unit(), Some(shapes::Unit::Hour));
}

// A class keeps its `instanceof` check, and a constructor overload still
// calls the namespaced class without a `js_name`.
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
    assert_eq!(span.hours(), 2.0);
    assert_eq!(span.unit(), shapes::Unit::Minute);
    let rounded = span.round(
        &shapes::RoundingOptions::builder()
            .smallest_unit(shapes::Unit::Hour)
            .build(),
    );
    assert_eq!(rounded.unit(), shapes::Unit::Hour);
    span.handle().close();

    assert!(JsValue::from(rounded).dyn_into::<shapes::Span>().is_ok());
    assert!(JsValue::from(Object::new())
        .dyn_into::<shapes::Span>()
        .is_err());
}
