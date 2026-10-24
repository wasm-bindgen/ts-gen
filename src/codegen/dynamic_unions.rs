//! Emission of the `<Anchor>Kind` enums synthesised for unions that would
//! otherwise erase to `JsValue`, and the `js_sys` enums that stand in for
//! some of them. Each enum is emitted in the `pub mod` of its first use
//! (see [`CodegenContext::synthesise_dynamic_union`]).

use proc_macro2::TokenStream;
use quote::quote;

use super::typemap::{self, CodegenContext, DynamicUnionInfo};
use crate::ir::TypeRef;

/// `js_sys` enums a union of exactly the same string literals stands for,
/// with their variants in the order `js_sys` declares them. Only enums
/// `js_sys` always exports are listed: `js_sys::Temporal` needs
/// `--cfg=js_sys_unstable_apis`.
const JS_SYS_ENUMS: &[(&str, &[(&str, &str)])] = &[
    (
        "::js_sys::Intl::RoundingMode",
        &[
            ("ceil", "Ceil"),
            ("floor", "Floor"),
            ("expand", "Expand"),
            ("trunc", "Trunc"),
            ("halfCeil", "HalfCeil"),
            ("halfFloor", "HalfFloor"),
            ("halfExpand", "HalfExpand"),
            ("halfTrunc", "HalfTrunc"),
            ("halfEven", "HalfEven"),
        ],
    ),
    (
        "::js_sys::Intl::RoundingPriority",
        &[
            ("auto", "Auto"),
            ("morePrecision", "MorePrecision"),
            ("lessPrecision", "LessPrecision"),
        ],
    ),
    (
        "::js_sys::Intl::TrailingZeroDisplay",
        &[("auto", "Auto"), ("stripIfInteger", "StripIfInteger")],
    ),
    (
        "::js_sys::Intl::LocaleMatcher",
        &[("lookup", "Lookup"), ("best fit", "BestFit")],
    ),
];

/// The `js_sys` enum whose values are exactly `members`, all string
/// literals, in any order.
pub(crate) fn js_sys_enum(members: &[TypeRef]) -> Option<&'static str> {
    let mut values: Vec<&str> = members
        .iter()
        .map(|m| match m {
            TypeRef::StringLiteral(s) => Some(s.as_str()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    values.sort_unstable();
    JS_SYS_ENUMS.iter().find_map(|(path, variants)| {
        let mut known: Vec<&str> = variants.iter().map(|(v, _)| *v).collect();
        known.sort_unstable();
        (known == values).then_some(*path)
    })
}

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
            .filter(|info| info.home == path && info.js_sys.is_none())
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
    quote! {
        #[wasm_bindgen(private #fallback)]
        pub enum #enum_ident {
            #(#variants),*
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_sys_enum_variants_match_synthesised_variant_names() {
        for (path, variants) in JS_SYS_ENUMS {
            let members: Vec<TypeRef> = variants
                .iter()
                .map(|(v, _)| TypeRef::StringLiteral((*v).to_string()))
                .collect();
            let expected: Vec<String> = variants.iter().map(|(_, n)| n.to_string()).collect();
            assert_eq!(
                super::super::dynamic_union_variant_names(&members),
                expected,
                "{path}"
            );
            assert_eq!(js_sys_enum(&members), Some(*path));
        }
    }

    #[test]
    fn js_sys_enum_needs_the_exact_value_set() {
        let lit = |s: &str| TypeRef::StringLiteral(s.to_string());
        assert_eq!(
            js_sys_enum(&[lit("best fit"), lit("lookup")]),
            Some("::js_sys::Intl::LocaleMatcher")
        );
        assert_eq!(js_sys_enum(&[lit("lookup")]), None);
        assert_eq!(
            js_sys_enum(&[lit("lookup"), lit("best fit"), lit("x")]),
            None
        );
        assert_eq!(js_sys_enum(&[lit("lookup"), TypeRef::String]), None);
    }
}
