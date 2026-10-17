//! The `--error-enum` lowering: every type named in a `@throws` across the
//! output becomes one variant of a single generated enum, so a caller has
//! one `?`-able error type and can still match on the JS error class.
//!
//! `@throws {TypeError | RangeError}` otherwise lowers to the LUB (`Error`)
//! and loses the union. An enum with a checked `From<JsValue>` keeps it:
//! the `From` impl tries `dyn_into` per variant, most derived class first,
//! and falls back to `Other(JsValue)`.

use std::cmp::Reverse;
use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::subtyping::builtin_parents;
use crate::codegen::surface::Surface;
use crate::codegen::typemap::{self, CodegenContext, TypePosition, JS_SYS_RESERVED};
use crate::context::TypeId;
use crate::ir::{Heritage, Member, ModuleContext, Throws, TypeDeclaration, TypeKind, TypeRef};
use crate::parse::scope::ScopeId;
use crate::util::naming::to_pascal_case;

/// A `@throws` type name (`Fault`, `Ns.Fault`) and the scope of the declaration
/// carrying it, which its names resolve from like any other reference.
#[derive(Debug, PartialEq)]
pub struct ThrowsName {
    pub path: String,
    pub scope: ScopeId,
}

/// Collect the type names mentioned by `@throws` on every callable and
/// getter of `ids` in the forms the output surface emits them in
/// (recursing into namespaces), in first-seen order.
/// Union members are flattened; `never` has already been dropped by the
/// parser.
pub fn collect_throws_names(ids: &[TypeId], surface: &Surface<'_>) -> Vec<ThrowsName> {
    let mut names = Vec::new();
    collect_ids(ids, surface, &mut names);
    names
}

fn collect_ids(ids: &[TypeId], surface: &Surface<'_>, names: &mut Vec<ThrowsName>) {
    for &id in ids {
        for decl in surface.get(id) {
            collect_decl(decl, surface, names);
        }
    }
}

fn collect_decl(decl: &TypeDeclaration, surface: &Surface<'_>, names: &mut Vec<ThrowsName>) {
    match &decl.kind {
        TypeKind::Class(c) => collect_members(&c.members, names),
        TypeKind::Interface(i) => collect_members(&i.members, names),
        TypeKind::Function(f) => {
            collect_throws(&f.throws, f.body_scope, names);
            for o in &f.overloads {
                collect_throws(&o.throws, o.body_scope, names);
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

/// Each member's names resolve from the declaration it is written in, which
/// differs between the declarations of a merged type.
fn collect_members(members: &[Member], names: &mut Vec<ThrowsName>) {
    for member in members {
        let (throws, scope) = match member {
            Member::Method(m) => (&m.throws, m.body_scope),
            Member::StaticMethod(m) => (&m.throws, m.body_scope),
            Member::Constructor(c) => (&c.throws, c.scope),
            Member::Getter(g) => (&g.throws, g.scope),
            Member::StaticGetter(g) => (&g.throws, g.scope),
            Member::Setter(_) | Member::StaticSetter(_) | Member::IndexSignature(_) => continue,
        };
        collect_throws(throws, scope, names);
    }
}

fn collect_throws(throws: &Throws, scope: ScopeId, names: &mut Vec<ThrowsName>) {
    let Throws::Type(ty) = throws else { return };
    collect_type(ty, scope, names);
}

fn collect_type(ty: &TypeRef, scope: ScopeId, names: &mut Vec<ThrowsName>) {
    let path = match ty {
        TypeRef::Union(union) => {
            return union
                .members()
                .iter()
                .for_each(|m| collect_type(m, scope, names));
        }
        TypeRef::Reference {
            segments,
            generic_args,
        } if generic_args.is_empty() => segments.join("."),
        // The parser only produces names; anything else reaches
        // `generate_error_enum` as an unbindable name.
        other => format!("{other:?}"),
    };
    let name = ThrowsName { path, scope };
    if !names.contains(&name) {
        names.push(name);
    }
}

/// One enum variant: the declaration a `@throws` name resolves to, or the
/// bare name for a `js_sys` / `--external` class not declared in the input.
struct Variant<'a> {
    name: &'a ThrowsName,
    resolved: Option<TypeId>,
    ident: String,
}

fn resolve(name: &ThrowsName, ctx: &CodegenContext<'_>) -> Option<TypeId> {
    ctx.gctx.resolve_path(name.scope, &name.path)
}

fn last_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// A `@throws` name is bindable when codegen can emit a type for it:
/// declared in the input, a `js_sys` class, or mapped by `--external`.
/// Anything else cannot be a variant payload.
fn is_bindable(path: &str, resolved: Option<TypeId>, ctx: &CodegenContext<'_>) -> bool {
    let name = last_segment(path);
    let external = || ctx.gctx.external_map.resolve_type(name).is_some();
    match resolved {
        Some(id) if ctx.gctx.builtin_type_ids.contains(&id) => {
            JS_SYS_RESERVED.contains(&name) || external()
        }
        Some(id) => ctx.local_type_ids.contains(&id) || external(),
        None => external(),
    }
}

/// A declared type's variant is named by its Rust path, so `A.Fault` and
/// `B.Fault` stay distinct (`AFault`, `BFault`).
fn variant_ident(path: &str, resolved: Option<TypeId>, ctx: &CodegenContext<'_>) -> String {
    let name = last_segment(path);
    let Some(id) = resolved.filter(|id| ctx.local_type_ids.contains(id)) else {
        return name.to_string();
    };
    let decl = ctx.gctx.get_type(id);
    let declared = typemap::type_decl_name(&decl.kind).unwrap_or(name);
    let declared = ctx
        .renamed_locals
        .get(declared)
        .map(String::as_str)
        .unwrap_or(declared);
    let mut ident: String = ctx
        .namespace_path(decl.scope_id)
        .iter()
        .map(|m| to_pascal_case(m))
        .collect();
    ident.push_str(declared);
    ident
}

/// A class in a variant's ancestry: the declaration it resolves to, or the
/// name of a builtin or `--external` class with no declaration.
#[derive(PartialEq)]
enum Ancestor {
    Decl(TypeId),
    Name(String),
}

/// `variant` and its ancestors, ending at `Object`. A declared class's
/// parent resolves from that class's own scope, so `class Sub extends
/// Fault` inside `namespace A` reaches `A.Fault`; a builtin's parents
/// come from the builtin table.
fn ancestry(variant: &Variant<'_>, ctx: &CodegenContext<'_>) -> Vec<Ancestor> {
    let mut chain = Vec::new();
    let mut name = last_segment(&variant.name.path).to_string();
    let mut next = Some((variant.resolved, variant.name.scope));
    while let Some((id, scope)) = next.take() {
        let ancestor = match id {
            Some(id) => Ancestor::Decl(id),
            None => Ancestor::Name(name.clone()),
        };
        if chain.contains(&ancestor) {
            break;
        }
        chain.push(ancestor);
        let parent = match id.filter(|id| !ctx.gctx.builtin_type_ids.contains(id)) {
            Some(id) => {
                let decl = ctx.gctx.get_type(id);
                let extends = match &decl.kind {
                    TypeKind::Class(c) => c.extends.as_ref(),
                    TypeKind::Interface(i) => i.extends.first(),
                    _ => None,
                };
                match extends {
                    Some(Heritage {
                        type_ref: TypeRef::Reference { segments, .. },
                        scope,
                    }) => Some((segments.join("."), *scope)),
                    _ => None,
                }
            }
            None => builtin_parents(&name)
                .first()
                .map(|p| (p.to_string(), scope)),
        };
        if let Some((path, scope)) = parent {
            next = Some((ctx.gctx.resolve_path(scope, &path), scope));
            name = last_segment(&path).to_string();
        }
    }
    if name != "Object" {
        chain.push(Ancestor::Name("Object".to_string()));
    }
    chain
}

/// Dispatch order for the `From<JsValue>` chain. A type precedes every
/// ancestor in its chain; otherwise deeper chains go first. A type whose
/// ancestry ts-gen cannot see (an `--external` mapping with no `extends`
/// in the input) goes before every builtin-rooted one, since an external
/// error class almost certainly extends `Error` and would otherwise be
/// swallowed by that arm. Ties keep source order.
fn order_variants<'a>(variants: Vec<Variant<'a>>, ctx: &CodegenContext<'_>) -> Vec<Variant<'a>> {
    let chains: Vec<Vec<Ancestor>> = variants.iter().map(|v| ancestry(v, ctx)).collect();
    let opaque = |i: usize| {
        chains[i].len() == 2
            && ctx
                .gctx
                .external_map
                .resolve_type(last_segment(&variants[i].name.path))
                .is_some()
    };
    let priority = |i: usize| (Reverse(opaque(i)), Reverse(chains[i].len()), i);

    let mut placed = vec![false; variants.len()];
    let mut order = Vec::with_capacity(variants.len());
    while order.len() < variants.len() {
        let pending = || (0..variants.len()).filter(|&i| !placed[i]);
        let free = pending()
            .filter(|&i| !pending().any(|j| j != i && chains[j][1..].contains(&chains[i][0])));
        let next = free
            .min_by_key(|&i| priority(i))
            .or_else(|| pending().min_by_key(|&i| priority(i)))
            .expect("pending variants remain");
        placed[next] = true;
        order.push(next);
    }
    let mut variants: Vec<Option<Variant<'a>>> = variants.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| variants[i].take().expect("placed once"))
        .collect()
}

/// Emit the enum plus its `From<JsValue>` / `Into<JsValue>` / `Display` /
/// `Error` impls, variants in [`order_variants`] order. Unbindable names
/// are reported and skipped — bindings that named them still return the
/// enum, landing in `Other`. Names resolving to one declaration (`Fault`
/// inside `Ns`, `Ns.Fault` outside) share a variant.
pub fn generate_error_enum(
    name: &str,
    throws_names: &[ThrowsName],
    ctx: &CodegenContext<'_>,
) -> TokenStream {
    let from_module = ModuleContext::Global;

    if ctx.local_types.contains(name) || ctx.renamed_locals.values().any(|n| n == name) {
        ctx.error(format!(
            "--error-enum name `{name}` collides with a declared type; pass a different name"
        ));
    }

    let mut seen = HashSet::new();
    let mut variants = Vec::new();
    for throws in throws_names {
        let path = &throws.path;
        let resolved = resolve(throws, ctx);
        if !is_bindable(path, resolved, ctx) {
            ctx.error(format!(
                "`@throws {{{path}}}` names a type that is not declared in the input, a `js_sys` \
                 error class, or mapped with --external; it is omitted from `{name}`"
            ));
            continue;
        }
        let key = resolved.ok_or_else(|| last_segment(path).to_string());
        if seen.insert(key) {
            variants.push(Variant {
                name: throws,
                resolved,
                ident: variant_ident(path, resolved, ctx),
            });
        }
    }
    let variants = order_variants(variants, ctx);

    let enum_ident = typemap::make_ident(name);
    let idents: Vec<syn::Ident> = variants
        .iter()
        .map(|v| typemap::make_ident(&v.ident))
        .collect();
    let payloads: Vec<TokenStream> = variants
        .iter()
        .map(|v| {
            let reference = TypeRef::Reference {
                segments: v.name.path.split('.').map(String::from).collect(),
                generic_args: Vec::new(),
            };
            typemap::to_syn_type(
                &reference,
                TypePosition::RETURN,
                Some(ctx),
                v.name.scope,
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

    fn with_ctx(src: &str, f: impl FnOnce(&CodegenContext<'_>, &[ThrowsName])) {
        with_ctx_external(src, &[], f)
    }

    fn with_ctx_external(
        src: &str,
        externals: &[&str],
        f: impl FnOnce(&CodegenContext<'_>, &[ThrowsName]),
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
        with_ctx(src, |ctx, names| {
            let mut paths: Vec<&str> = names.iter().map(|n| n.path.as_str()).collect();
            paths.dedup();
            assert_eq!(paths, &["TypeError", "RangeError", "Error", "SyntaxError"]);
            let tokens = generate_error_enum("JsException", names, ctx).to_string();
            assert_eq!(
                tokens.matches("RangeError (RangeError)").count(),
                1,
                "{tokens}"
            );
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

    #[test]
    fn qualified_names_resolve_through_namespaces() {
        let src = r#"
            declare namespace A {
                class Fault extends Error {}
                class B {
                    /** @throws {Fault} */
                    f(): void;
                }
            }
            declare namespace C {
                class Fault extends Error {}
            }
            declare class D {
                /** @throws {A.Fault | C.Fault | Error} */
                g(): void;
            }
        "#;
        with_ctx(src, |ctx, names| {
            let tokens = generate_error_enum("JsException", names, ctx).to_string();
            assert!(tokens.contains("AFault (a :: Fault)"), "{tokens}");
            assert!(tokens.contains("CFault (c :: Fault)"), "{tokens}");
            assert_eq!(tokens.matches("AFault (a :: Fault)").count(), 1, "{tokens}");
            assert!(tokens.find("CFault (").unwrap() < tokens.find("Error (Error)").unwrap());
            assert!(ctx.take_diagnostics().diagnostics.is_empty());
        });
    }

    #[test]
    fn namespaced_subclass_precedes_its_parent() {
        let src = r#"
            declare namespace A {
                class Fault extends Error {}
                class Sub extends Fault {}
            }
            declare class D {
                /** @throws {A.Fault | A.Sub | Error} */
                g(): void;
            }
        "#;
        with_ctx(src, |ctx, names| {
            let tokens = generate_error_enum("JsException", names, ctx).to_string();
            let order: Vec<usize> = ["ASub (a :: Sub)", "AFault (a :: Fault)", "Error (Error)"]
                .iter()
                .map(|v| tokens.find(v).unwrap_or_else(|| panic!("missing {v}")))
                .collect();
            assert!(order[0] < order[1] && order[1] < order[2], "{tokens}");
        });
    }

    #[test]
    fn unresolved_qualified_name_is_reported() {
        let src = r#"
            declare namespace A {}
            declare class B {
                /** @throws {A.Nope} */
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
                .any(|d| d.message.contains("`@throws {A.Nope}`")));
        });
    }
}
