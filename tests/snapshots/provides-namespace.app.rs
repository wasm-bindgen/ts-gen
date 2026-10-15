// Scaffold from ts-gen: the app behind the provides wrapper. Edit freely; it is not regenerated.

#[allow(unused_imports)]
use wasm_bindgen::prelude::*;
#[allow(unused_imports)]
use js_sys::*;
#[allow(unused_imports)]
use super::bindings::*;
/// A top-level function.
pub fn version() -> f64 {
    todo!()
}

/// Same leaf as a default member: both are kept apart in Rust.
pub fn add(_a: f64, _b: f64) -> f64 {
    todo!()
}

/// Types from js_sys in a provider signature.
pub fn sum(_values: Object<Number>) -> f64 {
    todo!()
}

pub fn describe_symbol(_s: Symbol) -> String {
    todo!()
}

pub fn double(_x: BigInt) -> BigInt {
    todo!()
}

pub fn keys(_o: Object) -> Vec<String> {
    todo!()
}

pub fn notify(_cb: Function<fn(JsNullable<Error>, Number) -> Undefined>) {}

pub fn ret_opt(_x: Option<String>) -> String {
    todo!()
}

/// Nested two deep.
pub fn api_math_add(_a: f64, _b: f64) -> f64 {
    todo!()
}

/// Same leaf name on another path.
pub fn api_str_add(_a: String, _b: String) -> String {
    todo!()
}

/// Receives a value, returns one.
pub fn issue(_id: f64) -> Ticket {
    todo!()
}

pub fn describe(_ticket: Option<Ticket>) -> String {
    todo!()
}

/// Collides with the top-level `add` by leaf.
pub fn default_add(_a: f64) -> f64 {
    todo!()
}
