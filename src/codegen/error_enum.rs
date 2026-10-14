//! The `--error-enum` lowering: every type named in a `@throws` across the
//! output becomes one variant of a single generated enum, so a caller has
//! one `?`-able error type and can still match on the JS error class.
//!
//! `@throws {TypeError | RangeError}` otherwise lowers to the LUB (`Error`)
//! and loses the union. An enum with a checked `From<JsValue>` keeps it:
//! the `From` impl tries `dyn_into` per variant, most derived class first,
//! and falls back to `Other(JsValue)`.

use std::cmp::Reverse;

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::subtyping::ancestor_chain;
use crate::codegen::surface::Surface;
use crate::codegen::typemap::{self, CodegenContext, TypePosition, JS_SYS_RESERVED};
use crate::context::TypeId;
use crate::ir::{Member, ModuleContext, Throws, TypeDeclaration, TypeKind, TypeRef};
use crate::parse::scope::ScopeId;

/// Collect the type names mentioned by `@throws` on every callable and
/// getter of `ids` in the forms the output surface emits them in
/// (recursing into namespaces), in first-seen order.
/// Union members are flattened; `never` has already been dropped by the
/// parser.
pub fn collect_throws_names(ids: &[TypeId], surface: &Surface<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_ids(ids, surface, &mut names);
    names
}

fn collect_ids(ids: &[TypeId], surface: &Surface<'_>, names: &mut Vec<String>) {
    for &id in ids {
        for decl in surface.get(id) {
            collect_decl(decl, surface, names);
        }
    }
}

fn collect_decl(decl: &TypeDeclaration, surface: &Surface<'_>, names: &mut Vec<String>) {
    match &decl.kind {
        TypeKind::Class(c) => collect_members(&c.members, names),
        TypeKind::Interface(i) => collect_members(&i.members, names),
        TypeKind::Function(f) => {
            collect_throws(&f.throws, names);
            for o in &f.overloads {
                collect_throws(&o.throws, names);
            }
        }
        TypeKind::Namespace(ns) => collect_ids(&ns.declarations, surface, names),
        TypeKind::DiscriminatedUnion(_)
        | TypeKind::TypeAlias(_)
        | TypeKind::StringEnum(_)
        | TypeKind::NumericEnum(_)
        | TypeKind::TsEnum(_)
        | TypeKind::Variable(_) => {}
    }
}

fn collect_members(members: &[Member], names: &mut Vec<String>) {
    for member in members {
        let throws = match member {
            Member::Method(m) => &m.throws,
            Member::StaticMethod(m) => &m.throws,
            Member::Constructor(c) => &c.throws,
            Member::Getter(g) => &g.throws,
            Member::StaticGetter(g) => &g.throws,
            Member::Setter(_) | Member::StaticSetter(_) | Member::IndexSignature(_) => continue,
        };
        collect_throws(throws, names);
    }
}

fn collect_throws(throws: &Throws, names: &mut Vec<String>) {
    let Throws::Type(ty) = throws else { return };
    collect_type(ty, names);
}

fn collect_type(ty: &TypeRef, names: &mut Vec<String>) {
    match ty {
        TypeRef::Union(union) => union.members().iter().for_each(|m| collect_type(m, names)),
        other => {
            if let Some(name) = other.as_ident() {
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
}

/// A `@throws` name is bindable when codegen can emit a type for it:
/// declared in the input, a `js_sys` class, or mapped by `--external`.
/// Anything else cannot be a variant payload. A bare name resolves from
/// the global scope, where script declarations and builtins live.
fn is_bindable(name: &str, ctx: &CodegenContext<'_>) -> bool {
    let resolved = ctx.gctx.scopes.resolve(ctx.root_scope, name);
    let external = || ctx.gctx.external_map.resolve_type(name).is_some();
    match resolved {
        Some(id) if ctx.gctx.builtin_type_ids.contains(&id) => {
            JS_SYS_RESERVED.contains(&name) || external()
        }
        Some(id) => ctx.local_type_ids.contains(&id) || external(),
        None => external(),
    }
}

/// Dispatch order for the `From<JsValue>` chain. A type precedes every
/// ancestor in its chain; otherwise deeper chains go first. A type whose
/// ancestry ts-gen cannot see (an `--external` mapping with no `extends`
/// in the input) goes before every builtin-rooted one, since an external
/// error class almost certainly extends `Error` and would otherwise be
/// swallowed by that arm. Ties keep source order.
fn order_variants<'a>(names: &[&'a str], ctx: &CodegenContext<'_>, scope: ScopeId) -> Vec<&'a str> {
    let chains: Vec<Vec<String>> = names
        .iter()
        .map(|n| ancestor_chain(n, ctx, scope))
        .collect();
    let opaque =
        |i: usize| chains[i].len() == 2 && ctx.gctx.external_map.resolve_type(names[i]).is_some();
    let priority = |i: usize| (Reverse(opaque(i)), Reverse(chains[i].len()), i);

    let mut placed = vec![false; names.len()];
    let mut order = Vec::with_capacity(names.len());
    while order.len() < names.len() {
        let pending = || (0..names.len()).filter(|&i| !placed[i]);
        let free = pending().filter(|&i| {
            !pending().any(|j| j != i && chains[j][1..].iter().any(|a| a == names[i]))
        });
        let next = free
            .min_by_key(|&i| priority(i))
            .or_else(|| pending().min_by_key(|&i| priority(i)))
            .expect("pending variants remain");
        placed[next] = true;
        order.push(names[next]);
    }
    order
}

/// Emit the enum plus its `From<JsValue>` / `Into<JsValue>` / `Display` /
/// `Error` impls, variants in [`order_variants`] order. Unbindable names
/// are reported and skipped — bindings that named them still return the
/// enum, landing in `Other`.
pub fn generate_error_enum(
    name: &str,
    throws_names: &[String],
    ctx: &CodegenContext<'_>,
) -> TokenStream {
    let scope = ctx.root_scope;
    let from_module = ModuleContext::Global;

    if ctx.local_types.contains(name) || ctx.renamed_locals.values().any(|n| n == name) {
        ctx.error(format!(
            "--error-enum name `{name}` collides with a declared type; pass a different name"
        ));
    }

    let bindable: Vec<&str> = throws_names
        .iter()
        .filter(|n| {
            let ok = is_bindable(n, ctx);
            if !ok {
                ctx.error(format!(
                    "`@throws {{{n}}}` names a type that is not declared in the input, a `js_sys` \
                     error class, or mapped with --external; it is omitted from `{name}`"
                ));
            }
            ok
        })
        .map(String::as_str)
        .collect();
    let variants = order_variants(&bindable, ctx, scope);

    let enum_ident = typemap::make_ident(name);
    let idents: Vec<syn::Ident> = variants.iter().map(|n| typemap::make_ident(n)).collect();
    let payloads: Vec<TokenStream> = variants
        .iter()
        .map(|n| {
            typemap::to_syn_type(
                &TypeRef::ident(n.to_string()),
                TypePosition::RETURN,
                Some(ctx),
                scope,
                &from_module,
            )
        })
        .collect();

    let doc = " Every JS error class named by a `@throws` in these bindings; `Other` is any \
               other thrown value. `Display` is the error's `message`.";
    quote! {
        #[doc = #doc]
        #[derive(Debug, Clone)]
        pub enum #enum_ident {
            #(#idents(#payloads),)*
            Other(JsValue),
        }
        impl From<JsValue> for #enum_ident {
            fn from(value: JsValue) -> Self {
                #(
                    let value = match value.dyn_into::<#payloads>() {
                        Ok(error) => return Self::#idents(error),
                        Err(value) => value,
                    };
                )*
                Self::Other(value)
            }
        }
        impl From<#enum_ident> for JsValue {
            fn from(error: #enum_ident) -> Self {
                match error {
                    #(#enum_ident::#idents(error) => error.into(),)*
                    #enum_ident::Other(value) => value,
                }
            }
        }
        impl AsRef<JsValue> for #enum_ident {
            fn as_ref(&self) -> &JsValue {
                match self {
                    #(Self::#idents(error) => error.as_ref(),)*
                    Self::Other(value) => value,
                }
            }
        }
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_name = String, catch)]
            fn __ts_gen_js_string(value: &JsValue) -> Result<String, JsValue>;
        }
        impl core::fmt::Display for #enum_ident {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let value: &JsValue = self.as_ref();
                if let Some(error) = value.dyn_ref::<::js_sys::Error>() {
                    return f.write_str(&String::from(error.message()));
                }
                if let Some(string) = value.as_string() {
                    return f.write_str(&string);
                }
                match __ts_gen_js_string(value) {
                    Ok(string) => f.write_str(&string),
                    Err(_) => write!(f, "{value:?}"),
                }
            }
        }
        impl std::error::Error for #enum_ident {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::GenerateOptions;

    fn with_ctx(src: &str, f: impl FnOnce(&CodegenContext<'_>, &[String])) {
        with_ctx_external(src, &[], f)
    }

    fn with_ctx_external(
        src: &str,
        externals: &[&str],
        f: impl FnOnce(&CodegenContext<'_>, &[String]),
    ) {
        let (module, mut gctx) = crate::parse_source(src, None).unwrap();
        for mapping in externals {
            gctx.external_map.add_mapping(mapping);
        }
        let options = GenerateOptions::new().error_enum("JsException");
        let surface = Surface::build(&module, &gctx, &options);
        let ctx =
            CodegenContext::from_module_full(&module, &gctx, &options, Some(&surface)).unwrap();
        let names = collect_throws_names(&module.types, &surface);
        f(&ctx, &names);
    }

    #[test]
    fn collects_union_members_and_getters_once() {
        let src = r#"
            declare class A {
                /** @throws {TypeError | RangeError} */
                f(): void;
                /** @throws {RangeError} */
                readonly g: string;
                /** @throws {never} */
                h(): void;
            }
            /** @throws {Error} */
            declare function k(): void;
            /** @throws {SyntaxError} */
            declare function k(x: string): void;
        "#;
        with_ctx(src, |_, names| {
            assert_eq!(names, &["TypeError", "RangeError", "Error", "SyntaxError"]);
        });
    }

    #[test]
    fn variants_sort_most_derived_first() {
        let src = r#"
            declare class Mine extends TypeError {}
            declare class A {
                /** @throws {Error | Mine | TypeError} */
                f(): void;
            }
        "#;
        with_ctx(src, |ctx, names| {
            let tokens = generate_error_enum("JsException", names, ctx).to_string();
            let order: Vec<usize> = ["Mine (Mine)", "TypeError (TypeError)", "Error (Error)"]
                .iter()
                .map(|v| tokens.find(v).unwrap_or_else(|| panic!("missing {v}")))
                .collect();
            assert!(order[0] < order[1] && order[1] < order[2], "{tokens}");
        });
    }

    #[test]
    fn external_variant_precedes_builtins_but_follows_its_subclass() {
        let src = r#"
            declare class Mine extends DomException {}
            declare class A {
                /** @throws {Error | DomException | Mine} */
                f(): void;
            }
        "#;
        with_ctx_external(
            src,
            &["DomException=::web_sys::DomException"],
            |ctx, names| {
                let tokens = generate_error_enum("JsException", names, ctx).to_string();
                let order: Vec<usize> = ["Mine (Mine)", "DomException (", "Error (Error)"]
                    .iter()
                    .map(|v| tokens.find(v).unwrap_or_else(|| panic!("missing {v}")))
                    .collect();
                assert!(order[0] < order[1] && order[1] < order[2], "{tokens}");
            },
        );
    }

    #[test]
    fn unknown_throws_type_is_reported_and_skipped() {
        let src = r#"
            declare class A {
                /** @throws {Nope} */
                f(): void;
            }
        "#;
        with_ctx(src, |ctx, names| {
            let tokens = generate_error_enum("JsException", names, ctx).to_string();
            assert!(!tokens.contains("Nope"));
            let diags = ctx.take_diagnostics();
            assert!(diags
                .diagnostics
                .iter()
                .any(|d| d.message.contains("`@throws {Nope}`")));
        });
    }
}
