//! Callable-level type parameters that no argument can infer.
//!
//! A method's `<T>` reaches Rust as a generic the caller must name when
//! the lowered arguments never mention it. The plain name therefore reads
//! it at what TypeScript infers with nothing to go on: the default
//! declared by the generic type `T` instantiates in the return
//! (`D1Result<T = unknown>`), else the method's own, else its class or
//! interface bound, else `any`. The caller's own choice is the `_typed`
//! twin, which keeps the parameters the return mentions
//! (`all_typed::<Row>()`).

use std::collections::HashMap;

use ts_gen::ir::{Member, Param, TypeParam, TypeRef};
use ts_gen::parse::scope::{Binding, ScopeId};
use ts_gen::util::naming::LocalScope;

use super::ir_callables::Site;
use super::signatures::{CallableSpec, FunctionSignature};
use super::typemap::{variadic_spreads_list, CodegenContext};

const TYPED: &str = "_typed";

/// The `_typed` twins of `plain`: `typed`, the callable's
/// [`Reading::Typed`], bound with `_typed` closing each name (`list_typed`,
/// `list_typed_promise`, `try_list_typed`).
///
/// `lowered` is a binding's return as Rust writes it, anchored at the
/// given name. A twin whose return is its plain binding's is left out: a
/// parameter the lowering drops anyway types nothing.
pub(crate) fn typed_twins(
    typed: &CallableSpec<'_>,
    site: Option<Site<'_>>,
    plain: &[FunctionSignature],
    names: &mut LocalScope,
    cgctx: Option<&CodegenContext<'_>>,
    lowered: &dyn Fn(&FunctionSignature, &str) -> String,
) -> Vec<FunctionSignature> {
    let mut twins = super::ir_callables::build_postfixed(typed, site, TYPED, names, cgctx);
    twins.retain(|twin| {
        let typed = twin.rust_name.rfind(TYPED).is_some_and(|at| {
            let name = format!(
                "{}{}",
                &twin.rust_name[..at],
                &twin.rust_name[at + TYPED.len()..]
            );
            plain
                .iter()
                .any(|p| p.rust_name == name && lowered(p, &name) != lowered(twin, &name))
        });
        if !typed {
            names.release(&twin.rust_name);
            names.release(&format!("__{}", twin.rust_name));
            for (_, arm) in twin.literal.iter().flat_map(|l| &l.arms) {
                names.release(arm);
            }
            if let Some(l) = &twin.literal {
                names.release(&l.binding);
            }
        }
        typed
    });
    twins
}

/// Rewrite the methods of a class-like declaration in place, returning
/// those with a `_typed` twin as the twin reads them.
pub(crate) fn erase_uninferrable_in_members(
    members: &mut [Member],
    cgctx: Option<&CodegenContext<'_>>,
) -> Vec<Member> {
    let mut twins = Vec::new();
    let Some(cgctx) = cgctx else {
        return twins;
    };
    let erase = |member: &mut Member, reading: Reading| match member {
        Member::Method(m) => erase_uninferrable(
            &mut m.type_params,
            &mut m.params,
            &mut m.return_type,
            cgctx,
            m.body_scope,
            reading,
        ),
        Member::StaticMethod(m) => erase_uninferrable(
            &mut m.type_params,
            &mut m.params,
            &mut m.return_type,
            cgctx,
            m.body_scope,
            reading,
        ),
        _ => 0,
    };
    for member in members {
        let mut twin = member.clone();
        if erase(member, Reading::Plain) > erase(&mut twin, Reading::Typed) {
            twins.push(twin);
        }
    }
    twins
}

/// Which binding of a callable is being read.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Reading {
    /// Every parameter no argument infers at its default.
    Plain,
    /// The `_typed` twin, which keeps those the return mentions for the
    /// caller to name.
    Typed,
}

/// Apply the rule to one callable. Erased parameters are substituted in
/// `params` and `return_type` and removed from `type_params`; how many
/// were.
pub(crate) fn erase_uninferrable(
    type_params: &mut Vec<TypeParam>,
    params: &mut [Param],
    return_type: &mut TypeRef,
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    reading: Reading,
) -> usize {
    if type_params.is_empty() {
        return 0;
    }
    let mut inferred = Vec::new();
    for p in params.iter() {
        // A `&[JsValue]` rest names nothing; a per-mono `&[T]` does.
        if p.variadic && !variadic_spreads_list(&p.type_ref, Some(cgctx), scope) {
            continue;
        }
        surviving_type_params(&p.type_ref, cgctx, scope, &mut inferred);
    }
    let own = type_params.clone();
    let mut erased: Vec<(String, TypeRef)> = Vec::new();
    type_params.retain(|tp| {
        let named =
            reading == Reading::Typed && mentions_any(return_type, std::slice::from_ref(tp));
        if inferred.contains(&tp.name) || named {
            return true;
        }
        let default = owning_type_default(return_type, &tp.name, cgctx, scope)
            .or_else(|| tp.default.clone())
            .or_else(|| bound(tp, &own, cgctx, scope))
            .unwrap_or(TypeRef::Any);
        erased.push((tp.name.clone(), default));
        false
    });
    if erased.is_empty() {
        return 0;
    }
    let map: HashMap<&str, TypeRef> = erased
        .iter()
        .map(|(n, d)| (n.as_str(), d.clone()))
        .collect();
    for p in params.iter_mut() {
        p.type_ref = p.type_ref.substitute(&map);
    }
    *return_type = return_type.substitute(&map);
    erased.len()
}

/// The class or interface `tp` extends, as the IR bounds it, unless that
/// names the callable's `own` parameters.
fn bound(
    tp: &TypeParam,
    own: &[TypeParam],
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
) -> Option<TypeRef> {
    let c = tp.constraint.as_ref()?;
    let TypeRef::Reference { segments, .. } = c else {
        return None;
    };
    let id = cgctx.gctx.resolve_path(scope, &segments.join("."))?;
    let declared = matches!(
        cgctx.gctx.get_type(id).kind,
        ts_gen::ir::TypeKind::Interface(_) | ts_gen::ir::TypeKind::Class(_)
    );
    (declared && !mentions_any(c, own)).then(|| c.clone())
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
                    ts_gen::ir::TypeKind::Interface(i) => &i.type_params,
                    ts_gen::ir::TypeKind::Class(c) => &c.type_params,
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
/// keep their type parameters.
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
            surviving_type_params(&sig.return_type, cgctx, scope, out);
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
