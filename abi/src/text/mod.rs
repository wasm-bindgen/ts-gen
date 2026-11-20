//! The IR text format (IR.md §2, §21).
//!
//! [`parse()`] reads a file; [`print()`] writes the canonical form, which reads
//! back to an equal [`Ir`] and is a fixed point of `print ∘ parse`.

mod lex;
mod parse;
mod print;

pub(crate) use print::expr as print_expr;

use std::fmt::{self, Write};

use crate::ir::{Ir, Path, Span, Type};

/// A positioned error; displays as `line:col: message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub span: Span,
    pub message: String,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Error {
        Error {
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.span, self.message)
    }
}

impl std::error::Error for Error {}

pub fn parse(src: &str) -> Result<Ir, Error> {
    let toks = lex::lex(src)?;
    parse::Parser::new(&toks).file()
}

/// A single type, in either the text format's spelling or the canonical
/// spelling keys use (`Map<Str,f64>`, `"node:buffer".Buffer`,
/// `fn(Event)->unit throws KvError`). The bridge reads the subject types of
/// protocol imports and mono key arguments with it.
pub fn parse_type(src: &str) -> Result<Type, Error> {
    let toks = lex::lex(src)?;
    let mut p = parse::Parser::new(&toks);
    let t = p.ty()?;
    p.end()?;
    Ok(t)
}

pub fn print(ir: &Ir) -> String {
    print::print(ir)
}

/// A type in the text format's spelling (`Map<Str, f64>`, `fn(Event) ->
/// str`). Keys use [`crate::key`]'s canonical spelling instead.
pub fn print_type(t: &Type) -> String {
    print::ty(t)
}

/// A path in the text format's spelling (`.Inner`, `"node:buffer".Buffer`).
pub fn print_path(p: &Path) -> String {
    print::path(p)
}

pub(crate) fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An identifier as is, anything else as a string.
pub(crate) fn name(s: &str) -> String {
    if is_ident(s) {
        s.into()
    } else {
        quote(s)
    }
}

fn describe(t: &lex::Tok) -> String {
    use lex::Tok;
    match t {
        Tok::Ident(s) => format!("`{s}`"),
        Tok::Str(s) => quote(s),
        Tok::Int(n) => format!("`{n}`"),
        Tok::Doc(_) => "a doc comment".into(),
        Tok::P(p) => format!("`{p}`"),
        Tok::Eof => "the end of the file".into(),
    }
}
