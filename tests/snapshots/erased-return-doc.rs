#[allow(unused_imports)]
use js_sys::*;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type Erased;
    #[doc = " Mixed primitives + objects — no LUB, erases to JsValue."]
    #[doc = ""]
    #[doc = " Returns: string | ArrayBuffer | ArrayBufferView"]
    #[wasm_bindgen(method, getter)]
    pub fn content(this: &Erased) -> JsValue;
    #[doc = " Inner-erased generic."]
    #[doc = ""]
    #[doc = " Returns: Array<32 | \"foo\">"]
    #[wasm_bindgen(method, getter)]
    pub fn tags(this: &Erased) -> Vec<JsValue>;
}
impl Erased {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[allow(unused_unsafe)]
        unsafe {
            JsValue::from(js_sys::Object::new()).unchecked_into()
        }
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type NoErasure;
    #[doc = " Literal widening — lowers to `string`, no Returns: line."]
    #[wasm_bindgen(method, getter)]
    pub fn variant(this: &NoErasure) -> String;
    #[doc = " Plain type — no Returns: line."]
    #[wasm_bindgen(method, getter)]
    pub fn name(this: &NoErasure) -> String;
}
impl NoErasure {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[allow(unused_unsafe)]
        unsafe {
            JsValue::from(js_sys::Object::new()).unchecked_into()
        }
    }
}
#[wasm_bindgen]
extern "C" {
    #[doc = " Async function returning erased inner union."]
    #[doc = ""]
    #[doc = " Returns: 32 | \"foo\""]
    #[wasm_bindgen(catch, js_name = "fetchValue")]
    pub async fn fetch_value() -> Result<JsValue, JsValue>;
}
