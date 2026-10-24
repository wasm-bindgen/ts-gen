#![cfg(target_arch = "wasm32")]

use js_sys::JsString;
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::alias_generics::{hidden, uses};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that an instantiation left unchased, like `H.Either<number>`,
// names an alias that is still emitted.
#[allow(dead_code)]
fn unchased_instantiations_name_emitted_aliases(
    triple: &hidden::Triple<js_sys::Number, JsString>,
    either: &hidden::Either,
) {
    uses::triple(triple);
    uses::either(either);
    let _: &JsValue = either;
}
