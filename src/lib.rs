//! `ts-gen`: read TypeScript `.d.ts` files and lower them to the ts-gen IR.
//!
//! This crate is the language-neutral front end: the parser, the parsed
//! model ([`ir`]) and its lowering to the IR ([`lower`]). A language
//! generator builds on it; `ts-gen-wasm-bindgen` is the one for Rust.
//!
//! # Pipeline
//!
//! ```text
//! .d.ts files
//!   → oxc_parser (AST)
//!   → First Pass phase 1: collect all type names into TypeRegistry
//!   → First Pass phase 2: populate full IR (resolve references, merge var+interface patterns)
//!   → Assembly: group by ModuleContext, resolve inheritance chains, classify types
//!   → Lowering: the ts-gen IR (`IR.md`), or a language generator
//! ```

pub mod context;
pub mod external_map;
pub mod input;
pub mod ir;
pub mod lower;
pub mod parse;
pub mod provides;
pub mod util;

use std::path::Path;

use anyhow::Result;

use crate::context::GlobalContext;

/// `std/std.tw`, the JS standard library as a lib IR, as text.
pub const STD_IR: &str = include_str!("../std/std.tw");

/// Parse `.d.ts` files and return the module + global context.
///
/// Diagnostics are collected on `GlobalContext.diagnostics` — callers should
/// inspect or display them as needed (e.g., `ctx.diagnostics.emit()`).
///
/// External type mappings can be configured on the returned `GlobalContext`
/// before passing it to a generator.
pub fn parse(
    paths: &[impl AsRef<Path>],
    lib_name: Option<&str>,
) -> Result<(ir::Module, GlobalContext)> {
    parse::parse_dts_files(paths, lib_name)
}

/// Parse a single `.d.ts` source string and return the module + global context.
///
/// Diagnostics are collected on `GlobalContext.diagnostics` — callers should
/// inspect or display them as needed (e.g., `ctx.diagnostics.emit()`).
pub fn parse_source(source: &str, lib_name: Option<&str>) -> Result<(ir::Module, GlobalContext)> {
    parse::parse_single_source(source, lib_name)
}
