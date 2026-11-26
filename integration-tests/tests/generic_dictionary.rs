#![cfg(target_arch = "wasm32")]
// Compile-only coverage of the generated bindings against the real wasm-bindgen.
#[allow(unused_imports)]
use ts_gen_integration_tests::generic_dictionary::*;
#[wasm_bindgen_test::wasm_bindgen_test]
fn bindings_compile() {}

// With no required field naming its parameter, a dictionary's `new()` and
// `builder()` are the default instantiation and need no annotation.
#[allow(dead_code)]
fn open_parameter_defaults() {
    let _: FetchOptions = FetchOptions::new();
    let _: FetchOptions = FetchOptions::builder().method("GET").build();
    let _: FetchOptions<js_sys::Number> = FetchOptions::<js_sys::Number>::new_typed();
    let _: FetchOptions<js_sys::Number> = FetchOptions::<js_sys::Number>::builder_typed()
        .cf(&js_sys::Number::from(1))
        .build();
}
