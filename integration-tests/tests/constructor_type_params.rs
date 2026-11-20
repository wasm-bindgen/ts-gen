#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Number};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::constructor_type_params::{Channel, Holder};

// Compile-only: the fixture has no backing JavaScript module. A
// constructor whose arguments leave a class parameter open returns it at
// its default, so it needs no annotation, and its `_typed` twin binds
// every class parameter.
#[allow(dead_code)]
fn constructors_compile() -> Result<(), JsValue> {
    let _: Holder = Holder::new()?;
    let _: Holder<Number> = Holder::<Number>::new_typed()?;
    let _: Holder<JsString> = Holder::new_with_initial(&JsString::from("a"))?;
    let _: Channel = Channel::new()?;
    let _: Channel<JsString, JsString> = Channel::new_with_capacity(1.0)?;
    let _: Channel<Number, JsString> = Channel::<Number, JsString>::new_with_capacity_typed(1.0)?;
    let _: Channel<Number, Number> = Channel::new_with_source_and_capacity(&Number::from(1), 1.0)?;
    let _: Channel<Number, JsString> =
        Channel::<Number, JsString>::new_with_source_and_capacity_typed(&Number::from(1), 1.0)?;
    Ok(())
}
