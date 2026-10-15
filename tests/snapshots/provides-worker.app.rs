// Scaffold from ts-gen: the app behind the provides wrapper. Edit freely; it is not regenerated.

#[allow(unused_imports)]
use wasm_bindgen::prelude::*;
#[allow(unused_imports)]
use js_sys::*;
#[allow(unused_imports)]
use super::bindings::*;
/// Handles a request.
pub async fn fetch(
    _request: Request,
    _env: Env,
    _ctx: ExecutionContext,
) -> Result<Response, JsValue> {
    Err(JsValue::from_str("not implemented"))
}

/// Receives trace events.
pub async fn tail(
    _events: Vec<String>,
    _env: Env,
    _ctx: ExecutionContext,
) -> Result<(), JsValue> {
    Err(JsValue::from_str("not implemented"))
}

/// Always provided: a plain method with an optional parameter.
pub fn health(_verbose: Option<bool>) -> String {
    todo!()
}
