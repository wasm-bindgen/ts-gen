#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::namespace_nested::{other, outer};

// `Outer.Inner.Kind` and `Other.Inner.Kind` only differ by their outer
// segment, so they only coexist when the linked module keeps the full path.
#[wasm_bindgen_test]
fn nested_enums_keep_their_own_values() {
    assert_eq!(
        JsValue::from(outer::inner::Kind::A).as_string(),
        Some("a".to_string())
    );
    assert_eq!(
        JsValue::from(other::inner::Kind::X).as_string(),
        Some("x".to_string())
    );
    assert_eq!(outer::inner::Level::One as u32, 1);
}

// The fixture has no backing module, so the global namespace is installed
// before the bindings first read it.
#[wasm_bindgen_test]
fn nested_imports_bind_to_the_full_path() {
    js_sys::eval(
        "globalThis.Outer = { Inner: {
            run() { globalThis.Outer.Inner.ran = true; },
            LIMIT: 7,
            Thing: class { static make() { return new this(); } },
        } };",
    )
    .unwrap();

    outer::inner::run();
    let ran = js_sys::eval("globalThis.Outer.Inner.ran").unwrap();
    assert_eq!(ran.as_bool(), Some(true));
    assert_eq!(outer::inner::limit.with(|v| *v), 7.0);
    let _: outer::inner::Thing = outer::inner::Thing::make();
}
