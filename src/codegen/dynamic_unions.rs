//! Emission of the `<Anchor>Kind` enums synthesised for unions that would
//! otherwise erase to `JsValue`. Each enum is emitted in the `pub mod` of
//! its first use (see [`CodegenContext::synthesise_dynamic_union`]).

use proc_macro2::TokenStream;
use quote::quote;

use super::typemap::{self, CodegenContext, DynamicUnionInfo};

/// The synthesised enums emitted in the `pub mod` at `path`, in order of
/// first use.
/// Payloads are lowered as seen from there.
pub(crate) fn generate_dynamic_unions_in(
    cgctx: &CodegenContext,
    path: &[String],
) -> Vec<TokenStream> {
    let infos: Vec<DynamicUnionInfo> = {
        let registry = cgctx.dynamic_unions.borrow();
        registry
            .by_key
            .values()
            .filter(|info| info.home == path)
            .cloned()
            .collect()
    };
    if infos.is_empty() {
        return Vec::new();
    }
    let outer = cgctx.emit_path.replace(path.to_vec());
    let enums = infos
        .iter()
        .map(|info| generate_dynamic_union(cgctx, info))
        .collect();
    cgctx.emit_path.replace(outer);
    enums
}

/// One synthesised enum. Each member lowers via the regular
/// return-position path, resolved from the scope and module of the
/// union's first use site.
fn generate_dynamic_union(cgctx: &CodegenContext, info: &DynamicUnionInfo) -> TokenStream {
    use crate::ir::TypeRef as T;
    let enum_ident = typemap::make_ident(&info.rust_name);
    if info.merged_shapes.len() > 1 {
        let names: Vec<String> = info.merged_shapes.iter().map(|m| m.format_ts()).collect();
        cgctx.warn(format!(
            "`{}` in one union can't be told apart at runtime; they share one `Object` arm \
             of `{}`",
            names.join("`, `"),
            info.rust_name
        ));
    }
    let variants: Vec<TokenStream> = info
        .members
        .iter()
        .zip(super::dynamic_union_variant_names(&info.members))
        .map(|(m, variant_name)| {
            let variant_ident = typemap::make_ident(&variant_name);
            match m {
                // Literal members become discriminant variants.
                // `typemap::dynamic_union_members` only keeps number
                // literals when every member is a 32-bit integer literal
                // (a C-style enum), and widens boolean literals to
                // `boolean`.
                T::StringLiteral(s) => quote! { #variant_ident = #s },
                T::NumberLiteral(n) if n.fract() == 0.0 => {
                    let lit = proc_macro2::Literal::i64_unsuffixed(n.abs() as i64);
                    let sign = (*n < 0.0).then(|| quote! { - });
                    quote! { #variant_ident = #sign #lit }
                }
                _ => {
                    let payload = typemap::to_syn_type(
                        m,
                        typemap::TypePosition::RETURN,
                        Some(cgctx),
                        info.scope,
                        &info.from_module,
                    );
                    quote! { #variant_ident(#payload) }
                }
            }
        })
        .collect();
    let fallback = info.fallback.then(|| quote! { , fallback });
    let has_payload = info.members.iter().any(|m| {
        !matches!(m, T::StringLiteral(_))
            && !matches!(m, T::NumberLiteral(n) if n.fract() == 0.0)
    });
    let derives = super::enums::enum_derives(has_payload);
    quote! {
        #[wasm_bindgen(private #fallback)]
        #derives
        pub enum #enum_ident {
            #(#variants),*
        }
    }
}
