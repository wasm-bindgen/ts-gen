#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::export_default::{
    export_default::{create::create, merged, renamed, version::version},
    Processor,
};

// None of these modules has a named export, so these only pass when each
// declaration binds as the module's `default`.
#[wasm_bindgen_test]
fn default_exports_bind_as_default() {
    let processor = Processor::new("a").unwrap();
    assert_eq!(processor.name(), "a");
    assert_eq!(Processor::named("b").name(), "named b");
    assert_eq!(create("c"), "created c");
    assert_eq!(version.with(Clone::clone), "2.0");
}

// Neither module exports `make`, so the namespace members only bind through
// the name the merged function is exported as, which names their module.
#[wasm_bindgen_test]
fn merged_namespaces_bind_through_the_export() {
    assert_eq!(merged::make(), "made");
    assert_eq!(merged::make::helper(), "helped");
    assert_eq!(renamed::create(), "made");
    assert_eq!(renamed::create::helper(), "helped");
}
