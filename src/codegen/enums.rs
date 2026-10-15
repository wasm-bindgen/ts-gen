//! Enum code generation: string enums and numeric enums.
//!
//! String enums use wasm_bindgen's native string enum support, where each
//! variant's discriminant is its JS string value:
//!
//! ```rust,ignore
//! #[wasm_bindgen(private)]
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! pub enum QueueContentType {
//!     Text = "text",
//!     Bytes = "bytes",
//! }
//! ```
//!
//! Numeric enums use `#[repr(u32)]` or `#[repr(i32)]` (selected based on
//! discriminant values) with wasm_bindgen:
//!
//! ```rust,ignore
//! #[wasm_bindgen(private)]
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! #[repr(u32)]
//! pub enum ImportType {
//!     Static = 1,
//!     Dynamic = 2,
//!     ImportMeta = 3,
//! }
//! ```
//!
//! A member repeating an earlier member's value is an alias of it. wasm-bindgen
//! rejects duplicate discriminants, and two string variants with one value
//! could not be told apart when decoding, so aliases become associated consts
//! of the first variant with that value:
//!
//! ```rust,ignore
//! impl ImportType {
//!     #[allow(non_upper_case_globals)]
//!     pub const Latest: ImportType = ImportType::ImportMeta;
//! }
//! ```
//!
//! for a declaration ending `ImportMeta = 3, Latest = 3`. The parser has
//! already evaluated member references and constant expressions
//! (`Latest = ImportMeta`, `All = Read | Write`) to their values.
//!
//! ```rust,ignore
//! ```

use proc_macro2::TokenStream;
use quote::quote;

use std::collections::HashMap;
use std::hash::Hash;

use crate::ir::{NumericEnumDecl, StringEnumDecl};

/// Split members into the variants (first member per value) and the aliases
/// (later members, paired with the variant they repeat).
fn split_aliases<V, K: Eq + Hash>(
    members: &[V],
    key: impl Fn(&V) -> K,
) -> (Vec<&V>, Vec<(&V, &V)>) {
    let mut first: HashMap<K, &V> = HashMap::new();
    let mut variants = Vec::new();
    let mut aliases = Vec::new();
    for m in members {
        match first.get(&key(m)) {
            Some(target) => aliases.push((m, *target)),
            None => {
                first.insert(key(m), m);
                variants.push(m);
            }
        }
    }
    (variants, aliases)
}

fn alias_impl(
    name: &syn::Ident,
    aliases: impl Iterator<Item = (TokenStream, syn::Ident, syn::Ident)>,
) -> TokenStream {
    let consts: Vec<_> = aliases
        .map(|(doc, alias, target)| {
            quote! {
                #doc
                #[allow(non_upper_case_globals)]
                pub const #alias: #name = #name::#target;
            }
        })
        .collect();
    if consts.is_empty() {
        return quote! {};
    }
    quote! {
        impl #name {
            #(#consts)*
        }
    }
}

/// Generated enums describe JS-side values rather than anything this crate
/// exports, so they are `private`: wasm-bindgen then keeps them off the JS
/// module's exports and lets same-named copies coexist. wasm-bindgen still
/// names them by JS name crate-wide, so one declared inside a namespace takes
/// a namespace-prefixed `js_name` to stay apart from a same-named enum
/// elsewhere. The `_` keeps `Timing.DurationUnit` apart from a top-level
/// `TimingDurationUnit`.
fn enum_attr(name: &str, js_namespace: Option<&str>) -> TokenStream {
    match js_namespace {
        Some(ns) => {
            let js_name = format!("{ns}_{name}");
            quote! { #[wasm_bindgen(private, js_name = #js_name)] }
        }
        None => quote! { #[wasm_bindgen(private)] },
    }
}

/// Generate a wasm_bindgen string enum.
pub fn generate_string_enum(decl: &StringEnumDecl, js_namespace: Option<&str>) -> TokenStream {
    let name = super::typemap::make_ident(&decl.name);
    let attr = enum_attr(&decl.name, js_namespace);

    let (members, aliases) = split_aliases(&decl.variants, |v| v.js_value.clone());
    let variants: Vec<_> = members
        .iter()
        .map(|v| {
            let rust_name = super::typemap::make_ident(&v.rust_name);
            let js_value = &v.js_value;
            quote! {
                #rust_name = #js_value
            }
        })
        .collect();
    let aliases = alias_impl(
        &name,
        aliases.into_iter().map(|(alias, target)| {
            (
                quote! {},
                super::typemap::make_ident(&alias.rust_name),
                super::typemap::make_ident(&target.rust_name),
            )
        }),
    );

    quote! {
        #attr
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum #name {
            #(#variants,)*
        }
        #aliases
    }
}

/// Determine the appropriate repr for a numeric enum based on its discriminant values.
fn numeric_enum_repr(decl: &NumericEnumDecl) -> TokenStream {
    let min = decl.variants.iter().map(|v| v.value).min().unwrap_or(0);
    let max = decl.variants.iter().map(|v| v.value).max().unwrap_or(0);

    if min >= 0 && max <= u32::MAX as i64 {
        quote! { u32 }
    } else if min >= i32::MIN as i64 && max <= i32::MAX as i64 {
        quote! { i32 }
    } else {
        // Fallback — wasm_bindgen doesn't support i64 repr, so clamp to i32
        // and emit the best we can. This is an extreme edge case.
        quote! { i32 }
    }
}

/// Generate a wasm_bindgen numeric enum with automatically selected repr.
pub fn generate_numeric_enum(decl: &NumericEnumDecl, js_namespace: Option<&str>) -> TokenStream {
    let name = super::typemap::make_ident(&decl.name);
    let attr = enum_attr(&decl.name, js_namespace);
    let repr = numeric_enum_repr(decl);

    let has_negative = decl.variants.iter().any(|v| v.value < 0);

    let (members, aliases) = split_aliases(&decl.variants, |v| v.value);
    let variants: Vec<_> = members
        .iter()
        .map(|v| {
            let rust_name = super::typemap::make_ident(&v.rust_name);
            let doc = crate::codegen::doc_tokens(&v.doc);
            if has_negative {
                let value = i32::try_from(v.value).unwrap_or({
                    // Value out of i32 range — truncation is lossy but we
                    // already warned at parse time if out of i64 range.
                    v.value as i32
                });
                quote! {
                    #doc
                    #rust_name = #value
                }
            } else {
                let value = u32::try_from(v.value).unwrap_or(v.value as u32);
                quote! {
                    #doc
                    #rust_name = #value
                }
            }
        })
        .collect();
    let aliases = alias_impl(
        &name,
        aliases.into_iter().map(|(alias, target)| {
            (
                crate::codegen::doc_tokens(&alias.doc),
                super::typemap::make_ident(&alias.rust_name),
                super::typemap::make_ident(&target.rust_name),
            )
        }),
    );

    quote! {
        #attr
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(#repr)]
        pub enum #name {
            #(#variants,)*
        }
        #aliases
    }
}
