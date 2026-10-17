#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use ts_gen_integration_tests::export_default::{
    export_default::{create::create, version::version},
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
