//! `x: union E` parameters (IR.md §7): every arm of the payload enum `E`
//! makes the same JS call, so an import also has a variant per arm, the
//! key with that arm's payload type in place of `E`, which the bridge
//! derives when a language imports it.

use std::collections::HashMap;

use crate::enums::{self, Flat, Shape};
use crate::ir::*;
use crate::key::{Keyed, Keys};
use crate::resolve::{Context, Symbols, TypeDecl};

/// Derived variants past this many per member aren't enumerated.
pub const MAX_VARIANTS: usize = 4096;

/// The enum a `union` parameter's type names, through its `?`s.
pub fn enum_path(t: &Type) -> Option<&Path> {
    match t {
        Type::Nullable(t, _) => enum_path(t),
        Type::Named(r) => Some(&r.path),
        _ => None,
    }
}

/// `t` with the type its `?`s wrap replaced by `with`.
pub fn substitute(t: &Type, with: &Type) -> Type {
    match t {
        Type::Nullable(t, n) => Type::Nullable(Box::new(substitute(t, with)), *n),
        _ => with.clone(),
    }
}

/// The payload type of each of `flat`'s arms that has one, readable from
/// any context: a lone payload is its type, several a tuple of them.
pub fn payloads(symbols: &Symbols<'_>, flat: &Flat<'_>) -> Vec<Type> {
    flat.arms
        .iter()
        .filter(|a| !a.arm.payload.is_empty())
        .map(|a| {
            let cx = a
                .owner
                .loc
                .as_ref()
                .map(|l| l.context())
                .unwrap_or_default();
            let q = |t: &Type| symbols.absolute(t, &cx, &HashMap::new());
            match a.arm.payload.as_slice() {
                [t] => q(t),
                ts => Type::Tuple(ts.iter().map(q).collect()),
            }
        })
        .collect()
}

/// Why `E`, a `union` parameter's enum, can't be one: not a payload
/// enum, or two of its arms the bridge can't tell apart (§8).
pub fn invalid(symbols: &Symbols<'_>, t: &Type, cx: &Context<'_>) -> Option<String> {
    let Some(path) = enum_path(t) else {
        return Some("`union` takes a payload enum".into());
    };
    let Some(e) = enums::resolve(symbols, path, cx) else {
        return Some("`union` takes a payload enum".into());
    };
    let Ok(flat) = enums::flatten(symbols, e) else {
        return None;
    };
    if flat.shape != Shape::Payload {
        return Some(format!(
            "`{}` has no payload arm; a literal-only enum is passed as a value",
            flat.this
        ));
    }
    let mut seen: Vec<(String, &str)> = Vec::new();
    for a in &flat.arms {
        if a.arm.payload.is_empty() || a.is_fallback() {
            continue;
        }
        let acx = a
            .owner
            .loc
            .as_ref()
            .map(|l| l.context())
            .unwrap_or_default();
        let test = match a.arm.payload.as_slice() {
            [t] => test(symbols, t, &acx),
            _ => Some("an array".into()),
        };
        let Some(test) = test else { continue };
        if let Some((_, other)) = seen.iter().find(|(t, _)| *t == test) {
            return Some(format!(
                "`{}`'s arms `{other}` and `{}` can't be told apart ({test})",
                flat.this, a.arm.name
            ));
        }
        seen.push((test, &a.arm.name));
    }
    None
}

/// The arm test a payload of type `t` runs (§8), where two equal tests
/// can't tell their arms apart; `None` for a nested enum, which is
/// discriminated by its own arms.
fn test(symbols: &Symbols<'_>, t: &Type, cx: &Context<'_>) -> Option<String> {
    Some(match symbols.head(t, cx) {
        Type::Str | Type::StrBox => "`string`".into(),
        Type::Scalar(Scalar::Bool) | Type::ScalarBox(Scalar::Bool) => "`boolean`".into(),
        Type::Scalar(Scalar::I64 | Scalar::U64)
        | Type::ScalarBox(Scalar::I64 | Scalar::U64)
        | Type::BigInt
        | Type::BigIntBox => "`bigint`".into(),
        Type::Scalar(_) | Type::ScalarBox(_) => "`number`".into(),
        Type::Array(_) | Type::ArrBox(_) | Type::Tuple(_) | Type::TupleBox(_) => "an array".into(),
        Type::Fn(_) => "a function".into(),
        Type::Future(..) => "a thenable".into(),
        Type::Vec(e) | Type::VecBox(e) | Type::MutVec(e) => format!("a `{e:?}[]`"),
        Type::Any | Type::Nullable(..) => "anything".into(),
        Type::Named(r) => match symbols.resolve_decl(&r.path, cx) {
            Some((_, TypeDecl::Enum(_))) => return None,
            Some((_, TypeDecl::Class(c))) if c.kind == ClassKind::Interface => "an object".into(),
            Some((loc, TypeDecl::Class(_))) => format!("`{}`", crate::key::location(&loc)),
            _ => return None,
        },
        _ => return None,
    })
}

/// Each arm-specialized variant of `k`'s `union` parameters, as its key
/// and parameter types: every parameter takes `E` or one arm's payload,
/// independently, and at least one takes an arm.
pub fn variants(keys: &Keys<'_>, k: &Keyed<'_>) -> Vec<(String, Vec<Type>)> {
    let m = k.member;
    if !m.params.iter().any(|p| p.union) {
        return Vec::new();
    }
    let own: Vec<String> = m.params.iter().map(|p| keys.ty(&p.ty, &k.cx)).collect();
    let tail = format!("({})", own.join(","));
    let Some(prefix) = k.key.strip_suffix(&tail) else {
        return Vec::new();
    };
    let mut choices: Vec<Vec<Type>> = Vec::new();
    for p in &m.params {
        let mut c = vec![p.ty.clone()];
        if p.union {
            if let Some(e) =
                enum_path(&p.ty).and_then(|path| enums::resolve(&keys.symbols, path, &k.cx))
            {
                if let Ok(flat) = enums::flatten(&keys.symbols, e) {
                    c.extend(
                        payloads(&keys.symbols, &flat)
                            .iter()
                            .map(|a| substitute(&p.ty, a)),
                    );
                }
            }
        }
        choices.push(c);
    }
    let total: usize = choices.iter().map(Vec::len).product();
    if total > MAX_VARIANTS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for n in 1..total {
        let mut at = n;
        let mut tys = Vec::with_capacity(choices.len());
        for c in &choices {
            tys.push(c[at % c.len()].clone());
            at /= c.len();
        }
        let spelled: Vec<String> = tys.iter().map(|t| keys.ty(t, &k.cx)).collect();
        out.push((format!("{prefix}({})", spelled.join(",")), tys));
    }
    out
}

/// Whether `a` and `b` make the same JS call: the same kind, name and
/// `@js`, with parameters matched by position.
pub fn same_call(a: &Member, b: &Member) -> bool {
    let norm = |m: &Member| {
        let names: Vec<&str> = m.params.iter().map(|p| p.name.as_str()).collect();
        let at = |n: &str| match names.iter().position(|x| *x == n) {
            Some(i) => format!("${i}"),
            None => n.to_string(),
        };
        let js: Vec<JsEntry> = m
            .meta
            .attrs
            .js
            .iter()
            .map(|j| match j {
                JsEntry::Args(t) => JsEntry::Args(t.iter().map(|e| telem(e, &at)).collect()),
                JsEntry::Super(t) => JsEntry::Super(t.iter().map(|e| telem(e, &at)).collect()),
                JsEntry::Let(x, e) => JsEntry::Let(x.clone(), expr(e, &at)),
                JsEntry::Cancel(e) => JsEntry::Cancel(expr(e, &at)),
                j => j.clone(),
            })
            .collect();
        (m.kind.clone(), m.is_static, js)
    };
    norm(a) == norm(b)
}

fn telem(e: &Telem, at: &dyn Fn(&str) -> String) -> Telem {
    match e {
        Telem::Param(p) => Telem::Param(at(p)),
        Telem::Spread(p) => Telem::Spread(at(p)),
        Telem::Fill { param, fields } => Telem::Fill {
            param: at(param),
            fields: fields
                .iter()
                .map(|(f, x)| (f.clone(), expr(x, at)))
                .collect(),
        },
        Telem::Expr(x) => Telem::Expr(expr(x, at)),
        e => e.clone(),
    }
}

fn expr(e: &Expr, at: &dyn Fn(&str) -> String) -> Expr {
    match e {
        Expr::Ref { path, call } => Expr::Ref {
            path: match path.as_slice() {
                [p, rest @ ..] => std::iter::once(at(p)).chain(rest.iter().cloned()).collect(),
                [] => Vec::new(),
            },
            call: call
                .as_ref()
                .map(|c| c.iter().map(|x| expr(x, at)).collect()),
        },
        e => e.clone(),
    }
}
