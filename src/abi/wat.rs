//! `--target wat`: the ABI as an annotated import template, the format
//! `ABI.md` specifies. Every function is an import a language module may
//! use and every provided function a stub export to fill in; `embed`
//! prunes the bindings to what a module actually imports.

pub use ts_gen_abi::wat::*;

use super::Abi;

pub fn generate(abi: &Abi) -> String {
    ts_gen_abi::format::emit(abi)
}
