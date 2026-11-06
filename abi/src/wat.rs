//! WAT text helpers shared by the `--target wat` view and `embed`.

use std::fmt::Write;

use crate::WasmType;

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

pub fn result(result: Option<WasmType>) -> String {
    match result {
        Some(t) => format!(" (result {})", t.wat()),
        None => String::new(),
    }
}
