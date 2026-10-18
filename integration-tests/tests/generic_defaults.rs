#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen::{JsCast, JsValue};

use ts_gen_integration_tests::generic_defaults::{Box_, Holder, Options};

// Compile-only: the defaults make the bare names well-formed Rust types
// while explicit arguments still work.
#[allow(dead_code)]
fn defaults_compile(holder: Holder, boxed: Box_, options: Options) {
    let _: JsValue = holder.get();
    let _: Box_<JsValue> = holder.boxed();
    let _: JsValue = boxed.value();
    let _: Option<Box_<JsString>> = options.initial();
    let _: Holder<JsString, Box_<JsString>> = JsValue::UNDEFINED.unchecked_into();
    let _ = Options::<Box_<JsString>>::builder().build();
}
