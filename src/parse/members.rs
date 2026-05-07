//! Convert oxc class/interface member AST nodes to our IR `Member`.
//!
//! Member converters take an enclosing `parent_scope: ScopeId` plus a
//! `&mut ParseCtx`. Methods (and any other type-parameter-bearing
//! member) create a fresh child scope on the arena, insert their type
//! parameters as `Binding::TypeParam`, and store the resulting
//! `body_scope` on the IR node so codegen can resolve names lexically.

use oxc_ast::ast::*;

use crate::ir::*;
use crate::parse::ctx::ParseCtx;
use crate::parse::docs::JsDocInfo;
use crate::parse::first_pass::converters::interface_from_signatures;
use crate::parse::scope::ScopeId;
use crate::parse::types::{
    binding_pattern_name, convert_formal_params_scoped, convert_ts_type_scoped, convert_type_params,
};
use crate::util::naming::{to_pascal_case, to_snake_case};

/// Split the result of `info_for_span` into a `(doc, info)` pair,
/// defaulting to empty info when no JSDoc is attached.
fn split_info(opt: Option<(String, JsDocInfo)>) -> (Option<String>, JsDocInfo) {
    match opt {
        Some((doc, info)) => (Some(doc), info),
        None => (None, JsDocInfo::default()),
    }
}

/// Push the given type parameters into a fresh child of `parent_scope`
/// and return the new scope. Used by every member converter that
/// declares its own `<T, U, ...>` — methods, free functions, hoisted
/// interfaces, type aliases. The resulting scope ID is what the IR
/// node stores as its `body_scope`, so codegen can resolve `T`
/// against the same chain.
pub(crate) fn create_body_scope(
    type_params: &[TypeParam],
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> ScopeId {
    let body_scope = ctx.scopes.create_child(parent_scope);
    for tp in type_params {
        ctx.scopes.insert_type_param(body_scope, tp.name.clone());
    }
    body_scope
}

/// Walk a [`TypeRef`] and collect every bare reference whose name
/// resolves to a [`crate::parse::scope::Binding::TypeParam`] in the
/// given scope chain, in source-appearance order, deduped.
///
/// Mirror of `codegen::signatures::collect_type_params` but operating
/// on the parse-time scope arena. Used by iterable-wrapper synthesis
/// to figure out which type parameters need to bubble up onto the
/// synthesized wrapper.
fn collect_in_scope_type_params(
    ty: &TypeRef,
    scope: ScopeId,
    scopes: &crate::parse::scope::ScopeArena,
    out: &mut Vec<String>,
) {
    use crate::parse::scope::Binding;
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            if segments.len() == 1 && generic_args.is_empty() {
                let name = &segments[0];
                if matches!(
                    scopes.resolve_binding(scope, name),
                    Some(Binding::TypeParam),
                ) {
                    if !out.iter().any(|n| n == name) {
                        out.push(name.clone());
                    }
                    return;
                }
            }
            for a in generic_args {
                collect_in_scope_type_params(a, scope, scopes, out);
            }
        }
        TypeRef::Array(inner) | TypeRef::Nullable(inner) => {
            collect_in_scope_type_params(inner, scope, scopes, out);
        }
        TypeRef::Union(members) | TypeRef::Intersection(members) | TypeRef::Tuple(members) => {
            for m in members {
                collect_in_scope_type_params(m, scope, scopes, out);
            }
        }
        TypeRef::Function(sig) => {
            for p in &sig.params {
                collect_in_scope_type_params(&p.type_ref, scope, scopes, out);
            }
            collect_in_scope_type_params(&sig.return_type, scope, scopes, out);
        }
        _ => {}
    }
}

/// If `ty` is a top-level `Iterable<T>` or `AsyncIterable<T>` return,
/// synthesize a wrapper interface exposing `[Symbol.iterator]()` (or
/// `[Symbol.asyncIterator]()`) and rewrite the return to reference
/// the wrapper. Returns the new return type, or `None` when no
/// rewriting was needed (the caller falls back to the original type).
///
/// The synthesized wrapper:
///
/// * Lives next to the parent declaration, pushed onto `ctx.synth`.
/// * Inherits any in-scope type parameters mentioned by the item
///   type (so `list<T>(): Iterable<[string, T]>` produces
///   `KeyValueStoreList<T>` rather than erasing `T`). The wrapper's
///   own body scope binds those parameters as [`Binding::TypeParam`].
/// * Has a single method `iterator` (or `async_iterator`) returning
///   `Iterator<T>` / `AsyncIterator<T>` — codegen recognises those
///   heads and lowers them to `js_sys::Iterator` / `AsyncIterator`.
///
/// Nested occurrences (inside a union, array, etc.) are not
/// synthesized; they erase to `JsValue` at codegen time.
pub(crate) fn try_synthesize_iterable_return(
    ty: &TypeRef,
    parent_name: &str,
    member_name: &str,
    enclosing_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<TypeRef> {
    let (is_async, item_type) = match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } if segments.len() == 1 && !generic_args.is_empty() => match segments[0].as_str() {
            "Iterable" => (false, generic_args[0].clone()),
            "AsyncIterable" => (true, generic_args[0].clone()),
            _ => return None,
        },
        _ => return None,
    };

    // Type parameters mentioned by the item type bubble up onto the
    // synthesized wrapper so it can carry them: an `Iterable<[string, T]>`
    // return becomes `<Parent><member><T>` rather than erasing `T`.
    let mut tp_names = Vec::new();
    collect_in_scope_type_params(&item_type, enclosing_scope, ctx.scopes, &mut tp_names);
    let synth_type_params: Vec<TypeParam> = tp_names
        .into_iter()
        .map(|name| TypeParam {
            name,
            constraint: None,
            default: None,
        })
        .collect();

    let synth_name = unique_type_name(parent_name, member_name, ctx.used_type_names);
    ctx.used_type_names.insert(synth_name.clone());

    // Body scope of the synthesized wrapper inherits the parent
    // method's scope and binds the wrapper's own type parameters.
    let body_scope = create_body_scope(&synth_type_params, enclosing_scope, ctx);

    // wasm-bindgen's `js_name` syntax for symbol-keyed methods is the
    // bracketed `[Symbol.foo]` form (matching JS computed-property
    // syntax), not bare `Symbol.foo`.
    let (symbol_name, iter_head) = if is_async {
        ("[Symbol.asyncIterator]", "AsyncIterator")
    } else {
        ("[Symbol.iterator]", "Iterator")
    };
    let iter_return = TypeRef::generic(iter_head, vec![item_type]);

    // The iterator method's body scope is the wrapper's body scope —
    // it has no type parameters of its own, but should resolve the
    // wrapper's `<T, ...>` lexically.
    let iter_method_body = ctx.scopes.create_child(body_scope);
    let iter_method = Member::Method(MethodMember {
        name: if is_async {
            "async_iterator".to_string()
        } else {
            "iterator".to_string()
        },
        js_name: symbol_name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: iter_return,
        optional: false,
        doc: Some(format!(
            "Conformance to the JS {} protocol — returns the underlying iterator.",
            if is_async {
                "async iteration"
            } else {
                "iteration"
            }
        )),
        throws: Throws::None,
        body_scope: iter_method_body,
    });

    ctx.synth.push(InterfaceDecl {
        name: synth_name.clone(),
        js_name: synth_name.clone(),
        type_params: synth_type_params.clone(),
        extends: Vec::new(),
        members: vec![iter_method],
        classification: crate::ir::InterfaceClassification::ClassLike,
        body_scope,
    });

    // The return type references the synthesized type with the same
    // type-param instantiation, so callers see `<Parent><member><T>`
    // (where `T` is in the parent method's scope).
    let generic_args: Vec<TypeRef> = synth_type_params
        .into_iter()
        .map(|tp| TypeRef::ident(tp.name))
        .collect();
    Some(TypeRef::Reference {
        segments: vec![synth_name],
        generic_args,
    })
}

/// Like `convert_formal_params`, but additionally hoists any directly-
/// inline `TSTypeLiteral` parameter types into named `InterfaceDecl`s
/// using the existing interface-building pipeline.
///
/// The synthesized name is `<Parent><Member>` PascalCased, deduped
/// against `ctx.used_type_names` with a numeric suffix on collision.
/// Hoisted interfaces are appended to `ctx.synth`.
///
/// Anonymous types nested inside generics, unions, etc. (rather than
/// flat at the top of the parameter type) are not hoisted — those
/// still erase to `Object`. Real-world `.d.ts` patterns put the
/// literal at the top level (e.g. `send(builder: { ... })`); deeper
/// hoisting is a follow-up.
pub(crate) fn convert_formal_params_with_synthesis(
    params: &FormalParameters<'_>,
    parent_name: &str,
    member_name: &str,
    scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Param> {
    let mut result_params = Vec::new();

    for (i, param) in params.items.iter().enumerate() {
        // Capture the original (un-snake_cased) parameter name first so we
        // can use it as the synthesized type's name segment. The Rust
        // param name still goes through snake_case below.
        let raw_param_name = binding_pattern_name(&param.pattern);
        let name = raw_param_name
            .as_deref()
            .map(to_snake_case)
            .unwrap_or_else(|| format!("arg{i}"));

        let type_ref = match param.type_annotation.as_ref() {
            Some(ann) => match try_synthesize_inline_param(
                &ann.type_annotation,
                parent_name,
                raw_param_name.as_deref().unwrap_or(member_name),
                scope,
                ctx,
            ) {
                Some(synth) => synth,
                None => convert_ts_type_scoped(&ann.type_annotation, scope, ctx),
            },
            None => TypeRef::Any,
        };

        let optional = param.optional;
        result_params.push(Param {
            name,
            type_ref,
            optional,
            variadic: false,
        });
    }

    if let Some(rest) = &params.rest {
        let name = binding_pattern_name(&rest.rest.argument).unwrap_or_else(|| "rest".to_string());
        let type_ref = rest
            .type_annotation
            .as_ref()
            .map(|ann| convert_ts_type_scoped(&ann.type_annotation, scope, ctx))
            .unwrap_or(TypeRef::Array(Box::new(TypeRef::Any)));
        result_params.push(Param {
            name,
            type_ref,
            optional: false,
            variadic: true,
        });
    }

    result_params
}

/// Compute a unique synthesized type name for an anonymous interface
/// hoisted from `<Parent>.<member>(...)`. Falls back to numeric
/// suffixes on collision (`Foo`, `Foo2`, `Foo3`, ...).
fn unique_type_name(
    parent: &str,
    member: &str,
    used: &std::collections::HashSet<String>,
) -> String {
    let base = format!("{}{}", parent, to_pascal_case(member));
    if !used.contains(&base) {
        return base;
    }
    for i in 2.. {
        let candidate = format!("{base}{i}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("HashSet exhaustion is impossible in practice");
}

/// Try to synthesize an anonymous-interface hoist for a parameter type.
///
/// Recognised shapes:
///
/// * Bare `{ ... }` — directly hoisted into a single interface whose
///   members are the literal's members.
/// * `{ ... } | { ... } | …` where every union branch is itself a
///   type literal — structurally merged into a single interface (see
///   [`merge_member_branches`]).
///
/// Anything else returns `None` so the caller falls back to the
/// regular type-mapping rules.
fn try_synthesize_inline_param(
    ts_type: &TSType<'_>,
    parent_name: &str,
    segment: &str,
    scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<TypeRef> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => {
            let synth_name = unique_type_name(parent_name, segment, ctx.used_type_names);
            ctx.used_type_names.insert(synth_name.clone());
            // Methods inside the hoisted interface may carry their own
            // anonymous parameter types — recurse and let those land in
            // `ctx.synth` alongside the parent.
            let iface = interface_from_signatures(
                synth_name.clone(),
                Vec::new(),
                Vec::new(),
                &literal.members,
                scope,
                ctx,
            );
            ctx.synth.push(iface);
            Some(TypeRef::ident(synth_name))
        }
        TSType::TSUnionType(union) if all_type_literals(&union.types) => {
            let synth_name = unique_type_name(parent_name, segment, ctx.used_type_names);
            ctx.used_type_names.insert(synth_name.clone());
            // Convert each branch through the regular signature pipeline
            // first, then structurally merge at the IR level.
            let branches: Vec<Vec<Member>> = union
                .types
                .iter()
                .filter_map(|t| match t {
                    TSType::TSTypeLiteral(lit) => Some(
                        lit.members
                            .iter()
                            .flat_map(|sig| {
                                convert_ts_signature(sig, Some(&synth_name), scope, ctx)
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .collect();
            let merged = crate::parse::literal_union::merge_member_branches(&branches);
            let classification = crate::parse::classify::classify_interface(&merged);
            // Hoisted union-merge interfaces have no type parameters of
            // their own, so the body scope is just the parent scope —
            // no need to create an extra child.
            let body_scope = scope;
            ctx.synth.push(InterfaceDecl {
                name: synth_name.clone(),
                js_name: synth_name.clone(),
                type_params: Vec::new(),
                extends: Vec::new(),
                members: merged,
                classification,
                body_scope,
            });
            Some(TypeRef::ident(synth_name))
        }
        _ => None,
    }
}

/// Return true when every entry in a union is a type literal — the
/// signal for "this is an anonymous-interface union we can merge."
fn all_type_literals(types: &[TSType<'_>]) -> bool {
    !types.is_empty() && types.iter().all(|t| matches!(t, TSType::TSTypeLiteral(_)))
}

/// Convert a `TSSignature` (interface body member) to our IR
/// `Member`(s).
///
/// `parent` is the surrounding type's Rust name when one is available
/// — passed down so that anonymous parameter types inside method
/// signatures can be hoisted into named interfaces. `parent_scope` is
/// the surrounding type's body scope: methods create their own child
/// of this scope to hold their type parameters.
pub fn convert_ts_signature(
    sig: &TSSignature<'_>,
    parent: Option<&str>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    match sig {
        TSSignature::TSPropertySignature(prop) => {
            convert_property_signature(prop, parent_scope, ctx)
        }
        TSSignature::TSMethodSignature(method) => {
            convert_method_signature(method, parent, parent_scope, ctx)
        }
        TSSignature::TSIndexSignature(idx) => convert_index_signature(idx, parent_scope, ctx)
            .into_iter()
            .collect(),
        TSSignature::TSConstructSignatureDeclaration(ctor) => {
            convert_construct_signature(ctor, parent, parent_scope, ctx)
                .into_iter()
                .collect()
        }
        TSSignature::TSCallSignatureDeclaration(_) => {
            ctx.diag
                .warn("Call signatures on interfaces are not supported, skipping");
            vec![]
        }
    }
}

/// Convert a `ClassElement` (class body member) to our IR `Member`(s).
pub fn convert_class_element(
    elem: &ClassElement<'_>,
    parent: Option<&str>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    match elem {
        ClassElement::MethodDefinition(method) => {
            convert_class_method(method, parent, parent_scope, ctx)
        }
        ClassElement::PropertyDefinition(prop) => convert_class_property(prop, parent_scope, ctx),
        ClassElement::AccessorProperty(acc) => convert_accessor_property(acc, parent_scope, ctx),
        ClassElement::TSIndexSignature(idx) => convert_index_signature(idx, parent_scope, ctx)
            .into_iter()
            .collect(),
        ClassElement::StaticBlock(_) => vec![],
    }
}

// ─── Interface member conversions ────────────────────────────────────

fn convert_property_signature(
    prop: &TSPropertySignature<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    let js_name = match property_key_name(&prop.key) {
        Some(n) => n,
        None => return vec![],
    };
    let doc = ctx.docs.for_span(prop.span.start);

    let type_ref = prop
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
        .unwrap_or(TypeRef::Any);

    let mut members = vec![Member::Getter(GetterMember {
        js_name: js_name.clone(),
        type_ref: type_ref.clone(),
        optional: prop.optional,
        doc,
    })];

    if !prop.readonly {
        members.push(Member::Setter(SetterMember {
            js_name,
            type_ref,
            doc: None,
        }));
    }

    members
}

fn convert_method_signature(
    method: &TSMethodSignature<'_>,
    parent: Option<&str>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    let js_name = match property_key_name(&method.key) {
        Some(n) => n,
        None => return vec![],
    };
    let (doc, info) = split_info(ctx.docs.info_for_span(method.span.start));

    let type_params = convert_type_params(method.type_parameters.as_ref(), ctx.diag);

    // The method's own type parameters live in a child scope of the
    // enclosing type's body. Type conversion below resolves names
    // against this scope so `T` and friends bind to the local
    // declaration first.
    let body_scope = create_body_scope(&type_params, parent_scope, ctx);

    // Hoist anonymous `{ ... }` parameter types into named interfaces
    // when we know the surrounding parent name. Without `parent` we
    // can't generate a sensible name, so fall back to the regular
    // path that erases inline objects to `Object`.
    let params = match parent {
        Some(p) => {
            convert_formal_params_with_synthesis(&method.params, p, &js_name, body_scope, ctx)
        }
        None => convert_formal_params_scoped(&method.params, body_scope, ctx),
    };
    let mut return_type = method
        .return_type
        .as_ref()
        .map(|rt| convert_ts_type_scoped(&rt.type_annotation, body_scope, ctx))
        .unwrap_or(TypeRef::Void);

    // Hoist top-level `Iterable<T>` / `AsyncIterable<T>` returns into
    // synthesized wrapper interfaces with a `[Symbol.iterator]` method.
    if let Some(parent_name) = parent {
        if let Some(rewritten) =
            try_synthesize_iterable_return(&return_type, parent_name, &js_name, body_scope, ctx)
        {
            return_type = rewritten;
        }
    }

    match method.kind {
        TSMethodSignatureKind::Get => vec![Member::Getter(GetterMember {
            js_name,
            type_ref: return_type,
            optional: method.optional,
            doc,
        })],
        TSMethodSignatureKind::Set => {
            let type_ref = params
                .into_iter()
                .next()
                .map(|p| p.type_ref)
                .unwrap_or(TypeRef::Any);
            vec![Member::Setter(SetterMember {
                js_name,
                type_ref,
                doc,
            })]
        }
        TSMethodSignatureKind::Method => vec![Member::Method(MethodMember {
            name: to_snake_case(&js_name),
            js_name,
            type_params,
            params,
            return_type,
            optional: method.optional,
            doc,
            throws: info.throws(),
            body_scope,
        })],
    }
}

fn convert_index_signature(
    idx: &TSIndexSignature<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<Member> {
    let key_type = idx
        .parameters
        .first()
        .map(|p| convert_ts_type_scoped(&p.type_annotation.type_annotation, parent_scope, ctx))
        .unwrap_or(TypeRef::String);

    let value_type =
        convert_ts_type_scoped(&idx.type_annotation.type_annotation, parent_scope, ctx);

    Some(Member::IndexSignature(IndexSigMember {
        key_type,
        value_type,
        readonly: idx.readonly,
    }))
}

fn convert_construct_signature(
    ctor: &TSConstructSignatureDeclaration<'_>,
    parent: Option<&str>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<Member> {
    // Constructors have no surface-level type parameters of their own
    // — they instantiate the parent type's parameters. Use the parent
    // scope directly.
    let params = match parent {
        Some(p) => {
            convert_formal_params_with_synthesis(&ctor.params, p, "Constructor", parent_scope, ctx)
        }
        None => convert_formal_params_scoped(&ctor.params, parent_scope, ctx),
    };
    let (doc, info) = split_info(ctx.docs.info_for_span(ctor.span.start));
    Some(Member::Constructor(ConstructorMember {
        params,
        doc,
        throws: info.throws(),
    }))
}

// ─── Class member conversions ────────────────────────────────────────

fn convert_class_method(
    method: &MethodDefinition<'_>,
    parent: Option<&str>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    let js_name = match property_key_name(&method.key) {
        Some(n) => n,
        None => return vec![],
    };
    let (doc, info) = split_info(ctx.docs.info_for_span(method.span.start));

    let func = &method.value;
    let type_params = convert_type_params(func.type_parameters.as_ref(), ctx.diag);

    // Constructors don't introduce their own type parameter scope —
    // they use the class body scope directly. Other class methods do.
    let body_scope = match method.kind {
        MethodDefinitionKind::Constructor => parent_scope,
        _ => create_body_scope(&type_params, parent_scope, ctx),
    };

    let params = match parent {
        Some(p) => {
            // Constructors use a special "Constructor" segment so the
            // synthesized type reads `<Parent>Constructor*` rather
            // than `<Parent>` alone (which would clash with the
            // parent itself).
            let member_name = match method.kind {
                MethodDefinitionKind::Constructor => "Constructor".to_string(),
                _ => js_name.clone(),
            };
            convert_formal_params_with_synthesis(&func.params, p, &member_name, body_scope, ctx)
        }
        None => convert_formal_params_scoped(&func.params, body_scope, ctx),
    };
    let mut return_type = func
        .return_type
        .as_ref()
        .map(|rt| convert_ts_type_scoped(&rt.type_annotation, body_scope, ctx))
        .unwrap_or(TypeRef::Void);

    // Same iterable hoisting as `convert_method_signature`. Skip for
    // constructors — they don't have a meaningful return type.
    if !matches!(method.kind, MethodDefinitionKind::Constructor) {
        if let Some(parent_name) = parent {
            if let Some(rewritten) =
                try_synthesize_iterable_return(&return_type, parent_name, &js_name, body_scope, ctx)
            {
                return_type = rewritten;
            }
        }
    }

    let is_static = method.r#static;

    match method.kind {
        MethodDefinitionKind::Constructor => {
            vec![Member::Constructor(ConstructorMember {
                params,
                doc,
                throws: info.throws(),
            })]
        }
        MethodDefinitionKind::Get => {
            if is_static {
                vec![Member::StaticGetter(StaticGetterMember {
                    js_name,
                    type_ref: return_type,
                    doc,
                })]
            } else {
                vec![Member::Getter(GetterMember {
                    js_name,
                    type_ref: return_type,
                    optional: method.optional,
                    doc,
                })]
            }
        }
        MethodDefinitionKind::Set => {
            let type_ref = params
                .into_iter()
                .next()
                .map(|p| p.type_ref)
                .unwrap_or(TypeRef::Any);
            if is_static {
                vec![Member::StaticSetter(StaticSetterMember {
                    js_name,
                    type_ref,
                    doc,
                })]
            } else {
                vec![Member::Setter(SetterMember {
                    js_name,
                    type_ref,
                    doc,
                })]
            }
        }
        MethodDefinitionKind::Method => {
            if is_static {
                vec![Member::StaticMethod(StaticMethodMember {
                    name: to_snake_case(&js_name),
                    js_name,
                    type_params,
                    params,
                    return_type,
                    doc,
                    throws: info.throws(),
                    body_scope,
                })]
            } else {
                vec![Member::Method(MethodMember {
                    name: to_snake_case(&js_name),
                    js_name,
                    type_params,
                    params,
                    return_type,
                    optional: method.optional,
                    doc,
                    throws: info.throws(),
                    body_scope,
                })]
            }
        }
    }
}

fn convert_class_property(
    prop: &PropertyDefinition<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    let js_name = match property_key_name(&prop.key) {
        Some(n) => n,
        None => return vec![],
    };
    let doc = ctx.docs.for_span(prop.span.start);

    let type_ref = prop
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
        .unwrap_or(TypeRef::Any);

    if prop.r#static {
        let mut members = vec![Member::StaticGetter(StaticGetterMember {
            js_name: js_name.clone(),
            type_ref: type_ref.clone(),
            doc,
        })];
        if !prop.readonly {
            members.push(Member::StaticSetter(StaticSetterMember {
                js_name,
                type_ref,
                doc: None,
            }));
        }
        members
    } else {
        let mut members = vec![Member::Getter(GetterMember {
            js_name: js_name.clone(),
            type_ref: type_ref.clone(),
            optional: prop.optional,
            doc,
        })];
        if !prop.readonly {
            members.push(Member::Setter(SetterMember {
                js_name,
                type_ref,
                doc: None,
            }));
        }
        members
    }
}

fn convert_accessor_property(
    acc: &AccessorProperty<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    let js_name = match property_key_name(&acc.key) {
        Some(n) => n,
        None => return vec![],
    };
    let doc = ctx.docs.for_span(acc.span.start);

    let type_ref = acc
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
        .unwrap_or(TypeRef::Any);

    if acc.r#static {
        vec![
            Member::StaticGetter(StaticGetterMember {
                js_name: js_name.clone(),
                type_ref: type_ref.clone(),
                doc,
            }),
            Member::StaticSetter(StaticSetterMember {
                js_name,
                type_ref,
                doc: None,
            }),
        ]
    } else {
        vec![
            Member::Getter(GetterMember {
                js_name: js_name.clone(),
                type_ref: type_ref.clone(),
                optional: false,
                doc,
            }),
            Member::Setter(SetterMember {
                js_name,
                type_ref,
                doc: None,
            }),
        ]
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Extract a string name from a `PropertyKey`.
pub fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
        _ => None,
    }
}
