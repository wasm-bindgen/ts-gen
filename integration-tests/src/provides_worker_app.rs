//! The app behind the generated `provides_worker_provides` wrapper: what the
//! ts-gen scaffold would be once filled in.

use wasm_bindgen::prelude::*;

use crate::provides_worker::*;

/// Handles a request.
pub async fn fetch(
    request: Request,
    _env: Env,
    ctx: ExecutionContext,
) -> Result<Response, JsValue> {
    ctx.wait_until(&js_sys::Promise::resolve(&JsValue::NULL));
    Ok(Response::new(&format!("echo {}", request.url()))?)
}

/// Receives trace events.
pub async fn tail(events: Vec<String>, _env: Env, _ctx: ExecutionContext) -> Result<(), JsValue> {
    assert!(!events.is_empty());
    Ok(())
}

/// Always provided: a plain method with an optional parameter.
pub fn health(verbose: Option<bool>) -> String {
    if verbose == Some(true) {
        "ok (verbose)".into()
    } else {
        "ok".into()
    }
}
