//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Number};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::constructor_type_params_mono::{Channel, Holder};

// Compile-only: the fixture has no backing JavaScript module. As in the
// default mode, an open class parameter is at its default under the plain
// name and bound by the `_typed` twin.
#[allow(dead_code)]
fn constructors_mono_compile() -> Result<(), JsValue> {
    let _: Holder = Holder::new()?;
    let _: Holder<Number> = Holder::<Number>::new_typed()?;
    let _: Holder<JsString> = Holder::new_with_initial(JsString::from("a"))?;
    let _: Channel = Channel::new()?;
    let _: Channel<Number, JsString> = Channel::<Number, JsString>::new_with_capacity_typed(1.0)?;
    Ok(())
}
