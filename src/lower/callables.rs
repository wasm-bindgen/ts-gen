//! Declarations and their members, with overload expansion (IR.md §6, §7).
//!
//! A TypeScript signature becomes one IR overload per combination of its
//! parameters' arms: each union member is an arm, `null` rides on a value
//! arm as `@js(null) T?`, and trailing omittable parameters give arity
//! overloads. Constants move
//! into `@js(args = (...))`. Overloads whose keys still collide are renamed
//! after their constants.

use std::collections::HashMap;

use ts_gen_abi::ir as abi;
use ts_gen_abi::ir::JsEntries;
use ts_gen_abi::naming;
use ts_gen_abi::text::print_type;

use super::types::{
    boxed, int_carrier, int_literals, pair_typed_arrays, Leaf, Leaves, Named, Read, Tcx,
};
use super::{names, Lowerer, Place};
use crate::context::TypeId;
use crate::ir::{
    ClassDecl, DiscriminatedUnionDecl, FunctionDecl, InterfaceClassification, InterfaceDecl,
    Member, NumericEnumDecl, Param, SharedMemory, SliceModeHint, StringEnumDecl, Throws, TypeKind,
    TypeParam, TypeRef, VariableDecl,
};
use crate::parse::scope::ScopeId;

/// One arm of a parameter.
#[derive(Clone, Debug)]
enum Arm {
    Value(abi::Type),
    Const(abi::Literal),
    Undefined,
}

/// One TypeScript signature to expand.
pub(crate) struct Sig<'s> {
    pub params: &'s [Param],
    pub scope: ScopeId,
}

/// One expanded overload's parameters and JS argument list.
struct Variant {
    params: Vec<abi::Param>,
    args: Vec<abi::Telem>,
    /// Whether `args` differs from the parameters in order.
    shaped: bool,
    slots: Vec<Slot>,
}

/// A source position of an overload's JS argument list, for naming it
/// when its key collides.
#[derive(Clone, Debug)]
pub(crate) enum Slot {
    Param {
        name: String,
        ty: abi::Type,
        rest: bool,
        /// The string literal the TS parameter type passes as a type
        /// argument (`KVNamespaceGetOptions<"json">`), which the IR type
        /// erases but which tells overloads apart.
        tag: Option<String>,
    },
    Const {
        name: String,
        value: abi::Literal,
    },
}

/// A member, with the argument list it was expanded from.
#[derive(Clone, Debug)]
pub(crate) struct Overload {
    pub member: abi::Member,
    slots: Vec<Slot>,
}

impl Overload {
    /// A member whose arguments are its parameters.
    pub(crate) fn plain(member: abi::Member) -> Overload {
        let slots = member
            .params
            .iter()
            .map(|p| Slot::Param {
                name: p.name.clone(),
                ty: p.ty.clone(),
                rest: false,
                tag: None,
            })
            .collect();
        Overload { member, slots }
    }
}

/// What a member is, for building it from variants.
#[derive(Clone)]
pub(crate) enum Form {
    Method(String),
    Ctor,
    Set(String),
}

impl Lowerer<'_> {
    fn this_type(
        &self,
        id: TypeId,
        tparams: &[abi::TypeParam],
        place: &Place,
    ) -> Option<abi::Type> {
        let info = self.info.get(&id)?;
        Some(abi::Type::Named(abi::TypeRef {
            path: self.path_to(&info.place, &info.name, place),
            args: tparams
                .iter()
                .map(|p| abi::Type::named([p.name.as_str()]))
                .collect(),
        }))
    }

    fn decl_meta(&self, id: TypeId, doc: Option<String>, js: &str) -> abi::Meta {
        let name = &self.info[&id].name;
        abi::Meta {
            doc,
            attrs: abi::Attrs {
                js: names::rename(name, js).into_iter().collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub(crate) fn class(&mut self, id: TypeId, c: &ClassDecl, doc: Option<String>) -> abi::Class {
        let place = self.info[&id].place.clone();
        let ir = self.info[&id].name.clone();
        let tparams = self.tparams(
            &c.type_params,
            c.body_scope,
            &place,
            Some(&c.js_name),
            Some(&ir),
        );
        let mut cx = Tcx::owned(c.body_scope, &place, &c.js_name);
        cx.subst = self.defaults(&c.type_params, &cx);
        cx.this = self.this_type(id, &tparams, &place);
        let extends = c
            .extends
            .iter()
            .filter_map(|h| self.parent(&h.type_ref, &cx.at(h.scope)))
            .collect();
        let members = c
            .members
            .iter()
            .filter(|m| !(c.is_abstract && matches!(m, Member::Constructor(_))))
            .collect::<Vec<_>>();
        let mut out = Vec::new();
        for m in members {
            self.member(m, &cx, &mut out);
        }
        for m in self.symbol_members(id, &c.js_name) {
            self.member(&m, &cx, &mut out);
        }
        symbol_keys(&mut out);
        resolve_conflicts(&mut out, &mut self.diagnostics);
        let out = out.into_iter().map(|o| o.member).collect();
        abi::Class {
            assoc: Vec::new(),
            meta: self.decl_meta(id, doc, &c.js_name),
            kind: abi::ClassKind::Class,
            export: false,
            name: self.info[&id].name.clone(),
            tparams,
            extends,
            members: out,
        }
    }

    /// A provided class: `export class`, its members exports (§17).
    pub(crate) fn export_class(&mut self, c: &ClassDecl, doc: Option<String>) -> abi::Class {
        let place = Place::default();
        if !c.type_params.is_empty() {
            self.warn(format!(
                "provides class {}: generic classes are not supported; its type parameters read as `Any`",
                c.name
            ));
        }
        let mut cx = Tcx::owned(c.body_scope, &place, &c.js_name);
        cx.this = Some(abi::Type::named([c.name.as_str()]));
        cx.export = true;
        let extends = c
            .extends
            .iter()
            .filter_map(|h| self.parent(&h.type_ref, &cx.at(h.scope)))
            .collect();
        let mut out = Vec::new();
        for m in &c.members {
            self.member(m, &cx, &mut out);
        }
        resolve_conflicts(&mut out, &mut self.diagnostics);
        let members: Vec<abi::Member> = out.into_iter().map(|o| o.member).collect();
        // An instance of a class with an imported base is constructed
        // through JS (`tw:new`), by a declared constructor. A base-less one
        // is constructed natively.
        if c.extends.is_some()
            && !members
                .iter()
                .any(|m| matches!(m.kind, abi::MemberKind::New(_)))
        {
            self.warn(format!(
                "provides class {}: no constructor, so the language can't construct one",
                c.name
            ));
        }
        abi::Class {
            assoc: Vec::new(),
            meta: abi::Meta {
                doc,
                ..Default::default()
            },
            kind: abi::ClassKind::Class,
            export: true,
            name: c.name.clone(),
            tparams: Vec::new(),
            extends,
            members,
        }
    }

    /// The symbol-keyed methods of the blocks that declare `id`, in
    /// declaration order.
    fn symbol_members(&self, id: TypeId, js_name: &str) -> Vec<Member> {
        let mut found: Vec<(ScopeId, &Member)> = self
            .gctx
            .scopes
            .symbol_members(js_name)
            .filter(|(s, _)| self.gctx.resolve_path(*s, js_name) == Some(id))
            .collect();
        found.sort_by_key(|(s, _)| s.0);
        found.into_iter().map(|(_, m)| m.clone()).collect()
    }

    pub(crate) fn interface(
        &mut self,
        id: TypeId,
        i: &InterfaceDecl,
        doc: Option<String>,
    ) -> abi::Class {
        let place = self.info[&id].place.clone();
        let ir = self.info[&id].name.clone();
        let tparams = self.tparams(
            &i.type_params,
            i.body_scope,
            &place,
            Some(&i.js_name),
            Some(&ir),
        );
        let mut cx = Tcx::owned(i.body_scope, &place, &i.js_name);
        cx.subst = self.defaults(&i.type_params, &cx);
        cx.this = self.this_type(id, &tparams, &place);
        let extends = i
            .extends
            .iter()
            .filter_map(|h| self.parent(&h.type_ref, &cx.at(h.scope)))
            .collect();
        let mut out = Vec::new();
        if self.is_dictionary(id, 0) {
            out.push(Overload::plain(object_ctor()));
        }
        for m in &i.members {
            self.member(m, &cx, &mut out);
        }
        for m in self.symbol_members(id, &i.js_name) {
            self.member(&m, &cx, &mut out);
        }
        symbol_keys(&mut out);
        resolve_conflicts(&mut out, &mut self.diagnostics);
        let out = out.into_iter().map(|o| o.member).collect();
        abi::Class {
            assoc: Vec::new(),
            meta: self.decl_meta(id, doc, &i.js_name),
            kind: abi::ClassKind::Interface,
            export: false,
            name: self.info[&id].name.clone(),
            tparams,
            extends,
            members: out,
        }
    }

    /// Whether an interface is a dictionary: properties alone, or an empty
    /// body over dictionary parents.
    fn is_dictionary(&self, id: TypeId, depth: usize) -> bool {
        let TypeKind::Interface(i) = &self.gctx.get_type(id).kind else {
            return false;
        };
        match i.classification {
            InterfaceClassification::Dictionary => true,
            _ if !i.members.is_empty() || i.extends.is_empty() || depth > 16 => false,
            _ => i.extends.iter().all(|h| {
                let TypeRef::Reference { segments, .. } = &h.type_ref else {
                    return false;
                };
                let cx = Tcx::new(h.scope, &Place::default(), "");
                self.resolve_id(segments, &cx)
                    .is_some_and(|p| self.is_dictionary(p, depth + 1))
            }),
        }
    }

    /// A discriminated union reads as one dictionary over every branch's
    /// properties.
    pub(crate) fn discriminated_union(
        &mut self,
        id: TypeId,
        d: &DiscriminatedUnionDecl,
        doc: Option<String>,
    ) -> abi::Class {
        let place = self.info[&id].place.clone();
        let ir = self.info[&id].name.clone();
        let tparams = self.tparams(
            &d.type_params,
            d.body_scope,
            &place,
            Some(&d.js_name),
            Some(&ir),
        );
        let mut cx = Tcx::owned(d.body_scope, &place, &d.js_name);
        cx.this = self.this_type(id, &tparams, &place);
        let mut out = vec![Overload::plain(object_ctor())];
        for m in &d.members {
            self.member(m, &cx, &mut out);
        }
        resolve_conflicts(&mut out, &mut self.diagnostics);
        let out = out.into_iter().map(|o| o.member).collect();
        abi::Class {
            assoc: Vec::new(),
            meta: self.decl_meta(id, doc, &d.js_name),
            kind: abi::ClassKind::Interface,
            export: false,
            name: self.info[&id].name.clone(),
            tparams,
            extends: Vec::new(),
            members: out,
        }
    }

    pub(crate) fn string_enum(
        &mut self,
        id: TypeId,
        e: &StringEnumDecl,
        doc: Option<String>,
    ) -> abi::Enum {
        let mut arms: Vec<abi::Arm> = Vec::new();
        let mut first: HashMap<&str, String> = HashMap::new();
        for v in &e.variants {
            let name = names::unique(&v.rust_name, |n| arms.iter().any(|a| a.name == n));
            let (js, value) = match first.get(v.js_value.as_str()) {
                Some(other) => (Vec::new(), Some(abi::ArmValue::Alias(other.clone()))),
                None => {
                    first.insert(&v.js_value, name.clone());
                    (vec![abi::JsEntry::Literal(v.js_value.clone())], None)
                }
            };
            arms.push(abi::Arm {
                meta: abi::Meta {
                    attrs: abi::Attrs {
                        js,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                name,
                payload: Vec::new(),
                value,
            });
        }
        abi::Enum {
            meta: self.decl_meta(id, doc, &e.name),
            name: self.info[&id].name.clone(),
            carrier: None,
            extends: Vec::new(),
            arms,
            members: Vec::new(),
        }
    }

    pub(crate) fn numeric_enum(
        &mut self,
        id: TypeId,
        e: &NumericEnumDecl,
        doc: Option<String>,
    ) -> abi::Enum {
        let values: Vec<i64> = e.variants.iter().map(|v| v.value).collect();
        let carrier = int_carrier(&values);
        // A repeated value is an alias of the first arm with it.
        let mut first: HashMap<i64, &str> = HashMap::new();
        let arms = e
            .variants
            .iter()
            .map(|v| abi::Arm {
                meta: abi::Meta {
                    doc: names::doc(&v.doc),
                    ..Default::default()
                },
                name: v.js_name.clone(),
                payload: Vec::new(),
                value: Some(match first.get(&v.value) {
                    Some(name) => abi::ArmValue::Alias(name.to_string()),
                    None => {
                        first.insert(v.value, &v.js_name);
                        abi::ArmValue::Int(v.value)
                    }
                }),
            })
            .collect();
        abi::Enum {
            meta: self.decl_meta(id, doc, &e.name),
            name: self.info[&id].name.clone(),
            carrier: Some(carrier),
            extends: Vec::new(),
            arms,
            members: Vec::new(),
        }
    }

    /// Type parameters, less those the parse erased to a primitive. A
    /// subtype constraint on a class or interface is the bound.
    ///
    /// A parameter bounded by string literals stays generic, bounded `in`
    /// the enums it's instantiated at, its bound's enum first (#199).
    /// Config bounds (`Owner.Param`) replace the bound.
    fn tparams(
        &mut self,
        ps: &[TypeParam],
        scope: ScopeId,
        place: &Place,
        key: Option<&str>,
        decl: Option<&str>,
    ) -> Vec<abi::TypeParam> {
        let cx = Tcx::new(scope, place, "");
        let kept: Vec<&TypeParam> = ps.iter().filter(|p| !self.dropped(p, scope)).collect();
        let mut out = Vec::new();
        for (k, p) in kept.into_iter().enumerate() {
            let bound = p.constraint.as_ref().and_then(|c| {
                let cx = cx.anchored(&p.name);
                match self.type_arg(c, true, &cx) {
                    t if self.literal_set(&t, place).is_some() => {
                        if let Some(ir) = decl {
                            self.lit_args
                                .insert((place.clone(), ir.to_string(), k), vec![t.clone()]);
                        }
                        Some(abi::Bound::In(vec![t]))
                    }
                    t if self.is_unit_enum(&t, place) => Some(abi::Bound::In(vec![t])),
                    // A payload enum's values aren't types to pick from.
                    t @ abi::Type::Named(_) if !self.is_enum(&t, place) => {
                        Some(abi::Bound::Subtype(t))
                    }
                    // `T extends unknown` is unbounded as written.
                    abi::Type::Nullable(t, _) if *t == abi::Type::Any && is_top(c) => None,
                    _ => {
                        let at = key.map_or_else(String::new, |k| format!("{k}."));
                        self.warn(format!(
                            "`{at}{} extends {}`: only a class, interface or enum bounds a type parameter, so it is unbounded",
                            p.name,
                            c.format_ts()
                        ));
                        None
                    }
                }
            });
            let configured = key.and_then(|js| {
                let key = format!("{js}.{}", p.name);
                let found = self.opts.bounds.iter().find(|(k, _)| *k == key);
                found.map(|(k, ts)| {
                    self.bounds_used.insert(k.clone());
                    ts.clone()
                })
            });
            let default = p
                .default
                .as_ref()
                .map(|d| boxed(self.type_arg(d, false, &cx.anchored(&p.name))));
            out.push(abi::TypeParam {
                name: p.name.clone(),
                bound: match configured {
                    Some(ts) => Some(abi::Bound::In(ts)),
                    None => bound,
                },
                default,
            });
        }
        out
    }

    fn parent(&mut self, t: &TypeRef, cx: &Tcx) -> Option<abi::TypeRef> {
        match self.ty(t, cx) {
            abi::Type::Named(r) => Some(r),
            _ => None,
        }
    }

    fn member(&mut self, m: &Member, cx: &Tcx, out: &mut Vec<Overload>) {
        match m {
            Member::Getter(g) => out.extend(
                self.getter(
                    &g.js_name,
                    &g.type_ref,
                    g.optional,
                    &g.throws,
                    names::doc(&g.doc),
                    false,
                    &cx.at(g.scope),
                )
                .map(Overload::plain),
            ),
            Member::StaticGetter(g) => out.extend(
                self.getter(
                    &g.js_name,
                    &g.type_ref,
                    g.optional,
                    &g.throws,
                    names::doc(&g.doc),
                    true,
                    &cx.at(g.scope),
                )
                .map(Overload::plain),
            ),
            Member::Setter(s) => out.extend(self.setter(
                &s.js_name,
                &s.type_ref,
                names::doc(&s.doc),
                false,
                &cx.at(s.scope),
            )),
            Member::StaticSetter(s) => out.extend(self.setter(
                &s.js_name,
                &s.type_ref,
                names::doc(&s.doc),
                true,
                &cx.at(s.scope),
            )),
            Member::Method(m) => {
                let cx = cx.at(m.body_scope).anchored(&m.js_name);
                out.extend(self.callable(
                    Form::Method(m.js_name.clone()),
                    false,
                    &m.type_params,
                    Sig {
                        params: &m.params,
                        scope: m.body_scope,
                    },
                    Some(&m.return_type),
                    &m.throws,
                    names::doc(&m.doc),
                    &cx,
                ))
            }
            Member::StaticMethod(m) => {
                let cx = cx.at(m.body_scope).anchored(&m.js_name);
                out.extend(self.callable(
                    Form::Method(m.js_name.clone()),
                    true,
                    &m.type_params,
                    Sig {
                        params: &m.params,
                        scope: m.body_scope,
                    },
                    Some(&m.return_type),
                    &m.throws,
                    names::doc(&m.doc),
                    &cx,
                ))
            }
            Member::Constructor(c) => {
                let mut cx = cx.at(c.scope).anchored("new");
                // `new <T>(…): Slot<T>` constructs the class at its own
                // parameters, in order: they're the class's.
                let own = match (&c.result, &cx.this) {
                    (
                        Some(TypeRef::Reference {
                            segments,
                            generic_args,
                        }),
                        Some(abi::Type::Named(this)),
                    ) if cx
                        .owner
                        .as_ref()
                        .is_some_and(|o| segments == std::slice::from_ref(o))
                        && generic_args.len() == c.type_params.len()
                        && this.args.len() == c.type_params.len()
                        && generic_args
                            .iter()
                            .zip(&c.type_params)
                            .all(|(a, p)| *a == TypeRef::ident(&p.name)) =>
                    {
                        let mut subst = (*cx.subst).clone();
                        for (p, t) in c.type_params.iter().zip(&this.args) {
                            subst.insert(p.name.clone(), t.clone());
                        }
                        cx.subst = std::rc::Rc::new(subst);
                        &[][..]
                    }
                    _ => &c.type_params[..],
                };
                out.extend(self.callable(
                    Form::Ctor,
                    false,
                    own,
                    Sig {
                        params: &c.params,
                        scope: c.scope,
                    },
                    None,
                    &c.throws,
                    names::doc(&c.doc),
                    &cx,
                ))
            }
            Member::IndexSignature(i) => {
                let cx = cx.at(i.scope).anchored("at");
                let key = match self.ty(&i.key_type, &cx) {
                    abi::Type::Str => abi::Type::Str,
                    abi::Type::Scalar(abi::Scalar::F64) => abi::Type::Scalar(abi::Scalar::U32),
                    _ => {
                        let owner = cx.owner.as_deref().unwrap_or("<anonymous>");
                        self.warn(format!(
                            "`{owner}[{}]`: an index signature keyed by neither string nor number is not lowered",
                            i.key_type.format_ts()
                        ));
                        return;
                    }
                };
                let (read, _) = self.result(&i.value_type, &cx, true);
                let write = (!i.readonly).then(|| self.inner(&i.value_type, &cx));
                out.extend(
                    index_members(key, read, write)
                        .into_iter()
                        .map(Overload::plain),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn getter(
        &mut self,
        js: &str,
        ty: &TypeRef,
        optional: bool,
        throws: &Throws,
        doc: Option<String>,
        is_static: bool,
        cx: &Tcx,
    ) -> Option<abi::Member> {
        let cx = cx.anchored(js);
        let (result, js_null) = self.result(ty, &cx, optional);
        let result = if js_null { with_null(result) } else { result };
        // `onmessage: never` is a property TS forbids.
        if result == abi::Type::Unit {
            let owner = cx.owner.as_ref().map_or(String::new(), |o| format!("{o}."));
            self.warn(format!(
                "`{owner}{js}: {}` holds no value, so it is not lowered",
                ty.format_ts()
            ));
            return None;
        }
        let name = names::escape_member(js);
        let js_entries: Vec<abi::JsEntry> = names::rename(&name, js).into_iter().collect();
        Some(abi::Member {
            meta: abi::Meta {
                doc,
                attrs: abi::Attrs {
                    js: js_entries,
                    ..Default::default()
                },
                ..Default::default()
            },
            export: false,
            is_static,
            kind: abi::MemberKind::Get(name),
            tparams: Vec::new(),
            params: Vec::new(),
            result,
            throws: match throws {
                Throws::Type(t) => Some(self.throws_enum(t, &cx)),
                _ => None,
            },
        })
    }

    /// A setter per value arm. `undefined` alone is expressed by not
    /// setting. An export receives either absence from JS, so its value
    /// arms are plain `T?`.
    fn setter(
        &mut self,
        js: &str,
        ty: &TypeRef,
        doc: Option<String>,
        is_static: bool,
        cx: &Tcx,
    ) -> Vec<Overload> {
        let cx = cx.anchored(js);
        let (mut arms, union) = self.arms(ty, &cx);
        if cx.export {
            arms = fold_absent(arms, true);
        }
        let mut out = Vec::new();
        for arm in arms {
            let mut v = Variant {
                params: Vec::new(),
                args: Vec::new(),
                shaped: false,
                slots: Vec::new(),
            };
            match arm {
                Arm::Value(t) => {
                    v.push_param("value", t, None);
                    v.params[0].union = union;
                }
                Arm::Const(lit) => v.push_const("value", lit),
                Arm::Undefined => continue,
            }
            out.push(build(
                &Form::Set(js.into()),
                is_static,
                v,
                Vec::new(),
                abi::Type::Unit,
                false,
                None,
                if out.is_empty() { doc.clone() } else { None },
            ));
        }
        // A setter of a constant is named for it, conflict or not: a bare
        // `set p()` would read as the property's own setter.
        if out.iter().any(|o| o.member.params.is_empty()) {
            let all: Vec<usize> = (0..out.len()).collect();
            rename_group(&mut out, &all);
        }
        out
    }

    pub(crate) fn function(
        &mut self,
        f: &FunctionDecl,
        place: &Place,
        doc: Option<String>,
    ) -> Vec<Overload> {
        let mut out = Vec::new();
        let cx = Tcx::new(f.body_scope, place, &f.js_name);
        out.extend(self.callable(
            Form::Method(f.js_name.clone()),
            false,
            &f.type_params,
            Sig {
                params: &f.params,
                scope: f.body_scope,
            },
            Some(&f.return_type),
            &f.throws,
            doc,
            &cx,
        ));
        for o in &f.overloads {
            let cx = Tcx::new(o.body_scope, place, &f.js_name);
            out.extend(self.callable(
                Form::Method(f.js_name.clone()),
                false,
                &o.type_params,
                Sig {
                    params: &o.params,
                    scope: o.body_scope,
                },
                Some(&o.return_type),
                &o.throws,
                names::doc(&o.doc),
                &cx,
            ));
        }
        resolve_conflicts(&mut out, &mut self.diagnostics);
        out
    }

    /// A global value is a live getter, and a setter unless it's `const`.
    pub(crate) fn variable(
        &mut self,
        v: &VariableDecl,
        scope: ScopeId,
        place: &Place,
        doc: Option<String>,
        is_static: bool,
    ) -> Vec<Overload> {
        if !v.constructs.is_empty() {
            match self.constructs(v, place, &doc) {
                Some(ctors) => {
                    for (target, o) in ctors {
                        self.folded.entry(target).or_default().push(o);
                    }
                    return Vec::new();
                }
                None => self.warn(format!(
                    "`{}`: construct signatures returning other than a class or interface are not lowered",
                    v.js_name
                )),
            }
        }
        let cx = Tcx::new(scope, place, &v.js_name);
        let mut out: Vec<Overload> = self
            .getter(
                &v.js_name,
                &v.type_ref,
                false,
                &Throws::None,
                doc,
                is_static,
                &cx,
            )
            .map(Overload::plain)
            .into_iter()
            .collect();
        if !v.is_const {
            out.extend(self.setter(&v.js_name, &v.type_ref, None, is_static, &cx));
        }
        out
    }

    /// A constructor-shaped value's construct signatures, as `@js(new = ...)`
    /// constructors of the classes and interfaces they return, by where
    /// those are declared (IR.md §6); `None` when one returns another type.
    fn constructs(
        &mut self,
        v: &VariableDecl,
        place: &Place,
        doc: &Option<String>,
    ) -> Option<Vec<((Place, String), Overload)>> {
        let mut out = Vec::new();
        for f in &v.constructs {
            let TypeRef::Reference {
                segments,
                generic_args,
            } = &f.return_type
            else {
                return None;
            };
            let id = self.resolve_id(segments, &Tcx::new(f.body_scope, place, &v.js_name))?;
            let decl = self.gctx.get_type(id);
            let (js, tps, tscope) = match &decl.kind {
                TypeKind::Class(c) => (c.js_name.clone(), c.type_params.clone(), c.body_scope),
                TypeKind::Interface(i) => (i.js_name.clone(), i.type_params.clone(), i.body_scope),
                _ => return None,
            };
            let info = self.info.get(&id)?;
            let (at, name) = (info.place.clone(), info.name.clone());
            let args: Vec<abi::Type> = tps
                .iter()
                .filter(|p| !self.dropped(p, tscope))
                .map(|p| abi::Type::named([p.name.as_str()]))
                .collect();
            let mut cx = Tcx::owned(f.body_scope, &at, &js).anchored("new");
            // `new <T>(value: T): Stub<T>` constructs the type at its own
            // parameters, in order, as a class constructor does.
            let own = if generic_args.len() == f.type_params.len()
                && args.len() == f.type_params.len()
                && generic_args
                    .iter()
                    .zip(&f.type_params)
                    .all(|(a, p)| *a == TypeRef::ident(&p.name))
            {
                let mut subst = (*cx.subst).clone();
                for (p, t) in f.type_params.iter().zip(&args) {
                    subst.insert(p.name.clone(), t.clone());
                }
                cx.subst = std::rc::Rc::new(subst);
                &[][..]
            } else {
                &f.type_params[..]
            };
            cx.this = Some(abi::Type::Named(abi::TypeRef {
                path: self.path_to(&at, &name, &at),
                args,
            }));
            let base = match (&place.module, &at.module) {
                (a, b) if a == b => abi::Base::Lexical,
                (None, _) => abi::Base::Root,
                (Some(spec), _) => abi::Base::Module(spec.clone()),
            };
            let segments: Vec<String> = place
                .ns
                .iter()
                .cloned()
                .chain([v.js_name.clone()])
                .collect();
            if let Some(spec) = &place.module {
                self.new_values.insert((spec.clone(), segments[0].clone()));
            }
            let value = abi::JsEntry::New(abi::Path { base, segments });
            let ctors = self.callable(
                Form::Ctor,
                false,
                own,
                Sig {
                    params: &f.params,
                    scope: f.body_scope,
                },
                None,
                &f.throws,
                doc.clone(),
                &cx,
            );
            out.extend(ctors.into_iter().map(|mut o| {
                o.member.meta.attrs.js.push(value.clone());
                ((at.clone(), name.clone()), o)
            }));
        }
        Some(out)
    }

    /// The payload enum a union of values (not only literals) reads as,
    /// when one can hold every member, and whether it's an import's
    /// `union` parameter (`finish` drops the flag where the bridge can't
    /// tell the arms apart). A named union is an enum, an export's when
    /// told apart, since it comes from JS. An import's anonymous one is
    /// when told apart and its typed arrays don't pair with copies, but not
    /// in a lib, whose enum names every input against it would lose.
    fn union_enum(
        &mut self,
        l: &Leaves,
        named: Option<&Named>,
        cx: &Tcx,
    ) -> Option<(abi::Type, bool)> {
        let kinds = l.values.len()
            + usize::from(!l.strs.is_empty())
            + usize::from(!l.nums.is_empty())
            + usize::from(!l.bools.is_empty());
        if l.values.is_empty() || kinds < 2 {
            return None;
        }
        let union = !cx.export;
        let named = named.filter(|n| n.id.is_some());
        let fits = match named {
            Some(_) => !cx.export || self.discriminable(l, cx),
            None => {
                union
                    && self.opts.lib.is_none()
                    && pair_typed_arrays(l.values.clone()) == l.values
                    && self.discriminable(l, cx)
            }
        };
        if !fits {
            return None;
        }
        let whole = Leaves {
            null: false,
            undefined: false,
            ..l.clone()
        };
        let (t, _) = self.join(whole, cx, Read::Inner, named.cloned());
        self.is_enum(&t, &cx.place).then_some((t, union))
    }

    /// The arms of a written value: each value type, `null` as a constant,
    /// literals as constants or one enum, and whether its value arm is a
    /// `union` parameter.
    fn arms(&mut self, ty: &TypeRef, cx: &Tcx) -> (Vec<Arm>, bool) {
        if let Some(t) = self.assoc_type(ty, cx) {
            let (null, undefined) = super::assoc::absent(ty);
            return (whole(t, null, undefined, self.opts.strict_nullable), false);
        }
        let mut raw = Vec::new();
        self.leaves(ty, cx, &mut raw);
        let raw: Vec<Leaf> = raw.into_iter().map(|(l, _)| l).collect();
        let mut l = Leaves::of(raw);
        let named = match ty {
            TypeRef::Reference {
                segments,
                generic_args,
            } => self.union_alias_name(segments, generic_args, cx),
            _ => None,
        };
        // `any | null` writes `null` as `@js(null) Any?`, as any `T | null`
        // does (IR.md §9); `any` alone takes either absence as `Any?`.
        if l.top && l.null {
            return (whole(abi::Type::Any, true, true, true), false);
        }
        if l.top {
            return (
                vec![Arm::Value(abi::Type::Nullable(
                    Box::new(abi::Type::Any),
                    false,
                ))],
                false,
            );
        }
        // `BodyInit | null` names its enum too.
        let alias = match ty {
            TypeRef::Union(u) => self.nullable_alias_name(u.members(), cx),
            _ => named.clone(),
        };
        if let Some((t, union)) = self.union_enum(&l, alias.as_ref(), cx) {
            return (
                whole(t, l.null, l.undefined, self.opts.strict_nullable),
                union,
            );
        }
        // `Body | null` reads the alias's enum as its one leaf.
        let union = alias.is_some()
            && !cx.export
            && l.strs.is_empty()
            && l.nums.is_empty()
            && l.bools.is_empty()
            && matches!(l.values.as_slice(), [v] if self.is_enum(v, &cx.place));
        let mut arms = Vec::new();
        if l.open {
            let strs = std::mem::take(&mut l.strs);
            let named = named
                .clone()
                .filter(|_| l.values.is_empty() && l.nums.is_empty());
            arms.push(Arm::Value(self.payload_enum(&strs, &[], true, cx, named)));
        }
        if let Some(t) = named.as_ref().and_then(|n| self.named_literal(n, &l, cx)) {
            arms.push(Arm::Value(t));
            l.values.clear();
            l.strs.clear();
        }
        match l.strs.len() {
            0 => {}
            1 => arms.push(Arm::Const(abi::Literal::Str(l.strs[0].clone()))),
            _ => {
                let strs = std::mem::take(&mut l.strs);
                let named = named
                    .clone()
                    .filter(|_| l.values.is_empty() && l.nums.is_empty());
                arms.push(Arm::Value(self.literal_enum(&strs, cx, named)));
            }
        }
        match l.nums.as_slice() {
            [] => {}
            [n] if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 => {
                arms.push(Arm::Const(abi::Literal::Int(*n as i64)))
            }
            nums => match int_literals(nums) {
                Some(ints) if l.values.is_empty() && arms.is_empty() => {
                    arms.push(Arm::Value(self.int_enum(&ints, cx, named)))
                }
                _ => {
                    if !l.values.contains(&abi::Type::Scalar(abi::Scalar::F64)) {
                        l.values.push(abi::Type::Scalar(abi::Scalar::F64));
                    }
                }
            },
        }
        for b in &l.bools {
            arms.push(Arm::Const(abi::Literal::Bool(*b)));
        }
        let values: Vec<Arm> = pair_typed_arrays(l.values)
            .into_iter()
            .map(Arm::Value)
            .collect();
        // Values first, so the plain arm keeps the plain name.
        let mut out = values;
        out.extend(arms);
        (
            absent_arms(out, l.null, l.undefined, self.opts.strict_nullable),
            union,
        )
    }

    /// Expand one signature into its IR overloads.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn callable(
        &mut self,
        form: Form,
        is_static: bool,
        tparams: &[TypeParam],
        sig: Sig<'_>,
        ret: Option<&TypeRef>,
        throws: &Throws,
        doc: Option<String>,
        cx: &Tcx,
    ) -> Vec<Overload> {
        let place = cx.place.clone();
        // The IR has no constructor type parameters: they read as their
        // default, else a class or interface they're bounded by, else `Any?`.
        let ctor_cx;
        let (own_tparams, cx) = if matches!(form, Form::Ctor) && !tparams.is_empty() {
            let any = abi::Type::Nullable(Box::new(abi::Type::Any), false);
            let mut subst = (*cx.subst).clone();
            for p in tparams {
                subst.insert(p.name.clone(), any.clone());
            }
            for p in tparams {
                let pcx = Tcx {
                    subst: std::rc::Rc::new(subst.clone()),
                    ..cx.clone()
                };
                let bound = p.constraint.as_ref().map(|c| self.ty(c, &pcx));
                let t = match (&p.default, bound) {
                    (Some(d), _) => self.inner(d, &pcx),
                    (None, Some(b @ abi::Type::Named(_))) if !self.is_enum(&b, &cx.place) => b,
                    _ => any.clone(),
                };
                self.warn(format!(
                    "{}: the constructor's own type parameter `{}` reads as `{}`",
                    cx.location(),
                    p.name,
                    print_type(&t)
                ));
                subst.insert(p.name.clone(), t);
            }
            ctor_cx = Tcx {
                subst: std::rc::Rc::new(subst),
                ..cx.clone()
            };
            (Vec::new(), &ctor_cx)
        } else {
            let key = match (&cx.owner, &form) {
                (Some(o), Form::Method(n)) => format!("{o}.{n}"),
                (None, Form::Method(n)) => n.clone(),
                _ => String::new(),
            };
            // A type parameter that types the rest parameter is the rest's
            // tuple, which the IR reads as its array bound.
            let rest = rest_tparam(tparams, sig.params);
            let cx = match rest {
                Some(p) => {
                    let mut subst = (*cx.subst).clone();
                    let bound = p.constraint.as_ref().expect("an array bound");
                    let t = match self.ty(bound, cx) {
                        t @ abi::Type::Array(_) => t,
                        _ => abi::Type::Array(Box::new(abi::Type::Nullable(
                            Box::new(abi::Type::Any),
                            false,
                        ))),
                    };
                    subst.insert(p.name.clone(), t);
                    ctor_cx = Tcx {
                        subst: std::rc::Rc::new(subst),
                        ..cx.clone()
                    };
                    &ctor_cx
                }
                None => cx,
            };
            let kept: Vec<TypeParam> = tparams
                .iter()
                .filter(|p| rest.is_none_or(|r| r.name != p.name))
                .cloned()
                .collect();
            (self.tparams(&kept, sig.scope, &place, Some(&key), None), cx)
        };
        let (mut result, js_null) = match ret {
            Some(TypeRef::Never) => (abi::Type::Never, false),
            Some(r) => self.result(r, cx, false),
            None => (abi::Type::Unit, false),
        };
        let specific = match throws {
            Throws::Type(t) => Some(self.throws_enum(t, cx)),
            _ => None,
        };
        // A `never` return always throws, whatever `@throws` says. A
        // promise's `@throws` names its rejections.
        let throws = match (throws, &mut result) {
            _ if matches!(ret, Some(TypeRef::Never)) => {
                Some(specific.unwrap_or_else(abi::Path::err))
            }
            (Throws::Never, _) => None,
            (_, abi::Type::Future(_, e)) => {
                if let Some(s) = specific {
                    *e = s;
                }
                None
            }
            _ => Some(specific.unwrap_or_else(abi::Path::err)),
        };
        let js_name = match &form {
            Form::Method(n) | Form::Set(n) => n.as_str(),
            Form::Ctor => "constructor",
        };
        let once = self.once_params(cx.owner.as_deref(), js_name);
        let cancel = match result {
            abi::Type::Future(..) => self.cancel_param(&sig, cx),
            _ => None,
        };
        let variants = self.expand(&sig, cx);
        variants
            .into_iter()
            .map(|mut v| {
                for p in &mut v.params {
                    p.once = once.contains(&p.name) && self.is_callback(&p.ty, &cx.place);
                }
                let mut lets = Vec::new();
                if let Some((param, controller)) = &cancel {
                    lets = cancellable(&mut v, param, controller);
                }
                let mut o = build(
                    &form,
                    is_static,
                    v,
                    own_tparams.clone(),
                    result.clone(),
                    js_null,
                    throws.clone(),
                    doc.clone(),
                );
                o.member.meta.attrs.js.extend(lets);
                o
            })
            .collect()
    }

    /// The parameters config marks `@once` on `owner.member` (JS names).
    fn once_params(&self, owner: Option<&str>, member: &str) -> Vec<String> {
        let path = match owner {
            Some(o) => format!("{o}.{member}."),
            None => format!("{member}."),
        };
        self.opts
            .once
            .iter()
            .filter_map(|spec| spec.strip_prefix(&path))
            .filter(|p| !p.contains('.'))
            .map(str::to_string)
            .collect()
    }

    /// The named throws enum of `@throws {T}`, so a typed member never
    /// reads as bare `throws` (IR.md §15). Its classes are the arms, a
    /// subclass before its parents, shared by every use of the same set.
    /// It extends `err` unless it names `TypeError` or `RangeError`, which
    /// `err` folds into one arm: then it's an arm per class and its own
    /// `@fallback Error(Str)`.
    fn throws_enum(&mut self, t: &TypeRef, cx: &Tcx) -> abi::Path {
        let builtin = |t: &TypeRef| match t {
            TypeRef::Reference { segments, .. } => match segments.last().map(String::as_str) {
                Some(n @ ("Error" | "TypeError" | "RangeError")) => Some(n.to_string()),
                _ => None,
            },
            _ => None,
        };
        let members = match t {
            TypeRef::Union(u) => u.members().to_vec(),
            t => vec![t.clone()],
        };
        // A builtin isn't read, which would stub it: `@js` names the global.
        let mut classes: Vec<abi::Path> = Vec::new();
        let mut raw = Vec::new();
        for m in &members {
            match builtin(m) {
                Some(n) => classes.push(abi::Path::new([n.as_str()])),
                None => self.leaves(m, cx, &mut raw),
            }
        }
        for (leaf, _) in raw {
            if let Leaf::Ty(abi::Type::Named(r)) = leaf {
                if !classes.contains(&r.path) {
                    classes.push(r.path);
                }
            }
        }
        classes.dedup();
        let is = |c: &abi::Path, n: &str| c.segments.len() == 1 && c.segments[0] == n;
        let own = classes
            .iter()
            .any(|c| is(c, "TypeError") || is(c, "RangeError"));
        let mut arms: Vec<abi::Path> = classes
            .iter()
            .filter(|c| !is(c, "Error"))
            .cloned()
            .collect();
        self.subclasses_first(&mut arms, cx);
        let mut printed: Vec<String> = arms.iter().map(|c| c.segments.join(".")).collect();
        printed.sort();
        let key = format!("throws:{:?}:{printed:?}:{own}", cx.place);
        if let Some(abi::Type::Named(r)) = self.synth_ref(&key, &cx.place) {
            return r.path;
        }
        let stems: Vec<String> = arms
            .iter()
            .map(|c| {
                let last = c.segments.last().unwrap();
                names::pascal(last.strip_suffix("Error").unwrap_or(last))
            })
            .collect();
        // `@throws {Error}` alone is `ErrorErr`, still apart from untyped.
        let name = match stems.is_empty() {
            false => format!("{}Err", stems.join("Or")),
            true => "ErrorErr".to_string(),
        };
        let mut out: Vec<abi::Arm> = Vec::new();
        for c in arms {
            let base = names::pascal(c.segments.last().unwrap());
            let arm = names::unique(&base, |n| {
                (!own && matches!(n, "TypeError"))
                    || n == "Error"
                    || out.iter().any(|a| a.name == n)
            });
            out.push(abi::Arm {
                meta: abi::Meta {
                    attrs: abi::Attrs {
                        js: vec![abi::JsEntry::Class(vec![abi::ClassPred {
                            class: c,
                            props: Vec::new(),
                        }])],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                name: arm,
                payload: vec![abi::Type::StrBox],
                value: None,
            });
        }
        let extends = match own {
            true => {
                out.push(abi::Arm {
                    meta: abi::Meta {
                        attrs: abi::Attrs {
                            fallback: true,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    name: "Error".into(),
                    payload: vec![abi::Type::StrBox],
                    value: None,
                });
                Vec::new()
            }
            false => vec![abi::Path::err()],
        };
        let named = Named {
            name,
            place: cx.place.clone(),
            id: None,
        };
        match self.synth_enum_with(key, out, extends, cx, named) {
            abi::Type::Named(r) => r.path,
            _ => abi::Path::err(),
        }
    }

    /// Order error classes so each comes before any class it extends: a
    /// subclass's arm must be tested first.
    fn subclasses_first(&self, classes: &mut Vec<abi::Path>, cx: &Tcx) {
        let mut out: Vec<abi::Path> = Vec::new();
        for c in std::mem::take(classes) {
            let at = out
                .iter()
                .position(|o| self.extends_class(&c, o, cx))
                .unwrap_or(out.len());
            out.insert(at, c);
        }
        *classes = out;
    }

    /// Whether the class `sub` extends `sup`, through the TS declarations.
    fn extends_class(&self, sub: &abi::Path, sup: &abi::Path, cx: &Tcx) -> bool {
        let mut at = self.resolve_id(&sub.segments, cx);
        for _ in 0..32 {
            let Some(id) = at else { return false };
            let TypeKind::Class(c) = &self.gctx.get_type(id).kind else {
                return false;
            };
            let Some(TypeRef::Reference { segments, .. }) = c.extends.as_ref().map(|h| &h.type_ref)
            else {
                return false;
            };
            if segments.last() == sup.segments.last() {
                return true;
            }
            at = self.resolve_id(segments, &cx.at(c.extends.as_ref().unwrap().scope));
        }
        false
    }

    /// The parameter whose dictionary takes an `AbortSignal` as `signal`,
    /// and the path of `AbortController`, when both are declared: a dropped
    /// future then aborts the call (IR.md §16).
    fn cancel_param(&mut self, sig: &Sig<'_>, cx: &Tcx) -> Option<(String, abi::Path)> {
        if !self.opts.cancel {
            return None;
        }
        let controller_ref = TypeRef::Reference {
            segments: vec!["AbortController".into()],
            generic_args: Vec::new(),
        };
        if !self.declares_controller(cx) {
            return None;
        }
        let abi::Type::Named(controller) = self.ty(&controller_ref, cx) else {
            return None;
        };
        let controller = controller.path;
        let names = self.param_names(sig.params, cx);
        for (p, name) in sig.params.iter().zip(names) {
            let ty = p
                .type_ref
                .split_empties()
                .map_or_else(|| p.type_ref.clone(), |(t, _)| t);
            let TypeRef::Reference { segments, .. } = &ty else {
                continue;
            };
            let Some(id) = self.resolve_id(segments, &cx.at(sig.scope)) else {
                continue;
            };
            let TypeKind::Interface(i) = &self.gctx.get_type(id).kind else {
                continue;
            };
            let has_signal = i.members.iter().any(|m| {
                matches!(m, Member::Getter(g) if g.js_name == "signal"
                    && g.type_ref.format_ts().contains("AbortSignal"))
            });
            if has_signal {
                return Some((name, controller));
            }
        }
        None
    }

    /// Whether the input declares an `AbortController` with the
    /// constructor, `signal` and `abort` that cancellation calls.
    fn declares_controller(&self, cx: &Tcx) -> bool {
        let Some(id) = self.resolve_id(&["AbortController".to_string()], cx) else {
            return false;
        };
        let TypeKind::Class(c) = &self.gctx.get_type(id).kind else {
            return false;
        };
        let has = |f: &dyn Fn(&Member) -> bool| c.members.iter().any(f);
        has(&|m| matches!(m, Member::Constructor(k) if k.params.is_empty()))
            && has(&|m| matches!(m, Member::Getter(g) if g.js_name == "signal"))
            && has(&|m| matches!(m, Member::Method(f) if f.js_name == "abort"))
    }

    /// Parameter names as TypeScript spells them, which generators make
    /// their own. Only a repeat, which the IR can't hold, is renamed.
    fn param_names(&mut self, params: &[Param], cx: &Tcx) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in params {
            let name = p.js_name.clone().unwrap_or_else(|| p.name.clone());
            let unique = names::unique(&name, |n| out.contains(&n.to_string()));
            if unique != name {
                self.warn(format!(
                    "{}: parameter `{name}` repeats an earlier one's name, so it's `{unique}`",
                    cx.location()
                ));
            }
            out.push(unique);
        }
        out
    }

    fn expand(&mut self, sig: &Sig<'_>, cx: &Tcx) -> Vec<Variant> {
        let params = sig.params;
        let (fixed, rest) = match params.split_last() {
            Some((last, init)) if last.variadic => (init, Some(last)),
            _ => (params, None),
        };
        let pnames = self.param_names(params, cx);
        let tags: Vec<Option<String>> = params.iter().map(|p| type_arg_tag(&p.type_ref)).collect();
        let unshared: Vec<bool> = params.iter().map(|p| rejects_shared(&p.type_ref)).collect();
        let mut never = None;
        let mut unions = Vec::new();
        let arms: Vec<(Vec<Arm>, bool)> = fixed
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let pcx = cx.anchored(&p.name);
                let mut raw = Vec::new();
                self.leaves(&p.type_ref, &pcx, &mut raw);
                if raw.is_empty() {
                    never.get_or_insert(i);
                }
                let (mut arms, union) = self.arms(&p.type_ref, &pcx);
                unions.push(union);
                // An export receives JS `null` as its own absence.
                if cx.export {
                    arms = fold_absent(arms, false);
                } else if p.slice_mode == Some(SliceModeHint::View) {
                    arms = with_views(arms);
                }
                let admits_undefined = arms.iter().any(|a| matches!(a, Arm::Undefined));
                (arms, admits_undefined)
            })
            .collect();
        // No argument is a `never`, so nothing can call the overload.
        if let Some(i) = never {
            self.warn(format!(
                "{}: parameter `{}` is `never`, so the overload is uncallable and isn't lowered",
                cx.location(),
                pnames[i]
            ));
            return Vec::new();
        }
        // A parameter is omittable when optional, or when it admits
        // `undefined` and every parameter after it is omittable.
        let mut omittable = vec![false; fixed.len()];
        let mut trailing = true;
        for i in (0..fixed.len()).rev() {
            omittable[i] = fixed[i].optional || (trailing && arms[i].1);
            trailing &= omittable[i];
        }
        let first_cut = (0..=fixed.len())
            .find(|&k| omittable[k..].iter().all(|&o| o))
            .unwrap_or(fixed.len());
        let rest_ty = rest.map(|r| {
            let rcx = cx.anchored(&r.name);
            match self.ty(&r.type_ref, &rcx) {
                t @ abi::Type::Array(_) => t,
                t => abi::Type::Array(Box::new(t)),
            }
        });
        let mut out = Vec::new();
        for cut in first_cut..=fixed.len() {
            // A rest argument follows every fixed one, so it rides only on
            // the full cut: omitting a parameter before it would shift it.
            let spread = rest.is_some() && cut == fixed.len();
            let mut choices: Vec<Vec<Arm>> = Vec::new();
            for i in 0..cut {
                let followed = i + 1 < cut || spread;
                let mut a: Vec<Arm> = arms[i]
                    .0
                    .iter()
                    .filter(|a| !matches!(a, Arm::Undefined))
                    .cloned()
                    .collect();
                // An absent argument the call still has to pass rides on the
                // first value arm as `T?`, or is passed as `undefined`.
                let absent = arms[i].1 || fixed[i].optional;
                if absent && (followed || !omittable[i]) {
                    match a.iter_mut().find(|a| matches!(a, Arm::Value(_))) {
                        Some(Arm::Value(t)) => match t {
                            // `x?: T | null` is three states under
                            // `--strict-nullable`, else one absence.
                            abi::Type::Nullable(v, true) if !self.opts.strict_nullable => {
                                let v = std::mem::replace(&mut **v, abi::Type::Unit);
                                *t = abi::Type::Nullable(Box::new(v), false);
                            }
                            abi::Type::Nullable(v, true) => {
                                let v = std::mem::replace(&mut **v, abi::Type::Unit);
                                *t = abi::Type::Nullable(
                                    Box::new(abi::Type::Nullable(Box::new(v), false)),
                                    false,
                                );
                            }
                            abi::Type::Nullable(..) => {}
                            _ => *t = abi::Type::Nullable(Box::new(t.clone()), false),
                        },
                        _ => a.push(Arm::Undefined),
                    }
                } else if absent && omittable[i] && !cx.export {
                    // Omitting the argument is its `undefined`, so a value
                    // arm keeps only the `null` that's left: none by
                    // default, as for `T | undefined`, and `@js(null) T?`
                    // under `--strict-nullable`.
                    if let Some(Arm::Value(t)) = a.iter_mut().find(|a| matches!(a, Arm::Value(_))) {
                        *t = match std::mem::replace(t, abi::Type::Unit) {
                            abi::Type::Nullable(v, n) if *v == abi::Type::Any => {
                                abi::Type::Nullable(v, n)
                            }
                            abi::Type::Nullable(v, _) => {
                                let v = match *v {
                                    abi::Type::Nullable(v, _) => *v,
                                    v => v,
                                };
                                if self.opts.strict_nullable {
                                    abi::Type::Nullable(Box::new(v), true)
                                } else {
                                    v
                                }
                            }
                            v => v,
                        };
                    }
                }
                if a.is_empty() {
                    a.push(Arm::Undefined);
                }
                choices.push(a);
            }
            for combo in product(&choices) {
                let mut v = Variant {
                    params: Vec::new(),
                    args: Vec::new(),
                    shaped: false,
                    slots: Vec::new(),
                };
                for (i, arm) in combo.into_iter().enumerate() {
                    let name = &pnames[i];
                    match arm {
                        Arm::Value(t) => {
                            let view = matches!(t, abi::Type::View { .. });
                            v.push_param(name, t, tags[i].clone());
                            let p = v.params.last_mut().unwrap();
                            p.unshared = view && unshared[i];
                            p.union = unions[i];
                        }
                        Arm::Const(lit) => v.push_const(name, lit),
                        Arm::Undefined => {
                            v.args.push(abi::Telem::Undefined);
                            v.shaped = true;
                        }
                    }
                }
                if let (true, Some(t)) = (spread, &rest_ty) {
                    let name = &pnames[fixed.len()];
                    v.params.push(param(name, t.clone()));
                    v.args.push(abi::Telem::Spread(name.clone()));
                    v.slots.push(Slot::Param {
                        name: name.clone(),
                        ty: t.clone(),
                        rest: true,
                        tag: None,
                    });
                    v.shaped = true;
                }
                out.push(v);
            }
        }
        out
    }
}

impl Variant {
    fn push_param(&mut self, name: &str, ty: abi::Type, tag: Option<String>) {
        self.slots.push(Slot::Param {
            name: name.into(),
            ty: ty.clone(),
            rest: false,
            tag,
        });
        self.params.push(param(name, ty));
        self.args.push(abi::Telem::Param(name.into()));
    }

    fn push_const(&mut self, name: &str, value: abi::Literal) {
        self.args.push(abi::Telem::Lit(value.clone()));
        self.slots.push(Slot::Const {
            name: name.into(),
            value,
        });
        self.shaped = true;
    }
}

/// Whether `t` admits every value: `any`, `unknown`, or a union with one.
fn is_top(t: &TypeRef) -> bool {
    match t {
        TypeRef::Any | TypeRef::Unknown => true,
        TypeRef::Union(u) => u.members().iter().any(is_top),
        _ => false,
    }
}

fn product(choices: &[Vec<Arm>]) -> Vec<Vec<Arm>> {
    let mut out = vec![Vec::new()];
    for c in choices {
        out = out
            .into_iter()
            .flat_map(|prefix| {
                c.iter().map(move |a| {
                    let mut p = prefix.clone();
                    p.push(a.clone());
                    p
                })
            })
            .collect();
    }
    out
}

/// The type parameter the rest parameter is typed by, when bounded by an
/// array or tuple (`...args: Args` with `Args extends any[]`).
fn rest_tparam<'a>(tparams: &'a [TypeParam], params: &[Param]) -> Option<&'a TypeParam> {
    let rest = params.last().filter(|p| p.variadic)?;
    let TypeRef::Reference {
        segments,
        generic_args,
        ..
    } = &rest.type_ref
    else {
        return None;
    };
    tparams.iter().find(|p| {
        matches!(&segments[..], [n] if *n == p.name)
            && generic_args.is_empty()
            && matches!(p.constraint, Some(TypeRef::Array(_) | TypeRef::Tuple(_)))
    })
}

/// The arms of a value read whole as `t`, beside `null` and `undefined`.
fn whole(t: abi::Type, null: bool, undefined: bool, strict: bool) -> Vec<Arm> {
    absent_arms(vec![Arm::Value(t)], null, undefined, strict)
}

/// `null` rides on the first value arm, as `@js(null) T?`, or beside
/// `undefined` as `T??` under `--strict-nullable` and plain `T?` otherwise
/// (IR.md §9). Only without a value arm is it a constant.
fn absent_arms(mut arms: Vec<Arm>, null: bool, undefined: bool, strict: bool) -> Vec<Arm> {
    if null {
        match arms.iter_mut().find(|a| matches!(a, Arm::Value(_))) {
            Some(Arm::Value(t)) => {
                let v = match std::mem::replace(t, abi::Type::Unit) {
                    abi::Type::Nullable(v, _) => *v,
                    v => v,
                };
                *t = if undefined && !strict {
                    abi::Type::Nullable(Box::new(v), false)
                } else if undefined && v != abi::Type::Any {
                    abi::Type::Nullable(Box::new(abi::Type::Nullable(Box::new(v), false)), false)
                } else {
                    abi::Type::Nullable(Box::new(v), true)
                };
            }
            _ => arms.push(Arm::Const(abi::Literal::Null)),
        }
    }
    if undefined || arms.is_empty() {
        arms.push(Arm::Undefined);
    }
    arms
}

/// Members keyed by a well-known symbol (`[Symbol.iterator]`) take its
/// name, with `@js(symbol = ...)` and no `name` (§6). Other symbol keys
/// have no IR form.
fn symbol_keys(out: &mut Vec<Overload>) {
    out.retain_mut(|o| {
        let name = match &o.member.kind {
            abi::MemberKind::Method(n) | abi::MemberKind::Get(n) | abi::MemberKind::Set(n) => n,
            _ => return true,
        };
        let Some(symbol) = name
            .strip_prefix("[Symbol.")
            .and_then(|s| s.strip_suffix(']'))
            .map(str::to_string)
        else {
            return true;
        };
        if !ts_gen_abi::validate::WELL_KNOWN_SYMBOLS.contains(&symbol.as_str()) {
            return false;
        }
        match &mut o.member.kind {
            abi::MemberKind::Method(n) | abi::MemberKind::Get(n) | abi::MemberKind::Set(n) => {
                *n = symbol.clone()
            }
            abi::MemberKind::New(_) => unreachable!(),
        }
        o.member
            .meta
            .attrs
            .js
            .retain(|e| !matches!(e, abi::JsEntry::Name(_)));
        o.member.meta.attrs.js.push(abi::JsEntry::Symbol(symbol));
        true
    });
}

/// `@js(index)` access by `key`: `at`, and `setAt` unless read-only.
pub(crate) fn index_members(
    key: abi::Type,
    read: abi::Type,
    write: Option<abi::Type>,
) -> Vec<abi::Member> {
    let key_name = if key == abi::Type::Str { "key" } else { "i" };
    let member = |name: &str, params, result| abi::Member {
        meta: abi::Meta {
            attrs: abi::Attrs {
                js: vec![abi::JsEntry::Index],
                ..Default::default()
            },
            ..Default::default()
        },
        export: false,
        is_static: false,
        kind: abi::MemberKind::Method(name.into()),
        tparams: Vec::new(),
        params,
        result,
        throws: None,
    };
    let mut out = vec![member("at", vec![param(key_name, key.clone())], read)];
    if let Some(value) = write {
        out.push(member(
            "setAt",
            vec![param(key_name, key), param("value", value)],
            abi::Type::Unit,
        ));
    }
    out
}

/// The value arms made nullable in place of a `null` arm, and of an
/// `undefined` one too when `undefined` is set.
fn fold_absent(arms: Vec<Arm>, undefined: bool) -> Vec<Arm> {
    let absent = |a: &Arm| match a {
        Arm::Const(abi::Literal::Null) => true,
        Arm::Undefined => undefined,
        _ => false,
    };
    // JS → Wasm, `null` and `undefined` are both absence.
    let arms: Vec<Arm> = arms
        .into_iter()
        .map(|a| match a {
            Arm::Value(abi::Type::Nullable(v, _)) => Arm::Value(match *v {
                abi::Type::Nullable(v, _) => abi::Type::Nullable(v, false),
                v => abi::Type::Nullable(Box::new(v), false),
            }),
            a => a,
        })
        .collect();
    let has_value = arms.iter().any(|a| matches!(a, Arm::Value(_)));
    if !has_value || !arms.iter().any(absent) {
        return arms;
    }
    arms.into_iter()
        .filter(|a| !absent(a))
        .map(|a| match a {
            Arm::Value(t) => Arm::Value(nullable(t)),
            other => other,
        })
        .collect()
}

/// `{@slice view}`: a zero-copy `*T[]` beside each `T[]` copy (IR.md §11).
fn with_views(arms: Vec<Arm>) -> Vec<Arm> {
    let mut out = Vec::new();
    for a in arms {
        let view = match &a {
            Arm::Value(abi::Type::Vec(e)) => Some(abi::Type::View {
                mutable: false,
                elem: e.clone(),
            }),
            _ => None,
        };
        out.push(a);
        out.extend(view.map(Arm::Value));
    }
    out
}

/// A typed array over an `ArrayBuffer` (`Uint8Array<ArrayBuffer>`), which
/// rejects a view of shared memory.
fn rejects_shared(t: &TypeRef) -> bool {
    match t {
        TypeRef::Union(u) => u.members().iter().any(rejects_shared),
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            segments.len() == 1
                && crate::ir::TypedArrayElement::of_typed_array(&segments[0]).is_some()
                && SharedMemory::of_typed_array_args(generic_args) == SharedMemory::Rejected
        }
        _ => false,
    }
}

fn nullable(t: abi::Type) -> abi::Type {
    match t {
        t @ abi::Type::Nullable(_, _) => t,
        t => abi::Type::Nullable(Box::new(t), false),
    }
}

/// A result whose absence crosses to JS as `null`: its `T?`, or its
/// `future<T?>`'s. A `T??` already has both.
pub(super) fn with_null(t: abi::Type) -> abi::Type {
    match t {
        abi::Type::Nullable(t, _) if !matches!(*t, abi::Type::Nullable(..)) => {
            abi::Type::Nullable(t, true)
        }
        abi::Type::Future(v, e) => abi::Type::Future(Box::new(with_null(*v)), e),
        t => t,
    }
}

fn param(name: &str, ty: abi::Type) -> abi::Param {
    abi::Param {
        name: name.into(),
        ty,
        once: false,
        unshared: false,
        union: false,
        span: abi::Span::default(),
    }
}

impl Lowerer<'_> {
    fn is_callback(&self, t: &abi::Type, from: &Place) -> bool {
        match t {
            abi::Type::Nullable(t, _) => self.is_callback(t, from),
            t => self.is_fn(t, from),
        }
    }
}

/// Make `v` cancellable through `param`'s `signal`: a fresh controller
/// lives with the future, its signal fills the dictionary, and dropping
/// the future aborts it. Returns the `let` and `cancel` entries; a variant
/// without `param` is left alone.
fn cancellable(v: &mut Variant, param: &str, controller: &abi::Path) -> Vec<abi::JsEntry> {
    let Some(p) = v.params.iter_mut().find(|p| p.name == param) else {
        return Vec::new();
    };
    // A fill creates the dictionary when it's absent.
    if !matches!(p.ty, abi::Type::Nullable(_, _)) {
        p.ty = abi::Type::Nullable(Box::new(p.ty.clone()), false);
    }
    if let Some(Slot::Param { ty, .. }) = v
        .slots
        .iter_mut()
        .find(|s| matches!(s, Slot::Param { name, .. } if name == param))
    {
        if !matches!(ty, abi::Type::Nullable(_, _)) {
            *ty = abi::Type::Nullable(Box::new(ty.clone()), false);
        }
    }
    let ctl = names::unique("ctl", |n| v.params.iter().any(|p| p.name == n));
    if !v.shaped {
        v.args = v
            .params
            .iter()
            .map(|p| abi::Telem::Param(p.name.clone()))
            .collect();
        v.shaped = true;
    }
    for t in &mut v.args {
        if matches!(t, abi::Telem::Param(p) if p == param) {
            *t = abi::Telem::Fill {
                param: param.to_string(),
                fields: vec![(
                    "signal".into(),
                    abi::Expr::Ref {
                        path: vec![ctl.clone(), "signal".into()],
                        call: None,
                    },
                )],
            };
        }
    }
    let mut new = controller.segments.clone();
    new.push("new".into());
    vec![
        abi::JsEntry::Let(
            ctl.clone(),
            abi::Expr::Ref {
                path: new,
                call: Some(Vec::new()),
            },
        ),
        abi::JsEntry::Cancel(abi::Expr::Ref {
            path: vec![ctl, "abort".into()],
            call: None,
        }),
    ]
}

/// `@js(object) new();` for a dictionary.
fn object_ctor() -> abi::Member {
    abi::Member {
        meta: abi::Meta {
            attrs: abi::Attrs {
                js: vec![abi::JsEntry::Object],
                ..Default::default()
            },
            ..Default::default()
        },
        export: false,
        is_static: false,
        kind: abi::MemberKind::New(None),
        tparams: Vec::new(),
        params: Vec::new(),
        result: abi::Type::Unit,
        throws: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    form: &Form,
    is_static: bool,
    v: Variant,
    tparams: Vec<abi::TypeParam>,
    result: abi::Type,
    js_null: bool,
    throws: Option<abi::Path>,
    doc: Option<String>,
) -> Overload {
    let slots = v.slots;
    let mut js = Vec::new();
    let kind = match form {
        Form::Method(n) => {
            let name = names::escape_member(n);
            js.extend(names::rename(&name, n));
            abi::MemberKind::Method(name)
        }
        Form::Ctor => abi::MemberKind::New(None),
        Form::Set(n) => {
            let name = names::escape_member(n);
            js.extend(names::rename(&name, n));
            abi::MemberKind::Set(name)
        }
    };
    if v.shaped {
        js.push(abi::JsEntry::Args(v.args));
    }
    let member = abi::Member {
        meta: abi::Meta {
            doc,
            attrs: abi::Attrs {
                js,
                ..Default::default()
            },
            ..Default::default()
        },
        export: false,
        is_static,
        kind,
        tparams,
        params: v.params,
        result: if js_null { with_null(result) } else { result },
        throws,
    };
    Overload { member, slots }
}

/// The member's key, less the declaration path: what must be unique
/// within one declaration.
fn local_key(m: &abi::Member) -> String {
    let (tag, name) = match &m.kind {
        abi::MemberKind::Method(n) => ("", n.as_str()),
        abi::MemberKind::Get(n) => ("get:", n.as_str()),
        abi::MemberKind::Set(n) => ("set:", n.as_str()),
        abi::MemberKind::New(n) => ("new:", n.as_deref().unwrap_or("")),
    };
    let params: Vec<String> = m.params.iter().map(|p| print_type(&p.ty)).collect();
    format!(
        "{}{tag}{name}({})",
        if m.is_static { "." } else { "#" },
        params.join(",")
    )
}

fn js_name(m: &abi::Member) -> Option<String> {
    m.meta.attrs.js.iter().find_map(|e| match e {
        abi::JsEntry::Name(n) => Some(n.clone()),
        _ => None,
    })
}

/// The IR name, and the name the convention is applied to: a
/// constructor's is `new`.
fn base_name(m: &abi::Member) -> String {
    match &m.kind {
        abi::MemberKind::Method(n) | abi::MemberKind::Get(n) | abi::MemberKind::Set(n) => {
            js_name(m).unwrap_or_else(|| n.clone())
        }
        abi::MemberKind::New(_) => "new".into(),
    }
}

fn rename_member(m: &mut abi::Member, to: &str) {
    let original = base_name(m);
    match &mut m.kind {
        abi::MemberKind::Method(n) | abi::MemberKind::Get(n) | abi::MemberKind::Set(n) => {
            *n = names::escape_member(to);
            m.meta
                .attrs
                .js
                .retain(|e| !matches!(e, abi::JsEntry::Name(_)));
            let symbol = m
                .meta
                .attrs
                .js
                .iter()
                .any(|e| matches!(e, abi::JsEntry::Symbol(_)));
            if *n != original && !symbol {
                m.meta.attrs.js.insert(0, abi::JsEntry::Name(original));
            }
        }
        // `newWithRequest` is `new withRequest(...)`; the JS call is still
        // `new C(...)`.
        abi::MemberKind::New(n) => {
            *n = match to.strip_prefix("new") {
                Some("") | None => None,
                Some(rest) => Some(names::camel(rest)),
            }
        }
    }
}

/// Name every overload of `group` (indices into `overloads`, the first
/// being the primary) by the constant-expansion convention.
fn rename_group(overloads: &mut [Overload], group: &[usize]) {
    let base = base_name(&overloads[group[0]].member);
    // A tagged parameter names its overload like a constant.
    let tags: Vec<Vec<Option<abi::Literal>>> = group
        .iter()
        .map(|&i| {
            overloads[i]
                .slots
                .iter()
                .map(|s| match s {
                    Slot::Param { tag: Some(t), .. } => Some(abi::Literal::Str(t.clone())),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let sigs: Vec<Vec<naming::Slot<'_>>> = group
        .iter()
        .zip(&tags)
        .map(|(&i, tags)| {
            overloads[i]
                .slots
                .iter()
                .zip(tags)
                .flat_map(|(s, tag)| match (s, tag) {
                    (Slot::Param { name, .. }, Some(value)) => {
                        vec![naming::Slot::Const { name, value }]
                    }
                    (Slot::Param { name, ty, rest, .. }, None) => vec![naming::Slot::Param {
                        name,
                        ty,
                        rest: *rest,
                    }],
                    (Slot::Const { name, value }, _) => vec![naming::Slot::Const { name, value }],
                })
                .collect()
        })
        .collect();
    let names = naming::variant_names(&base, &sigs);
    for (&i, name) in group.iter().zip(names) {
        rename_member(&mut overloads[i].member, &name);
    }
}

/// A type constructed through more than one source, its own constructors
/// or a value's, names the later sources' constructors after their values
/// (`@js(new = Wrap2) new wrap2(...)`).
pub(crate) fn name_constructs(overloads: &mut [Overload]) {
    let mut first = None;
    for o in overloads {
        let m = &mut o.member;
        let abi::MemberKind::New(name) = &mut m.kind else {
            continue;
        };
        let source = m.meta.attrs.js.new_value().cloned();
        match (&first, &source) {
            (None, _) => first = Some(source),
            (Some(f), Some(p)) if *f != source && name.is_none() => {
                *name = Some(names::escape_member(&names::camel(
                    p.segments.last().unwrap(),
                )));
            }
            _ => {}
        }
    }
}

/// Rename overloads whose keys collide, only on conflict (IR.md §7).
/// Identical overloads are one import. Each set of colliding overloads is
/// named by the constant-expansion convention; anything a rename makes
/// collide again is numbered.
pub(crate) fn resolve_conflicts(overloads: &mut Vec<Overload>, diagnostics: &mut Vec<String>) {
    let mut seen: Vec<Overload> = Vec::new();
    for o in std::mem::take(overloads) {
        if !seen.iter().any(|s| same_overload(&s.member, &o.member)) {
            seen.push(o);
        }
    }
    // TypeScript resolves a call to its first matching overload, so a later
    // one making the same call, differing only in its result, is
    // unreachable: an omitted trailing options object, say.
    let mut reachable: Vec<Overload> = Vec::new();
    for o in seen {
        if !reachable.iter().any(|r| same_call(r, &o)) {
            reachable.push(o);
        }
    }
    *overloads = reachable;
    let keys: Vec<String> = overloads.iter().map(|o| local_key(&o.member)).collect();
    let mut done = vec![false; overloads.len()];
    for i in 0..overloads.len() {
        if done[i] {
            continue;
        }
        let group: Vec<usize> = (i..overloads.len())
            .filter(|&j| keys[j] == keys[i])
            .collect();
        for &j in &group {
            done[j] = true;
        }
        if group.len() > 1 {
            rename_group(overloads, &group);
        }
    }
    let mut taken: Vec<String> = Vec::new();
    for o in overloads.iter_mut() {
        let key = local_key(&o.member);
        if taken.contains(&key) {
            let base = base_name(&o.member);
            let to = names::unique(&base, |n| {
                let mut probe = o.member.clone();
                rename_member(&mut probe, n);
                taken.contains(&local_key(&probe))
            });
            diagnostics.push(format!(
                "overloads of `{base}` collide; one is renamed `{to}`"
            ));
            rename_member(&mut o.member, &to);
        }
        taken.push(local_key(&o.member));
    }
}

/// Overloads making the same JS call: one key, one argument template and
/// the same tags.
fn same_call(a: &Overload, b: &Overload) -> bool {
    let args = |m: &abi::Member| {
        m.meta
            .attrs
            .js
            .iter()
            .filter(|e| matches!(e, abi::JsEntry::Args(_)))
            .cloned()
            .collect::<Vec<_>>()
    };
    let tags = |o: &Overload| {
        o.slots
            .iter()
            .map(|s| match s {
                Slot::Param { tag, .. } => tag.clone(),
                Slot::Const { .. } => None,
            })
            .collect::<Vec<_>>()
    };
    let symbol = |m: &abi::Member| {
        m.meta
            .attrs
            .js
            .iter()
            .find(|e| matches!(e, abi::JsEntry::Symbol(_)))
            .cloned()
    };
    local_key(&a.member) == local_key(&b.member)
        && js_name(&a.member) == js_name(&b.member)
        && symbol(&a.member) == symbol(&b.member)
        && args(&a.member) == args(&b.member)
        && tags(a) == tags(b)
}

/// The string literals a parameter type passes as type arguments, which
/// tell otherwise-equal overloads apart.
fn type_arg_tag(t: &TypeRef) -> Option<String> {
    let t = t.split_empties().map_or_else(|| t.clone(), |(t, _)| t);
    let TypeRef::Reference { generic_args, .. } = &t else {
        return None;
    };
    let lits: Vec<&str> = generic_args
        .iter()
        .filter_map(|a| match a {
            TypeRef::StringLiteral(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    (!lits.is_empty()).then(|| lits.join("-"))
}

/// Overloads that are the same import: equal but for the doc.
fn same_overload(a: &abi::Member, b: &abi::Member) -> bool {
    let strip = |m: &abi::Member| {
        let mut m = m.clone();
        m.meta.doc = None;
        m
    };
    strip(a) == strip(b)
}
