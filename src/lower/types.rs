//! Type lowering: TypeScript types to IR types.
//!
//! Unions are read off as [`Leaf`]s, through aliases, so a parameter can
//! expand them into overloads ([`super::callables`]) and any other position
//! can join them into one type: `T?`, `T??`, or an enum.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ts_gen_abi::ir as abi;
use ts_gen_abi::text::print_type;

use super::{names, LitParam, Lowerer, Place};
use crate::context::TypeId;
use crate::ir::{FunctionSig, TypeKind, TypeParam, TypeRef};
use crate::parse::scope::{Binding, ScopeId};

/// Where a type is read, and what it reads against.
#[derive(Clone)]
pub(crate) struct Tcx {
    pub scope: ScopeId,
    /// Where the use is: paths print relative to it, and enums synthesized
    /// for it are emitted here.
    pub place: Place,
    /// The name a synthesized enum takes from this use (`<Anchor>Kind`).
    pub anchor: String,
    /// The enclosing type, which TS `this` names.
    pub this: Option<abi::Type>,
    /// An alias's parameters, bound to their lowered arguments.
    pub subst: Rc<HashMap<String, abi::Type>>,
    /// The aliases being expanded, which a recursive alias reaches again.
    pub aliases: Rc<Vec<TypeId>>,
    /// The JS name of the enclosing class or interface, for config that
    /// names members (`Owner.member.param`).
    pub owner: Option<String>,
    /// Inside an export, whose values come from JS.
    pub export: bool,
    /// In a union of `bigint` with other values (a comparator's
    /// `number | bigint`), where `bigint` is no typed array element.
    pub mixed: bool,
}

impl Tcx {
    /// Where the use is, for diagnostics: `Owner.Anchor`.
    pub fn location(&self) -> String {
        match &self.owner {
            Some(o) if names::pascal(o) != self.anchor => format!("`{o}.{}`", self.anchor),
            _ => format!("`{}`", self.anchor),
        }
    }

    pub fn new(scope: ScopeId, place: &Place, anchor: &str) -> Tcx {
        Tcx {
            scope,
            place: place.clone(),
            anchor: names::pascal(anchor),
            this: None,
            subst: Rc::default(),
            aliases: Rc::default(),
            owner: None,
            export: false,
            mixed: false,
        }
    }

    pub fn owned(scope: ScopeId, place: &Place, owner: &str) -> Tcx {
        Tcx {
            owner: Some(owner.to_string()),
            ..Tcx::new(scope, place, owner)
        }
    }

    pub fn at(&self, scope: ScopeId) -> Tcx {
        Tcx {
            scope,
            ..self.clone()
        }
    }

    pub fn anchored(&self, anchor: &str) -> Tcx {
        Tcx {
            anchor: names::pascal(anchor),
            ..self.clone()
        }
    }
}

/// One member of a union, as written or reached through aliases.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Leaf {
    Ty(abi::Type),
    /// `any` / `unknown`: every value, null and undefined included.
    Top,
    Null,
    Undefined,
    Str(String),
    Num(f64),
    Bool(bool),
}

/// A union's leaves, sorted by what positions do with them.
#[derive(Clone, Default, Debug)]
pub(crate) struct Leaves {
    pub top: bool,
    pub null: bool,
    pub undefined: bool,
    /// Value types, in order and deduplicated; the literals are folded in
    /// (a lone string literal is a `str` here only once it's the whole
    /// union, see [`Lowerer::join`]).
    pub values: Vec<abi::Type>,
    pub strs: Vec<String>,
    pub nums: Vec<f64>,
    pub bools: Vec<bool>,
    /// String literals beside `string`: an open set, whose enum keeps any
    /// other string in a `@fallback Other(Str)` arm. `str` isn't in
    /// `values` then.
    pub open: bool,
    /// The value types and string literals in source order, which a name
    /// spelled from them follows.
    pub order: Vec<Leaf>,
}

impl Leaves {
    pub fn of(leaves: Vec<Leaf>) -> Leaves {
        let mut l = Leaves::default();
        for leaf in leaves {
            if matches!(leaf, Leaf::Ty(_) | Leaf::Str(_)) && !l.order.contains(&leaf) {
                l.order.push(leaf.clone());
            }
            match leaf {
                Leaf::Ty(t) => {
                    if !l.values.contains(&t) {
                        l.values.push(t);
                    }
                }
                Leaf::Top => l.top = true,
                Leaf::Null => l.null = true,
                Leaf::Undefined => l.undefined = true,
                Leaf::Str(s) => {
                    if !l.strs.contains(&s) {
                        l.strs.push(s);
                    }
                }
                Leaf::Num(n) => {
                    if !l.nums.contains(&n) {
                        l.nums.push(n);
                    }
                }
                Leaf::Bool(b) => {
                    if !l.bools.contains(&b) {
                        l.bools.push(b);
                    }
                }
            }
        }
        if !l.strs.is_empty() && l.values.contains(&abi::Type::Str) {
            l.values.retain(|v| *v != abi::Type::Str);
            l.open = true;
        }
        // A literal adds nothing beside its primitive.
        if l.values.contains(&abi::Type::Scalar(abi::Scalar::F64)) {
            l.nums.clear();
        }
        if l.values.contains(&abi::Type::Scalar(abi::Scalar::Bool)) || l.bools.len() == 2 {
            l.bools.clear();
            if !l.values.contains(&abi::Type::Scalar(abi::Scalar::Bool)) {
                l.values.push(abi::Type::Scalar(abi::Scalar::Bool));
            }
        }
        l
    }
}

/// A union alias read whole, which names the enum it becomes.
#[derive(Clone)]
pub(crate) struct Named {
    pub name: String,
    pub place: Place,
    /// The alias, when one names it.
    pub id: Option<TypeId>,
}

impl Named {
    pub(crate) fn key(&self) -> String {
        format!("alias:{:?}:{}", self.place, self.name)
    }
}

/// How a joined union reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Read {
    /// A member's own result: `null` and `undefined` stay distinct (`T??`)
    /// under `--strict-nullable`, and a lone `null` asks for `@js(null)`.
    Result,
    /// Anywhere else: one absence.
    Inner,
}

const INTERFACE_BUILTINS: &[&str] = &[
    "ArrayBufferView",
    "ArrayLike",
    "AsyncGenerator",
    "AsyncIterable",
    "AsyncIterableIterator",
    "AsyncIterator",
    "Generator",
    "Iterable",
    "IterableIterator",
    "Iterator",
    "IteratorNext",
];

impl Lowerer<'_> {
    /// `t` in a position that holds one type.
    pub(crate) fn ty(&mut self, t: &TypeRef, cx: &Tcx) -> abi::Type {
        use abi::Type as T;
        match t {
            TypeRef::Boolean => T::Scalar(abi::Scalar::Bool),
            TypeRef::Number => T::Scalar(abi::Scalar::F64),
            TypeRef::String => T::Str,
            TypeRef::BooleanLiteral(_) => self.widened(t, T::Scalar(abi::Scalar::Bool), cx),
            TypeRef::NumberLiteral(_) => self.widened(t, T::Scalar(abi::Scalar::F64), cx),
            TypeRef::StringLiteral(_) | TypeRef::Template { .. } => self.widened(t, T::Str, cx),
            TypeRef::AbiScalar(s) => T::Scalar(abi_scalar(*s)),
            // A 64-bit typed array's elements are its element type, not
            // the unbounded `bigint` the lib spells them as.
            TypeRef::BigInt => match cx
                .owner
                .as_deref()
                .map(|o| o.trim_end_matches("Constructor"))
            {
                Some(o @ ("BigInt64Array" | "BigUint64Array")) if !cx.mixed => {
                    T::Scalar(typed_array_scalar(o).unwrap())
                }
                _ => T::BigInt,
            },
            TypeRef::Void | TypeRef::Undefined | TypeRef::Never => T::Unit,
            TypeRef::Null => {
                self.warn(format!("{}: `null` alone reads as `Any?`", cx.location()));
                T::Nullable(Box::new(T::Any), false)
            }
            TypeRef::Any | TypeRef::Unknown => T::Nullable(Box::new(T::Any), false),
            // An interface, so the bridge tests for a non-null object.
            TypeRef::Object => self.stub("Object", Vec::new(), abi::ClassKind::Interface, cx),
            TypeRef::Symbol => self.stub("Symbol", Vec::new(), abi::ClassKind::Class, cx),
            TypeRef::ArrayBufferView => {
                self.stub("ArrayBufferView", Vec::new(), abi::ClassKind::Interface, cx)
            }
            TypeRef::TypedSlice(e, _) => T::Vec(Box::new(T::Scalar(
                abi::Scalar::from_name(e.rust()).unwrap_or(abi::Scalar::U8),
            ))),
            TypeRef::Array(e) => T::Array(Box::new(self.inner(e, cx))),
            TypeRef::Tuple(items) if items.is_empty() => {
                self.warn(format!(
                    "{}: an empty tuple type reads as `Any`",
                    cx.location()
                ));
                T::Any
            }
            TypeRef::Tuple(items) => T::Tuple(items.iter().map(|i| self.inner(i, cx)).collect()),
            TypeRef::Union(_) | TypeRef::PropertyKey => self.join_type(t, cx, Read::Inner).0,
            TypeRef::Intersection(items) => self.intersection(items, cx),
            TypeRef::Function(sig) => T::Fn(Box::new(self.fn_type(sig, cx))),
            TypeRef::Reference { .. } if self.assoc_type(t, cx).is_some() => {
                self.assoc_type(t, cx).unwrap()
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => self.reference(segments, generic_args, cx),
            TypeRef::Unresolved(s) if s == "this" => cx.this.clone().unwrap_or(T::Any),
            other => {
                self.warn(format!(
                    "{}: `{}` has no IR type and reads as `Any`",
                    cx.location(),
                    other.format_ts()
                ));
                T::Any
            }
        }
    }

    /// A literal type read as the primitive it's a value of.
    fn widened(&mut self, t: &TypeRef, to: abi::Type, cx: &Tcx) -> abi::Type {
        self.warn(format!(
            "{}: the literal type `{}` widens to `{}`",
            cx.location(),
            t.format_ts(),
            print_type(&to)
        ));
        to
    }

    /// `t` inside another type: never `unit`.
    pub(crate) fn inner(&mut self, t: &TypeRef, cx: &Tcx) -> abi::Type {
        match self.ty(t, cx) {
            abi::Type::Unit => {
                if matches!(t, TypeRef::Undefined | TypeRef::Void | TypeRef::Never) {
                    self.warn(format!(
                        "{}: `{}` inside another type reads as `Any?`",
                        cx.location(),
                        t.format_ts()
                    ));
                }
                abi::Type::Nullable(Box::new(abi::Type::Any), false)
            }
            t => t,
        }
    }

    /// A member result: `Promise<T>` is `future<T>`, and a union joins with
    /// [`Read::Result`]. The flag asks for `@js(null)`.
    pub(crate) fn result(&mut self, t: &TypeRef, cx: &Tcx, optional: bool) -> (abi::Type, bool) {
        let t = if optional {
            TypeRef::union([t.clone(), TypeRef::Undefined])
        } else {
            t.clone()
        };
        let (ty, js_null) = self.join_type(&t, cx, Read::Result);
        // A future resolving to `null` reads it at the member, as a sync
        // result does.
        let js_null = js_null
            || matches!(ty, abi::Type::Future(..))
                && t.as_promise_inner().is_some_and(|inner| {
                    let mut leaves = Vec::new();
                    self.leaves(inner, cx, &mut leaves);
                    leaves.iter().any(|(l, _)| matches!(l, Leaf::Null))
                });
        (ty, js_null)
    }

    /// A [`result`](Self::result) nested in another type, which carries its
    /// own `@js(null)`.
    fn nulled(&mut self, t: &TypeRef, cx: &Tcx) -> abi::Type {
        match self.result(t, cx, false) {
            (t, true) => super::callables::with_null(t),
            (t, false) => t,
        }
    }

    /// The leaves of `t`, through unions and aliases.
    pub(crate) fn leaves(&mut self, t: &TypeRef, cx: &Tcx, out: &mut Vec<(Leaf, Tcx)>) {
        match t {
            TypeRef::Union(u) => {
                for m in u.members() {
                    match self.nested_alias(m, cx) {
                        Some(ls) => out.extend(ls.into_iter().map(|l| (l, cx.clone()))),
                        None => self.leaves(m, cx, out),
                    }
                }
            }
            TypeRef::PropertyKey => {
                for m in [TypeRef::String, TypeRef::Number, TypeRef::Symbol] {
                    self.leaves(&m, cx, out);
                }
            }
            TypeRef::Null => out.push((Leaf::Null, cx.clone())),
            TypeRef::Undefined | TypeRef::Void => out.push((Leaf::Undefined, cx.clone())),
            TypeRef::Never => {}
            TypeRef::Any | TypeRef::Unknown => out.push((Leaf::Top, cx.clone())),
            TypeRef::StringLiteral(s) => out.push((Leaf::Str(s.clone()), cx.clone())),
            TypeRef::NumberLiteral(n) => out.push((Leaf::Num(*n), cx.clone())),
            TypeRef::BooleanLiteral(b) => out.push((Leaf::Bool(*b), cx.clone())),
            TypeRef::Reference {
                segments,
                generic_args,
            } if self.utility(segments).is_some() => {
                self.utility_leaves(segments, generic_args, cx, out)
            }
            TypeRef::Reference { segments, .. }
                if self
                    .resolve_id(segments, cx)
                    .is_some_and(|id| self.keeps_alias(id)) =>
            {
                let t = self.ty(t, cx);
                out.push((Leaf::Ty(t), cx.clone()));
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => match self.alias(segments, generic_args, cx) {
                Some((target, acx)) => self.leaves(&target, &acx, out),
                None => {
                    let t = self.ty(t, cx);
                    out.push((Leaf::Ty(t), cx.clone()));
                }
            },
            _ => {
                let t = self.ty(t, cx);
                out.push((Leaf::Ty(t), cx.clone()));
            }
        }
    }

    /// The enum of a union alias `m` names as an arm of a larger union, which
    /// nests whole (IR.md §8), and the alias's `null` and `undefined`, which
    /// join the parent's. A literal alias's literals join the parent's own
    /// instead.
    fn nested_alias(&mut self, m: &TypeRef, cx: &Tcx) -> Option<Vec<Leaf>> {
        let TypeRef::Reference {
            segments,
            generic_args,
        } = m
        else {
            return None;
        };
        if self.utility(segments).is_some() {
            return None;
        }
        self.union_alias_name(segments, generic_args, cx)?;
        let mut raw = Vec::new();
        self.leaves(m, cx, &mut raw);
        if !raw
            .iter()
            .any(|(l, _)| matches!(l, Leaf::Ty(_) | Leaf::Top))
        {
            return None;
        }
        let mut t = self.join_type(m, cx, Read::Inner).0;
        while let abi::Type::Nullable(inner, _) = t {
            t = *inner;
        }
        let empties = raw
            .into_iter()
            .map(|(l, _)| l)
            .filter(|l| matches!(l, Leaf::Null | Leaf::Undefined));
        Some(std::iter::once(Leaf::Ty(t)).chain(empties).collect())
    }

    /// The lib union utility `segments` names, unless the input declares
    /// its own.
    fn utility(&self, segments: &[String]) -> Option<&'static str> {
        let name = match segments {
            [n] if n == "Exclude" => "Exclude",
            [n] if n == "Extract" => "Extract",
            [n] if n == "NonNullable" => "NonNullable",
            _ => return None,
        };
        let declared = self
            .gctx
            .scopes
            .resolve(self.module.global_scope, name)
            .is_some_and(|id| !self.gctx.builtin_type_ids.contains(&id));
        (!declared).then_some(name)
    }

    /// `Exclude`, `Extract` and `NonNullable` over leaves, with string
    /// enums read as their literals.
    fn utility_leaves(
        &mut self,
        segments: &[String],
        args: &[TypeRef],
        cx: &Tcx,
        out: &mut Vec<(Leaf, Tcx)>,
    ) {
        let flat = |this: &mut Self, t: Option<&TypeRef>| -> Vec<Leaf> {
            let mut raw = Vec::new();
            if let Some(t) = t {
                this.leaves(t, cx, &mut raw);
            }
            raw.into_iter()
                .flat_map(|(l, _)| match &l {
                    Leaf::Ty(t) => match this.literal_set(t, &cx.place) {
                        Some(set) => set.into_iter().map(Leaf::Str).collect(),
                        None => vec![l],
                    },
                    _ => vec![l],
                })
                .collect()
        };
        let base = flat(self, args.first());
        let kept: Vec<Leaf> = match self.utility(segments) {
            Some("NonNullable") => base
                .into_iter()
                .filter(|l| !matches!(l, Leaf::Null | Leaf::Undefined))
                .collect(),
            Some(u) => {
                let other = flat(self, args.get(1));
                base.into_iter()
                    .filter(|l| other.contains(l) == (u == "Extract"))
                    .collect()
            }
            None => base,
        };
        out.extend(kept.into_iter().map(|l| (l, cx.clone())));
    }

    /// The name of an evaluated type: `Exclude<TimeUnit, "hour">` is
    /// `ExcludeTimeUnitHour`.
    fn evaluated_name(t: &TypeRef) -> String {
        match t {
            TypeRef::Reference {
                segments,
                generic_args,
            } => {
                let mut out = segments.last().cloned().unwrap_or_default();
                for a in generic_args {
                    out.push_str(&Self::evaluated_name(a));
                }
                out
            }
            TypeRef::StringLiteral(s) => names::pascal(s),
            TypeRef::Array(e) => format!("{}Array", Self::evaluated_name(e)),
            TypeRef::Union(u) => u
                .members()
                .iter()
                .map(Self::evaluated_name)
                .collect::<Vec<_>>()
                .join("Or"),
            other => names::pascal(&other.format_ts()),
        }
    }

    /// `t` as one type. A union alias read whole is its own enum, named
    /// after it.
    pub(crate) fn join_type(&mut self, t: &TypeRef, cx: &Tcx, read: Read) -> (abi::Type, bool) {
        if let Some(value) = self.assoc_type(t, cx) {
            let (null, undefined) = super::assoc::absent(t);
            let l = Leaves {
                null,
                undefined,
                ..Leaves::default()
            };
            return self.absent(value, &l, read);
        }
        let named = match t {
            TypeRef::Reference {
                segments,
                generic_args,
            } if self.utility(segments).is_some() => Some(Named {
                name: Self::evaluated_name(t),
                place: cx.place.clone(),
                id: None,
            }),
            TypeRef::Reference {
                segments,
                generic_args,
            } => self.union_alias_name(segments, generic_args, cx),
            TypeRef::Union(u) => self.nullable_alias_name(u.members(), cx),
            _ => None,
        };
        // A union alias's enum reads at the alias, then is named from here.
        let mut acx = match &named {
            Some(n) => Tcx {
                place: n.place.clone(),
                ..cx.clone()
            },
            None => cx.clone(),
        };
        if let TypeRef::Union(u) = t {
            let values = || {
                u.members()
                    .iter()
                    .filter(|m| !matches!(m, TypeRef::Null | TypeRef::Undefined))
            };
            acx.mixed |=
                values().any(|m| *m == TypeRef::BigInt) && values().any(|m| *m != TypeRef::BigInt);
        }
        let mut raw = Vec::new();
        self.leaves(t, &acx, &mut raw);
        let leaves = Leaves::of(raw.into_iter().map(|(l, _)| l).collect());
        let key = named.as_ref().map(Named::key);
        let (mut ty, js_null) = self.join(leaves, &acx, read, named);
        if let Some(key) = key.filter(|_| acx.place != cx.place) {
            if let (Some(there), Some(here)) = (
                self.synth_ref(&key, &acx.place),
                self.synth_ref(&key, &cx.place),
            ) {
                repath(&mut ty, &there, &here);
            }
        }
        (ty, js_null)
    }

    /// The union alias a union of it with `null` or `undefined` names.
    pub(crate) fn nullable_alias_name(&mut self, members: &[TypeRef], cx: &Tcx) -> Option<Named> {
        let rest: Vec<&TypeRef> = members
            .iter()
            .filter(|m| !matches!(m, TypeRef::Null | TypeRef::Undefined))
            .collect();
        if let [t @ TypeRef::Reference { segments, .. }] = rest.as_slice() {
            if self.utility(segments).is_some() {
                return Some(Named {
                    name: Self::evaluated_name(t),
                    place: cx.place.clone(),
                    id: None,
                });
            }
        }
        let mut named = rest.iter().filter_map(|m| match m {
            TypeRef::Reference {
                segments,
                generic_args,
            } => Some((m, segments, generic_args)),
            _ => None,
        });
        let (alias, segments, args) = named.next()?;
        let n = self.union_alias_name(segments, args, cx)?;
        if rest.len() == 1 {
            return Some(n);
        }
        // `WorkflowTimeoutDuration | number` is the alias when the alias
        // already holds the rest.
        let same = |a: &Leaves, b: &Leaves| {
            a.values == b.values
                && a.strs == b.strs
                && a.nums == b.nums
                && a.bools == b.bools
                && a.open == b.open
        };
        let of = |this: &mut Self, ts: &[&TypeRef]| {
            let mut raw = Vec::new();
            for t in ts {
                this.leaves(t, cx, &mut raw);
            }
            Leaves::of(raw.into_iter().map(|(l, _)| l).collect())
        };
        let whole = of(self, &rest);
        let own = of(self, &[*alias]);
        same(&whole, &own).then_some(n)
    }

    /// Join a union's leaves into one type.
    pub(crate) fn join(
        &mut self,
        mut l: Leaves,
        cx: &Tcx,
        read: Read,
        named: Option<Named>,
    ) -> (abi::Type, bool) {
        use abi::Type as T;
        if l.top {
            return (T::Nullable(Box::new(T::Any), false), false);
        }
        if !l.nums.is_empty() {
            let only = l.values.is_empty() && l.strs.is_empty() && l.bools.is_empty();
            match only.then(|| int_literals(&l.nums)).flatten() {
                Some(ints) if ints.len() > 1 => {
                    let value = self.int_enum(&ints, cx, named);
                    return self.absent(value, &l, read);
                }
                _ => {
                    let nums: Vec<String> = l.nums.iter().map(f64::to_string).collect();
                    self.warn(format!(
                        "{}: the literal type `{}` widens to `f64`",
                        cx.location(),
                        nums.join(" | ")
                    ));
                    l.values.push(T::Scalar(abi::Scalar::F64));
                }
            }
            l.nums.clear();
        }
        if l.bools.len() == 1 {
            self.warn(format!(
                "{}: the literal type `{}` widens to `bool`",
                cx.location(),
                l.bools[0]
            ));
        }
        for _ in l.bools.drain(..) {
            if !l.values.contains(&T::Scalar(abi::Scalar::Bool)) {
                l.values.push(T::Scalar(abi::Scalar::Bool));
            }
        }
        if l.open {
            let value = self.payload_enum(&l.strs, &l.values, true, cx, named);
            return self.absent(value, &l, read);
        }
        if let Some(value) = named.as_ref().and_then(|n| self.named_literal(n, &l, cx)) {
            return self.absent(value, &l, read);
        }
        if let Some(value) = self.merged_literal(&l, cx) {
            return self.absent(value, &l, read);
        }
        let value = match (l.values.len(), l.strs.len()) {
            // Nothing but `undefined` is no result.
            (0, 0) if read == Read::Result && !l.null => return (T::Unit, false),
            (0, 0) => T::Any,
            (1, 0) => l.values.pop().unwrap(),
            (0, 1) => {
                self.warn(format!(
                    "{}: the literal type `{:?}` widens to `str`",
                    cx.location(),
                    l.strs[0]
                ));
                T::Str
            }
            (0, _) => self.literal_enum(&l.strs, cx, named),
            _ => self.payload_enum(&l.strs, &l.values, false, cx, named),
        };
        self.absent(value, &l, read)
    }

    /// `value`, made optional by the union's `null` and `undefined`.
    fn absent(&mut self, value: abi::Type, l: &Leaves, read: Read) -> (abi::Type, bool) {
        use abi::Type as T;
        if !(l.null || l.undefined) {
            return (value, false);
        }
        let value = match value {
            T::Nullable(inner, _) => *inner,
            v => v,
        };
        let null = l.null && !l.undefined;
        match read {
            _ if l.null && l.undefined && value != T::Any && self.opts.strict_nullable => (
                T::Nullable(Box::new(T::Nullable(Box::new(value), false)), false),
                false,
            ),
            Read::Result => (T::Nullable(Box::new(value), false), null),
            Read::Inner => (T::Nullable(Box::new(value), null), false),
        }
    }

    /// A unit enum over string literals, shared by every use of the same
    /// values (#194). A union alias over another literal union alias
    /// extends its enum.
    pub(crate) fn literal_enum(
        &mut self,
        strs: &[String],
        cx: &Tcx,
        named: Option<Named>,
    ) -> abi::Type {
        let mut sorted = strs.to_vec();
        sorted.sort();
        let key = match &named {
            Some(n) => n.key(),
            None => format!("lit:{sorted:?}"),
        };
        if let Some(t) = self.synth_ref(&key, &cx.place) {
            self.anchor(&key, cx);
            return t;
        }
        let mut bases = Vec::new();
        if let Some(n) = &named {
            for (path, _, base_strs) in self.literal_bases(n, cx) {
                bases.push((path, base_strs));
            }
        }
        self.literal_enum_ext(strs, bases, key, cx, named)
    }

    /// A unit enum over `strs`, extending `bases` (paths from where it's
    /// emitted) for the literals they hold. An unnamed or evaluated set
    /// reuses an enum with the same literals.
    fn literal_enum_ext(
        &mut self,
        strs: &[String],
        bases: Vec<(abi::Path, Vec<String>)>,
        key: String,
        cx: &Tcx,
        named: Option<Named>,
    ) -> abi::Type {
        if let Some(t) = self.synth_ref(&key, &cx.place) {
            return t;
        }
        if named.as_ref().is_none_or(|n| n.id.is_none()) {
            if let Some(t) = self.lib_literal(strs, &cx.place) {
                return t;
            }
            if let Some(t) = self.same_literals(strs, &cx.place) {
                return t;
            }
        }
        let mut extends = Vec::new();
        let mut inherited: Vec<String> = Vec::new();
        // Arm names are unique across a hierarchy, so a base sharing a
        // literal with an earlier one isn't extended. The bases' indices are
        // numbered apart once every enum is lowered.
        for (path, base_strs) in bases {
            if base_strs.iter().any(|s| inherited.contains(s)) || extends.contains(&path) {
                continue;
            }
            extends.push(path);
            inherited.extend(base_strs);
        }
        let mut arms: Vec<abi::Arm> = Vec::new();
        for s in strs.iter().filter(|s| !inherited.contains(s)) {
            push_literal(&mut arms, s);
        }
        // One more than one anchor shares is spelled by its arms, as a
        // payload enum is.
        if named.is_none() {
            let spelled = arms
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join("Or");
            self.made.insert(key.clone(), self.anchors.len());
            self.anchors.push(super::Shared {
                anchors: HashSet::new(),
                spelled: if spelled.len() <= 32 {
                    spelled
                } else {
                    String::new()
                },
                key: key.clone(),
            });
            self.anchor(&key, cx);
        }
        let t = self.synth_enum(key.clone(), arms, extends, None, cx, named);
        if let Some((place, name)) = self.synth.get(&key).cloned() {
            let mut full = place.ns.clone();
            full.push(name);
            self.literals.insert((place.module, full), strs.to_vec());
        }
        t
    }

    /// A `with` lib's enum holding exactly `strs`, from `from`.
    fn lib_literal(&mut self, strs: &[String], from: &Place) -> Option<abi::Type> {
        let mut want = strs.to_vec();
        want.sort();
        want.dedup();
        let (lib, path, _) = self
            .lib_literals
            .iter()
            .find(|(_, _, have)| *have == want)?
            .clone();
        self.with_used.insert(lib);
        Some(abi::Type::Named(abi::TypeRef {
            path: self.global_path(path.split('.').map(String::from).collect(), from),
            args: Vec::new(),
        }))
    }

    /// An enum already holding exactly `strs`, from `from`.
    fn same_literals(&mut self, strs: &[String], from: &Place) -> Option<abi::Type> {
        let mut want = strs.to_vec();
        want.sort();
        want.dedup();
        let mut found: Vec<&(Option<String>, Vec<String>)> = self
            .literals
            .iter()
            .filter(|(_, have)| {
                let mut have = (*have).clone();
                have.sort();
                have.dedup();
                have == want
            })
            .map(|(k, _)| k)
            .collect();
        found.sort();
        let (module, full) = found.first().copied()?.clone();
        self.pull((module.clone(), full.clone()));
        let (name, ns) = full.split_last()?;
        let place = Place {
            module: module.clone(),
            ns: ns.to_vec(),
        };
        Some(abi::Type::Named(abi::TypeRef {
            path: self.path_to(&place, name, from),
            args: Vec::new(),
        }))
    }

    /// The literals of the string enum `t` names, read at `from`.
    pub(crate) fn literal_set(&self, t: &abi::Type, from: &Place) -> Option<Vec<String>> {
        let key = self.type_key(t, from)?;
        self.literals.get(&key).cloned()
    }

    /// The full path of the declaration `t` names, read at `from`.
    pub(crate) fn type_key(
        &self,
        t: &abi::Type,
        from: &Place,
    ) -> Option<(Option<String>, Vec<String>)> {
        let abi::Type::Named(r) = t else {
            return None;
        };
        self.path_key(&r.path, from)
    }

    /// The full path of the declaration `p` names, read at `from`.
    fn path_key(&self, p: &abi::Path, from: &Place) -> Option<(Option<String>, Vec<String>)> {
        let key = match &p.base {
            abi::Base::Lexical => return self.resolves(&p.segments, from),
            abi::Base::Root => (None, p.segments.clone()),
            abi::Base::Module(spec) => (Some(spec.clone()), p.segments.clone()),
        };
        self.declared.contains(&key).then_some(key)
    }

    /// `t`, read at `from`, as it's spelled at `to`.
    pub(crate) fn rebase(&self, t: &abi::Type, from: &Place, to: &Place) -> abi::Type {
        match self.type_key(t, from) {
            Some((module, full)) => {
                let (name, ns) = full.split_last().unwrap();
                let place = Place {
                    module,
                    ns: ns.to_vec(),
                };
                abi::Type::Named(abi::TypeRef {
                    path: self.path_to(&place, name, to),
                    args: Vec::new(),
                })
            }
            None => t.clone(),
        }
    }

    /// The literal enums a union alias lists directly, each lowered, with
    /// its path from the alias, its type from `cx`, and its literals.
    fn literal_bases(&mut self, n: &Named, cx: &Tcx) -> Vec<(abi::Path, abi::Type, Vec<String>)> {
        let Some(TypeKind::TypeAlias(alias)) = n.id.map(|id| &self.gctx.get_type(id).kind) else {
            return Vec::new();
        };
        let TypeRef::Union(u) = &alias.target else {
            return Vec::new();
        };
        let members: Vec<TypeRef> = u.members().to_vec();
        let acx = Tcx {
            scope: alias.body_scope,
            ..cx.clone()
        };
        let there = Tcx {
            place: n.place.clone(),
            ..acx.clone()
        };
        let mut out = Vec::new();
        for m in &members {
            let TypeRef::Reference {
                segments,
                generic_args,
            } = m
            else {
                continue;
            };
            if let Some(TypeKind::StringEnum(e)) = self
                .resolve_id(segments, &acx)
                .map(|id| &self.gctx.get_type(id).kind)
            {
                let strs = e.variants.iter().map(|v| v.js_value.clone()).collect();
                if let (abi::Type::Named(r), here) = (self.ty(m, &there), self.ty(m, &acx)) {
                    out.push((r.path, here, strs));
                }
                continue;
            }
            let Some(base) = self.union_alias_name(segments, generic_args, &acx) else {
                continue;
            };
            let mut raw = Vec::new();
            self.leaves(m, &acx, &mut raw);
            let l = Leaves::of(raw.into_iter().map(|(l, _)| l).collect());
            let literal_only = l.values.is_empty()
                && l.nums.is_empty()
                && l.bools.is_empty()
                && !l.top
                && !l.null
                && !l.undefined
                && l.strs.len() > 1;
            if literal_only {
                let here = self.literal_enum(&l.strs, &acx, Some(base.clone()));
                if let Some(abi::Type::Named(r)) = self.synth_ref(&base.key(), &n.place) {
                    out.push((r.path, here, l.strs));
                }
            }
        }
        out
    }

    /// The enum of a union alias whose members are all string literals or
    /// literal enums, which it extends.
    pub(crate) fn named_literal(&mut self, n: &Named, l: &Leaves, cx: &Tcx) -> Option<abi::Type> {
        if l.top || !l.nums.is_empty() || !l.bools.is_empty() || l.values.is_empty() {
            return None;
        }
        let bases = self.literal_bases(n, cx);
        let mut strs = Vec::new();
        for v in &l.values {
            let (_, _, b) = bases.iter().find(|(_, here, _)| here == v)?;
            strs.extend(b.iter().cloned());
        }
        strs.extend(
            l.strs
                .iter()
                .filter(|s| !strs.contains(s))
                .cloned()
                .collect::<Vec<_>>(),
        );
        Some(self.literal_enum(&strs, cx, Some(n.clone())))
    }

    /// The enum of a union of string enums (and literals): every literal,
    /// extending each enum, named after its members (`DateUnitOrTimeUnit`).
    pub(crate) fn merged_literal(&mut self, l: &Leaves, cx: &Tcx) -> Option<abi::Type> {
        if l.top
            || !l.nums.is_empty()
            || !l.bools.is_empty()
            || l.values.len() < 2 && l.strs.is_empty()
        {
            return None;
        }
        if l.values.is_empty() {
            return None;
        }
        let mut bases = Vec::new();
        let mut strs = Vec::new();
        for v in &l.values {
            let set = self.literal_set(v, &cx.place)?;
            let abi::Type::Named(r) = v else { return None };
            strs.extend(
                set.iter()
                    .filter(|s| !strs.contains(*s))
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            bases.push((r.path.clone(), set));
        }
        let mut own = Vec::new();
        for s in &l.strs {
            if !strs.contains(s) {
                strs.push(s.clone());
                own.push(s);
            }
        }
        let stems: Vec<String> = l
            .order
            .iter()
            .filter_map(|leaf| match leaf {
                Leaf::Ty(abi::Type::Named(r)) => r
                    .path
                    .segments
                    .last()
                    .map(|s| names::unescape_type(s).to_string()),
                Leaf::Str(s) if own.contains(&s) => Some(names::pascal(s)),
                _ => None,
            })
            .collect();
        let mut sorted = strs.clone();
        sorted.sort();
        let named = Named {
            name: stems.join("Or"),
            place: cx.place.clone(),
            id: None,
        };
        let key = format!("lit:{sorted:?}");
        Some(self.literal_enum_ext(&strs, bases, key, cx, Some(named)))
    }

    /// A carrier enum over integer literals; any value is legal on its
    /// carrier, so the set stays open.
    pub(crate) fn int_enum(&mut self, ints: &[i64], cx: &Tcx, named: Option<Named>) -> abi::Type {
        let mut sorted = ints.to_vec();
        sorted.sort();
        let key = match &named {
            Some(n) => n.key(),
            None => format!("int:{sorted:?}"),
        };
        if let Some(t) = self.synth_ref(&key, &cx.place) {
            return t;
        }
        let carrier = int_carrier(ints);
        let mut arms: Vec<abi::Arm> = Vec::new();
        for &v in ints {
            let base = if v < 0 {
                format!("Neg{}", v.unsigned_abs())
            } else {
                format!("V{v}")
            };
            let name = names::unique(&base, |n| arms.iter().any(|a| a.name == n));
            arms.push(abi::Arm {
                meta: abi::Meta::default(),
                name,
                payload: Vec::new(),
                value: Some(abi::ArmValue::Int(v)),
            });
        }
        self.synth_enum(key, arms, Vec::new(), Some(carrier), cx, named)
    }

    fn mentions_tparam(&self, t: &abi::Type, cx: &Tcx) -> bool {
        use abi::Type as T;
        match t {
            T::Named(r) => {
                let tparam = matches!(r.path.segments.as_slice(), [n]
                    if matches!(self.gctx.scopes.resolve_binding(cx.scope, n), Some(Binding::TypeParam)));
                tparam || r.args.iter().any(|a| self.mentions_tparam(a, cx))
            }
            T::Vec(e)
            | T::VecBox(e)
            | T::MutVec(e)
            | T::Array(e)
            | T::ArrBox(e)
            | T::Nullable(e, _)
            | T::Future(e, _) => self.mentions_tparam(e, cx),
            T::View { elem, .. } => self.mentions_tparam(elem, cx),
            T::Tuple(ts) | T::TupleBox(ts) => ts.iter().any(|e| self.mentions_tparam(e, cx)),
            T::Fn(f) => {
                f.params.iter().any(|e| self.mentions_tparam(e, cx))
                    || self.mentions_tparam(&f.result, cx)
            }
            _ => false,
        }
    }

    /// A payload enum over a union's value types, after a unit arm per
    /// literal beside them, keyed by the member that declares it (#194).
    /// An `open` set ends in `@fallback Other(Str)`, for any other string;
    /// one of literals alone is shared by every unnamed use.
    pub(crate) fn payload_enum(
        &mut self,
        strs: &[String],
        values: &[abi::Type],
        open: bool,
        cx: &Tcx,
        named: Option<Named>,
    ) -> abi::Type {
        let mut sorted = strs.to_vec();
        sorted.sort();
        let printed: Vec<String> = values.iter().map(print_type).collect();
        let literal_set = open && values.is_empty();
        // One union is one enum per place, wherever it's written, and a
        // written one reuses an alias's of the same members. One whose types
        // all read the same from the root is the root's, which every module
        // reaches.
        let home = match self.rooted(values, &cx.place) {
            true => Place::default(),
            false => cx.place.clone(),
        };
        let kind = if open { "open" } else { "pay" };
        let shape = format!("{kind}:{home:?}:{printed:?}:{strs:?}");
        let key = match &named {
            Some(n) => n.key(),
            None if literal_set => format!("open:{sorted:?}"),
            None => shape.clone(),
        };
        if let Some(t) = self.synth_ref(&key, &cx.place) {
            self.anchor(&key, cx);
            return t;
        }
        let unnamed = named.as_ref().is_none_or(|n| n.id.is_none());
        if let Some(k) = (literal_set && unnamed)
            .then(|| self.open_sets.get(&sorted).cloned())
            .flatten()
        {
            if let Some(t) = self.synth_ref(&k, &cx.place) {
                self.anchor(&k, cx);
                return t;
            }
        }
        // An enum declaration has no type parameters to carry the union's.
        if values.iter().any(|v| self.mentions_tparam(v, cx)) {
            self.warn(format!(
                "{}: a union over type parameters ({}) reads as `Any`",
                cx.location(),
                printed.join(" | ")
            ));
            return abi::Type::Any;
        }
        if literal_set {
            self.open_sets.entry(sorted).or_insert_with(|| key.clone());
        }
        let mut arms: Vec<abi::Arm> = Vec::new();
        for s in strs {
            push_literal(&mut arms, s);
        }
        for v in values {
            let name = names::unique(&names::arm_name(v), |n| arms.iter().any(|a| a.name == n));
            arms.push(abi::Arm {
                meta: abi::Meta::default(),
                name,
                payload: vec![v.clone()],
                value: None,
            });
        }
        if open {
            let name = names::unique("Other", |n| arms.iter().any(|a| a.name == n));
            arms.push(abi::Arm {
                meta: abi::Meta {
                    attrs: abi::Attrs {
                        fallback: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                name,
                payload: vec![abi::Type::StrBox],
                value: None,
            });
        }
        // An inline union of two named members, or one more than one anchor
        // shares, is spelled by its arms (`ArrayBufferOrArrayBufferView`,
        // `FooOrF64`), which no one use's anchor does.
        let spelling = Some(
            arms.iter()
                .filter(|a| !a.meta.attrs.fallback)
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join("Or"),
        )
        .filter(|n| n.len() <= 32);
        let two_named = strs.is_empty()
            && values.len() == 2
            && values
                .iter()
                .all(|v| matches!(v, abi::Type::Named(r) if r.args.is_empty()));
        let spelled = (named.is_none() && two_named)
            .then(|| spelling.clone())
            .flatten()
            .map(|name| Named {
                name,
                place: home.clone(),
                id: None,
            });
        if named.is_none() && spelled.is_none() {
            self.made.insert(key.clone(), self.anchors.len());
            self.anchors.push(super::Shared {
                anchors: HashSet::new(),
                spelled: spelling.unwrap_or_default(),
                key: key.clone(),
            });
            self.anchor(&key, cx);
        }
        let reg = named.as_ref().is_some_and(|n| n.place == home) && !literal_set;
        let at = Tcx {
            place: home,
            ..cx.clone()
        };
        self.synth_enum(key.clone(), arms, Vec::new(), None, &at, named.or(spelled));
        if reg && !self.synth.contains_key(&shape) {
            if let Some(entry) = self.synth.get(&key).cloned() {
                self.synth.insert(shape, entry);
            }
        }
        self.synth_ref(&key, &cx.place).unwrap()
    }

    /// Whether every type `values` name reads the same from the root as from
    /// `from`.
    fn rooted(&self, values: &[abi::Type], from: &Place) -> bool {
        let root = Place::default();
        let mut ok = true;
        for v in values {
            v.walk(&mut |t| {
                if let abi::Type::Named(r) = t {
                    ok &= self.path_key(&r.path, from) == self.path_key(&r.path, &root);
                }
            });
        }
        ok
    }

    fn anchor(&mut self, key: &str, cx: &Tcx) {
        if let Some(&i) = self.made.get(key) {
            self.anchors[i]
                .anchors
                .insert((cx.owner.clone(), cx.anchor.clone()));
        }
    }

    fn synth_enum(
        &mut self,
        key: String,
        arms: Vec<abi::Arm>,
        extends: Vec<abi::Path>,
        carrier: Option<abi::Scalar>,
        cx: &Tcx,
        named: Option<Named>,
    ) -> abi::Type {
        // A use's enum that another already named takes its owner's name
        // before a number, as `<Owner><Anchor>Kind`.
        let alias = named.is_some();
        let (base, owned, place) = match named {
            Some(n) => (n.name, None, n.place),
            None => {
                // `kind: "a" | "b"` is `<Owner>Kind`, not `…KindKind`.
                let kind = |n: String| match n.ends_with("Kind") {
                    true => n,
                    false => n + "Kind",
                };
                let owned = cx.owner.as_ref().map(|o| {
                    let o = names::pascal(o);
                    match o.ends_with(&cx.anchor) || cx.anchor == "Kind" {
                        true => kind(o),
                        false => kind(format!("{o}{}", cx.anchor)),
                    }
                });
                // A one-word anchor (`Value`, `Audio`) is too generic alone,
                // so it keeps its owner.
                let generic = !cx.anchor.chars().skip(1).any(|c| c.is_ascii_uppercase());
                let base = match &owned {
                    Some(o) if generic => o.clone(),
                    _ => kind(cx.anchor.clone()),
                };
                (base, owned, cx.place.clone())
            }
        };
        let base = names::escape_type(&base);
        // A TS name is unique where it's declared, as a declaration's is.
        let declared = |n: &str| {
            let mut path = place.ns.clone();
            path.push(n.to_string());
            self.declared.contains(&(place.module.clone(), path))
        };
        let taken = |n: &str| self.taken.contains(n);
        let name = match owned {
            _ if alias => names::unique(&base, declared),
            Some(owned) if taken(&base) => {
                let name = names::unique(&names::escape_type(&owned), taken);
                self.qualified.push((base.clone(), name.clone()));
                name
            }
            _ => names::unique(&base, taken),
        };
        self.taken.insert(name.clone());
        let mut full = place.ns.clone();
        full.push(name.clone());
        self.declared.insert((place.module.clone(), full.clone()));
        if arms.iter().any(|a| !a.payload.is_empty()) {
            self.payloads.insert((place.module.clone(), full.clone()));
        }
        self.enums.insert((place.module.clone(), full));
        self.container(&place)
            .synth
            .push(abi::Decl::Enum(abi::Enum {
                meta: abi::Meta::default(),
                name: name.clone(),
                carrier,
                extends,
                arms,
                members: Vec::new(),
            }));
        self.synth.insert(key.clone(), (place, name));
        self.synth_ref(&key, &cx.place).unwrap()
    }

    pub(crate) fn synth_enum_with(
        &mut self,
        key: String,
        arms: Vec<abi::Arm>,
        extends: Vec<abi::Path>,
        cx: &Tcx,
        named: Named,
    ) -> abi::Type {
        self.synth_enum(key, arms, extends, None, cx, Some(named))
    }

    pub(crate) fn synth_ref(&self, key: &str, from: &Place) -> Option<abi::Type> {
        let (place, name) = self.synth.get(key)?;
        Some(abi::Type::Named(abi::TypeRef {
            path: self.path_to(place, name, from),
            args: Vec::new(),
        }))
    }

    fn intersection(&mut self, items: &[TypeRef], cx: &Tcx) -> abi::Type {
        // `A[] & B[]` is an array of `A & B`.
        let elems: Option<Vec<TypeRef>> = items
            .iter()
            .map(|i| match i {
                TypeRef::Array(e) => Some((**e).clone()),
                _ => None,
            })
            .collect();
        if let Some(elems) = elems.filter(|e| !e.is_empty()) {
            return abi::Type::Array(Box::new(self.intersection(&elems, cx)));
        }
        let spelled = TypeRef::Intersection(items.to_vec()).format_ts();
        for i in items {
            if let t @ abi::Type::Named(_) = self.ty(i, cx) {
                if items.len() > 1 {
                    self.warn(format!(
                        "{}: `{spelled}` reads as `{}` alone",
                        cx.location(),
                        i.format_ts()
                    ));
                }
                return t;
            }
        }
        self.warn(format!(
            "{}: `{spelled}` has no IR type and reads as `Any`",
            cx.location()
        ));
        abi::Type::Any
    }

    pub(crate) fn fn_type(&mut self, sig: &FunctionSig, cx: &Tcx) -> abi::FnType {
        let params = sig
            .params
            .iter()
            .map(|p| {
                let t = self.inner(&p.type_ref, cx);
                if p.variadic {
                    match t {
                        abi::Type::Array(_) => t,
                        t => abi::Type::Array(Box::new(t)),
                    }
                } else if p.optional && !matches!(t, abi::Type::Nullable(_, _)) {
                    abi::Type::Nullable(Box::new(t), false)
                } else {
                    t
                }
            })
            .collect();
        let result = match *sig.return_type {
            TypeRef::Never => abi::Type::Never,
            _ => self.nulled(&sig.return_type, cx),
        };
        abi::FnType {
            params,
            rest: sig.params.last().is_some_and(|p| p.variadic),
            result,
            throws: None,
        }
    }

    /// The alias a reference names, with its target and the context its
    /// target reads in; `None` for anything else, and for an alias that
    /// reaches itself.
    pub(crate) fn alias(
        &mut self,
        segments: &[String],
        args: &[TypeRef],
        cx: &Tcx,
    ) -> Option<(TypeRef, Tcx)> {
        let id = self.resolve_id(segments, cx)?;
        let TypeKind::TypeAlias(alias) = &self.gctx.get_type(id).kind else {
            return None;
        };
        if cx.aliases.contains(&id) {
            self.warn(format!(
                "{}: recursive alias `{}` reads as `Any`",
                cx.location(),
                alias.name
            ));
            return None;
        }
        let alias = alias.clone();
        let subst = self.bind(&alias.type_params, args, alias.body_scope, cx, None);
        let mut aliases = (*cx.aliases).clone();
        aliases.push(id);
        let acx = Tcx {
            scope: alias.body_scope,
            subst: Rc::new(subst),
            aliases: Rc::new(aliases),
            ..cx.clone()
        };
        Some((alias.target, acx))
    }

    /// Whether the IR declares the alias `id`: one of the input's that
    /// isn't a union (an enum) and lowers the same wherever it's used.
    pub(crate) fn keeps_alias(&self, id: TypeId) -> bool {
        self.keeps_alias_at(id, 0)
    }

    fn keeps_alias_at(&self, id: TypeId, depth: usize) -> bool {
        let TypeKind::TypeAlias(alias) = &self.gctx.get_type(id).kind else {
            return false;
        };
        if alias.from_module.is_some()
            || self.gctx.builtin_type_ids.contains(&id)
            || !self.alias_places.contains_key(&id)
        {
            return false;
        }
        // Lowering reads other types' structure, which an alias would
        // hide, but for a function's, which only arm tests and `@once` do.
        let top = matches!(
            &alias.target,
            TypeRef::Function(_) | TypeRef::Reference { .. }
        );
        if !top || !self.plain(&alias.target) {
            return false;
        }
        // An alias of another is kept with it (`type A = B`).
        match &alias.target {
            TypeRef::Reference { segments, .. } => {
                let target = match &segments[..] {
                    [name] => match self.gctx.scopes.resolve_binding(alias.body_scope, name) {
                        Some(Binding::Declared(t)) => Some(t),
                        _ => None,
                    },
                    _ => self
                        .gctx
                        .resolve_path(alias.body_scope, &segments.join(".")),
                };
                match target.map(|t| (t, &self.gctx.get_type(t).kind)) {
                    Some((t, TypeKind::TypeAlias(_))) => {
                        depth < 16 && self.keeps_alias_at(t, depth + 1)
                    }
                    _ => false,
                }
            }
            _ => true,
        }
    }

    /// Whether `t` reads the same wherever it's written: nothing a use
    /// evaluates (`keyof`, `T[K]`, mapped and template types, utilities)
    /// or names for itself (object literals, intersections).
    fn plain(&self, t: &TypeRef) -> bool {
        match t {
            TypeRef::KeyOf(_)
            | TypeRef::IndexedAccess(..)
            | TypeRef::Mapped(_)
            | TypeRef::Template { .. }
            | TypeRef::ObjectLiteral(_)
            | TypeRef::Intersection(_)
            | TypeRef::Unresolved(_)
            | TypeRef::StringLiteral(_)
            | TypeRef::NumberLiteral(_)
            | TypeRef::BooleanLiteral(_) => false,
            TypeRef::Reference {
                segments,
                generic_args,
            } => {
                self.utility(segments).is_none()
                    && segments.as_slice() != ["this"]
                    && generic_args.iter().all(|a| self.plain(a))
            }
            TypeRef::Array(t) => self.plain(t),
            TypeRef::Tuple(ts) => ts.iter().all(|t| self.plain(t)),
            TypeRef::Union(u) => u.members().iter().all(|t| self.plain(t)),
            TypeRef::Function(f) => {
                f.params.iter().all(|p| self.plain(&p.type_ref)) && self.plain(&f.return_type)
            }
            _ => true,
        }
    }

    /// A reference to an alias the IR declares, declaring it at its first
    /// use: its type lowered where the alias is, its parameters open.
    fn kept_alias(&mut self, id: TypeId, args: &[TypeRef], cx: &Tcx) -> Option<abi::Type> {
        if !self.keeps_alias(id) {
            return None;
        }
        let place = self.alias_place(id);
        let name = match self.kept.get(&id) {
            Some(n) => n.clone(),
            None => {
                let TypeKind::TypeAlias(alias) = &self.gctx.get_type(id).kind else {
                    unreachable!()
                };
                let alias = alias.clone();
                let doc = names::doc(&self.gctx.get_type(id).doc);
                let name = names::escape_type(&alias.name);
                self.kept.insert(id, name.clone());
                let mut full = place.ns.clone();
                full.push(name.clone());
                self.declared.insert((place.module.clone(), full.clone()));
                self.fn_aliases.insert((place.module.clone(), full));
                self.taken.insert(name.clone());
                let acx = Tcx::new(alias.body_scope, &place, &alias.name);
                let tparams = alias
                    .type_params
                    .iter()
                    .map(|p| abi::TypeParam {
                        name: p.name.clone(),
                        bound: None,
                        default: p.default.as_ref().map(|d| self.type_arg(d, false, &acx)),
                    })
                    .collect();
                let ty = self.ty(&alias.target, &acx);
                self.container(&place)
                    .synth
                    .push(abi::Decl::Alias(abi::Alias {
                        meta: abi::Meta {
                            doc,
                            ..abi::Meta::default()
                        },
                        name: name.clone(),
                        tparams,
                        ty,
                    }));
                name
            }
        };
        let args = args.iter().map(|a| self.type_arg(a, false, cx)).collect();
        Some(abi::Type::Named(abi::TypeRef {
            path: self.path_to(&place, &name, &cx.place),
            args,
        }))
    }

    /// A type argument, or a parameter's constraint. A few literals alone
    /// (`RoundingOptions<"year" | "month">`) are named for themselves, as
    /// `YearOrMonth`, not the use. With `one`, for a literal-only
    /// parameter, a literal alone (`RoundingOptions<"hour">`) is an enum
    /// too, as `Hour`, rather than its primitive.
    pub(crate) fn type_arg(&mut self, a: &TypeRef, one: bool, cx: &Tcx) -> abi::Type {
        if let Some(t) = self.void_arg(a, cx) {
            return t;
        }
        let members = match a {
            TypeRef::Union(u) => u.members(),
            a if one => std::slice::from_ref(a),
            _ => &[],
        };
        if !members.is_empty() {
            let strs: Option<Vec<String>> = members
                .iter()
                .map(|m| match m {
                    TypeRef::StringLiteral(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();
            if let Some(strs) = strs.filter(|s| s.len() <= 4) {
                let named = Named {
                    name: strs
                        .iter()
                        .map(|s| names::pascal(s))
                        .collect::<Vec<_>>()
                        .join("Or"),
                    place: cx.place.clone(),
                    id: None,
                };
                return self.literal_enum(&strs, cx, Some(named));
            }
        }
        self.inner(a, cx)
    }

    /// `undefined`, `void` or `never` as a type argument: `unit`, the
    /// unboxed `Unit` (IR.md §3).
    fn void_arg(&mut self, a: &TypeRef, cx: &Tcx) -> Option<abi::Type> {
        match a {
            TypeRef::Undefined | TypeRef::Void => Some(abi::Type::Unit),
            // A type parameter bound to `unit` (`Pair<T, T>` with `T =
            // undefined`) passes it on.
            TypeRef::Reference {
                segments,
                generic_args,
            } if generic_args.is_empty()
                && matches!(&segments[..], [n] if cx.subst.get(n) == Some(&abi::Type::Unit)
                    && matches!(self.gctx.scopes.resolve_binding(cx.scope, n), Some(Binding::TypeParam))) =>
            {
                Some(abi::Type::Unit)
            }
            TypeRef::Never => {
                self.warn(format!(
                    "{}: `never` as a type argument reads as `unit`",
                    cx.location()
                ));
                Some(abi::Type::Unit)
            }
            _ => None,
        }
    }

    /// The name and place of the enum a union alias becomes.
    pub(crate) fn union_alias_name(
        &mut self,
        segments: &[String],
        args: &[TypeRef],
        cx: &Tcx,
    ) -> Option<Named> {
        let mut id = self.resolve_id(segments, cx)?;
        let mut args = args.to_vec();
        let mut cx = cx.clone();
        // `type RetentionDuration = SleepDuration` is `SleepDuration`.
        let mut seen = HashSet::new();
        let alias = loop {
            let TypeKind::TypeAlias(alias) = &self.gctx.get_type(id).kind else {
                return None;
            };
            if !seen.insert(id) || cx.aliases.contains(&id) {
                return None;
            }
            match &alias.target {
                TypeRef::Reference {
                    segments,
                    generic_args,
                } if alias.type_params.is_empty() => {
                    cx = cx.at(alias.body_scope);
                    id = self.resolve_id(segments, &cx)?;
                    args.clone_from(generic_args);
                }
                TypeRef::Union(u) if u.values().len() > 1 => break alias.clone(),
                _ => return None,
            }
        };
        // A generic alias is named for the arguments its union reads, so
        // each instantiation is one enum.
        let target = alias.target.format_ts();
        let used: Vec<&TypeParam> = alias
            .type_params
            .iter()
            .filter(|p| mentions(&target, &p.name))
            .collect();
        let name = if used.is_empty() {
            alias.name.clone()
        } else {
            let given = self.bind(&alias.type_params, &args, alias.body_scope, &cx, None);
            let defaults = self.bind(&alias.type_params, &[], alias.body_scope, &cx, None);
            let spelled: Vec<String> = used
                .iter()
                .filter(|p| given.get(&p.name) != defaults.get(&p.name))
                .map(|p| names::arm_name(&given[&p.name]))
                .collect();
            crate::parse::eval::instance_spelling(&alias.name, &spelled, None)
        };
        Some(Named {
            name,
            place: self.alias_place(id),
            id: Some(id),
        })
    }

    fn alias_place(&self, id: TypeId) -> Place {
        let decl = self.gctx.get_type(id);
        if let Some(place) = self.alias_places.get(&id) {
            return place.clone();
        }
        let place = Place::of(&decl.module_context);
        // The builtin `Intl.LocalesArgument` keeps its namespace.
        let intl = self
            .gctx
            .scopes
            .get(self.module.global_scope)
            .namespace("Intl");
        match self.gctx.builtin_type_ids.contains(&id) && intl == Some(decl.scope_id) {
            true => place.child("Intl"),
            false => place,
        }
    }

    /// An alias's or declaration's parameters bound to the arguments a use
    /// gives, or their defaults. `decl` names the declaration, whose
    /// literal-only parameters take a literal alone as an enum.
    fn bind(
        &mut self,
        params: &[TypeParam],
        args: &[TypeRef],
        scope: ScopeId,
        cx: &Tcx,
        decl: Option<(&Place, &str)>,
    ) -> HashMap<String, abi::Type> {
        let mut map = (*cx.subst).clone();
        for (i, p) in params.iter().enumerate() {
            let t = match (args.get(i), &p.default) {
                (Some(a), _) => {
                    let one = decl.is_some_and(|(place, name)| {
                        let k = params[..i]
                            .iter()
                            .filter(|p| !self.dropped(p, scope))
                            .count();
                        self.lit_args
                            .contains_key(&(place.clone(), name.to_string(), k))
                    });
                    self.type_arg(a, one, cx)
                }
                // A default may name the parameters before it.
                (None, Some(d)) => {
                    let dcx = Tcx {
                        scope,
                        subst: Rc::new(map.clone()),
                        ..cx.clone()
                    };
                    self.type_arg(d, false, &dcx)
                }
                (None, None) => abi::Type::Nullable(Box::new(abi::Type::Any), false),
            };
            map.insert(p.name.clone(), t);
        }
        map
    }

    pub(crate) fn resolve_id(&self, segments: &[String], cx: &Tcx) -> Option<TypeId> {
        match segments {
            [name] => match self.gctx.scopes.resolve_binding(cx.scope, name)? {
                Binding::Declared(id) => Some(id),
                _ => None,
            },
            _ => self.gctx.resolve_path(cx.scope, &segments.join(".")),
        }
    }

    fn reference(&mut self, segments: &[String], args: &[TypeRef], cx: &Tcx) -> abi::Type {
        if let [name] = segments {
            match self.gctx.scopes.resolve_binding(cx.scope, name) {
                Some(Binding::TypeParam) => {
                    return match cx.subst.get(name) {
                        Some(t) => t.clone(),
                        None => abi::Type::named([name.as_str()]),
                    };
                }
                Some(Binding::Erased(e)) => return self.ty(&e.type_ref(), cx),
                _ => {}
            }
        }
        if self.utility(segments).is_some() {
            let whole = TypeRef::Reference {
                segments: segments.to_vec(),
                generic_args: args.to_vec(),
            };
            return self.join_type(&whole, cx, Read::Inner).0;
        }
        let Some(id) = self.resolve_id(segments, cx) else {
            return self.lib(&segments.join("."), args, cx);
        };
        let kind = &self.gctx.get_type(id).kind;
        let is_enum = matches!(kind, TypeKind::StringEnum(_) | TypeKind::NumericEnum(_));
        let outside_lib =
            self.opts.lib.is_some() && self.info.contains_key(&id) && !self.is_root(id) && !is_enum;
        if self.gctx.builtin_type_ids.contains(&id) || outside_lib {
            return self.lib(&segments.join("."), args, cx);
        }
        if let (true, Some(info)) = (is_enum, self.info.get(&id)) {
            let mut path = info.place.ns.clone();
            path.push(info.name.clone());
            self.pull((info.place.module.clone(), path));
        }
        let kind = &self.gctx.get_type(id).kind;
        match kind {
            TypeKind::TypeAlias(_) => {
                if self.union_alias_name(segments, args, cx).is_some() {
                    let whole = TypeRef::Reference {
                        segments: segments.to_vec(),
                        generic_args: args.to_vec(),
                    };
                    return self.join_type(&whole, cx, Read::Inner).0;
                }
                if let Some(t) = self.kept_alias(id, args, cx) {
                    return t;
                }
                match self.alias(segments, args, cx) {
                    Some((target, acx)) => self.ty(&target, &acx),
                    None => abi::Type::Any,
                }
            }
            TypeKind::Class(_)
            | TypeKind::Interface(_)
            | TypeKind::DiscriminatedUnion(_)
            | TypeKind::StringEnum(_)
            | TypeKind::NumericEnum(_) => {
                let Some(info) = self.info.get(&id).cloned() else {
                    self.warn(format!(
                        "{}: `{}` is declared outside the input and reads as `Any`",
                        cx.location(),
                        segments.join(".")
                    ));
                    return abi::Type::Any;
                };
                let args = self.decl_args(id, args, cx);
                abi::Type::Named(abi::TypeRef {
                    path: self.path_to(&info.place, &info.name, &cx.place),
                    args,
                })
            }
            _ => {
                self.warn(format!(
                    "{}: `{}` names a value, not a type, and reads as `Any`",
                    cx.location(),
                    segments.join(".")
                ));
                abi::Type::Any
            }
        }
    }

    /// The arguments of a reference to a generic declaration: one per
    /// parameter the IR keeps, an omitted one taking its default (the IR
    /// has none).
    fn decl_args(&mut self, id: TypeId, args: &[TypeRef], cx: &Tcx) -> Vec<abi::Type> {
        let (params, scope) = match &self.gctx.get_type(id).kind {
            TypeKind::Class(c) => (c.type_params.clone(), c.body_scope),
            TypeKind::Interface(i) => (i.type_params.clone(), i.body_scope),
            TypeKind::DiscriminatedUnion(d) => (d.type_params.clone(), d.body_scope),
            _ => return Vec::new(),
        };
        let info = self.info[&id].clone();
        let bound = self.bind(&params, args, scope, cx, Some((&info.place, &info.name)));
        let kept: Vec<&TypeParam> = params.iter().filter(|p| !self.dropped(p, scope)).collect();
        for (k, p) in kept.iter().enumerate() {
            let t = &bound[&p.name];
            let to = (info.place.clone(), info.name.clone(), k);
            if let Some(from) = self.lit_param(t, cx) {
                if self.lit_args.contains_key(&to)
                    && !self.passed.contains(&(from.clone(), to.clone()))
                {
                    self.passed.push((from, to));
                }
                continue;
            }
            if self.literal_set(t, &cx.place).is_some() {
                let t = self.rebase(t, &cx.place, &info.place);
                if let Some(args) =
                    self.lit_args
                        .get_mut(&(info.place.clone(), info.name.clone(), k))
                {
                    if !args.contains(&t) {
                        args.push(t);
                    }
                }
            }
        }
        kept.iter().map(|p| boxed(bound[&p.name].clone())).collect()
    }

    /// The enclosing declaration's literal-only parameter `t` names.
    pub(crate) fn lit_param(&self, t: &abi::Type, cx: &Tcx) -> Option<LitParam> {
        let Some(abi::Type::Named(this)) = &cx.this else {
            return None;
        };
        let k = this.args.iter().position(|a| a == t)?;
        let key = (cx.place.clone(), this.path.segments.last()?.clone(), k);
        self.lit_args.contains_key(&key).then_some(key)
    }

    /// Whether a type parameter has no IR form: the parse erased it to a
    /// primitive, or a lib root's default stands for it.
    pub(crate) fn dropped(&self, p: &TypeParam, scope: ScopeId) -> bool {
        matches!(
            self.gctx.scopes.resolve_binding(scope, &p.name),
            Some(Binding::Erased(_))
        ) || (self.opts.lib.is_some() && p.default.is_some())
    }

    /// A lib root's defaulted type parameters, bound to their defaults.
    pub(crate) fn defaults(
        &mut self,
        ps: &[TypeParam],
        cx: &Tcx,
    ) -> Rc<HashMap<String, abi::Type>> {
        let mut subst = (*cx.subst).clone();
        if self.opts.lib.is_some() {
            for p in ps {
                if let Some(d) = &p.default {
                    let t = self.inner(d, cx);
                    subst.insert(p.name.clone(), t);
                }
            }
        }
        Rc::new(subst)
    }

    /// A lib type, which the input references but doesn't declare.
    fn lib(&mut self, name: &str, args: &[TypeRef], cx: &Tcx) -> abi::Type {
        match name {
            // The lib's class of bigints is the box of `bigint`.
            "BigInt" => abi::Type::BigIntBox,
            _ if self
                .opts
                .provides
                .classes
                .iter()
                .any(|c| c.decl.name == name) =>
            {
                abi::Type::named([name])
            }
            "Promise" | "PromiseLike" => {
                let inner = match args.first() {
                    Some(TypeRef::Never) => abi::Type::Never,
                    Some(a) => self.nulled(a, cx),
                    None => abi::Type::Unit,
                };
                abi::Type::Future(Box::new(inner), abi::Path::err())
            }
            "Object" => abi::Type::Any,
            "Record" => self.record(args, cx),
            "ArrayBufferLike" => self.reference(&["ArrayBuffer".to_string()], &[], cx),
            "Array" | "ReadonlyArray" => match args.first() {
                Some(a) => abi::Type::Array(Box::new(self.inner(a, cx))),
                None => abi::Type::Array(Box::new(abi::Type::Nullable(
                    Box::new(abi::Type::Any),
                    false,
                ))),
            },
            // The buffer type parameter is the TS lib's, with nothing to cross.
            _ if typed_array_scalar(name).is_some() => {
                self.stub(name, Vec::new(), abi::ClassKind::Class, cx)
            }
            _ if name.contains('.') && !self.lib_types.contains_key(name) => {
                self.warn(format!(
                    "{}: lib type `{name}` reads as `Any`",
                    cx.location()
                ));
                abi::Type::Any
            }
            _ => {
                let args = args
                    .iter()
                    .map(|a| boxed(self.void_arg(a, cx).unwrap_or_else(|| self.inner(a, cx))))
                    .collect();
                // A lib's own interfaces outside its roots stay interfaces.
                let declared = self.resolve_id(&[name.to_string()], cx);
                let declared_interface = self.opts.lib.is_some()
                    && declared.is_some_and(|id| {
                        matches!(self.gctx.get_type(id).kind, TypeKind::Interface(_))
                    });
                let kind = if INTERFACE_BUILTINS.contains(&name) || declared_interface {
                    abi::ClassKind::Interface
                } else {
                    abi::ClassKind::Class
                };
                self.stub(name, args, kind, cx)
            }
        }
    }

    /// `Record<K, V>`: a lib interface of `@js(index)` access, keyed by
    /// string unless `K` is `number`.
    fn record(&mut self, args: &[TypeRef], cx: &Tcx) -> abi::Type {
        let name = match args.first() {
            Some(TypeRef::Number) => "NumberRecord",
            _ => "Record",
        };
        let value = match args.get(1) {
            Some(v) => self.inner(v, cx),
            None => abi::Type::Nullable(Box::new(abi::Type::Any), false),
        };
        self.taken.insert(name.to_string());
        self.declared.insert((None, vec![name.to_string()]));
        self.records.insert(name);
        abi::Type::Named(abi::TypeRef {
            path: abi::Path::new([name]),
            args: vec![boxed(value)],
        })
    }

    fn stub(
        &mut self,
        name: &str,
        mut args: Vec<abi::Type>,
        kind: abi::ClassKind,
        cx: &Tcx,
    ) -> abi::Type {
        if let Some((lib, arity, _)) = self.lib_types.get(name).cloned() {
            self.with_used.insert(lib);
            args.resize(arity, abi::Type::Nullable(Box::new(abi::Type::Any), false));
            return abi::Type::Named(abi::TypeRef {
                path: self.global_path(name.split('.').map(String::from).collect(), &cx.place),
                args,
            });
        }
        let arity = args.len();
        let entry = self.stubs.entry(name.to_string()).or_insert((kind, arity));
        entry.1 = entry.1.max(arity);
        self.taken.insert(name.to_string());
        self.declared.insert((None, vec![name.to_string()]));
        abi::Type::Named(abi::TypeRef {
            path: self.global_path(vec![name.to_string()], &cx.place),
            args,
        })
    }

    /// The global `segs`, rooted where a declaration of the input shadows it.
    fn global_path(&self, segs: Vec<String>, from: &Place) -> abi::Path {
        match self.resolves(&segs, from) {
            Some(key) if key != (None, segs.clone()) => abi::Path::rooted(segs),
            _ => abi::Path::new(segs),
        }
    }

    /// The shortest path to `name` at `place` that resolves to it from
    /// `from`, else one based at its module block or the root (IR.md §4).
    pub(crate) fn path_to(&self, place: &Place, name: &str, from: &Place) -> abi::Path {
        let mut path = self.path_to_ts(place, name, from);
        let n = path.segments.len();
        for s in &mut path.segments[..n - 1] {
            *s = names::escape_type(s);
        }
        path
    }

    /// [`Lowerer::path_to`], with namespaces by their TypeScript names.
    fn path_to_ts(&self, place: &Place, name: &str, from: &Place) -> abi::Path {
        let mut full = place.ns.clone();
        full.push(name.to_string());
        let target = (place.module.clone(), full.clone());
        for start in (0..full.len()).rev() {
            if self.resolves(&full[start..], from).as_ref() == Some(&target) {
                return abi::Path::new(full[start..].to_vec());
            }
        }
        match &place.module {
            Some(spec) => abi::Path::in_module(spec, full),
            None => abi::Path::rooted(full),
        }
    }

    /// Whether `t`, read at `from`, names an interface, which the bridge
    /// tests for as any non-null object.
    fn is_interface(&self, t: &abi::Type, from: &Place) -> bool {
        let abi::Type::Named(r) = t else {
            return false;
        };
        if let Some((_, _, interface)) = self.lib_types.get(&r.path.segments.join(".")) {
            return *interface;
        }
        if let [name] = r.path.segments.as_slice() {
            if let Some((kind, _)) = self.stubs.get(name) {
                return *kind == abi::ClassKind::Interface;
            }
        }
        self.type_key(t, from).is_some_and(|k| {
            !self.classes.contains(&k) && !self.enums.contains(&k) && !self.fn_aliases.contains(&k)
        })
    }

    /// Whether `t`, read at `from`, is a function type, through aliases.
    pub(crate) fn is_fn(&self, t: &abi::Type, from: &Place) -> bool {
        matches!(t, abi::Type::Fn(_))
            || self
                .type_key(t, from)
                .is_some_and(|k| self.fn_aliases.contains(&k))
    }

    /// Whether the bridge can tell apart incoming values of the union with
    /// leaves `l` by its arm tests (IR.md §8), in order.
    pub(crate) fn discriminable(&self, l: &Leaves, cx: &Tcx) -> bool {
        use abi::Type as T;
        let mut tests: Vec<String> = Vec::new();
        if !l.strs.is_empty() {
            tests.push("string".into());
        }
        if !l.nums.is_empty() {
            tests.push("number".into());
        }
        if !l.bools.is_empty() {
            tests.push("boolean".into());
        }
        let mut object = false;
        for v in &l.values {
            let test = match v {
                T::Str | T::StrBox => "string".to_string(),
                T::Scalar(abi::Scalar::Bool) | T::ScalarBox(abi::Scalar::Bool) => "boolean".into(),
                T::Scalar(abi::Scalar::I64 | abi::Scalar::U64)
                | T::ScalarBox(abi::Scalar::I64 | abi::Scalar::U64)
                | T::BigInt
                | T::BigIntBox => "bigint".into(),
                T::Scalar(_) | T::ScalarBox(_) => "number".into(),
                T::Array(_) | T::Tuple(_) => "array".into(),
                T::Fn(_) => "function".into(),
                _ if self.is_fn(v, &cx.place) => "function".into(),
                T::Future(..) => "thenable".into(),
                T::Vec(e) => format!("vec {}", print_type(e)),
                // A nested enum's literals or payloads may be anything.
                _ if self.is_enum(v, &cx.place) => return false,
                // An interface takes any object, so it comes after every
                // other object arm.
                _ if self.is_interface(v, &cx.place) => {
                    if object {
                        return false;
                    }
                    object = true;
                    continue;
                }
                T::Named(r) if !object => format!("class {:?}", r.path.segments),
                _ => return false,
            };
            if tests.contains(&test) {
                return false;
            }
            tests.push(test);
        }
        true
    }

    /// Whether `t`, read at `from`, names an enum.
    pub(crate) fn is_enum(&self, t: &abi::Type, from: &Place) -> bool {
        self.type_key(t, from)
            .is_some_and(|k| self.enums.contains(&k))
    }

    /// Whether `t`, read at `from`, names an enum with no payload arm.
    pub(crate) fn is_unit_enum(&self, t: &abi::Type, from: &Place) -> bool {
        self.type_key(t, from)
            .is_some_and(|k| self.enums.contains(&k) && !self.payloads.contains(&k))
    }

    fn resolves(&self, segs: &[String], from: &Place) -> Option<(Option<String>, Vec<String>)> {
        let scopes: Vec<Option<String>> = match &from.module {
            Some(m) => vec![Some(m.clone()), None],
            None => vec![None],
        };
        for scope in scopes {
            let depth = if scope == from.module {
                from.ns.len()
            } else {
                0
            };
            for k in (0..=depth).rev() {
                let mut full = from.ns[..k].to_vec();
                full.extend(segs.iter().cloned());
                let key = (scope.clone(), full);
                if self.declared.contains(&key) {
                    return Some(key);
                }
            }
        }
        None
    }
}

pub(crate) fn abi_scalar(s: crate::ir::AbiScalar) -> abi::Scalar {
    abi::Scalar::from_name(s.as_str()).expect("every tag scalar is an IR scalar")
}

/// A type argument: the box of an unboxed type (IR.md §13).
pub(crate) fn boxed(t: abi::Type) -> abi::Type {
    use abi::Type as T;
    match t {
        T::Scalar(s) => T::ScalarBox(s),
        T::Str => T::StrBox,
        T::BigInt => T::BigIntBox,
        T::Vec(e) => T::VecBox(e),
        T::Array(e) => T::ArrBox(e),
        T::Tuple(ts) => T::TupleBox(ts),
        T::Nullable(t, n) => T::Nullable(Box::new(boxed(*t)), n),
        t => t,
    }
}

/// Swap `from` for `to` where `t` is that enum, or an optional of it.
fn repath(t: &mut abi::Type, from: &abi::Type, to: &abi::Type) {
    match t {
        abi::Type::Nullable(e, _) => repath(e, from, to),
        t if t == from => *t = to.clone(),
        _ => {}
    }
}

/// The values of numeric literals, when every one is an integer.
pub(crate) fn int_literals(nums: &[f64]) -> Option<Vec<i64>> {
    nums.iter()
        .map(|&n| (n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15).then_some(n as i64))
        .collect()
}

/// The narrowest of `i32`, `u32` and `i64` holding every value.
pub(crate) fn int_carrier(values: &[i64]) -> abi::Scalar {
    let fits = |lo: i64, hi: i64| values.iter().all(|v| (lo..=hi).contains(v));
    if fits(i32::MIN.into(), i32::MAX.into()) {
        abi::Scalar::I32
    } else if fits(0, u32::MAX.into()) {
        abi::Scalar::U32
    } else {
        abi::Scalar::I64
    }
}

/// The element of a typed array class (IR.md §11). `Uint8ClampedArray`
/// has none: a `u8[]` crosses as a `Uint8Array`.
fn typed_array_scalar(name: &str) -> Option<abi::Scalar> {
    use abi::Scalar as S;
    Some(match name {
        "Int8Array" => S::I8,
        "Uint8Array" => S::U8,
        "Int16Array" => S::I16,
        "Uint16Array" => S::U16,
        "Int32Array" => S::I32,
        "Uint32Array" => S::U32,
        "BigInt64Array" => S::I64,
        "BigUint64Array" => S::U64,
        "Float16Array" => S::F16,
        "Float32Array" => S::F32,
        "Float64Array" => S::F64,
        _ => return None,
    })
}

/// A unit arm for the literal `s`, named for it.
fn push_literal(arms: &mut Vec<abi::Arm>, s: &str) {
    let name = names::unique(&names::pascal(s), |n| arms.iter().any(|a| a.name == n));
    arms.push(abi::Arm {
        meta: abi::Meta {
            attrs: abi::Attrs {
                js: vec![abi::JsEntry::Literal(s.to_string())],
                ..Default::default()
            },
            ..Default::default()
        },
        name,
        payload: Vec::new(),
        value: None,
    });
}

/// A written value's types, each typed array paired with its copy,
/// `T[]`: the class keeps identity, the copy reads the language's own
/// memory. A `number[]` (`bigint[]`) beside it is the same copy.
pub(crate) fn pair_typed_arrays(values: Vec<abi::Type>) -> Vec<abi::Type> {
    use abi::Type as T;
    let mut out: Vec<T> = Vec::new();
    let mut copies = Vec::new();
    for v in values {
        let elem = match &v {
            T::Named(r) if r.args.is_empty() => {
                r.path.segments.last().and_then(|n| typed_array_scalar(n))
            }
            _ => None,
        };
        out.push(v);
        if let Some(s) = elem {
            let copy = T::Vec(Box::new(T::Scalar(s)));
            if !out.contains(&copy) {
                out.push(copy);
                copies.push(s);
            }
        }
    }
    if let [s] = copies.as_slice() {
        let array = if matches!(s, abi::Scalar::I64 | abi::Scalar::U64) {
            abi::Scalar::I64
        } else {
            abi::Scalar::F64
        };
        out.retain(|v| *v != T::Array(Box::new(T::Scalar(array))));
    }
    out
}

/// Whether the TS spelling `ts` names the identifier `name`.
fn mentions(ts: &str, name: &str) -> bool {
    ts.match_indices(name).any(|(i, _)| {
        let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
        !ts[..i].ends_with(word) && !ts[i + name.len()..].starts_with(word)
    })
}
