#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen::{JsCast, JsValue};

use ts_gen_integration_tests::generic_defaults::{
    loose_box, loose_holder, mixed_holder, string_holder, Box, Holder, Options,
};

// Compile-only: the defaults make the bare names well-formed Rust types
// while explicit arguments still work.
#[allow(dead_code)]
fn defaults_compile(holder: Holder, boxed: Box, options: Options) {
    let _: JsValue = holder.get();
    let _: Box<JsValue> = holder.boxed();
    let _: JsValue = boxed.value();
    let _: Option<Box<JsString>> = options.initial();
    let _: Holder<JsString, Box<JsString>> = JsValue::UNDEFINED.unchecked_into();
    let _ = Options::<Box<JsString>>::builder().build();
}

// Arguments left to the declared defaults name the same types as the
// spelled-out instantiations.
#[allow(dead_code)]
fn trimmed_defaults_compile() {
    let _: Box<JsValue> = loose_box();
    let _: Holder<JsValue, Box<JsValue>> = loose_holder();
    let _: Holder<JsString, Box<JsString>> = string_holder();
    let _: Holder<JsString, Box<JsValue>> = mixed_holder();
}
