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
use crate::parse::literal_union::{merge_member_branches, MergeMode};
use crate::parse::scope::ScopeId;
use crate::parse::types::{
    binding_pattern_name, convert_formal_params_scoped, convert_ts_type_scoped, convert_type_params,
};
use crate::util::naming::{to_pascal_case, to_snake_case, LocalScope, Suffix};

/// Split the result of `info_for_span` into a `(doc, info)` pair,
/// defaulting to empty info when no JSDoc is attached.
fn split_info(opt: Option<(String, JsDocInfo)>) -> (Option<String>, JsDocInfo) {
    match opt {
        Some((doc, info)) => (Some(doc), info),
        None => (None, JsDocInfo::default()),
    }
}

/// Parse a `{@slice <mode>}` mode name into a [`SliceModeHint`], warning
/// on an unknown mode.
fn slice_hint(
    mode: Option<&str>,
    site: &str,
    diag: &mut crate::util::diagnostics::DiagnosticCollector,
) -> Option<crate::ir::SliceModeHint> {
    let mode = mode?;
    let hint = crate::ir::SliceModeHint::parse(mode);
    if hint.is_none() {
        diag.warn(format!(
            "unknown slice mode `{{@slice {mode}}}` on `{site}`; expected `view`, `array`, `copy` or `handle`"
        ));
    }
    hint
}

/// Parse a `{@string <mode>}` mode name into a [`StringModeHint`],
/// warning on an unknown mode.
pub(crate) fn string_hint(
    mode: Option<&str>,
    site: &str,
    diag: &mut crate::util::diagnostics::DiagnosticCollector,
) -> Option<StringModeHint> {
    let mode = mode?;
    let hint = StringModeHint::parse(mode);
    if hint.is_none() {
        diag.warn(format!(
            "unknown string mode `{{@string {mode}}}` on `{site}`; expected `copy`, `ref` or `both`"
        ));
    }
    hint
}

/// A declaration-level `{@string mode}` is the default of each member
/// that returns a value and carries no tag of its own.
pub(crate) fn apply_string_default(members: &mut [Member], mode: Option<StringModeHint>) {
    let Some(mode) = mode else { return };
    for m in members {
        let slot = match m {
            Member::Getter(g) => &mut g.string_mode,
            Member::StaticGetter(g) => &mut g.string_mode,
            Member::Method(m) => &mut m.string_mode,
            Member::StaticMethod(m) => &mut m.string_mode,
            _ => continue,
        };
        slot.get_or_insert(mode);
    }
}

/// Attach `{@slice mode}` hints from `@param` tags to the matching
/// parameters, warning when a tag names no parameter.
pub(crate) fn apply_slice_hints(
    params: &mut [Param],
    info: &JsDocInfo,
    site: &str,
    diag: &mut crate::util::diagnostics::DiagnosticCollector,
) {
    for (name, mode) in &info.param_slice_modes {
        let Some(hint) = slice_hint(Some(mode), site, diag) else {
            continue;
        };
        let ir_name = to_snake_case(name);
        match params.iter_mut().find(|p| p.name == ir_name) {
            Some(p) => {
                p.slice_mode = Some(hint);
                if let Some(widened) = hint.widen_bare_typed_array(&p.type_ref) {
                    p.type_ref = widened;
                }
            }
            None => diag.warn(format!(
                "`{{@slice {mode}}}` on `{site}` names unknown parameter `{name}`"
            )),
        }
    }
}

/// Apply `@rust <type>` to the member's own type and `@rust <param>:
/// <type>` to the named parameters. Unknown scalars, incompatible
/// leaves and unknown parameter names are error diagnostics (fatal in
/// an `--annotate` side file), not silent no-ops.
pub(crate) fn apply_rust_tags(
    info: &JsDocInfo,
    own: Option<&mut TypeRef>,
    params: &mut [Param],
    member: &str,
    ctx: &mut ParseCtx<'_, '_>,
) {
    use crate::ir::RustScalar;
    if let Some(name) = &info.rust_type {
        match (RustScalar::parse(name), own) {
            (Some(scalar), Some(ty)) => match ty.with_rust_scalar(scalar) {
                Some(forced) => *ty = forced,
                None => ctx.diag.error(incompatible(name, member, ty)),
            },
            (Some(_), None) => ctx.diag.error(format!(
                "`@rust {name}` on `{member}` has no type to apply to"
            )),
            (None, _) => ctx
                .diag
                .error(format!("`@rust {name}` on `{member}`: unknown scalar type")),
        }
    }
    for (param, name) in &info.rust_params {
        let Some(scalar) = RustScalar::parse(name) else {
            ctx.diag.error(format!(
                "`@rust {param}: {name}` on `{member}`: unknown scalar type"
            ));
            continue;
        };
        // IR parameter names are already snake_case; tags use the TS name.
        let ir_name = to_snake_case(param);
        match params.iter_mut().find(|p| p.name == ir_name) {
            Some(p) => match p.type_ref.with_rust_scalar(scalar) {
                Some(forced) => p.type_ref = forced,
                None => ctx.diag.error(incompatible(
                    name,
                    &format!("{member}({param})"),
                    &p.type_ref,
                )),
            },
            None => ctx.diag.error(format!(
                "`@rust {param}: {name}` on `{member}`: no such parameter"
            )),
        }
    }
}

/// Diagnostic for a `@rust` scalar whose target type cannot carry it.
pub(crate) fn incompatible(scalar: &str, member: &str, ty: &TypeRef) -> String {
    format!(
        "`@rust {scalar}` on `{member}`: `{}` cannot cross as `{scalar}` (u64/i64 need \
         `number` or `bigint`, other scalars `number`)",
        ty.format_ts()
    )
}

/// Push the given type parameters into a fresh child of `parent_scope`
/// and return the new scope. Used by every member converter that
/// declares its own `<T, U, ...>` — methods, free functions, hoisted
/// interfaces, type aliases. The resulting scope ID is what the IR
/// node stores as its `body_scope`, so codegen can resolve `T`
/// against the same chain.
///
/// Parameters that erase to a primitive (see [`TypeParam::erasure`]) are
/// bound as that primitive and removed from `type_params`, so the
/// declaration they belong to is emitted without them.
pub(crate) fn create_body_scope(
    type_params: &mut Vec<TypeParam>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> ScopeId {
    let body_scope = ctx.scopes.create_child(parent_scope);
    ctx.scopes.bind_type_params(body_scope, type_params);
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
        TypeRef::Array(inner) => {
            collect_in_scope_type_params(inner, scope, scopes, out);
        }
        TypeRef::Union(u) => {
            for m in u.members() {
                collect_in_scope_type_params(m, scope, scopes, out);
            }
        }
        TypeRef::Intersection(members) | TypeRef::Tuple(members) | TypeRef::HeadRest(members) => {
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
    let mut synth_type_params: Vec<TypeParam> = tp_names
        .into_iter()
        .map(|name| TypeParam {
            name,
            constraint: None,
            default: None,
        })
        .collect();

    let synth_name = unique_type_name(parent_name, member_name, ctx.used_type_names);

    // Body scope of the synthesized wrapper inherits the parent
    // method's scope and binds the wrapper's own type parameters.
    let body_scope = create_body_scope(&mut synth_type_params, enclosing_scope, ctx);

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
        async_modifier: false,
        body_scope: iter_method_body,
        string_mode: None,
    });

    ctx.synth.push(InterfaceDecl {
        construct: Vec::new(),
        name: synth_name.clone(),
        js_name: synth_name.clone(),
        type_params: synth_type_params.clone(),
        extends: Vec::new(),
        members: vec![iter_method],
        classification: crate::ir::InterfaceClassification::ClassLike,
        body_scope,
        string_mode: None,
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
/// The synthesized name is `<Parent><Member>` PascalCased (see
/// [`crate::parse::ParseOptions`] for the per-mono form), deduped
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

        let segment = match raw_param_name.as_deref() {
            Some(p) if ctx.options.experimental_generic_mono && member_name != "Constructor" => {
                format!("{}{}", to_pascal_case(member_name), to_pascal_case(p))
            }
            Some(p) => p.to_string(),
            None => member_name.to_string(),
        };
        let type_ref = match param.type_annotation.as_ref() {
            Some(ann) => match try_synthesize_inline_param(
                &ann.type_annotation,
                parent_name,
                &segment,
                scope,
                ctx,
            ) {
                Some(synth) => synth,
                None => convert_ts_type_scoped(&ann.type_annotation, scope, ctx),
            },
            None => TypeRef::Any,
        };

        let optional = param.optional || param.initializer.is_some();
        result_params.push(Param {
            name,
            type_ref,
            optional,
            variadic: false,
            slice_mode: None,
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
            slice_mode: None,
        });
    }

    result_params
}

/// Claim a type name for an anonymous interface hoisted from
/// `<Parent>.<member>(...)`: `<Parent><Member>`, numbered on collision.
fn unique_type_name(parent: &str, member: &str, used: &mut LocalScope) -> String {
    used.declare(
        &format!("{parent}{}", to_pascal_case(member)),
        Suffix::Pascal,
    )
}

/// Hoist a method's inline `{ … }` return type (or union of such) the
/// way parameters are hoisted, named `<Parent><Member>Return` to match
/// the `Return` anchor of synthesized return enums.
fn convert_return_type_with_synthesis(
    ts_type: &TSType<'_>,
    parent_name: &str,
    member_name: &str,
    scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> TypeRef {
    let segment = format!("{member_name}Return");
    try_synthesize_inline_param(ts_type, parent_name, &segment, scope, ctx)
        .unwrap_or_else(|| convert_ts_type_scoped(ts_type, scope, ctx))
}

/// An inline type that promotes to a named declaration. Detected
/// syntactically by [`inline_shape`]; both the parameter and the
/// type-alias hoists go through [`synthesize_inline`] with it.
pub(crate) enum InlineShape<'a, 'b> {
    /// `{ … }`
    Literal(&'a TSTypeLiteral<'b>),
    /// `{ … } | { … }` — branches are joined (see `literal_union`).
    Union(Vec<&'a TSTypeLiteral<'b>>),
    /// `A & B & { … }` — references become `extends`, literals are met
    /// into the own members, so the result is exactly the interface the
    /// user would have written as `interface X extends A, B { … }`.
    Intersection {
        parents: Vec<&'a TSType<'b>>,
        literals: Vec<&'a TSTypeLiteral<'b>>,
        /// `({ … } | { … })` parts, each joined into one set of members.
        unions: Vec<Vec<&'a TSTypeLiteral<'b>>>,
        /// Parts that only narrow in TS (a conditional or indexed-access
        /// type, a union not of literals) and have no structural reading;
        /// dropped with a warning.
        dropped: Vec<&'static str>,
    },
}

/// Recognise the shapes that hoist. Anything else returns `None` so the
/// caller falls back to the regular type-mapping rules (an intersection
/// with a primitive or function member, for instance, is bounded at
/// codegen instead).
pub(crate) fn inline_shape<'a, 'b>(ts_type: &'a TSType<'b>) -> Option<InlineShape<'a, 'b>> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => Some(InlineShape::Literal(literal)),
        TSType::TSUnionType(union) if all_type_literals(&union.types) => {
            Some(InlineShape::Union(type_literals(&union.types)))
        }
        TSType::TSIntersectionType(inter) => {
            let mut parents = Vec::new();
            let mut literals = Vec::new();
            let mut unions = Vec::new();
            let mut dropped = Vec::new();
            for t in &inter.types {
                let t = t.without_parenthesized();
                match t {
                    TSType::TSTypeLiteral(lit) => literals.push(&**lit),
                    TSType::TSTypeReference(_) => parents.push(t),
                    TSType::TSUnionType(union) if all_type_literals(&union.types) => {
                        unions.push(type_literals(&union.types))
                    }
                    TSType::TSObjectKeyword(_) | TSType::TSUnknownKeyword(_) => {}
                    TSType::TSConditionalType(_) => dropped.push("conditional type"),
                    TSType::TSUnionType(_) => dropped.push("union"),
                    TSType::TSIndexedAccessType(_) => dropped.push("indexed access type"),
                    _ => return None,
                }
            }
            if parents.is_empty() && literals.is_empty() && unions.is_empty() {
                return None;
            }
            Some(InlineShape::Intersection {
                parents,
                literals,
                unions,
                dropped,
            })
        }
        _ => None,
    }
}

/// The interface a shape promotes to, plus the per-branch members for a
/// union so the alias hoist can check for discriminators.
pub(crate) struct SynthesizedInline {
    pub iface: InterfaceDecl,
    pub branches: Vec<Vec<Member>>,
}

/// Build the interface for `shape` under `name`. `None` when an
/// intersection parent is not something an interface could `extends`:
/// a type parameter, a name that does not resolve here, or one that
/// resolves to anything but a class or interface (an alias to a
/// primitive, say). Top-type parents (`object`, `unknown`) are dropped
/// since they add nothing to the shape.
pub(crate) fn synthesize_inline(
    shape: &InlineShape<'_, '_>,
    name: String,
    mut type_params: Vec<TypeParam>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<SynthesizedInline> {
    let body_scope = create_body_scope(&mut type_params, parent_scope, ctx);
    let (literals, mode, extends, unions) = match shape {
        InlineShape::Literal(lit) => (vec![*lit], MergeMode::Join, Vec::new(), &[][..]),
        InlineShape::Union(lits) => (lits.clone(), MergeMode::Join, Vec::new(), &[][..]),
        InlineShape::Intersection {
            parents,
            literals,
            unions,
            dropped,
        } => {
            let mut extends: Vec<TypeRef> = Vec::new();
            for parent in parents {
                let converted = convert_ts_type_scoped(parent, body_scope, ctx);
                match &converted {
                    TypeRef::Object | TypeRef::Unknown => {}
                    TypeRef::Reference { segments, .. } if segments.len() == 1 => {
                        if !is_extendable(&segments[0], body_scope, ctx) {
                            return None;
                        }
                        if !extends.contains(&converted) {
                            extends.push(converted);
                        }
                    }
                    // Qualified names are not resolved at parse time; the
                    // codegen `extends` lowering drops one that erases.
                    TypeRef::Reference { .. } => {
                        if !extends.contains(&converted) {
                            extends.push(converted);
                        }
                    }
                    _ => return None,
                }
            }
            for kind in dropped {
                ctx.diag.warn(format!(
                    "Intersection `{name}`: dropping a {kind} part (no structural reading)"
                ));
            }
            (
                literals.clone(),
                MergeMode::Meet,
                extends,
                unions.as_slice(),
            )
        }
    };
    let branches: Vec<Vec<Member>> = literals
        .iter()
        .map(|lit| {
            lit.members
                .iter()
                .flat_map(|sig| convert_ts_signature(sig, Some(&name), body_scope, ctx))
                .collect()
        })
        .collect();
    let mut members = merge_member_branches(&branches, mode);
    if !unions.is_empty() {
        // Members declared alongside win over a union part's narrowing
        // (`truncated: boolean` over `true` / `false`).
        let mut union_members = Vec::new();
        for union in unions {
            let union_branches: Vec<Vec<Member>> = union
                .iter()
                .map(|lit| {
                    lit.members
                        .iter()
                        .flat_map(|sig| convert_ts_signature(sig, Some(&name), body_scope, ctx))
                        .collect()
                })
                .collect();
            union_members.extend(merge_member_branches(&union_branches, MergeMode::Join));
        }
        // An intersection tries its first part's overloads first, which
        // `Later` places ahead of the union part's.
        crate::parse::merge_members(&mut union_members, members, crate::parse::Declared::Later);
        members = union_members;
    }
    let classification = crate::parse::classify::classify_interface(&members);
    Some(SynthesizedInline {
        iface: InterfaceDecl {
            construct: Vec::new(),
            name: name.clone(),
            js_name: name,
            type_params,
            extends: extends
                .into_iter()
                .map(|type_ref| crate::ir::Heritage {
                    type_ref,
                    scope: body_scope,
                })
                .collect(),
            members,
            classification,
            body_scope,
            string_mode: None,
        },
        branches,
    })
}

/// Whether a single-segment name may appear in an interface's `extends`.
/// A type parameter cannot; nor can an alias phase 1 already knows is not
/// an object type or a generic alias whose target mentions its own type
/// parameters. A name
/// with no local binding (an import, an `--external` mapping, or a typo)
/// is allowed through, as is an alias whose target is still unknown:
/// codegen resolves them, and drops a parent that erases, keeping the
/// literal's own members.
fn is_extendable(name: &str, scope: ScopeId, ctx: &ParseCtx<'_, '_>) -> bool {
    is_extendable_in(name, scope, ctx, &mut std::collections::HashSet::new())
}

/// [`is_extendable`], following an alias to another name and an
/// intersection alias's own parents, so that each placeholder along the
/// chain is checked.
fn is_extendable_in(
    name: &str,
    scope: ScopeId,
    ctx: &ParseCtx<'_, '_>,
    visited: &mut std::collections::HashSet<crate::context::TypeId>,
) -> bool {
    let id = match ctx.scopes.resolve_binding(scope, name) {
        Some(crate::parse::scope::Binding::Declared(id)) => id,
        Some(crate::parse::scope::Binding::TypeParam | crate::parse::scope::Binding::Erased(_)) => {
            return false
        }
        None => return true,
    };
    // A cycle is not a valid TS type, so never hoists.
    if !visited.insert(id) {
        return false;
    }
    let decl = &ctx.type_arena[id.index()];
    let follows = |target: &TypeRef, visited: &mut std::collections::HashSet<_>| match target {
        TypeRef::Reference { segments, .. } if segments.len() == 1 => {
            is_extendable_in(&segments[0], decl.scope_id, ctx, visited)
        }
        _ => true,
    };
    let extendable = match &decl.kind {
        TypeKind::Class(_) => true,
        TypeKind::Interface(i) => i
            .extends
            .iter()
            .all(|parent| follows(&parent.type_ref, visited)),
        // Lowering the target doesn't substitute the alias's arguments, so
        // `Id<Local>` for `type Id<T> = T` would lower to a bare `T`.
        TypeKind::TypeAlias(a) if mentions_param(&a.target, &a.type_params) => false,
        TypeKind::TypeAlias(a) => match &a.target {
            TypeRef::Any => true,
            target @ TypeRef::Reference { .. } => follows(target, visited),
            _ => false,
        },
        _ => false,
    };
    visited.remove(&id);
    extendable
}

fn mentions_param(ty: &TypeRef, params: &[TypeParam]) -> bool {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            (segments.len() == 1 && params.iter().any(|p| p.name == segments[0]))
                || generic_args.iter().any(|arg| mentions_param(arg, params))
        }
        TypeRef::Array(elem) => mentions_param(elem, params),
        TypeRef::Intersection(parts) => parts.iter().any(|part| mentions_param(part, params)),
        _ => false,
    }
}

/// Try to synthesize an anonymous-interface hoist for a parameter type
/// (see [`inline_shape`] for the recognised forms). The hoisted
/// interface lands in `ctx.synth`; the returned `TypeRef` names it.
///
/// An intersection of named types alone (`Derived & Base`) is not hoisted
/// here: a parameter typed as a fresh subtype would force callers to cast
/// a value they already hold, and the codegen bound names the member the
/// value actually is. An alias supplies a name of its own, so it hoists.
fn try_synthesize_inline_param(
    ts_type: &TSType<'_>,
    parent_name: &str,
    segment: &str,
    scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<TypeRef> {
    let shape = inline_shape(ts_type)?;
    if matches!(&shape, InlineShape::Intersection { literals, unions, .. } if literals.is_empty() && unions.is_empty())
    {
        return None;
    }
    // Named before the body converts so hoists nested in it dedup against
    // it; given back if a parent turns out not to be extendable.
    let synth_name = unique_type_name(parent_name, segment, ctx.used_type_names);
    let Some(synth) = synthesize_inline(&shape, synth_name.clone(), Vec::new(), scope, ctx) else {
        ctx.used_type_names.release(&synth_name);
        return None;
    };
    let mut iface = synth.iface;

    // Enclosing type parameters the literal mentions (`value: T` inside
    // a method of `Cursor<T>`) become the hoisted interface's own, so
    // its getters and builder bind them and the reference instantiates
    // them: `CursorNextReturn<T>`.
    let mut tp_names = Vec::new();
    for member in &iface.members {
        for ty in member_type_refs(member) {
            collect_in_scope_type_params(ty, scope, ctx.scopes, &mut tp_names);
        }
    }
    for name in &tp_names {
        ctx.scopes.insert_type_param(iface.body_scope, name.clone());
    }
    iface.type_params = tp_names
        .iter()
        .map(|name| TypeParam {
            name: name.clone(),
            constraint: None,
            default: None,
        })
        .collect();
    let reference = TypeRef::Reference {
        segments: vec![iface.name.clone()],
        generic_args: tp_names.into_iter().map(TypeRef::ident).collect(),
    };
    ctx.synth.push(iface);
    Some(reference)
}

fn member_type_refs(member: &Member) -> Vec<&TypeRef> {
    match member {
        Member::Getter(g) => vec![&g.type_ref],
        Member::Setter(s) => vec![&s.type_ref],
        Member::StaticGetter(g) => vec![&g.type_ref],
        Member::StaticSetter(s) => vec![&s.type_ref],
        Member::Method(m) => m
            .params
            .iter()
            .map(|p| &p.type_ref)
            .chain(std::iter::once(&m.return_type))
            .collect(),
        Member::StaticMethod(m) => m
            .params
            .iter()
            .map(|p| &p.type_ref)
            .chain(std::iter::once(&m.return_type))
            .collect(),
        Member::Constructor(c) => c.params.iter().map(|p| &p.type_ref).collect(),
        Member::IndexSignature(i) => vec![&i.value_type],
    }
}

/// Return true when every entry in a union is a type literal — the
/// signal for "this is an anonymous-interface union we can merge."
fn all_type_literals(types: &[TSType<'_>]) -> bool {
    !types.is_empty() && types.iter().all(|t| matches!(t, TSType::TSTypeLiteral(_)))
}

fn type_literals<'a, 'b>(types: &'a [TSType<'b>]) -> Vec<&'a TSTypeLiteral<'b>> {
    types
        .iter()
        .filter_map(|t| match t {
            TSType::TSTypeLiteral(lit) => Some(&**lit),
            _ => None,
        })
        .collect()
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
        // An object type with `new` is a constructor *value*; only a
        // `declare var X: ...` static side (handled in `merge.rs`) makes
        // it the constructor of a class.
        TSSignature::TSConstructSignatureDeclaration(_) => {
            ctx.diag.warn(format!(
                "Construct signature on `{}` is not a class constructor, skipping",
                parent.unwrap_or("<anonymous>")
            ));
            vec![]
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
    let (doc, info) = split_info(ctx.docs.info_for_span(prop.span.start));
    let string_mode = string_hint(info.string_mode.as_deref(), &js_name, ctx.diag);
    let slice_mode = slice_hint(info.slice_mode.as_deref(), &js_name, ctx.diag);

    let mut type_ref = prop
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
        .unwrap_or(TypeRef::Any);
    apply_rust_tags(&info, Some(&mut type_ref), &mut [], &js_name, ctx);

    let mut members = vec![Member::Getter(GetterMember {
        scope: parent_scope,
        js_name: js_name.clone(),
        type_ref: type_ref.clone(),
        optional: prop.optional,
        doc,
        throws: info.throws(),
        string_mode,
    })];

    if !prop.readonly {
        members.push(Member::Setter(SetterMember {
            scope: parent_scope,
            js_name,
            type_ref,
            doc: None,
            slice_mode,
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
    let js_name = match property_key_name(&method.key).or_else(|| symbol_key_name(&method.key)) {
        Some(n) => n,
        None => return vec![],
    };
    let (doc, info) = split_info(ctx.docs.info_for_span(method.span.start));
    let string_mode = string_hint(info.string_mode.as_deref(), &js_name, ctx.diag);

    let mut type_params = convert_type_params(method.type_parameters.as_ref(), ctx.diag);

    // The method's own type parameters live in a child scope of the
    // enclosing type's body. Type conversion below resolves names
    // against this scope so `T` and friends bind to the local
    // declaration first.
    let body_scope = create_body_scope(&mut type_params, parent_scope, ctx);

    // Hoist anonymous `{ ... }` parameter types into named interfaces
    // when we know the surrounding parent name. Without `parent` we
    // can't generate a sensible name, so fall back to the regular
    // path that erases inline objects to `Object`.
    let mut params = match parent {
        Some(p) => {
            convert_formal_params_with_synthesis(&method.params, p, &js_name, body_scope, ctx)
        }
        None => convert_formal_params_scoped(&method.params, body_scope, ctx),
    };
    apply_slice_hints(&mut params, &info, &js_name, ctx.diag);
    let slice_mode = slice_hint(info.slice_mode.as_deref(), &js_name, ctx.diag)
        .or_else(|| params.first().and_then(|p| p.slice_mode));
    let mut return_type = match (&method.return_type, parent) {
        (Some(rt), Some(p)) if method.kind == TSMethodSignatureKind::Method => {
            convert_return_type_with_synthesis(&rt.type_annotation, p, &js_name, body_scope, ctx)
        }
        (Some(rt), _) => convert_ts_type_scoped(&rt.type_annotation, body_scope, ctx),
        (None, _) => TypeRef::Void,
    };
    apply_rust_tags(&info, Some(&mut return_type), &mut params, &js_name, ctx);

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
            scope: parent_scope,
            js_name,
            type_ref: return_type,
            optional: method.optional,
            doc,
            throws: info.throws(),
            string_mode,
        })],
        TSMethodSignatureKind::Set => {
            let type_ref = params
                .into_iter()
                .next()
                .map(|p| p.type_ref)
                .unwrap_or(TypeRef::Any);
            vec![Member::Setter(SetterMember {
                scope: parent_scope,
                js_name,
                type_ref,
                doc,
                slice_mode,
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
            async_modifier: false,
            string_mode,
            body_scope,
        })],
    }
}

fn convert_index_signature(
    idx: &TSIndexSignature<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<Member> {
    let key_type = convert_ts_type_scoped(
        &idx.parameter.type_annotation.type_annotation,
        parent_scope,
        ctx,
    );

    let value_type =
        convert_ts_type_scoped(&idx.type_annotation.type_annotation, parent_scope, ctx);

    Some(Member::IndexSignature(IndexSigMember {
        scope: parent_scope,
        key_type,
        value_type,
        readonly: idx.readonly,
    }))
}

// ─── Class member conversions ────────────────────────────────────────

fn convert_class_method(
    method: &MethodDefinition<'_>,
    parent: Option<&str>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<Member> {
    let js_name = match property_key_name(&method.key).or_else(|| symbol_key_name(&method.key)) {
        Some(n) => n,
        None => return vec![],
    };
    let (doc, info) = split_info(ctx.docs.info_for_span(method.span.start));
    let string_mode = string_hint(info.string_mode.as_deref(), &js_name, ctx.diag);

    let func = &method.value;
    let mut type_params = convert_type_params(func.type_parameters.as_ref(), ctx.diag);

    // Constructors don't introduce their own type parameter scope —
    // they use the class body scope directly. Other class methods do.
    let body_scope = match method.kind {
        MethodDefinitionKind::Constructor => parent_scope,
        _ => create_body_scope(&mut type_params, parent_scope, ctx),
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
    let mut params = params;
    apply_slice_hints(&mut params, &info, &js_name, ctx.diag);
    let slice_mode = slice_hint(info.slice_mode.as_deref(), &js_name, ctx.diag)
        .or_else(|| params.first().and_then(|p| p.slice_mode));
    let mut return_type = match (&func.return_type, parent) {
        (Some(rt), Some(p)) if method.kind == MethodDefinitionKind::Method => {
            convert_return_type_with_synthesis(&rt.type_annotation, p, &js_name, body_scope, ctx)
        }
        (Some(rt), _) => convert_ts_type_scoped(&rt.type_annotation, body_scope, ctx),
        (None, _) => TypeRef::Void,
    };
    apply_rust_tags(&info, Some(&mut return_type), &mut params, &js_name, ctx);

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
                scope: parent_scope,
                type_params,
                params,
                doc,
                throws: info.throws(),
                return_type: None,
            })]
        }
        MethodDefinitionKind::Get => {
            if is_static {
                vec![Member::StaticGetter(StaticGetterMember {
                    scope: parent_scope,
                    js_name,
                    type_ref: return_type,
                    optional: false,
                    doc,
                    throws: info.throws(),
                    string_mode,
                })]
            } else {
                vec![Member::Getter(GetterMember {
                    scope: parent_scope,
                    js_name,
                    type_ref: return_type,
                    optional: method.optional,
                    doc,
                    throws: info.throws(),
                    string_mode,
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
                    scope: parent_scope,
                    js_name,
                    type_ref,
                    doc,
                })]
            } else {
                vec![Member::Setter(SetterMember {
                    scope: parent_scope,
                    js_name,
                    type_ref,
                    doc,
                    slice_mode,
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
                    async_modifier: func.r#async,
                    string_mode,
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
                    async_modifier: func.r#async,
                    string_mode,
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
    let (doc, info) = split_info(ctx.docs.info_for_span(prop.span.start));
    let string_mode = string_hint(info.string_mode.as_deref(), &js_name, ctx.diag);
    let slice_mode = slice_hint(info.slice_mode.as_deref(), &js_name, ctx.diag);

    let mut type_ref = prop
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
        .unwrap_or(TypeRef::Any);
    apply_rust_tags(&info, Some(&mut type_ref), &mut [], &js_name, ctx);

    if prop.r#static {
        let mut members = vec![Member::StaticGetter(StaticGetterMember {
            scope: parent_scope,
            js_name: js_name.clone(),
            type_ref: type_ref.clone(),
            optional: prop.optional,
            doc,
            throws: info.throws(),
            string_mode,
        })];
        if !prop.readonly {
            members.push(Member::StaticSetter(StaticSetterMember {
                scope: parent_scope,
                js_name,
                type_ref,
                doc: None,
            }));
        }
        members
    } else {
        let mut members = vec![Member::Getter(GetterMember {
            scope: parent_scope,
            js_name: js_name.clone(),
            type_ref: type_ref.clone(),
            optional: prop.optional,
            doc,
            throws: info.throws(),
            string_mode,
        })];
        if !prop.readonly {
            members.push(Member::Setter(SetterMember {
                scope: parent_scope,
                js_name,
                type_ref,
                doc: None,
                slice_mode,
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
    let (doc, info) = split_info(ctx.docs.info_for_span(acc.span.start));
    let string_mode = string_hint(info.string_mode.as_deref(), &js_name, ctx.diag);

    let type_ref = acc
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
        .unwrap_or(TypeRef::Any);

    if acc.r#static {
        vec![
            Member::StaticGetter(StaticGetterMember {
                scope: parent_scope,
                js_name: js_name.clone(),
                type_ref: type_ref.clone(),
                optional: false,
                doc,
                throws: info.throws(),
                string_mode,
            }),
            Member::StaticSetter(StaticSetterMember {
                scope: parent_scope,
                js_name,
                type_ref,
                doc: None,
            }),
        ]
    } else {
        vec![
            Member::Getter(GetterMember {
                scope: parent_scope,
                js_name: js_name.clone(),
                type_ref: type_ref.clone(),
                optional: false,
                doc,
                throws: info.throws(),
                string_mode,
            }),
            Member::Setter(SetterMember {
                scope: parent_scope,
                js_name,
                type_ref,
                doc: None,
                slice_mode: None,
            }),
        ]
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// The bracketed `js_name` of a well-known-symbol method key
/// (`[Symbol.iterator]`). Its member keeps that name as its `name` too,
/// which is how codegen tells it from a synthesized wrapper's `iterator`.
fn symbol_key_name(key: &PropertyKey<'_>) -> Option<String> {
    let PropertyKey::StaticMemberExpression(m) = key else {
        return None;
    };
    let oxc_ast::ast::Expression::Identifier(obj) = &m.object else {
        return None;
    };
    let prop = m.property.name.as_str();
    (obj.name == "Symbol" && matches!(prop, "iterator" | "asyncIterator"))
        .then(|| format!("[Symbol.{prop}]"))
}

/// Extract a string name from a `PropertyKey`.
pub fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
        _ => None,
    }
}
