//! Matching the language module's imports to what the bridge provides: IR
//! members by key, and `tw` protocol operations by group and subject.

use std::collections::HashMap;
use std::rc::Rc;

use ts_gen_abi::ir::{Bound, MemberKind, Path, Type};
use ts_gen_abi::key::{Keyed, Role};
use ts_gen_abi::layout::{
    EnumLayout, Flow, FuncType, Kind, LangCore, Layouts, Repr, Signature, ValType,
};
use ts_gen_abi::profile::{Results, Throws};
use ts_gen_abi::protocol;
use ts_gen_abi::resolve::Context;
use ts_gen_abi::text;
use ts_gen_abi::union;

use crate::module::{ImportKind, Module};
use crate::{Error, Result};

pub enum Op<'a> {
    Member(Keyed<'a>),
    InstanceOf(Path),
    Box(Repr),
    Unbox {
        repr: Repr,
        checked: bool,
    },
    /// `("tw:new", ctor)`: `new C(...)` through JS (§17).
    New(Keyed<'a>),
    /// `("tw:tryUnbox", C)`, on the `C?` result's repr.
    TryUnbox(Repr),
    ArrLen,
    ArrGet(Repr),
    IsNullish,
    IsUndefined,
    /// `("tw:unit")`, `("tw:true")` or `("tw:false")`, by its default group:
    /// served by the bridge module.
    Constant(&'static str),
    /// `("tw:destroy", F | C)` on the handle position's repr (§14).
    Destroy(Repr),
    /// `("tw:call", F)`: the language calls a JS function (§18).
    CallFn,
    /// `("tw:future", ...)` (§16).
    Poll,
    Take(Option<Repr>),
    FutureError(Rc<EnumLayout>),
    FutureDrop,
    FutureNew,
    Resolve(Option<Repr>),
    Reject(Rc<EnumLayout>),
    /// `("tw:ret", vt)` and `("tw:ret", "push:vt")` (§12): served by the
    /// bridge module's spill.
    RetPop(&'static str),
    RetPush(&'static str),
    /// `("tw:err", E)` and `("tw:err", "set:E")` (§15).
    ErrRead(Rc<EnumLayout>),
    ErrSet(Rc<EnumLayout>),
    /// `("tw:loop", ...)` (§16).
    WorkAvailable,
    LoopDrop,
}

impl Op<'_> {
    /// An operation whose core type is fixed, not its signature's.
    fn fixed(&self) -> bool {
        matches!(
            self,
            Op::InstanceOf(_)
                | Op::ArrLen
                | Op::IsNullish
                | Op::IsUndefined
                | Op::Constant(_)
                | Op::Poll
                | Op::FutureDrop
                | Op::FutureNew
                | Op::WorkAvailable
                | Op::LoopDrop
        )
    }
}

pub struct Func<'a> {
    pub module: String,
    pub name: String,
    pub op: Op<'a>,
    pub sig: Signature,
    /// The core type as the language declares it, and its spilled slots.
    pub core: LangCore,
}

pub struct Tag {
    pub module: String,
    pub name: String,
    pub layout: Rc<EnumLayout>,
}

pub struct Plan<'a> {
    pub funcs: Vec<Func<'a>>,
    pub tags: Vec<Tag>,
}

impl<'a> Plan<'a> {
    pub fn new(l: &Layouts<'a>, m: &Module) -> Result<Plan<'a>> {
        let p = l.profile;
        let mut members: HashMap<(String, String), Keyed<'a>> = HashMap::new();
        let mut ctors: HashMap<String, Keyed<'a>> = HashMap::new();
        // Members with a `union` parameter, by import module, whose arm
        // variants are derived when imported (§7).
        let mut unions: HashMap<String, Vec<Keyed<'a>>> = HashMap::new();
        for k in l.keys.members() {
            match &k.role {
                Role::Import(module) => {
                    if k.member.params.iter().any(|p| p.union) {
                        unions.entry(module.clone()).or_default().push(k.clone());
                    }
                    members.entry((module.clone(), k.key.clone())).or_insert(k);
                }
                Role::Export if matches!(k.member.kind, MemberKind::New(_)) => {
                    ctors.insert(k.key.clone(), k);
                }
                Role::Export => {}
            }
        }
        let group = |default: &str| p.name(default).to_string();
        let global = group(protocol::GLOBAL);
        let groups: Vec<String> = [
            protocol::GLOBAL,
            protocol::INSTANCEOF,
            protocol::DESTROY,
            protocol::CALL,
            protocol::BOX,
            protocol::UNBOX,
            protocol::UNBOX_UNCHECKED,
            protocol::TRY_UNBOX,
            protocol::NEW,
            protocol::ARR,
            protocol::IS_NULLISH,
            protocol::IS_UNDEFINED,
            protocol::UNIT,
            protocol::TRUE,
            protocol::FALSE,
            protocol::TAG,
            protocol::FUTURE,
            protocol::LOOP,
            protocol::RET,
            protocol::ERR,
        ]
        .iter()
        .map(|g| group(g))
        .collect();
        let mut plan = Plan {
            funcs: Vec::new(),
            tags: Vec::new(),
        };
        for i in &m.imports {
            let what = || format!("(\"{}\", \"{}\")", i.module, i.name);
            let user_module = if i.module == global {
                protocol::GLOBAL.to_string()
            } else {
                i.module.clone()
            };
            // A module neither the IR nor the protocol names is the
            // embedder's, passed to `instantiate` alongside the bridge.
            if !groups.contains(&i.module)
                && !i.module.starts_with(protocol::PREFIX)
                && !members.keys().any(|(m, _)| *m == user_module)
            {
                continue;
            }
            if let ImportKind::Unreadable(e) = &i.kind {
                return Err(Error::Import(format!("{}: {e}", what())));
            }
            if let ImportKind::Tag(ty) = &i.kind {
                if p.throws == Throws::Js {
                    return Err(Error::Import(format!(
                        "{}: `throws = js` uses no tags; catch JS exceptions and read them with `tw:err`",
                        what()
                    )));
                }
                if i.module != group(protocol::TAG) {
                    return Err(Error::Import(format!("{}: an unknown tag", what())));
                }
                let path = type_path(&i.name, &what)?;
                let layout = l.enum_of(&path, &Context::default(), Flow::ToLang)?;
                let want = l.tag(&path, &Context::default())?;
                check(&what(), &ty.params, &want)?;
                plan.tags.push(Tag {
                    module: i.module.clone(),
                    name: i.name.clone(),
                    layout,
                });
                continue;
            }
            let ImportKind::Func(ty) = &i.kind else {
                return Err(Error::Import(format!(
                    "{}: the bridge provides only functions and tags",
                    what()
                )));
            };
            let cx = Context::default();
            let arm = || {
                unions.get(&user_module)?.iter().find_map(|k| {
                    union::variants(&l.keys, k)
                        .into_iter()
                        .find(|(key, _)| *key == i.name)
                        .map(|(_, tys)| (k.clone(), tys))
                })
            };
            let (op, sig) = if let Some(k) = members.get(&(user_module.clone(), i.name.clone())) {
                let sig = l.member(k, &[])?;
                (Op::Member(k.clone()), sig)
            } else if let Some((k, tys)) = arm() {
                // An arm variant: the same call, the arm's payload converted.
                let sig = l.member_as(&k, &[], Some(&tys))?;
                (Op::Member(k), sig)
            } else if i.module == group(protocol::INSTANCEOF) {
                let path = type_path(&i.name, &what)?;
                let sig = Signature::default();
                (Op::InstanceOf(path), sig)
            } else if i.module == group(protocol::BOX) {
                let t = subject(&i.name, &what)?;
                let sig = l.box_(&t, &cx)?;
                (Op::Box(sig.params[0].repr.clone()), sig)
            } else if i.module == group(protocol::UNBOX)
                || i.module == group(protocol::UNBOX_UNCHECKED)
            {
                let t = subject(&i.name, &what)?;
                let sig = l.unbox(&t, &cx)?;
                let repr = match sig.results.first() {
                    Some(r) => r.repr.clone(),
                    None => l.repr(&t, &cx, Flow::ToLang)?,
                };
                let checked = i.module == group(protocol::UNBOX);
                (Op::Unbox { repr, checked }, sig)
            } else if i.module == group(protocol::NEW) {
                let Some(k) = ctors.get(&i.name) else {
                    return Err(Error::Import(format!(
                        "{}: not an exported class's constructor",
                        what()
                    )));
                };
                (Op::New(k.clone()), l.construct(k)?)
            } else if i.module == group(protocol::TRY_UNBOX) {
                let t = subject(&i.name, &what)?;
                let sig = l.try_unbox(&t, &cx)?;
                (Op::TryUnbox(sig.results[0].repr.clone()), sig)
            } else if i.module == group(protocol::IS_NULLISH) && i.name.is_empty() {
                (Op::IsNullish, Signature::default())
            } else if i.module == group(protocol::IS_UNDEFINED) && i.name.is_empty() {
                (Op::IsUndefined, Signature::default())
            } else if let Some(c) = [protocol::UNIT, protocol::TRUE, protocol::FALSE]
                .into_iter()
                .find(|c| i.module == group(c) && i.name.is_empty())
            {
                (Op::Constant(c), Signature::default())
            } else if i.module == group(protocol::ARR) && i.name == "len" {
                let sig = Signature::default();
                (Op::ArrLen, sig)
            } else if i.module == group(protocol::ARR) && i.name.starts_with("get<") {
                let t = subject(&i.name["get<".len()..i.name.len() - 1], &what)?;
                let sig = l.arr_get(&t, &cx)?;
                let repr = l.repr(&t, &cx, Flow::ToLang)?;
                (Op::ArrGet(repr), sig)
            } else if i.module == group(protocol::DESTROY) {
                let t = subject(&i.name, &what)?;
                let handle = l.box_(&t, &cx)?.params.remove(0);
                if !matches!(
                    handle.repr.kind,
                    Kind::Handle { .. }
                        | Kind::Fn {
                            provided: Some(_),
                            ..
                        }
                ) {
                    return Err(Error::Import(format!(
                        "{}: only exported classes and function types have handles",
                        what()
                    )));
                }
                let sig = Signature {
                    params: vec![handle.clone()],
                    results: Vec::new(),
                    throws: None,
                };
                (Op::Destroy(handle.repr), sig)
            } else if i.module == group(protocol::CALL) {
                let Type::Fn(f) = subject(&i.name, &what)? else {
                    return Err(Error::Import(format!(
                        "{}: expected a function type",
                        what()
                    )));
                };
                (Op::CallFn, l.call_import(&f, &cx)?)
            } else if i.module == group(protocol::FUTURE) {
                let empty = Signature::default();
                let arg = |prefix: &str| {
                    i.name
                        .strip_prefix(prefix)
                        .and_then(|n| n.strip_suffix('>'))
                };
                match i.name.as_str() {
                    "poll" => (Op::Poll, empty),
                    "drop" => (Op::FutureDrop, empty),
                    "new" => (Op::FutureNew, empty),
                    _ => {
                        if let Some(t) = arg("take<") {
                            let sig = l.future_take(&subject(t, &what)?, &cx)?;
                            (Op::Take(sig.results.first().map(|r| r.repr.clone())), sig)
                        } else if let Some(t) = arg("resolve<") {
                            let sig = l.future_resolve(&subject(t, &what)?, &cx)?;
                            (Op::Resolve(sig.params.get(1).map(|p| p.repr.clone())), sig)
                        } else if let Some(e) = arg("error<") {
                            let path = type_path(e, &what)?;
                            let sig = l.future_error(&path, &cx)?;
                            (Op::FutureError(l.enum_of(&path, &cx, Flow::ToLang)?), sig)
                        } else if let Some(e) = arg("reject<") {
                            let path = type_path(e, &what)?;
                            let sig = l.future_reject(&path, &cx)?;
                            (Op::Reject(l.enum_of(&path, &cx, Flow::ToJs)?), sig)
                        } else {
                            return Err(Error::Import(format!(
                                "{}: not a future operation",
                                what()
                            )));
                        }
                    }
                }
            } else if i.module == group(protocol::RET) {
                if p.results != Results::Single {
                    return Err(Error::Import(format!(
                        "{}: the spill needs `results = single`",
                        what()
                    )));
                }
                let (push, vt) = match i.name.strip_prefix(protocol::RET_PUSH) {
                    Some(vt) => (true, vt),
                    None => (false, i.name.as_str()),
                };
                let Some(vt) = protocol::SPILL_TYPES.iter().find(|t| **t == vt) else {
                    return Err(Error::Import(format!("{}: not a spill type", what())));
                };
                let empty = Signature::default();
                (
                    if push {
                        Op::RetPush(vt)
                    } else {
                        Op::RetPop(vt)
                    },
                    empty,
                )
            } else if i.module == group(protocol::ERR) {
                if p.throws != Throws::Js {
                    return Err(Error::Import(format!(
                        "{}: `tw:err` needs `throws = js`",
                        what()
                    )));
                }
                let (set, e) = match i.name.strip_prefix(protocol::ERR_SET) {
                    Some(e) => (true, e),
                    None => (false, i.name.as_str()),
                };
                let path = type_path(e, &what)?;
                let empty = Signature::default();
                if set {
                    (Op::ErrSet(l.enum_of(&path, &cx, Flow::ToJs)?), empty)
                } else {
                    (Op::ErrRead(l.enum_of(&path, &cx, Flow::ToLang)?), empty)
                }
            } else if i.module == group(protocol::LOOP) {
                let empty = Signature::default();
                match i.name.as_str() {
                    "work_available" => (Op::WorkAvailable, empty),
                    "drop" => (Op::LoopDrop, empty),
                    _ => return Err(Error::Import(format!("{}: not a loop operation", what()))),
                }
            } else if let Some((base, targs)) = mono(&i.name) {
                // A mono variant (§13): the generic member, with its type
                // arguments substituted.
                let Some(k) = members.get(&(user_module.clone(), base)) else {
                    return Err(Error::Import(format!(
                        "{}: no generic IR member has this name",
                        what()
                    )));
                };
                let targs = mono_args(&targs, &what)?;
                bounds(l, k, &targs, &what)?;
                let sig = l.member(k, &targs)?;
                (Op::Member(k.clone()), sig)
            } else {
                return Err(Error::Import(format!(
                    "{}: no IR member or protocol operation has this name",
                    what()
                )));
            };
            let want = if let Op::RetPop(vt) | Op::RetPush(vt) = &op {
                let v = spill_type(vt);
                if matches!(op, Op::RetPush(_)) {
                    FuncType {
                        params: vec![v],
                        results: Vec::new(),
                    }
                } else {
                    FuncType {
                        params: Vec::new(),
                        results: vec![v],
                    }
                }
            } else if let Op::ErrRead(e) | Op::ErrSet(e) = &op {
                let wire = l.tag(&type_path(&e.path, &what)?, &cx)?;
                if matches!(op, Op::ErrSet(_)) {
                    FuncType {
                        params: wire,
                        results: Vec::new(),
                    }
                } else {
                    FuncType {
                        params: vec![ValType::EXTERNREF],
                        results: wire,
                    }
                }
            } else if op.fixed() {
                let default = match &op {
                    Op::InstanceOf(_) => protocol::INSTANCEOF,
                    Op::ArrLen => protocol::ARR,
                    Op::IsNullish => protocol::IS_NULLISH,
                    Op::IsUndefined => protocol::IS_UNDEFINED,
                    Op::Constant(c) => c,
                    Op::WorkAvailable | Op::LoopDrop => protocol::LOOP,
                    _ => protocol::FUTURE,
                };
                l.fixed_op(Some(default), &i.name)
                    .expect("a fixed protocol operation")
            } else {
                sig.core()
            };
            let core = if matches!(op, Op::RetPop(_) | Op::RetPush(_)) {
                LangCore {
                    ty: want,
                    spill: Vec::new(),
                }
            } else {
                l.lang_core(&want)?
            };
            check(&what(), &ty.params, &core.ty.params)?;
            check(&what(), &ty.results, &core.ty.results)?;
            plan.funcs.push(Func {
                module: i.module.clone(),
                name: i.name.clone(),
                op,
                sig,
                core,
            });
        }
        Ok(plan)
    }
}

fn spill_type(vt: &str) -> ValType {
    match vt {
        "i32" => ValType::I32,
        "i64" => ValType::I64,
        "f32" => ValType::F32,
        "f64" => ValType::F64,
        _ => ValType::EXTERNREF,
    }
}

/// Splits a mono key into the generic key and its type argument list:
/// `Map#get<Str,f64>(K)` is `Map#get(K)` and `Str,f64`.
pub(crate) fn mono(key: &str) -> Option<(String, String)> {
    let open = key.find(['<', '('])?;
    if key.as_bytes()[open] != b'<' {
        return None;
    }
    let bytes = key.as_bytes();
    let mut depth = 0i32;
    for (i, &c) in bytes.iter().enumerate().skip(open) {
        match c {
            b'<' | b'(' => depth += 1,
            b'>' if i > 0 && bytes[i - 1] == b'-' => {}
            b'>' | b')' => {
                depth -= 1;
                if depth == 0 {
                    if c != b'>' || !key[i + 1..].starts_with('(') {
                        return None;
                    }
                    let base = format!("{}{}", &key[..open], &key[i + 1..]);
                    return Some((base, key[open + 1..i].to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn mono_args(list: &str, what: &dyn Fn() -> String) -> Result<Vec<Type>> {
    match subject(&format!("T<{list}>"), what)? {
        Type::Named(r) => Ok(r.args),
        _ => Err(Error::Import(format!("{}: bad type arguments", what()))),
    }
}

/// A concrete bound `T in (A, B)` admits only its listed types (§13).
pub(crate) fn bounds(
    l: &Layouts<'_>,
    k: &Keyed<'_>,
    targs: &[Type],
    what: &dyn Fn() -> String,
) -> Result<()> {
    let tparams = k
        .owner
        .iter()
        .flat_map(|c| &c.tparams)
        .chain(&k.member.tparams);
    let global = Context::default();
    for (p, t) in tparams.zip(targs) {
        let Some(Bound::In(allowed)) = &p.bound else {
            continue;
        };
        let got = l.keys.ty(t, &global);
        if !allowed.iter().any(|a| l.keys.ty(a, &k.cx) == got) {
            return Err(Error::Import(format!(
                "{}: `{got}` isn't in the bound of `{}`",
                what(),
                p.name
            )));
        }
    }
    Ok(())
}

fn subject(s: &str, what: &dyn Fn() -> String) -> Result<Type> {
    text::parse_type(s).map_err(|e| Error::Import(format!("{}: {e}", what())))
}

fn type_path(s: &str, what: &dyn Fn() -> String) -> Result<Path> {
    match subject(s, what)? {
        Type::Named(r) if r.args.is_empty() => Ok(r.path),
        _ => Err(Error::Import(format!(
            "{}: expected a declaration path",
            what()
        ))),
    }
}

pub(crate) fn check(what: &str, got: &[ValType], want: &[ValType]) -> Result<()> {
    if got == want {
        return Ok(());
    }
    let list = |vs: &[ValType]| {
        let vs: Vec<String> = vs.iter().map(ToString::to_string).collect();
        format!("({})", vs.join(", "))
    };
    Err(Error::Import(format!(
        "{what}: the language declares {}, the layout is {}",
        list(got),
        list(want)
    )))
}
