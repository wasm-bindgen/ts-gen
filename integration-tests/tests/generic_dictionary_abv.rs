#![cfg(target_arch = "wasm32")]

use js_sys::{Float32Array, JsString, Uint8Array};
use ts_gen_integration_tests::generic_dictionary_abv::{GenAbv, ReqAbv, U};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that the synthesised typed-array generic doesn't shadow the
// dictionary's own `<T>` or a declared type named `U`.
#[allow(dead_code)]
fn generic_dictionary_abv_compile(bytes: &Uint8Array, floats: &Float32Array, owner: &U) {
    let value = JsString::from("v");
    let gen: GenAbv<JsString> = GenAbv::builder().value(&value).data(floats).build();
    gen.set_data(bytes);
    let req: ReqAbv<JsString> = ReqAbv::new(&value, bytes, floats, owner);
    req.set_other(floats);
}
