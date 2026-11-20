//! The lowered IR as JSON: every declaration, and every member with its key
//! and its layout under one profile.
//!
//! Language generators that aren't written in Rust read this, so the IR
//! text, keys and layouts have one implementation. The format is versioned
//! by [`VERSION`]; fields are only ever added within a version.
//!
//! ```text
//! { "format": "ts-gen-abi/lowered", "version": 1, "profile": {..},
//!   "decls": [..], "members": [..] }
//! ```
//!
//! A member whose layout can't be computed under the profile carries
//! `"error"` in place of `"sig"`, and its approximations under `"warnings"`.
//! A member's `"scope"` is its module block and `"namespace"` its enclosing
//! namespaces. `"variant"` is the member's name within its group of
//! same-named siblings (§7 conflict renaming, with type arguments spelled),
//! for a generator whose own overloading conflates signatures the IR keeps
//! apart.
//!
//! A function type reaching itself through a recursive alias (§3) has
//! `"kind": "rec"` where it recurs: its layouts, each way, are under
//! `"recs"` by its `"ty"`, with `"name"` for a generator that declares it
//! by name.
//!
//! An exported class, an enum and a function type the language provides
//! carry `"box"` and `"unbox"`: the `tw:box` and `tw:unbox` imports of the
//! type and their core types, for a generator's box helpers (§13).

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::Write;

use crate::ir::*;
use crate::key::{self, Keyed, Role};
use crate::layout::*;
use crate::naming::{self, Slot};
use crate::profile::{Carrier, Encoding, Profile, StrCarrier, Throws};
use crate::protocol;
use crate::resolve::{Context, Location, Target, TypeDecl};
use crate::text::name;

pub const FORMAT: &str = "ts-gen-abi/lowered";
pub const VERSION: i64 = 1;

/// A JSON value. Objects keep insertion order, so output is stable.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn obj() -> Obj {
        Obj(Vec::new())
    }

    fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    fn opt_str(s: Option<impl Into<String>>) -> Json {
        s.map_or(Json::Null, Json::str)
    }

    fn is_scalar(&self) -> bool {
        !matches!(self, Json::Arr(_) | Json::Obj(_))
    }

    /// Two-space indented, with arrays of scalars on one line.
    pub fn pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0);
        s.push('\n');
        s
    }

    fn write(&self, out: &mut String, depth: usize) {
        let pad = |out: &mut String, d: usize| out.extend(std::iter::repeat_n("  ", d));
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Str(s) => escape(out, s),
            Json::Arr(xs) if xs.is_empty() => out.push_str("[]"),
            Json::Arr(xs) if xs.iter().all(Json::is_scalar) => {
                out.push('[');
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    x.write(out, depth);
                }
                out.push(']');
            }
            Json::Arr(xs) => {
                out.push_str("[\n");
                for (i, x) in xs.iter().enumerate() {
                    pad(out, depth + 1);
                    x.write(out, depth + 1);
                    out.push_str(if i + 1 < xs.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push(']');
            }
            Json::Obj(fs) if fs.is_empty() => out.push_str("{}"),
            Json::Obj(fs) => {
                out.push_str("{\n");
                for (i, (k, v)) in fs.iter().enumerate() {
                    pad(out, depth + 1);
                    escape(out, k);
                    out.push_str(": ");
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < fs.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push('}');
            }
        }
    }
}

fn escape(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Obj(Vec<(String, Json)>);

impl Obj {
    fn set(mut self, k: &str, v: Json) -> Obj {
        self.0.push((k.into(), v));
        self
    }

    fn done(self) -> Json {
        Json::Obj(self.0)
    }

    /// `k` only when `v` isn't `null`.
    fn some(self, k: &str, v: Json) -> Obj {
        match v {
            Json::Null => self,
            v => self.set(k, v),
        }
    }
}

fn carrier(c: Carrier) -> &'static str {
    match c {
        Carrier::Linear => "linear",
        Carrier::Gc => "gc",
        Carrier::Host => "host",
    }
}

fn str_carrier(c: StrCarrier) -> String {
    let enc = |e: Encoding| match e {
        Encoding::Utf8 => "utf8",
        Encoding::Wtf16 => "wtf16",
    };
    match c {
        StrCarrier::Linear(e) => format!("linear+{}", enc(e)),
        StrCarrier::Gc(e) => format!("gc+{}", enc(e)),
        StrCarrier::Host => "host".into(),
    }
}

fn wire(w: &[ValType]) -> Json {
    Json::Arr(w.iter().map(|v| Json::str(v.to_string())).collect())
}

fn import(i: protocol::Import) -> Json {
    Json::obj()
        .set("module", Json::str(i.module))
        .set("name", Json::str(i.name))
        .done()
}

/// `("tw:box", T)` and `("tw:unbox", T)` of a non-null `T`, for a generator's
/// box helpers (§13): the import and its core type, since `T`'s own layout
/// gives the conversions. `null` where `T` has no layout.
fn boxing(l: &Layouts<'_>, ty: &str, r: crate::layout::Result<Repr>) -> (Json, Json) {
    let op = |i: protocol::Import, s: crate::layout::Result<Signature>| {
        s.and_then(|s| l.lang_core(&s.core()))
            .map(|c| {
                Json::obj()
                    .set("import", import(i))
                    .set("params", wire(&c.ty.params))
                    .set("results", wire(&c.ty.results))
                    .set("spill", wire(&c.spill))
                    .done()
            })
            .unwrap_or(Json::Null)
    };
    match r {
        Ok(r) => (
            op(protocol::box_(ty), l.box_of(r.clone())),
            op(protocol::unbox(ty), l.unbox_of(&r)),
        ),
        Err(_) => (Json::Null, Json::Null),
    }
}

fn doc(m: &Meta) -> Json {
    Json::opt_str(m.doc.clone())
}

fn js_name(m: &Meta) -> Json {
    Json::opt_str(m.attrs.js.iter().find_map(|e| match e {
        JsEntry::Name(n) => Some(n.clone()),
        _ => None,
    }))
}

/// The lowered form of `ir` under `profile`.
pub fn lower(ir: &Ir, profile: &Profile) -> Json {
    lower_with(ir, &[], profile)
}

/// The lowered form of `ir` under `profile`, with paths resolved against
/// `libs`, the IRs its `with` names, in order (§5). Each lib's declarations
/// are listed under `libs`, without members: the lib's own bindings provide
/// them.
pub fn lower_with(ir: &Ir, libs: &[&Ir], profile: &Profile) -> Json {
    let l = Layouts::with_libs(ir, libs, profile);
    let mut decls = Vec::new();
    walk_decls(&l, &ir.items, &Context::default(), false, &mut decls);
    let lib_decls = libs
        .iter()
        .map(|lib| {
            let rest: Vec<&Ir> = libs
                .iter()
                .copied()
                .filter(|o| !std::ptr::eq(*o, *lib))
                .collect();
            let ll = Layouts::with_libs(lib, &rest, profile);
            let mut ds = Vec::new();
            walk_decls(&ll, &lib.items, &Context::default(), false, &mut ds);
            Json::obj()
                .set("name", Json::opt_str(lib.lib.clone()))
                .set("decls", Json::Arr(ds))
                .done()
        })
        .collect();
    let keyed = l.keys.members();
    let variants = variants(&keyed);
    let members = keyed
        .iter()
        .zip(variants)
        .map(|(k, v)| member(&l, k, v))
        .collect();
    let mut recs = Vec::new();
    let mut seen = HashSet::new();
    while let Some((ty, t)) = RECS.with(|r| r.borrow_mut().pop()) {
        if !seen.insert(ty.clone()) {
            continue;
        }
        let cx = Context::default();
        let flows = [("toJs", Flow::ToJs), ("toLang", Flow::ToLang)].map(|(k, f)| {
            let r = l.repr(&t, &cx, f).map(|r| repr(&l, &r));
            (
                k,
                r.unwrap_or_else(|e| Json::obj().set("error", Json::str(e.to_string())).done()),
            )
        });
        let name = match &t {
            Type::Named(r) => {
                r.path.segments.last().cloned().unwrap_or_default()
                    + &r.args.iter().map(spelled_word).collect::<String>()
            }
            _ => String::new(),
        };
        let [(a, ja), (b, jb)] = flows;
        recs.push(
            Json::obj()
                .set("ty", Json::str(ty))
                .set("name", Json::str(name))
                .set(a, ja)
                .set(b, jb)
                .done(),
        );
    }
    let mut p = Json::obj();
    for line in profile.to_string().lines() {
        if let Some((k, v)) = line.split_once(" = ") {
            p = p.set(k, Json::str(v));
        }
    }
    Json::obj()
        .set("format", Json::str(FORMAT))
        .set("version", Json::Int(VERSION))
        .set("profile", p.done())
        .set("lib", Json::opt_str(ir.lib.clone()))
        .set("with", Json::Arr(ir.with.iter().map(Json::str).collect()))
        .set("libs", Json::Arr(lib_decls))
        .set("decls", Json::Arr(decls))
        .set("members", Json::Arr(members))
        .set("recs", Json::Arr(recs))
        .done()
}

thread_local! {
    /// The back-edges [`repr`] met, to lay out under `"recs"`.
    static RECS: RefCell<Vec<(String, Type)>> = const { RefCell::new(Vec::new()) };
}

fn walk_decls<'a>(
    l: &Layouts<'a>,
    items: &'a [Decl],
    cx: &Context<'a>,
    exported: bool,
    out: &mut Vec<Json>,
) {
    let loc = |n: &str| Location {
        module: cx.module.map(Into::into),
        path: cx
            .namespace
            .iter()
            .map(|s| s.to_string())
            .chain([n.to_string()])
            .collect(),
    };
    for d in items {
        match d {
            Decl::Namespace(n) => {
                out.push(
                    Json::obj()
                        .set("kind", Json::str("namespace"))
                        .set("path", Json::str(key::location(&loc(&n.name))))
                        .set("module", Json::opt_str(cx.module))
                        .set("name", Json::str(&n.name))
                        .set("export", Json::Bool(exported || n.export))
                        .set("doc", doc(&n.meta))
                        .done(),
                );
                let mut inner = cx.clone();
                inner.namespace.push(&n.name);
                walk_decls(l, &n.items, &inner, exported || n.export, out);
            }
            Decl::Module(m) => {
                let exports = m.exports.as_ref().map_or(Json::Null, |es| {
                    Json::Arr(
                        es.iter()
                            .map(|e| {
                                Json::obj()
                                    .set("name", Json::str(&e.name))
                                    .set("alias", Json::opt_str(e.alias.clone()))
                                    .done()
                            })
                            .collect(),
                    )
                });
                out.push(
                    Json::obj()
                        .set("kind", Json::str("module"))
                        .set("spec", Json::str(&m.spec))
                        .set("exports", exports)
                        .set("doc", doc(&m.meta))
                        .done(),
                );
                let inner = Context {
                    module: Some(&m.spec),
                    ..Context::default()
                };
                walk_decls(l, &m.items, &inner, false, out);
            }
            Decl::Class(c) => {
                let ccx = cx.with_class(c);
                let tparams = c.tparams.iter().map(|p| tparam(l, p, &ccx)).collect();
                let extends = c
                    .extends
                    .iter()
                    .map(|r| Json::str(l.keys.ty(&Type::Named(r.clone()), &ccx)))
                    .collect();
                let assoc = c
                    .assoc
                    .iter()
                    .map(|a| {
                        let rows = a
                            .rows
                            .iter()
                            .map(|(k, t)| {
                                Json::obj()
                                    .set("arg", Json::str(l.keys.ty(k, &ccx)))
                                    .set("type", Json::str(l.keys.ty(t, &ccx)))
                                    .done()
                            })
                            .collect();
                        Json::obj()
                            .set("name", Json::str(&a.name))
                            .set("param", Json::str(&a.param))
                            .set("rows", Json::Arr(rows))
                            .done()
                    })
                    .collect();
                let kind = match c.kind {
                    ClassKind::Class => "class",
                    ClassKind::Interface => "interface",
                };
                let path = key::location(&loc(&c.name));
                let (box_, unbox) = match (exported || c.export) && c.tparams.is_empty() {
                    true => boxing(
                        l,
                        &path,
                        l.repr(&Type::named([c.name.as_str()]), cx, Flow::ToJs),
                    ),
                    false => (Json::Null, Json::Null),
                };
                out.push(
                    Json::obj()
                        .set("kind", Json::str(kind))
                        .set("path", Json::str(key::location(&loc(&c.name))))
                        .set("module", Json::opt_str(cx.module))
                        .set("name", Json::str(&c.name))
                        .set("jsName", js_name(&c.meta))
                        .set("export", Json::Bool(exported || c.export))
                        .set("tparams", Json::Arr(tparams))
                        .set("assoc", Json::Arr(assoc))
                        .set("extends", Json::Arr(extends))
                        .set("doc", doc(&c.meta))
                        .some("box", box_)
                        .some("unbox", unbox)
                        .done(),
                );
            }
            Decl::Enum(e) => {
                let path = Path::new([e.name.as_str()]);
                let (box_, unbox) = boxing(
                    l,
                    &key::location(&loc(&e.name)),
                    l.repr(&Type::named([e.name.as_str()]), cx, Flow::ToLang),
                );
                let layout = match l.enum_of(&path, cx, Flow::ToLang) {
                    Ok(layout) => ("layout", enum_layout(l, &layout)),
                    Err(err) => ("error", Json::str(err.to_string())),
                };
                out.push(
                    Json::obj()
                        .set("kind", Json::str("enum"))
                        .set("path", Json::str(key::location(&loc(&e.name))))
                        .set("module", Json::opt_str(cx.module))
                        .set("name", Json::str(&e.name))
                        .set("jsName", js_name(&e.meta))
                        .set("doc", doc(&e.meta))
                        .set(layout.0, layout.1)
                        .some("box", box_)
                        .some("unbox", unbox)
                        .done(),
                );
            }
            Decl::Alias(a) => {
                let acx = cx.with_tparams(&a.tparams);
                let at = loc(&a.name);
                let tparams = a.tparams.iter().map(|p| tparam(l, p, &acx)).collect();
                let this = Type::Named(TypeRef {
                    path: Path::new([a.name.as_str()]),
                    args: a
                        .tparams
                        .iter()
                        .map(|p| Type::named([p.name.as_str()]))
                        .collect(),
                });
                out.push(
                    Json::obj()
                        .set("kind", Json::str("alias"))
                        .set("path", Json::str(key::location(&at)))
                        .set("module", Json::opt_str(cx.module))
                        .set("name", Json::str(&a.name))
                        .set("tparams", Json::Arr(tparams))
                        .set("type", Json::str(l.keys.ty(&this, &acx)))
                        .set("recursive", Json::Bool(l.keys.symbols.recursive(&at)))
                        .set(
                            "repr",
                            match a.tparams.is_empty() {
                                true => l
                                    .repr(&this, &acx, Flow::ToLang)
                                    .map_or(Json::Null, |r| repr(l, &r)),
                                false => Json::Null,
                            },
                        )
                        .set("doc", doc(&a.meta))
                        .done(),
                );
            }
            Decl::Member(_) => {}
        }
    }
}

fn tparam(l: &Layouts<'_>, p: &TypeParam, cx: &Context<'_>) -> Json {
    let bound = match &p.bound {
        None => Json::Null,
        Some(Bound::Subtype(t)) => Json::obj()
            .set("subtype", Json::str(l.keys.ty(t, cx)))
            .done(),
        Some(Bound::In(ts)) => Json::obj()
            .set(
                "in",
                Json::Arr(ts.iter().map(|t| Json::str(l.keys.ty(t, cx))).collect()),
            )
            .done(),
    };
    // The default's layout, for a generator that erases to it; `null` when
    // it has none (it names a type parameter, for one).
    let default = p
        .default
        .as_ref()
        .and_then(|t| l.repr(t, cx, Flow::ToLang).ok())
        .map_or(Json::Null, |r| repr(l, &r));
    Json::obj()
        .set("name", Json::str(&p.name))
        .set("bound", bound)
        .set("default", default)
        .done()
}

/// Each member's [`naming::variant_names`] name among the members sharing
/// its owner, namespace, staticness, kind and name.
fn variants(keyed: &[Keyed<'_>]) -> Vec<Option<String>> {
    let mut groups: Vec<(GroupKey<'_>, Vec<usize>)> = Vec::new();
    for (i, k) in keyed.iter().enumerate() {
        let g = group_key(k);
        match groups.iter_mut().find(|(o, _)| *o == g) {
            Some((_, ix)) => ix.push(i),
            None => groups.push((g, vec![i])),
        }
    }
    let mut out = vec![None; keyed.len()];
    for ((.., base), ix) in &groups {
        let Some(base) = base else { continue };
        let slots: Vec<Vec<Slot<'_>>> = ix
            .iter()
            .map(|&i| {
                keyed[i]
                    .member
                    .params
                    .iter()
                    .map(|p| Slot::Param {
                        name: &p.name,
                        ty: &p.ty,
                        rest: false,
                    })
                    .collect()
            })
            .collect();
        let names = naming::variant_names_with(base, &slots, &spelled_word);
        for (&i, n) in ix.iter().zip(names) {
            out[i] = Some(n);
        }
    }
    out
}

type GroupKey<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Vec<&'a str>,
    bool,
    &'static str,
    Option<&'a str>,
);

fn group_key<'a>(k: &Keyed<'a>) -> GroupKey<'a> {
    let m = k.member;
    let (kind, n) = match &m.kind {
        MemberKind::Method(n) => ("method", Some(n.as_str())),
        MemberKind::Get(n) => ("get", Some(n.as_str())),
        MemberKind::Set(n) => ("set", Some(n.as_str())),
        MemberKind::New(n) => ("new", n.as_deref().or(Some("new"))),
    };
    (
        k.owner.map(|c| c.name.as_str()),
        k.cx.module,
        k.cx.namespace.to_vec(),
        m.is_static,
        kind,
        n,
    )
}

/// [`naming::type_word`] with type arguments spelled, so `Iterable<F64>`
/// and `Iterable<T>` name apart.
fn spelled_word(t: &Type) -> String {
    match t {
        Type::Named(r) if !r.args.is_empty() => {
            naming::type_word(t) + &r.args.iter().map(spelled_word).collect::<String>()
        }
        Type::Nullable(t, _) => spelled_word(t),
        Type::Array(t) | Type::ArrBox(t) => spelled_word(t) + "Array",
        _ => naming::type_word(t),
    }
}

fn member(l: &Layouts<'_>, k: &Keyed<'_>, variant: Option<String>) -> Json {
    l.take_warnings();
    let m = k.member;
    let (role, module) = match &k.role {
        Role::Import(module) => ("import", Json::str(module)),
        Role::Export => ("export", Json::Null),
    };
    let owner =
        k.owner.and_then(
            |c| match l.keys.symbols.resolve(&Path::new([c.name.as_str()]), &k.cx) {
                Some(Target::Decl(loc, _)) => Some(key::location(loc)),
                _ => None,
            },
        );
    let (kind, mname) = match &m.kind {
        MemberKind::Method(n) => ("method", Some(n.clone())),
        MemberKind::Get(n) => ("get", Some(n.clone())),
        MemberKind::Set(n) => ("set", Some(n.clone())),
        MemberKind::New(n) => ("new", n.clone()),
    };
    let tparams = m.tparams.iter().map(|p| tparam(l, p, &k.cx)).collect();
    let params = m
        .params
        .iter()
        .map(|p| {
            let t = match &p.ty {
                Type::Nullable(t, _) => t,
                _ => &p.ty,
            };
            let alias = match t {
                Type::Named(r) => match l.keys.symbols.resolve(&r.path, &k.cx) {
                    Some(Target::Decl(loc, TypeDecl::Alias(_))) => Some(key::location(loc)),
                    _ => None,
                },
                _ => None,
            };
            Json::obj()
                .set("name", Json::str(name(&p.name)))
                .set("alias", Json::opt_str(alias))
                .set("once", Json::Bool(p.once || m.meta.attrs.once))
                .set("unshared", Json::Bool(p.unshared))
                .set("union", Json::Bool(p.union))
                .done()
        })
        .collect();
    // The constructor of an exported class with an imported base, as the
    // `tw:new` import (§17).
    let construct = match (&k.role, &m.kind) {
        (Role::Export, MemberKind::New(_)) => l
            .construct(k)
            .and_then(|s| signature(l, &s))
            .unwrap_or(Json::Null),
        _ => Json::Null,
    };
    let sig = match l.member(k, &[]).and_then(|s| signature(l, &s)) {
        Ok(s) => ("sig", s),
        Err(e) => ("error", Json::str(e.to_string())),
    };
    let o = Json::obj()
        .set("key", Json::str(&k.key))
        .set("role", Json::str(role))
        .set("module", module)
        .set("owner", Json::opt_str(owner))
        .set("scope", Json::opt_str(k.cx.module))
        .set(
            "namespace",
            Json::Arr(k.cx.namespace.iter().map(|n| Json::str(*n)).collect()),
        )
        .set("kind", Json::str(kind))
        .set("name", Json::opt_str(mname))
        .set("variant", Json::opt_str(variant))
        .set("static", Json::Bool(m.is_static))
        .set("jsName", js_name(&m.meta))
        .set("tparams", Json::Arr(tparams))
        .set("params", Json::Arr(params))
        .set("doc", doc(&m.meta))
        .set(sig.0, sig.1)
        .set("construct", construct);
    // The member's approximations, such as a box `T??` under
    // `box-none = null`.
    let warnings = l.take_warnings();
    if warnings.is_empty() {
        o.done()
    } else {
        o.set(
            "warnings",
            Json::Arr(warnings.iter().map(Json::str).collect()),
        )
        .done()
    }
}

/// `core` is the language's declaration: under `results = single`, slot 0
/// only, with the other result slots in `spill`.
fn signature(l: &Layouts<'_>, s: &Signature) -> crate::layout::Result<Json> {
    let pos = |ps: &[Position]| Json::Arr(ps.iter().map(|p| position(l, p)).collect());
    let core = l.lang_core(&s.core())?;
    Ok(Json::obj()
        .set("params", pos(&s.params))
        .set("results", pos(&s.results))
        .set("throws", throws(l, s.throws.as_deref()))
        .set(
            "core",
            Json::obj()
                .set("params", wire(&core.ty.params))
                .set("results", wire(&core.ty.results))
                .set("spill", wire(&core.spill))
                .done(),
        )
        .done())
}

/// Under `throws = js`, `read` and `set` replace the tag (§15).
fn throws(l: &Layouts<'_>, e: Option<&EnumLayout>) -> Json {
    let Some(e) = e else { return Json::Null };
    let o = match l.profile.throws {
        Throws::Tag => Json::obj().set("tag", import(protocol::tag(&e.path))),
        Throws::Js => Json::obj()
            .set("read", import(protocol::err_read(&e.path)))
            .set("set", import(protocol::err_set(&e.path))),
    };
    o.set("enum", enum_layout(l, e)).done()
}

fn position(l: &Layouts<'_>, p: &Position) -> Json {
    let (role, index) = match &p.role {
        Pos::This => ("this", None),
        Pos::Handle => ("handle", None),
        Pos::Env => ("env", None),
        Pos::Callee => ("callee", None),
        Pos::Future => ("future", None),
        Pos::Index => ("index", None),
        Pos::Param(i) => ("param", Some(*i as i64)),
        Pos::Result => ("result", None),
    };
    let mut o = Json::obj().set("role", Json::str(role));
    if let Some(i) = index {
        o = o.set("index", Json::Int(i));
    }
    // The function value of its own operation: its wire only, since its
    // repr is the one being described.
    if matches!(p.role, Pos::Env | Pos::Callee) {
        return o.set("wire", wire(&p.wire)).done();
    }
    o.set("repr", repr(l, &p.repr)).done()
}

/// A type's layout: its canonical spelling, its wire values, and what it is.
pub fn repr(l: &Layouts<'_>, r: &Repr) -> Json {
    let w = l.wire(r).map(|w| wire(&w)).unwrap_or(Json::Null);
    let o = Json::obj().set("ty", Json::str(&r.ty)).set("wire", w);
    let nullable = |o: Obj, n: bool| o.set("nullable", Json::Bool(n));
    match &r.kind {
        Kind::Unit => o.set("kind", Json::str("unit")),
        Kind::Never => o.set("kind", Json::str("never")),
        Kind::Scalar(s) => o
            .set("kind", Json::str("scalar"))
            .set("scalar", Json::str(s.name())),
        Kind::NullScalar(s, n) => o
            .set("kind", Json::str("scalar"))
            .set("scalar", Json::str(s.name()))
            .set("nullable", Json::Bool(true))
            .set(
                "niche",
                match n {
                    ScalarNull::Niche(v) => Json::Int(*v),
                    ScalarNull::Present => Json::Null,
                },
            ),
        Kind::Str {
            carrier,
            nullable: n,
        } => nullable(
            o.set("kind", Json::str("str"))
                .set("carrier", Json::str(str_carrier(*carrier))),
            *n,
        ),
        Kind::Vec {
            carrier: c,
            nullable: n,
            bigint: true,
            ..
        } => nullable(
            o.set("kind", Json::str("bigint"))
                .set("carrier", Json::str(carrier(*c))),
            *n,
        ),
        Kind::Vec {
            elem,
            carrier: c,
            writeback,
            nullable: n,
            ..
        } => nullable(
            o.set("kind", Json::str("vec"))
                .set("elem", Json::str(elem.name()))
                .set("carrier", Json::str(carrier(*c)))
                .set("writeback", Json::Bool(*writeback)),
            *n,
        ),
        Kind::Array {
            elem,
            carrier: c,
            nullable: n,
        } => nullable(
            o.set("kind", Json::str("array"))
                .set("elem", repr(l, elem))
                .set("carrier", Json::str(carrier(*c))),
            *n,
        ),
        Kind::Tuple { elems, nullable: n } => nullable(
            o.set("kind", Json::str("tuple")).set(
                "elems",
                Json::Arr(elems.iter().map(|e| repr(l, e)).collect()),
            ),
            *n,
        ),
        Kind::Enum {
            layout,
            nullable: n,
        } => nullable(
            o.set("kind", Json::str("enum"))
                .set("enum", enum_layout(l, layout)),
            *n,
        ),
        Kind::Host { of, nullable: n } => {
            let o = o.set("kind", Json::str("host"));
            let o = match of {
                Host::Class { path, interface } => o
                    .set(
                        "of",
                        Json::str(if *interface { "interface" } else { "class" }),
                    )
                    .set("path", Json::str(path)),
                Host::Any => o.set("of", Json::str("any")),
                Host::Box(inner) => o
                    .set("of", Json::str("box"))
                    .set("inner", repr(l, inner))
                    .set("box", import(protocol::box_(&inner.ty)))
                    .set("unbox", import(protocol::unbox(&inner.ty))),
                Host::Future { value, error } => o
                    .set("of", Json::str("future"))
                    .set("value", repr(l, value))
                    .set("rejection", enum_layout(l, error))
                    .set("take", import(protocol::future_take(&value.ty)))
                    .set("errorOp", import(protocol::future_error(&error.path)))
                    .set("new", import(protocol::future_new()))
                    .set("resolve", import(protocol::future_resolve(&value.ty)))
                    .set("reject", import(protocol::future_reject(&error.path))),
            };
            nullable(o, *n)
        }
        Kind::Handle {
            class,
            carrier: c,
            nullable: n,
        } => nullable(
            o.set("kind", Json::str("handle"))
                .set("class", Json::str(class))
                .set("carrier", Json::str(carrier(*c))),
            *n,
        ),
        Kind::Fn {
            sig,
            provided,
            nullable: n,
        } => {
            let params = Json::Arr(sig.params.iter().map(|p| repr(l, p)).collect());
            let o = o
                .set("kind", Json::str("fn"))
                .set("params", params)
                .set("result", repr(l, &sig.result))
                .set("throws", throws(l, sig.throws.as_deref()));
            let o = match provided {
                Some(c) => {
                    let ty = r.ty.trim_end_matches('?');
                    let some = Repr {
                        ty: ty.into(),
                        kind: Kind::Fn {
                            sig: sig.clone(),
                            provided: *provided,
                            nullable: false,
                        },
                        js_null: false,
                    };
                    let (box_, unbox) = boxing(l, ty, Ok(some));
                    o.set("provided", Json::str(carrier(*c)))
                        .set("trampoline", Json::str(protocol::call_export(&r.ty)))
                        .some("box", box_)
                        .some("unbox", unbox)
                }
                None => o
                    .set("provided", Json::Null)
                    .set("call", import(protocol::call(&r.ty))),
            };
            let op = l
                .fn_op(r)
                .and_then(|s| signature(l, &s))
                .unwrap_or_else(|e| Json::obj().set("error", Json::str(e.to_string())).done());
            nullable(o.set("op", op), *n)
        }
        Kind::Erased { nullable: n } => nullable(o.set("kind", Json::str("erased")), *n),
        Kind::Rec {
            ty,
            provided,
            nullable: n,
            ..
        } => {
            RECS.with(|recs| {
                recs.borrow_mut()
                    .push((r.ty.trim_end_matches('?').to_string(), (**ty).clone()))
            });
            nullable(
                o.set("kind", Json::str("rec")).set(
                    "provided",
                    provided.map_or(Json::Null, |c| Json::str(carrier(c))),
                ),
                *n,
            )
        }
        Kind::View { elem, mutable } => o
            .set("kind", Json::str("view"))
            .set("elem", Json::str(elem.name()))
            .set("mutable", Json::Bool(*mutable)),
    }
    .done()
}

fn enum_layout(l: &Layouts<'_>, e: &EnumLayout) -> Json {
    let origin = match &e.origin {
        Origin::Decl(_) => "decl",
        Origin::Err => "err",
        Origin::Nested => "nested",
    };
    let shape = match &e.shape {
        Shape::Index => Json::obj().set("kind", Json::str("index")).done(),
        Shape::Carrier(s) => Json::obj()
            .set("kind", Json::str("carrier"))
            .set("scalar", Json::str(s.name()))
            .done(),
        Shape::Payload { slots } => Json::obj()
            .set("kind", Json::str("payload"))
            .set("slots", wire(slots))
            .done(),
    };
    let arms = e
        .arms
        .iter()
        .map(|a| {
            let literal = a.js.literal().map(String::from);
            Json::obj()
                .set("name", Json::str(&a.name))
                .set("index", Json::Int(a.index))
                .set("fallback", Json::Bool(a.fallback))
                .set("literal", Json::opt_str(literal))
                .set(
                    "payload",
                    Json::Arr(a.payload.iter().map(|p| repr(l, p)).collect()),
                )
                .set(
                    "slots",
                    Json::Arr(a.slots.iter().map(|s| Json::Int(*s as i64)).collect()),
                )
                .done()
        })
        .collect();
    Json::obj()
        .set("path", Json::str(&e.path))
        .set("origin", Json::str(origin))
        .set("shape", shape)
        .set("arms", Json::Arr(arms))
        .done()
}
