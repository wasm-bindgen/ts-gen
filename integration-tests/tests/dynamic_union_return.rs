#![cfg(target_arch = "wasm32")]
// Compile-only coverage of the generated bindings against the real wasm-bindgen.
#[allow(unused_imports)]
use ts_gen_integration_tests::dynamic_union_return::*;
#[wasm_bindgen_test::wasm_bindgen_test]
fn bindings_compile() {}
