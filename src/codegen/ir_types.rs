//! IR members in the generator's signature model.
//!
//! The Rust signature pipeline renders [`crate::ir`] types. A callable read
//! from the IR keeps its IR overloads, already expanded by the producer,
//! and states each type in that model. A type the model reads differently
//! from the IR (a synthesized enum the parse never saw, a name that resolves
//! elsewhere from the parse scope) has no statement, and the callable stays
//! on the parse.

use std::collections::HashMap;

use ts_gen_abi::ir as abi;
use ts_gen_abi::resolve::{Context, Location, Symbols, Target, TypeDecl};

use crate::context::{GlobalContext, TypeId};
use crate::ir::{AbiScalar, FunctionSig, Param, Throws, TypeKind, TypeRef};
use crate::parse::scope::ScopeId;

/// One IR overload, stated in the signature model.
pub struct Overload {
    pub params: Vec<Param>,
    /// The JS arguments it passes, in order.
    pub args: Vec<Arg>,
    /// Its own type parameters.
    pub tparams: Vec<String>,
    /// Each of [`Self::tparams`]' default.
    pub defaults: Vec<Option<TypeRef>>,
    pub return_type: TypeRef,
    pub throws: Throws,
    pub doc: Option<String>,
    /// Each parameter's enum arms, where it takes an enum: the types an
    /// overload per arm takes for it.
    pub members: Vec<Option<Vec<TypeRef>>>,
    /// Whether Rust takes each parameter's enum whole: one of only
    /// literals, or of two or more payload arms, each a primitive or a
    /// non-generic class. Otherwise it expands an overload per arm.
    pub whole: Vec<bool>,
}

/// What a callable's types resolve against.
pub struct Cx<'a> {
    pub gctx: &'a GlobalContext,
    pub symbols: &'a Symbols<'a>,
    pub ids: &'a HashMap<Location, TypeId>,
    pub locations: &'a HashMap<TypeId, Location>,
    /// The container the members are declared in.
    pub module: Option<&'a str>,
    pub namespace: Vec<&'a str>,
    /// The type parameters in scope: the class's, then the member's.
    pub tparams: Vec<&'a str>,
    /// The class's associated types.
    pub assoc: Vec<&'a abi::Assoc>,
    /// Whether an associated type reads as its parameter's (mono), rather
    /// than its first row (erased).
    pub mono: bool,
    /// The parse scope the callable's types resolve in.
    pub scope: ScopeId,
    /// Type parameters of a parent the child doesn't pass its own to,
    /// which read as the parent's default instantiation does: `Any`.
    pub erased: Vec<&'a str>,
    /// The member's own type parameters, which its own overload binds
    /// when another overload's scope doesn't.
    pub own: Vec<&'a str>,
    /// The scopes of the callable's other parse overloads, whose type
    /// parameters the IR's may name (a generic constructor's, which the
    /// IR states as the class's).
    pub bound: &'a [ScopeId],
}

impl Cx<'_> {
    fn context(&self) -> Context<'_> {
        Context {
            module: self.module,
            namespace: self.namespace.clone(),
            tparams: self.tparams.clone(),
            assoc: self.assoc.clone(),
        }
    }
}

/// One JS argument of a call.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    /// The parameter at this index, spread when it's variadic.
    Param(usize),
    Str(String),
    Null,
}

/// Whether `m` is the member JS names `js_name`: `[Symbol.iterator]` for a
/// `@js(symbol = iterator)` one.
pub fn is_named(m: &abi::Member, js_name: &str) -> bool {
    match js_name
        .strip_prefix("[Symbol.")
        .and_then(|s| s.strip_suffix(']'))
    {
        Some(symbol) => m
            .meta
            .attrs
            .js
            .iter()
            .any(|e| matches!(e, abi::JsEntry::Symbol(s) if s == symbol)),
        None => self::js_name(m) == Some(js_name),
    }
}

/// The JS name a member binds.
pub fn js_name(m: &abi::Member) -> Option<&str> {
    m.meta
        .attrs
        .js
        .iter()
        .find_map(|e| match e {
            abi::JsEntry::Name(n) => Some(n.as_str()),
            _ => None,
        })
        .or_else(|| m.kind.name())
}

/// `m` as a plain call: no `@js` facts beyond a rename, no type parameters.
fn plain(m: &abi::Member) -> bool {
    m.tparams.is_empty()
        && m.meta
            .attrs
            .js
            .iter()
            .all(|e| matches!(e, abi::JsEntry::Name(_)))
        && !m.meta.attrs.once
}

/// A method or function overload, or `None` when the model can't state it.
pub fn overload(cx: &Cx<'_>, m: &abi::Member) -> Option<Overload> {
    let mut template = None;
    for e in &m.meta.attrs.js {
        match e {
            // The value a constructor `new`s is the binding's `js_class`.
            abi::JsEntry::Name(_) | abi::JsEntry::Symbol(_) | abi::JsEntry::New(_) => {}
            // wasm-bindgen: a binding can't cancel its call, so reads it
            // without the `AbortSignal` it would pass.
            abi::JsEntry::Let(..) | abi::JsEntry::Cancel(_) => {}
            abi::JsEntry::Args(t) => template = Some(t),
            _ => return None,
        }
    }
    if m.meta.attrs.once {
        return None;
    }
    let mut cx = Cx {
        tparams: cx.tparams.clone(),
        namespace: cx.namespace.clone(),
        assoc: cx.assoc.clone(),
        erased: cx.erased.clone(),
        own: m.tparams.iter().map(|p| p.name.as_str()).collect(),
        ..*cx
    };
    cx.tparams.extend(m.tparams.iter().map(|p| p.name.as_str()));
    let cx = &cx;
    let index = |n: &str| m.params.iter().position(|p| p.name == n);
    let args = match template {
        None => (0..m.params.len()).map(Arg::Param).collect(),
        Some(t) => t
            .iter()
            .map(|e| match e {
                abi::Telem::Param(n)
                | abi::Telem::Spread(n)
                | abi::Telem::Fill { param: n, .. } => index(n).map(Arg::Param),
                abi::Telem::Lit(abi::Literal::Str(v)) => Some(Arg::Str(v.clone())),
                abi::Telem::Lit(abi::Literal::Null) => Some(Arg::Null),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
    };
    let spread = |n: &str| {
        template.is_some_and(|t| {
            t.iter()
                .any(|e| matches!(e, abi::Telem::Spread(s) if s == n))
        })
    };
    // wasm-bindgen: a filled argument is absent only to fill it with the
    // `AbortSignal` a binding doesn't pass, so takes its type alone.
    let filled = |n: &str| {
        template.is_some_and(|t| {
            t.iter()
                .any(|e| matches!(e, abi::Telem::Fill { param, .. } if param == n))
        })
    };
    let params = m
        .params
        .iter()
        .map(|p| {
            if p.once {
                return None;
            }
            let t = ty(cx, &p.ty)?;
            // A view of the module's memory, shared unless the callee
            // rejects it.
            let view = matches!(p.ty, abi::Type::View { mutable: false, .. });
            let t = match t {
                TypeRef::TypedSlice(e, _) if view => TypeRef::TypedSlice(
                    e,
                    match p.unshared {
                        true => crate::ir::SharedMemory::Rejected,
                        false => crate::ir::SharedMemory::Accepted,
                    },
                ),
                t => t,
            };
            Some(Param {
                name: crate::util::naming::to_snake_case(&p.name),
                js_name: None,
                type_ref: match filled(&p.name) {
                    true => t.without_undefined(),
                    false => t,
                },
                optional: false,
                variadic: spread(&p.name),
                slice_mode: view.then_some(crate::ir::SliceModeHint::View),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let return_type = ty(cx, &m.result)?;
    // A named throws enum is typed, by the classes it declares; `err` is
    // untyped.
    // A future rejects with its own error.
    let rejects = match &m.result {
        abi::Type::Future(_, p) => Some(p),
        _ => None,
    };
    let throws = match m.throws.as_ref().or(rejects) {
        Some(p) if p.is_err() => Throws::None,
        Some(p) if throws_enum(cx, p) => match throws_classes(cx, p) {
            Some(t) => Throws::Type(t),
            None => Throws::None,
        },
        Some(_) => return None,
        None if return_type.as_promise_inner().is_some() => Throws::None,
        None => Throws::Never,
    };
    Some(Overload {
        params,
        args,
        tparams: m.tparams.iter().map(|p| p.name.clone()).collect(),
        defaults: m
            .tparams
            .iter()
            .map(|p| p.default.as_ref().and_then(|d| ty(cx, d)))
            .collect(),
        return_type,
        throws,
        doc: m.meta.doc.clone(),
        members: m.params.iter().map(|p| members(cx, &p.ty)).collect(),
        whole: m.params.iter().map(|p| whole(cx, &p.ty)).collect(),
    })
}

fn whole(cx: &Cx<'_>, t: &abi::Type) -> bool {
    let t = match t {
        abi::Type::Nullable(t, _) => t,
        t => t,
    };
    let abi::Type::Named(r) = t else {
        return false;
    };
    let Some((loc, TypeDecl::Enum(decl))) = cx.symbols.resolve_decl(&r.path, &cx.context()) else {
        return false;
    };
    let Ok(flat) = ts_gen_abi::enums::flatten(
        cx.symbols,
        ts_gen_abi::enums::EnumRef {
            loc: Some(loc.clone()),
            decl,
        },
    ) else {
        return false;
    };
    let payloads: Vec<&abi::Type> = flat
        .arms
        .iter()
        .filter_map(|a| match &a.arm.payload[..] {
            [] => None,
            [p] => Some(p),
            // Several payloads are no one primitive or class.
            _ => Some(t),
        })
        .collect();
    payloads.is_empty()
        || payloads.len() >= 2
            && payloads.iter().all(|p| match p {
                abi::Type::Scalar(_)
                | abi::Type::ScalarBox(_)
                | abi::Type::Str
                | abi::Type::StrBox => true,
                abi::Type::Named(r) => {
                    r.args.is_empty()
                        && matches!(
                            cx.symbols.resolve_decl(&r.path, &cx.context()),
                            Some((_, TypeDecl::Class(c))) if c.tparams.is_empty()
                        )
                }
                _ => false,
            })
}

/// The arms of the enum `t` names, as the type each stands for: its
/// literal, or its one payload's type.
fn members(cx: &Cx<'_>, t: &abi::Type) -> Option<Vec<TypeRef>> {
    // An optional enum's arms, each as itself or omitted.
    if let abi::Type::Nullable(t, _) = t {
        let arms = members(cx, t)?;
        let omitted = arms
            .iter()
            .map(|a| TypeRef::union([a.clone(), TypeRef::Undefined]));
        return Some(arms.iter().cloned().chain(omitted).collect());
    }
    let abi::Type::Named(r) = t else {
        return None;
    };
    let (loc, TypeDecl::Enum(decl)) = cx.symbols.resolve_decl(&r.path, &cx.context())? else {
        return None;
    };
    let flat = ts_gen_abi::enums::flatten(
        cx.symbols,
        ts_gen_abi::enums::EnumRef {
            loc: Some(loc.clone()),
            decl,
        },
    )
    .ok()?;
    flat.arms
        .iter()
        .map(|a| match &a.arm.payload[..] {
            [] => a.arm.meta.attrs.js.iter().find_map(|e| match e {
                abi::JsEntry::Literal(s) => Some(TypeRef::StringLiteral(s.clone())),
                _ => None,
            }),
            [p] => ty(cx, p),
            _ => None,
        })
        .collect()
}

/// Whether `p` is a throws enum (IR.md §15): one extending `err`, or one
/// with its own `@fallback` (`@throws {TypeError | RangeError}`).
fn throws_enum(cx: &Cx<'_>, p: &abi::Path) -> bool {
    matches!(
        cx.symbols.resolve_decl(p, &cx.context()),
        Some((_, TypeDecl::Enum(e))) if e.extends.iter().any(abi::Path::is_err)
            || e.arms.last().is_some_and(|a| a.meta.attrs.fallback)
                && e.arms.iter().all(|a| a.payload == [abi::Type::StrBox])
    )
}

/// What the getter `m` throws: a named throws enum's classes, else nothing,
/// as a getter catches only a typed `@throws`.
pub fn getter_throws(cx: &Cx<'_>, m: &abi::Member) -> Throws {
    match &m.throws {
        Some(p) if !p.is_err() && throws_enum(cx, p) => {
            throws_classes(cx, p).map_or(Throws::None, Throws::Type)
        }
        _ => Throws::None,
    }
}

/// The classes the throws enum `p` declares arms for, as the parse names
/// them, beside `err`'s or its own fallback.
fn throws_classes(cx: &Cx<'_>, p: &abi::Path) -> Option<TypeRef> {
    let Some((_, TypeDecl::Enum(e))) = cx.symbols.resolve_decl(p, &cx.context()) else {
        return None;
    };
    let classes: Vec<TypeRef> = e
        .arms
        .iter()
        .filter(|a| !a.meta.attrs.fallback)
        .filter_map(|a| {
            a.meta.attrs.js.iter().find_map(|j| match j {
                abi::JsEntry::Class(preds) => preds.first(),
                _ => None,
            })
        })
        .map(|pred| {
            named(cx, &pred.class)
                .filter(|t| declared(t).is_none())
                .unwrap_or_else(|| {
                    TypeRef::ident(pred.class.segments.last().cloned().unwrap_or_default())
                })
        })
        .collect();
    match classes.len() {
        0 => None,
        1 => classes.into_iter().next(),
        _ => Some(TypeRef::union(classes)),
    }
}

/// What a getter reads or a setter writes, unless it stands for one arm of
/// a property. What it throws is the parse's.
pub fn accessor(cx: &Cx<'_>, m: &abi::Member) -> Option<TypeRef> {
    let throws = |p: &abi::Path| p.is_err() || throws_enum(cx, p);
    if let Some(t) = constant_write(m) {
        return Some(t);
    }
    if !plain(m) || m.throws.as_ref().is_some_and(|p| !throws(p)) {
        return None;
    }
    match (&m.kind, &m.params[..]) {
        (abi::MemberKind::Get(_), []) => ty(cx, &m.result),
        (abi::MemberKind::Set(_), [p]) if !p.once => ty(cx, &p.ty),
        _ => None,
    }
}

/// The literal a setter of a constant arm writes, `@js(name = "kind",
/// args = ("text")) set kindText()`, as the parse's literal type.
fn constant_write(m: &abi::Member) -> Option<TypeRef> {
    if !matches!(m.kind, abi::MemberKind::Set(_)) || !m.params.is_empty() || m.throws.is_some() {
        return None;
    }
    let mut lit = None;
    for e in &m.meta.attrs.js {
        match e {
            abi::JsEntry::Name(_) => {}
            abi::JsEntry::Args(args) => match &args[..] {
                [abi::Telem::Lit(l)] => lit = Some(l),
                _ => return None,
            },
            _ => return None,
        }
    }
    Some(match lit? {
        abi::Literal::Str(s) => TypeRef::StringLiteral(s.clone()),
        abi::Literal::Int(n) => TypeRef::NumberLiteral(*n as f64),
        abi::Literal::Bool(b) => TypeRef::BooleanLiteral(*b),
        abi::Literal::Null => TypeRef::Null,
    })
}

/// `t` in the signature model. `@js(null)` reads `T?` as `T | null` rather
/// than `T | undefined`.
fn ty(cx: &Cx<'_>, t: &abi::Type) -> Option<TypeRef> {
    use abi::Type as T;
    Some(match t {
        T::Unit => TypeRef::Void,
        T::Never => TypeRef::Never,
        T::Str => TypeRef::String,
        T::Any => TypeRef::Any,
        T::Scalar(s) => scalar(*s)?,
        // `any` already holds `null` and `undefined`.
        T::Nullable(inner, _) if **inner == T::Any => TypeRef::Any,
        // A three-state read, `x?: T | null`.
        T::Nullable(inner, _) if matches!(**inner, T::Nullable(..)) => {
            let T::Nullable(t, _) = &**inner else {
                unreachable!()
            };
            TypeRef::union([ty(cx, t)?, TypeRef::Null, TypeRef::Undefined])
        }
        T::Nullable(inner, js_null) => {
            let absent = if *js_null {
                TypeRef::Null
            } else {
                TypeRef::Undefined
            };
            TypeRef::union([ty(cx, inner)?, absent])
        }
        // The classes a throws enum adds to `err` are the parse's `@throws`.
        T::Future(inner, err) if err.is_err() || throws_enum(cx, err) => TypeRef::Reference {
            segments: vec!["Promise".into()],
            generic_args: vec![ty(cx, inner)?],
        },
        // Boxes are how a generic argument holds a value type.
        T::StrBox => TypeRef::String,
        T::BigInt | T::BigIntBox => TypeRef::BigInt,
        T::ScalarBox(s) => scalar(*s)?,
        T::Vec(e)
        | T::View {
            mutable: false,
            elem: e,
        } => match **e {
            T::Scalar(s) => {
                use crate::ir::TypedArrayElement as E;
                let element = match s {
                    abi::Scalar::I8 => E::I8,
                    abi::Scalar::U8 => E::U8,
                    abi::Scalar::I16 => E::I16,
                    abi::Scalar::U16 => E::U16,
                    abi::Scalar::I32 => E::I32,
                    abi::Scalar::U32 => E::U32,
                    abi::Scalar::F32 => E::F32,
                    abi::Scalar::F64 => E::F64,
                    abi::Scalar::I64 => E::I64,
                    abi::Scalar::U64 => E::U64,
                    _ => return None,
                };
                TypeRef::TypedSlice(element, crate::ir::SharedMemory::Accepted)
            }
            _ => return None,
        },
        T::Array(e) | T::ArrBox(e) => TypeRef::Array(Box::new(ty(cx, e)?)),
        T::Tuple(ts) | T::TupleBox(ts) => {
            TypeRef::Tuple(ts.iter().map(|t| ty(cx, t)).collect::<Option<_>>()?)
        }
        T::Fn(f) if f.throws.is_none() => TypeRef::Function(FunctionSig {
            params: f
                .params
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    Some(Param {
                        name: format!("arg{i}"),
                        js_name: None,
                        type_ref: ty(cx, t)?,
                        optional: false,
                        variadic: f.rest && i + 1 == f.params.len(),
                        slice_mode: None,
                    })
                })
                .collect::<Option<_>>()?,
            return_type: Box::new(ty(cx, &f.result)?),
        }),
        T::Named(r) if r.args.is_empty() && r.path.segments == [ts_gen_abi::layout::SYMBOL] => {
            TypeRef::Symbol
        }
        T::Named(r) if index_only(cx, r).is_some() => index_only(cx, r)?,
        T::Named(r) => {
            let named = named(cx, &r.path)?;
            let segments = match named {
                TypeRef::Reference { segments, .. } if segments[0] != SYNTHESISED => segments,
                named => return Some(named),
            };
            // Under mono, an enum argument the producer synthesised keeps
            // its literals, for the use site to declare it by.
            let generic_args = r
                .args
                .iter()
                .map(|t| match ty(cx, t)? {
                    // A `unit` argument is TypeScript's `undefined`.
                    TypeRef::Void => Some(TypeRef::Undefined),
                    a if cx.mono && synthesised(&a).is_some_and(|(_, l)| l.is_empty()) => {
                        literals(cx, t).or(Some(a))
                    }
                    a => Some(a),
                })
                .collect::<Option<Vec<_>>>()?;
            TypeRef::Reference {
                segments,
                generic_args,
            }
        }
        _ => return None,
    })
}

/// An interface of index access alone (a `Record`, an index signature)
/// the parse doesn't declare, as the `Record<K, V>` the model reads as
/// `Object<V>`.
fn index_only(cx: &Cx<'_>, r: &abi::TypeRef) -> Option<TypeRef> {
    let Some((loc, TypeDecl::Class(c))) = cx.symbols.resolve_decl(&r.path, &cx.context()) else {
        return None;
    };
    // One the parse declares too reads by its name.
    if cx.ids.contains_key(&loc) {
        return None;
    }
    let index = |m: &abi::Member| m.meta.attrs.js.contains(&abi::JsEntry::Index);
    if c.members.is_empty() || !c.members.iter().all(index) {
        return None;
    }
    let at = c
        .members
        .iter()
        .find(|m| matches!(&m.kind, abi::MemberKind::Method(n) if n == "at"))?;
    let [key] = &at.params[..] else {
        return None;
    };
    let key = match key.ty {
        abi::Type::Str => TypeRef::String,
        abi::Type::Scalar(_) => TypeRef::Number,
        _ => return None,
    };
    let value = match &at.result {
        abi::Type::Nullable(v, _) => &**v,
        v => v,
    };
    // The class's own type parameters stand for its arguments.
    let value = match value {
        abi::Type::Named(v) if v.args.is_empty() && v.path.segments.len() == 1 => c
            .tparams
            .iter()
            .position(|p| p.name == v.path.segments[0])
            .map_or(Some(value), |i| r.args.get(i))?,
        v => v,
    };
    Some(TypeRef::Reference {
        segments: vec!["Record".into()],
        generic_args: vec![key, ty(cx, value)?],
    })
}

fn scalar(s: abi::Scalar) -> Option<TypeRef> {
    use abi::Scalar as S;
    Some(match s {
        S::Bool => TypeRef::Boolean,
        S::F64 => TypeRef::Number,
        S::I8 => TypeRef::AbiScalar(AbiScalar::I8),
        S::U8 => TypeRef::AbiScalar(AbiScalar::U8),
        S::I16 => TypeRef::AbiScalar(AbiScalar::I16),
        S::U16 => TypeRef::AbiScalar(AbiScalar::U16),
        S::I32 => TypeRef::AbiScalar(AbiScalar::I32),
        S::U32 => TypeRef::AbiScalar(AbiScalar::U32),
        S::F32 => TypeRef::AbiScalar(AbiScalar::F32),
        S::I53 => TypeRef::AbiScalar(AbiScalar::I53),
        S::U53 => TypeRef::AbiScalar(AbiScalar::U53),
        S::F16 => TypeRef::AbiScalar(AbiScalar::F16),
        // `bigint` and `@tw-abi i64` both lower to `i64`, which the parse
        // tells apart.
        S::I64 => TypeRef::BigInt,
        S::U64 => TypeRef::AbiScalar(AbiScalar::U64),
    })
}

/// An enum of string literals alone, as the parse's union of them, which
/// takes the enum's name (see [`synthesised`]).
fn literals(cx: &Cx<'_>, t: &abi::Type) -> Option<TypeRef> {
    let abi::Type::Named(r) = t else {
        return None;
    };
    let (loc, TypeDecl::Enum(decl)) = cx.symbols.resolve_decl(&r.path, &cx.context())? else {
        return None;
    };
    let flat = ts_gen_abi::enums::flatten(
        cx.symbols,
        ts_gen_abi::enums::EnumRef {
            loc: Some(loc.clone()),
            decl,
        },
    )
    .ok()?;
    if flat.shape != ts_gen_abi::enums::Shape::Unit {
        return None;
    }
    let members = flat
        .arms
        .iter()
        .map(|a| {
            a.arm.meta.attrs.js.iter().find_map(|e| match e {
                abi::JsEntry::Literal(s) => Some(TypeRef::StringLiteral(s.clone())),
                _ => None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(TypeRef::Reference {
        segments: vec![SYNTHESISED.into(), loc.path.last()?.clone()],
        generic_args: members,
    })
}

/// The first segment of the path [`ty`] reads an enum the producer
/// synthesised as, which no TS name can be: the parse reads its union.
pub const SYNTHESISED: &str = "#enum";

/// The segment that names a declaration by its id, `#decl.<id>`, where
/// the IR's path names it otherwise than the parse does in scope: in
/// another module, or shadowed. The parse's name for it restates it.
pub const DECLARED: &str = "#decl";

fn declared_ref(id: crate::context::TypeId) -> TypeRef {
    TypeRef::Reference {
        segments: vec![DECLARED.into(), id.index().to_string()],
        generic_args: Vec::new(),
    }
}

/// The id of the declaration `t` names by its id.
pub fn declared(t: &TypeRef) -> Option<&str> {
    match t {
        TypeRef::Reference { segments, .. } if segments[0] == DECLARED => Some(&segments[1]),
        _ => None,
    }
}

/// The name of the enum the producer synthesised for `t`, if it is one,
/// and the literals it stands for, when the parse's own reading isn't.
pub fn synthesised(t: &TypeRef) -> Option<(&str, &[TypeRef])> {
    match t {
        TypeRef::Reference {
            segments,
            generic_args,
        } if segments[0] == SYNTHESISED => Some((&segments[1], generic_args)),
        _ => None,
    }
}

/// A path's TS names, where the producer escaped a built-in IR name
/// (`Unit_`).
pub(crate) fn ts_segments(path: &abi::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|s| match s.strip_suffix('_') {
            Some(js) if abi::is_builtin_name(js) => js.to_string(),
            _ => s.clone(),
        })
        .collect()
}

/// A declared class, interface or enum, as the name the parse scope
/// resolves to the same declaration.
fn named(cx: &Cx<'_>, path: &abi::Path) -> Option<TypeRef> {
    let segments = ts_segments(path);
    let parsed = cx.gctx.resolve_path(cx.scope, &segments.join("."));
    // A path that names another declaration from the container root may
    // be the producer printing a shadowed outer name.
    let root = Context {
        module: cx.module,
        ..Context::default()
    };
    let at_root = match cx.symbols.resolve(path, &root) {
        Some(Target::Decl(loc, _)) => Some(loc.clone()),
        _ => None,
    };
    match cx.symbols.resolve(path, &cx.context())? {
        Target::Assoc => {
            let a = cx.context().assoc(path)?;
            if cx.mono {
                return Some(TypeRef::Reference {
                    segments: vec![a.param.clone(), a.name.clone()],
                    generic_args: Vec::new(),
                });
            }
            let (_, row) = a.rows.first()?;
            return match ty(cx, row)? {
                t if synthesised(&t).is_some() => literals(cx, row).or(Some(t)),
                t => Some(t),
            };
        }
        Target::TypeParam if segments.len() == 1 && cx.erased.contains(&segments[0].as_str()) => {
            return Some(TypeRef::Any);
        }
        Target::TypeParam if segments.len() == 1 && cx.own.contains(&segments[0].as_str()) => {}
        // A type parameter the parse binds by the same name.
        Target::TypeParam => {
            let binds = |scope| {
                matches!(
                    cx.gctx.scopes.resolve_binding(scope, &segments[0]),
                    Some(crate::parse::scope::Binding::TypeParam)
                )
            };
            if !binds(cx.scope) && !cx.bound.iter().copied().any(binds) {
                return None;
            }
        }
        Target::Decl(loc, _) if at_root.as_ref().is_some_and(|r| r != loc) => {
            return cx.ids.get(loc).map(|&id| declared_ref(id));
        }
        // An alias, back-edges included, as the parse's alias of that name.
        Target::Decl(_, TypeDecl::Alias(_)) => {
            let id = parsed?;
            if !matches!(cx.gctx.get_type(id).kind, TypeKind::TypeAlias(_)) {
                return None;
            }
        }
        Target::Decl(loc, decl) => {
            // The model's own name for a lib type.
            if !cx.ids.contains_key(loc) && segments == ["ArrayBufferView"] {
                return Some(TypeRef::ArrayBufferView);
            }
            // std's `Object`, which TS `object` lowers to.
            if !cx.ids.contains_key(loc) && segments == ["Object"] {
                return Some(TypeRef::Object);
            }
            if !cx.ids.contains_key(loc) && matches!(decl, TypeDecl::Enum(_)) {
                // The enum of a named union, which the parse reads as its
                // alias.
                if let Some(id) = parsed {
                    let alias = matches!(cx.gctx.get_type(id).kind, TypeKind::TypeAlias(_));
                    return alias.then_some(TypeRef::Reference {
                        segments,
                        generic_args: Vec::new(),
                    });
                }
                return Some(TypeRef::Reference {
                    segments: vec![SYNTHESISED.into(), loc.path.last()?.clone()],
                    generic_args: Vec::new(),
                });
            }
            // A lib class, which the parse resolves to a lib declaration of
            // its own, if any.
            if !cx.ids.contains_key(loc) {
                let lib = parsed.is_none_or(|id| !cx.locations.contains_key(&id));
                if !matches!(decl, TypeDecl::Class(_)) || !lib {
                    return None;
                }
            } else if parsed != cx.ids.get(loc).copied() {
                return cx.ids.get(loc).map(|&id| declared_ref(id));
            }
        }
        _ => return None,
    }
    Some(TypeRef::Reference {
        segments,
        generic_args: Vec::new(),
    })
}
