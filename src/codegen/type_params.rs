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

use super::typemap::{variadic_spreads_list, CodegenContext};

/// Rewrite the methods of a class-like declaration in place. Per-mono,
/// returns the methods that lost a parameter as declared: each also gets
/// a `_typed` twin keeping its parameters (`list_typed::<M>()` beside
/// `list()`), as a class's constructors do.
pub(crate) fn erase_uninferrable_in_members(
    members: &mut [Member],
    cgctx: Option<&CodegenContext<'_>>,
) -> Vec<Member> {
    let mut typed = Vec::new();
    let Some(cgctx) = cgctx else {
        return typed;
    };
    let mono = CodegenContext::generic_mono(Some(cgctx));
    for member in members {
        let declared = mono.then(|| member.clone());
        let before = type_param_count(member);
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
        if let Some(declared) = declared.filter(|_| type_param_count(member) != before) {
            typed.push(declared);
        }
    }
    typed
}

fn type_param_count(member: &Member) -> usize {
    match member {
        Member::Method(m) => m.type_params.len(),
        Member::StaticMethod(m) => m.type_params.len(),
        _ => 0,
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
        // A `&[JsValue]` rest names nothing; a per-mono `&[T]` does.
        if p.variadic && !variadic_spreads_list(&p.type_ref, Some(cgctx), scope) {
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
                let type_params = match &decl.kind {
                    crate::ir::TypeKind::Interface(i) => &i.type_params,
                    crate::ir::TypeKind::Class(c) => &c.type_params,
                    _ => return None,
                };
                let default = type_params.get(pos)?.default.clone()?;
                // A default naming one of the type's own parameters
                // (`U = Box<T>`) has nothing to stand for in the method's
                // scope, where that name is unbound or another type.
                return (!mentions_any(&default, type_params)).then_some(default);
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

/// `ty` names one of `params` anywhere inside it.
fn mentions_any(ty: &TypeRef, params: &[TypeParam]) -> bool {
    let any = |ts: &[TypeRef]| ts.iter().any(|t| mentions_any(t, params));
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            matches!(segments.as_slice(), [n] if params.iter().any(|p| &p.name == n))
                || any(generic_args)
        }
        TypeRef::Array(inner) | TypeRef::KeyOf(inner) => mentions_any(inner, params),
        TypeRef::IndexedAccess(obj, key) => mentions_any(obj, params) || mentions_any(key, params),
        TypeRef::Union(u) => any(u.members()),
        TypeRef::Intersection(ts) | TypeRef::Tuple(ts) => any(ts),
        TypeRef::Function(sig) => {
            sig.params.iter().any(|p| mentions_any(&p.type_ref, params))
                || mentions_any(&sig.return_type, params)
        }
        _ => false,
    }
}

/// Type parameters an argument of type `ty` still names once lowered.
/// Mirrors `to_syn_type`'s callback lowering: a callback with a rest
/// parameter is the untyped `Function` (a `fn` pointer has fixed arity),
/// so it mentions nothing, while another callback's parameters and return
/// keep their type parameters. A `Promise<T>` callback return does not
/// count: wasm-bindgen cannot pass a `Function` returning `Promise<T>` for
/// a generic `T`, so `T` is left to the return rules, which erase it
/// unless it is the whole return.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn param(name: &str) -> TypeParam {
        TypeParam {
            name: name.into(),
            constraint: None,
            default: None,
        }
    }

    #[test]
    fn mentions_any_sees_nested_own_parameters() {
        let own = [param("T"), param("U")];
        let boxed = TypeRef::generic("Box", vec![TypeRef::ident("T")]);
        assert!(mentions_any(&boxed, &own));
        assert!(mentions_any(&TypeRef::Array(Box::new(boxed)), &own));
        assert!(mentions_any(
            &TypeRef::union([TypeRef::ident("U"), TypeRef::Null]),
            &own
        ));
        assert!(!mentions_any(
            &TypeRef::generic("Box", vec![TypeRef::String]),
            &own
        ));
        // Only a bare name is a parameter; `T.U` is a qualified type.
        let qualified = TypeRef::Reference {
            segments: vec!["T".into(), "U".into()],
            generic_args: vec![],
        };
        assert!(!mentions_any(&qualified, &own));
    }
}
