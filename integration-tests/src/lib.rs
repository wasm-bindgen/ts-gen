#![allow(non_snake_case, unused_imports, clippy::all)]

#[cfg(target_arch = "wasm32")]
include!(concat!(env!("OUT_DIR"), "/_lib.rs"));

/// A user error type for fixtures that pass `--error-type crate::CaughtError`:
/// any `From<JsValue>` type works there.
#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub struct CaughtError(pub wasm_bindgen::JsValue);

#[cfg(target_arch = "wasm32")]
impl From<wasm_bindgen::JsValue> for CaughtError {
    fn from(value: wasm_bindgen::JsValue) -> Self {
        CaughtError(value)
    }
}
