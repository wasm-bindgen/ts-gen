//! WAT text helpers shared by the template writer and `embed`.

use std::fmt::Write;

use crate::{AbiFunction, AbiParam, WasmType};

/// `Type.member` as a WAT identifier: `$Type.member` and
/// `$node:fs:readFileSync` are legal since `.`, `:` and `/` are id
/// characters; anything else becomes `_`.
pub fn ident(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "!#$%&'*+-./:<=>?@\\^_`|~".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A WAT string literal. Non-ASCII characters are written as they are,
/// which the text format allows; control characters are escaped.
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The type of a variadic rest parameter.
pub const ANYREFS: &str = "(ref null $anyrefs)";

pub fn params(params: &[AbiParam]) -> String {
    params_variadic(params, false)
}

pub fn params_variadic(params: &[AbiParam], variadic: bool) -> String {
    let mut s = String::new();
    for (i, p) in params.iter().enumerate() {
        let is_rest = variadic && i == params.len() - 1;
        let ty = if is_rest { ANYREFS } else { p.ty.wat() };
        let _ = write!(s, " (param ${} {})", p.name, ty);
    }
    s
}

pub fn result(result: Option<WasmType>) -> String {
    match result {
        Some(t) => format!(" (result {})", t.wat()),
        None => String::new(),
    }
}

/// Signature text of a function, for matching a module's import types.
pub fn signature(f: &AbiFunction) -> String {
    let params_str: String = f
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_rest = f.variadic && i == f.params.len() - 1;
            let ty = if is_rest { ANYREFS } else { p.ty.wat() };
            format!(" (param {})", ty)
        })
        .collect();
    format!("(func{}{})", params_str, result(f.result))
}
