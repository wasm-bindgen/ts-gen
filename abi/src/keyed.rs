//! Type-level keys (§13): literal types, `K in keyof M` and `M[K]`.

use std::collections::HashMap;

use crate::ir::*;
use crate::resolve::{Context, Symbols, Target, TypeDecl};

/// The type a literal type's value has.
pub fn base(l: &Literal) -> Type {
    match l {
        Literal::Str(_) => Type::Str,
        Literal::Int(_) => Type::Scalar(Scalar::F64),
        Literal::Bool(_) => Type::Scalar(Scalar::Bool),
        Literal::Null => Type::Unit,
    }
}

/// What a type maps its keys to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Map {
    /// A declaration's properties, its own before its parents': a closed
    /// key set.
    Closed(Vec<(String, Type)>),
    /// An `@js(index)` getter's value, under any key.
    Open(Type),
}

/// The bound of the type parameter `t` names, innermost first.
pub fn bound<'a>(sym: &Symbols<'_>, t: &Type, cx: &Context<'a>) -> Option<&'a Bound> {
    let Type::Named(r) = t else { return None };
    let [name] = &r.path.segments[..] else {
        return None;
    };
    if !matches!(sym.resolve(&r.path, cx), Some(Target::TypeParam)) {
        return None;
    }
    let p = cx.bounds.iter().rev().find(|p| p.name == *name)?;
    p.bound.as_ref()
}

/// Whether `t` names a type parameter bounded `in keyof`.
pub fn is_key(sym: &Symbols<'_>, t: &Type, cx: &Context<'_>) -> bool {
    matches!(bound(sym, t, cx), Some(Bound::KeyOf(_)))
}

/// The map `m` is: a class or interface, an alias of one, or a type
/// parameter bounded by one. Its types read in `cx`.
pub fn map(sym: &Symbols<'_>, m: &Type, cx: &Context<'_>) -> Option<Map> {
    map_at(sym, m, cx, 0)
}

fn map_at(sym: &Symbols<'_>, m: &Type, cx: &Context<'_>, depth: usize) -> Option<Map> {
    if depth > 32 {
        return None;
    }
    let Type::Named(r) = m else { return None };
    match sym.resolve(&r.path, cx)? {
        Target::TypeParam => match bound(sym, m, cx)? {
            Bound::Subtype(t) => map_at(sym, t, cx, depth + 1),
            _ => None,
        },
        Target::Decl(_, TypeDecl::Alias(_)) => {
            let (_, u) = sym.unfold(r, cx)?;
            map_at(sym, &u, cx, depth + 1)
        }
        Target::Decl(loc, TypeDecl::Class(c)) => {
            let ccx = loc.context().with_class(c);
            let none = HashMap::new();
            let mut subst = HashMap::new();
            for (i, p) in c.tparams.iter().enumerate() {
                let t = match (r.args.get(i), &p.default) {
                    (Some(t), _) => sym.absolute(t, cx, &none),
                    (None, Some(d)) => sym.absolute(d, &ccx, &subst),
                    (None, None) => Type::Any,
                };
                subst.insert(p.name.as_str(), t);
            }
            let mut props: Vec<(String, Type)> = Vec::new();
            for m in c.members.iter().filter(|m| !m.is_static) {
                let index = m.meta.attrs.js.iter().any(|e| matches!(e, JsEntry::Index));
                if index && m.params.len() == 1 {
                    let v = match &m.result {
                        Type::Nullable(v, _) => v,
                        v => v,
                    };
                    return Some(Map::Open(sym.absolute(v, &ccx, &subst)));
                }
                if let MemberKind::Get(n) = &m.kind {
                    props.push((n.clone(), sym.absolute(&m.result, &ccx, &subst)));
                }
            }
            for e in &c.extends {
                let parent = sym.absolute(&Type::Named(e.clone()), &ccx, &subst);
                match map_at(sym, &parent, cx, depth + 1) {
                    Some(Map::Closed(ps)) => {
                        for p in ps {
                            if !props.iter().any(|q| q.0 == p.0) {
                                props.push(p);
                            }
                        }
                    }
                    Some(open) => return Some(open),
                    None => {}
                }
            }
            Some(Map::Closed(props))
        }
        _ => None,
    }
}

fn key_name(l: &Literal) -> Option<String> {
    match l {
        Literal::Str(s) => Some(s.clone()),
        Literal::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `m[key]`: the type of that property, reading in `cx`.
pub fn index(sym: &Symbols<'_>, m: &Type, key: &Literal, cx: &Context<'_>) -> Option<Type> {
    let mut out = Vec::new();
    let t = Type::Index(Box::new(m.clone()), Box::new(Type::Lit(key.clone())));
    (readings(sym, &t, cx, &mut out) && out.len() == 1).then(|| out.remove(0))
}

/// The types `t` is over the keys its indexes leave open; `false` when an
/// index names no property.
fn readings(sym: &Symbols<'_>, t: &Type, cx: &Context<'_>, out: &mut Vec<Type>) -> bool {
    let Type::Index(m, k) = t else {
        if !out.contains(t) {
            out.push(t.clone());
        }
        return true;
    };
    let mut ms = Vec::new();
    if !readings(sym, m, cx, &mut ms) {
        return false;
    }
    for m in ms {
        let found: Vec<Type> = match (map(sym, &m, cx), &**k) {
            (Some(Map::Open(v)), _) => vec![v],
            (Some(Map::Closed(ps)), Type::Lit(l)) => {
                let name = key_name(l);
                match ps.into_iter().find(|p| Some(&p.0) == name.as_ref()) {
                    Some(p) => vec![p.1],
                    None => return false,
                }
            }
            (Some(Map::Closed(ps)), _) => ps.into_iter().map(|p| p.1).collect(),
            (None, _) => return false,
        };
        for t in found {
            if !readings(sym, &t, cx, out) {
                return false;
            }
        }
    }
    true
}

/// `t` as an erasing reading has it (§13): a literal type and a key
/// parameter as the base type, `M[K]` as the one type every key gives,
/// else the declared enum of those types, else `Any`.
pub fn erased(sym: &Symbols<'_>, t: &Type, cx: &Context<'_>) -> Type {
    t.map(&|t| match t {
        Type::Lit(l) => Some(base(l)),
        Type::Named(r) if r.args.is_empty() && is_key(sym, t, cx) => Some(Type::Str),
        Type::Index(..) => {
            let mut ts = Vec::new();
            if !readings(sym, t, cx, &mut ts) {
                return Some(Type::Any);
            }
            let mut out: Vec<Type> = Vec::new();
            for t in &ts {
                let t = erased(sym, t, cx);
                if !out.contains(&t) {
                    out.push(t);
                }
            }
            Some(match out.len() {
                0 => Type::Any,
                1 => out.remove(0),
                _ => sym.enum_of(&out, cx).unwrap_or(Type::Any),
            })
        }
        _ => None,
    })
}

/// Whether `t` mentions a literal type, a key parameter or an index.
pub fn is_keyed(sym: &Symbols<'_>, t: &Type, cx: &Context<'_>) -> bool {
    let mut found = false;
    t.walk(&mut |t| {
        found |= matches!(t, Type::Lit(_) | Type::Index(..)) || is_key(sym, t, cx);
    });
    found
}

/// `m` as an erasing reading has it (§13), when it is keyed: its key
/// parameters dropped and its types [`erased`]. It is still called by the
/// key of `m`.
pub fn erased_member(sym: &Symbols<'_>, m: &Member, cx: &Context<'_>) -> Option<Member> {
    let key = |p: &TypeParam| matches!(p.bound, Some(Bound::KeyOf(_)));
    let keyed = m.tparams.iter().any(key)
        || m.params.iter().any(|p| is_keyed(sym, &p.ty, cx))
        || is_keyed(sym, &m.result, cx);
    keyed.then(|| Member {
        tparams: m.tparams.iter().filter(|p| !key(p)).cloned().collect(),
        params: m
            .params
            .iter()
            .map(|p| Param {
                ty: erased(sym, &p.ty, cx),
                ..p.clone()
            })
            .collect(),
        result: erased(sym, &m.result, cx),
        ..m.clone()
    })
}
