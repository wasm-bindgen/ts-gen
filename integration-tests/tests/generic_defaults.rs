#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen::{JsCast, JsValue};

use ts_gen_integration_tests::generic_defaults::{
    loose_box, loose_holder, make_holder, mixed_holder, string_holder, Box_, Holder, Options,
};

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

// Arguments left to the declared defaults name the same types as the
// spelled-out instantiations.
#[allow(dead_code)]
fn trimmed_defaults_compile() {
    let _: Box_<JsValue> = loose_box();
    let _: Holder<JsValue, Box_<JsValue>> = loose_holder();
    let _: Holder<JsString, Box_<JsString>> = string_holder();
    let _: Holder<JsString, Box_<JsValue>> = mixed_holder();
}

// An uninferrable `V` in `Holder<string, V>` skips `U`'s default `Box<T>`,
// which names Holder's own `T`, and erases to `any`.
#[allow(dead_code)]
fn erased_past_dependent_default_compile() {
    let _: Holder<JsString, JsValue> = make_holder();
    let _: Holder<JsString, JsValue> = Holder::make();
}
