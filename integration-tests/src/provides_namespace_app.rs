//! The app behind the generated `provides_namespace_provides` wrapper.

use js_sys::{Array, BigInt, Error, Function, JsNullable, Nullable, Number, Object, Symbol, Undefined};
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
    // Under `js_sys_unstable_apis` `Object::values` is fallible and typed
    // `Array<Number>`; without it, a plain `Array`.
    #[cfg(js_sys_unstable_apis)]
    let values = js_sys::Object::values(&values).unwrap_or_else(|e| wasm_bindgen::throw_val(e));
    #[cfg(not(js_sys_unstable_apis))]
    let values = js_sys::Object::values(&values);
    values.iter().map(|v| v.as_f64().unwrap_or(0.0)).sum()
}

pub fn describe_symbol(s: Symbol) -> String {
    String::from(s.to_string())
}

pub fn double(x: BigInt) -> BigInt {
    x * BigInt::from(2u32)
}

pub fn keys(o: Object) -> Vec<String> {
    js_sys::Object::keys(&o)
        .unchecked_into::<Array>()
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

/// A nullable result is a write: `Nullable<String>`, so JS receives `null`.
pub fn find(id: f64) -> Nullable<String> {
    if id == 7.0 {
        Nullable::Value("seven".into())
    } else {
        Nullable::Null
    }
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
