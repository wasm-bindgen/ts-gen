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
/// module's exports and lets same-named copies coexist. wasm-bindgen defines
/// a JS object for a numeric enum, so one declared inside a namespace
/// carries its `js_namespace`, which places that object.
fn enum_attr(js_namespace: &[String]) -> TokenStream {
    let js_namespace = super::js_namespace_attr(js_namespace).map(|ns| quote! { , #ns });
    quote! { #[wasm_bindgen(private #js_namespace)] }
}

/// The derives of every generated enum. One of only unit variants (string
/// or integer discriminants) is a plain value, copied and compared like any
/// other; one with a tuple variant holds a `String` or a JS handle, so it is
/// cloned instead.
pub(crate) fn enum_derives(has_payload: bool) -> TokenStream {
    if has_payload {
        quote! { #[derive(Clone, Debug)] }
    } else {
        quote! { #[derive(Debug, Clone, Copy, PartialEq, Eq)] }
    }
}

/// Generate a wasm_bindgen string enum.
///
/// A string enum is only its values; wasm-bindgen never names it in JS, and
/// it carries a `js_namespace` through to its metadata without reading it.
/// So one inside a namespace is emitted without one.
pub fn generate_string_enum(decl: &StringEnumDecl) -> TokenStream {
    let name = super::typemap::make_ident(&decl.name);
    let attr = enum_attr(&[]);
    let derives = enum_derives(false);

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
        #derives
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
pub fn generate_numeric_enum(decl: &NumericEnumDecl, js_namespace: &[String]) -> TokenStream {
    let name = super::typemap::make_ident(&decl.name);
    let attr = enum_attr(js_namespace);
    let derives = enum_derives(false);
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
        #derives
        #[repr(#repr)]
        pub enum #name {
            #(#variants,)*
        }
        #aliases
    }
}

/// `From` conversions from each synthesised string enum into every other
/// enum of the same module whose values are a strict superset of its own:
/// `PluralizeTimeUnit` into `PluralizeUnit`, so a narrowed value passes
/// where the wider type is taken. `enums` pairs each enum with whether
/// type-level evaluation synthesised it; only those convert from, since a
/// declared enum naming a subset is a type of its own. A string JS gave
/// that the narrower enum doesn't know (wasm-bindgen's hidden `__Invalid`)
/// stays unknown.
pub fn generate_subset_conversions(enums: &[(&StringEnumDecl, bool)]) -> Vec<TokenStream> {
    let values = |decl: &StringEnumDecl| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for v in &decl.variants {
            if !out.contains(&v.js_value) {
                out.push(v.js_value.clone());
            }
        }
        out
    };
    let mut out = Vec::new();
    for &(sub, synthesised) in enums {
        if !synthesised {
            continue;
        }
        let sub_values = values(sub);
        for &(sup, _) in enums {
            let sup_values = values(sup);
            if sup.name == sub.name
                || sup_values.len() <= sub_values.len()
                || !sub_values.iter().all(|v| sup_values.contains(v))
            {
                continue;
            }
            let variant = |decl: &StringEnumDecl, value: &str| {
                let v = decl.variants.iter().find(|v| v.js_value == value).unwrap();
                super::typemap::make_ident(&v.rust_name)
            };
            let sub_name = super::typemap::make_ident(&sub.name);
            let sup_name = super::typemap::make_ident(&sup.name);
            let arms = sub_values.iter().map(|value| {
                let from = variant(sub, value);
                let to = variant(sup, value);
                quote! { #sub_name::#from => #sup_name::#to }
            });
            out.push(quote! {
                impl From<#sub_name> for #sup_name {
                    fn from(value: #sub_name) -> #sup_name {
                        match value {
                            #(#arms,)*
                            #sub_name::__Invalid => #sup_name::__Invalid,
                        }
                    }
                }
            });
        }
    }
    out
}
