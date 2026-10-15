//! The app behind the generated `provides_namespace_provides` wrapper.

use js_sys::{BigInt, Error, Function, JsNullable, Number, Object, Symbol, Undefined};
use wasm_bindgen::prelude::*;

use crate::provides_namespace::*;

/// A top-level function.
pub fn version() -> f64 {
    3.0
}

/// Same leaf as a default member: both are kept apart in Rust.
pub fn add(a: f64, b: f64) -> f64 {
    a + b
}

/// Types from js_sys in a provider signature.
pub fn sum(values: Object<Number>) -> f64 {
    js_sys::Object::values(&values)
        .iter()
        .map(|v| v.as_f64().unwrap_or(0.0))
        .sum()
}

pub fn describe_symbol(s: Symbol) -> String {
    String::from(s.to_string())
}

pub fn double(x: BigInt) -> BigInt {
    x * BigInt::from(2u32)
}

pub fn keys(o: Object) -> Vec<String> {
    js_sys::Object::keys(&o)
        .iter()
        .map(|k| k.as_string().unwrap_or_default())
        .collect()
}

pub fn notify(cb: Function<fn(JsNullable<Error>, Number) -> Undefined>) {
    let _ = cb.call2(&JsValue::NULL, &JsNullable::new(), &Number::from(1.0));
}

pub fn ret_opt(x: Option<String>) -> String {
    x.unwrap_or_else(|| "none".into())
}

/// Nested two deep.
pub fn api_math_add(a: f64, b: f64) -> f64 {
    a + b
}

/// Same leaf name on another path.
pub fn api_str_add(a: String, b: String) -> String {
    format!("{a}{b}")
}

/// Receives a value, returns one.
pub fn issue(id: f64) -> Ticket {
    Ticket::new(id).unwrap()
}

pub fn describe(ticket: Option<Ticket>) -> String {
    match ticket {
        Some(t) => format!("ticket {}", t.id()),
        None => "no ticket".into(),
    }
}

/// Collides with the top-level `add` by leaf.
pub fn default_add(a: f64) -> f64 {
    a + 100.0
}
