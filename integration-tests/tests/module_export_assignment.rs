#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::module_export_assignment::{
    check, module_export_assignment::counter::Counter, nested, strict_equal, version,
};

// The members of the namespace the module assigns with `export =` are module
// exports, so these only pass when they bind as `import { strictEqual }`
// rather than through a `check` export the module does not have.
#[wasm_bindgen_test]
fn export_assigned_namespace_members_are_module_exports() {
    assert!(strict_equal(&JsValue::from(1), &JsValue::from(1)));
    assert!(!strict_equal(&JsValue::from(1), &JsValue::from(2)));
    assert_eq!(version.with(Clone::clone), "1.0");
    assert_eq!(nested::run(), "nested");
}

// The assigned binding is the module's default export.
#[wasm_bindgen_test]
fn export_assigned_binding_is_the_default_export() {
    check(&JsValue::TRUE);
    let counter = Counter::new(1.0).unwrap();
    counter.increment();
    assert_eq!(counter.value(), 2.0);
}
