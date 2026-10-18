//! Per-monomorphization expansion of `M[K]` over a concrete `M`.
//!
//! `EventTarget<EventMap>`'s `addEventListener<Type extends keyof
//! EventMap>(type: Type, handler: EventListener<EventMap[Type]>)` types the
//! handler by the event name, which a Rust generic cannot. Where `M` is a
//! declared interface (`WebSocketEventMap`, reached through `WebSocket
//! extends EventTarget<WebSocketEventMap>`), the callable gains one
//! signature per property of `M`: `K` is the property name as a string
//! literal and `M[K]` its type, so the literal names the binding
//! (`add_event_listener_message(&Function<fn(MessageEvent)>)`). Trailing
//! optional parameters that do not mention the key are left to the generic
//! form.

use std::collections::HashMap;

use crate::ir::{Member, MethodMember, Param, TypeKind, TypeRef};
use crate::parse::scope::ScopeId;

use super::typemap::CodegenContext;

/// One signature per key of the first `M[K]` whose `M` is a declared
/// interface; empty when there is none. TypeScript only admits `M[K]` for
/// `K extends keyof M`, which erases to `string` where the parameters name
/// it bare, so the key is the one `string` parameter (none or several:
/// no expansion).
pub(crate) fn expand_keyof(
    params: &[Param],
    return_type: &TypeRef,
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
) -> Vec<(Vec<Param>, TypeRef)> {
    let mut access = None;
    for p in params {
        visit(&p.type_ref, &mut |t| {
            if let (None, TypeRef::IndexedAccess(map, key)) = (&access, t) {
                if let (Some(key), Some(keys)) = (key.as_ident(), map_keys(map, cgctx, scope)) {
                    access = Some(((**map).clone(), key.to_string(), keys));
                }
            }
        });
    }
    let Some((map, key_param, keys)) = access else {
        return Vec::new();
    };
    let [key_at] = params
        .iter()
        .enumerate()
        .filter(|(_, p)| p.type_ref == TypeRef::String)
        .map(|(i, _)| i)
        .collect::<Vec<_>>()[..]
    else {
        return Vec::new();
    };
    let keyed: Vec<bool> = params
        .iter()
        .enumerate()
        .map(|(i, p)| i == key_at || mentions(&p.type_ref, &key_param))
        .collect();
    let Some(last) = keyed.iter().rposition(|k| *k) else {
        return Vec::new();
    };
    let required = params
        .iter()
        .rposition(|p| !p.optional)
        .map_or(0, |i| i + 1);
    let kept = (last + 1).max(required);
    keys.into_iter()
        .map(|(key, value)| {
            let at = |ty: &TypeRef| apply(ty, &key_param, &map, &key, &value);
            let params = params[..kept]
                .iter()
                .enumerate()
                .map(|(i, p)| Param {
                    type_ref: if i == key_at {
                        TypeRef::StringLiteral(key.clone())
                    } else {
                        at(&p.type_ref)
                    },
                    ..p.clone()
                })
                .collect();
            (params, at(return_type))
        })
        .collect()
}

/// The per-key methods of `members` (see [`expand_keyof`]).
pub(crate) fn expand_keyof_methods(
    members: &[Member],
    cgctx: Option<&CodegenContext<'_>>,
) -> Vec<Member> {
    let Some(cgctx) = cgctx.filter(|c| CodegenContext::generic_mono(Some(c))) else {
        return Vec::new();
    };
    members
        .iter()
        .flat_map(|m| match m {
            Member::Method(m) => expand_keyof(&m.params, &m.return_type, cgctx, m.body_scope)
                .into_iter()
                .map(|(params, return_type)| {
                    Member::Method(MethodMember {
                        params,
                        return_type,
                        ..m.clone()
                    })
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn map_keys(
    map: &TypeRef,
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
) -> Option<Vec<(String, TypeRef)>> {
    let TypeRef::Reference { segments, .. } = map else {
        return None;
    };
    let id = cgctx.gctx.resolve_path(scope, &segments.join("."))?;
    let TypeKind::Interface(iface) = &cgctx.gctx.get_type(id).kind else {
        return None;
    };
    let keys: Vec<(String, TypeRef)> = iface
        .members
        .iter()
        .filter_map(|m| match m {
            Member::Getter(g) => Some((g.js_name.clone(), g.type_ref.clone())),
            _ => None,
        })
        .collect();
    (!keys.is_empty()).then_some(keys)
}

fn mentions(ty: &TypeRef, key: &str) -> bool {
    let mut found = false;
    visit(ty, &mut |t| {
        found |= t.as_ident() == Some(key);
    });
    found
}

fn visit(ty: &TypeRef, f: &mut impl FnMut(&TypeRef)) {
    f(ty);
    match ty {
        TypeRef::Reference { generic_args, .. } => generic_args.iter().for_each(|t| visit(t, f)),
        TypeRef::Array(t) => visit(t, f),
        TypeRef::Union(u) => u.members().iter().for_each(|t| visit(t, f)),
        TypeRef::Intersection(ts) | TypeRef::Tuple(ts) => ts.iter().for_each(|t| visit(t, f)),
        TypeRef::Function(sig) => {
            sig.params.iter().for_each(|p| visit(&p.type_ref, f));
            visit(&sig.return_type, f);
        }
        TypeRef::IndexedAccess(o, i) => {
            visit(o, f);
            visit(i, f);
        }
        _ => {}
    }
}

/// `ty` with `M[K]` replaced by `value` and the bare `K` by the key.
fn apply(ty: &TypeRef, key_param: &str, map: &TypeRef, key: &str, value: &TypeRef) -> TypeRef {
    let substituted = substitute_indexed(ty, key_param, map, value);
    let literal: HashMap<&str, TypeRef> =
        HashMap::from([(key_param, TypeRef::StringLiteral(key.to_string()))]);
    substituted.substitute(&literal)
}

fn substitute_indexed(ty: &TypeRef, key_param: &str, map: &TypeRef, value: &TypeRef) -> TypeRef {
    let sub = |t: &TypeRef| substitute_indexed(t, key_param, map, value);
    match ty {
        TypeRef::IndexedAccess(o, i) if **o == *map && i.as_ident() == Some(key_param) => {
            value.clone()
        }
        TypeRef::Reference {
            segments,
            generic_args,
        } => TypeRef::Reference {
            segments: segments.clone(),
            generic_args: generic_args.iter().map(sub).collect(),
        },
        TypeRef::Array(t) => TypeRef::Array(Box::new(sub(t))),
        TypeRef::Union(u) => TypeRef::union(u.members().iter().map(sub)),
        TypeRef::Intersection(ts) => TypeRef::Intersection(ts.iter().map(sub).collect()),
        TypeRef::Tuple(ts) => TypeRef::Tuple(ts.iter().map(sub).collect()),
        TypeRef::Function(sig) => TypeRef::Function(crate::ir::FunctionSig {
            params: sig
                .params
                .iter()
                .map(|p| Param {
                    type_ref: sub(&p.type_ref),
                    ..p.clone()
                })
                .collect(),
            return_type: Box::new(sub(&sig.return_type)),
        }),
        other => other.clone(),
    }
}
