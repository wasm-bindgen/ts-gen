//! Callable-level type parameters that no argument can infer.
//!
//! A method's `<T>` reaches Rust as a generic the caller must name when
//! the lowered arguments never mention it. That is worth keeping only
//! where the turbofish is itself the cast: a return of exactly `T`,
//! `T | undefined` or `T | null` (`storage.get::<JsString>(key)`).
//! Nested in a container (`Promise<D1Result<T>>`, `Map<string, T>`) the
//! generic is pure ceremony — the caller can `unchecked_into` the
//! container just as well — so it erases to a default and leaves the
//! signature. The default is the one declared by the generic type `T`
//! instantiates in the return (`D1Result<T = unknown>`), else the
//! method's own, else `any`.

use std::collections::HashMap;

use crate::ir::{Member, Param, TypeParam, TypeRef};
use crate::parse::scope::{Binding, ScopeId};

use super::typemap::CodegenContext;

/// Rewrite the methods of a class-like declaration in place.
pub(crate) fn erase_uninferrable_in_members(
    members: &mut [Member],
    cgctx: Option<&CodegenContext<'_>>,
) {
    let Some(cgctx) = cgctx else {
        return;
    };
    for member in members {
        match member {
            Member::Method(m) => erase_uninferrable(
                &mut m.type_params,
                &mut m.params,
                &mut m.return_type,
                cgctx,
                m.body_scope,
            ),
            Member::StaticMethod(m) => erase_uninferrable(
                &mut m.type_params,
                &mut m.params,
                &mut m.return_type,
                cgctx,
                m.body_scope,
            ),
            _ => {}
        }
    }
}

/// Apply the rule to one callable. Erased parameters are substituted in
/// `params` and `return_type` and removed from `type_params`.
pub(crate) fn erase_uninferrable(
    type_params: &mut Vec<TypeParam>,
    params: &mut [Param],
    return_type: &mut TypeRef,
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
) {
    if type_params.is_empty() {
        return;
    }
    let mut inferred = Vec::new();
    for p in params.iter() {
        if p.variadic {
            continue;
        }
        surviving_type_params(&p.type_ref, cgctx, scope, &mut inferred);
    }
    let mut erased: Vec<(String, TypeRef)> = Vec::new();
    type_params.retain(|tp| {
        if inferred.contains(&tp.name) || is_whole_return(return_type, &tp.name) {
            return true;
        }
        let default = owning_type_default(return_type, &tp.name, cgctx, scope)
            .or_else(|| tp.default.clone())
            .unwrap_or(TypeRef::Any);
        erased.push((tp.name.clone(), default));
        false
    });
    if erased.is_empty() {
        return;
    }
    let map: HashMap<&str, TypeRef> = erased
        .iter()
        .map(|(n, d)| (n.as_str(), d.clone()))
        .collect();
    for p in params.iter_mut() {
        p.type_ref = p.type_ref.substitute(&map);
    }
    *return_type = return_type.substitute(&map);
}

/// The (awaited) return is exactly the parameter, optionally nullable.
fn is_whole_return(return_type: &TypeRef, name: &str) -> bool {
    let awaited = return_type.as_promise_inner().unwrap_or(return_type);
    match awaited.split_empties() {
        Some((core, _)) => core.as_ident() == Some(name),
        None => awaited.as_ident() == Some(name),
    }
}

/// Default declared by the first generic type in the return that takes
/// the parameter as a direct argument, when that default is concrete.
fn owning_type_default(
    ty: &TypeRef,
    name: &str,
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
) -> Option<TypeRef> {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            if let Some(pos) = generic_args.iter().position(|a| a.as_ident() == Some(name)) {
                let id = cgctx.gctx.resolve_path(scope, &segments.join("."))?;
                let decl = cgctx.gctx.get_type(id);
                let (type_params, body_scope) = match &decl.kind {
                    crate::ir::TypeKind::Interface(i) => (&i.type_params, i.body_scope),
                    crate::ir::TypeKind::Class(c) => (&c.type_params, c.body_scope),
                    _ => return None,
                };
                let default = type_params.get(pos)?.default.clone()?;
                // A default naming one of the type's own parameters has
                // nothing to stand for here.
                let refers_to_param = default.as_ident().is_some_and(|n| {
                    matches!(
                        cgctx.gctx.scopes.resolve_binding(body_scope, n),
                        Some(Binding::TypeParam)
                    )
                });
                return (!refers_to_param).then_some(default);
            }
            generic_args
                .iter()
                .find_map(|a| owning_type_default(a, name, cgctx, scope))
        }
        TypeRef::Array(inner) => owning_type_default(inner, name, cgctx, scope),
        TypeRef::Union(u) => u
            .members()
            .iter()
            .find_map(|m| owning_type_default(m, name, cgctx, scope)),
        TypeRef::Intersection(members) | TypeRef::Tuple(members) => members
            .iter()
            .find_map(|m| owning_type_default(m, name, cgctx, scope)),
        _ => None,
    }
}

/// Type parameters an argument of type `ty` still names once lowered.
/// Mirrors `to_syn_type`'s callback lowering: a variadic callback is the
/// untyped `Function`, and a `Promise<T>` callback return erases, so
/// neither mentions anything.
fn surviving_type_params(
    ty: &TypeRef,
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    out: &mut Vec<String>,
) {
    match ty {
        TypeRef::Function(sig) => {
            if sig.params.iter().any(|p| p.variadic) {
                return;
            }
            for p in &sig.params {
                surviving_type_params(&p.type_ref, cgctx, scope, out);
            }
            let promised_param = sig.return_type.as_promise_inner().is_some_and(|inner| {
                inner.as_ident().is_some_and(|n| {
                    matches!(
                        cgctx.gctx.scopes.resolve_binding(scope, n),
                        Some(Binding::TypeParam)
                    )
                })
            });
            if !promised_param {
                surviving_type_params(&sig.return_type, cgctx, scope, out);
            }
        }
        TypeRef::Reference { generic_args, .. } => {
            if let Some(n) = ty.as_ident() {
                if matches!(
                    cgctx.gctx.scopes.resolve_binding(scope, n),
                    Some(Binding::TypeParam)
                ) && !out.iter().any(|o| o == n)
                {
                    out.push(n.to_string());
                }
                return;
            }
            for a in generic_args {
                surviving_type_params(a, cgctx, scope, out);
            }
        }
        TypeRef::Array(inner) => surviving_type_params(inner, cgctx, scope, out),
        TypeRef::Union(u) => {
            for m in u.members() {
                surviving_type_params(m, cgctx, scope, out);
            }
        }
        TypeRef::Intersection(members) | TypeRef::Tuple(members) => {
            for m in members {
                surviving_type_params(m, cgctx, scope, out);
            }
        }
        _ => {}
    }
}
