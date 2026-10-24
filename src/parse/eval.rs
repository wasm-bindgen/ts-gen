//! Evaluate type-level TypeScript once every file is parsed.
//!
//! `keyof`, indexed access, mapped types and the object and union utilities
//! (`Partial`, `Pick`, `Omit`, `Exclude`, …) describe a type in terms of
//! others, so they have no lowering of their own: a generator computes them
//! or erases them. Computing one needs every declaration it reaches
//! populated, merged and cross-file ones included, so this runs over the IR
//! after population, like `enums`, and rewrites each type in place:
//!
//! * a string-literal result named by a reference (`Exclude<Unit, "hour">`,
//!   `PluralizeUnit<TimeUnit>`) becomes a string enum of that spelling;
//! * an object result (`Partial<Options>`, a mapped type) becomes a
//!   dictionary interface;
//! * a utility or generic alias in an interface's `extends`, which has no
//!   parent to `Deref` to, flattens into the interface's own members;
//! * an alias whose target is one of these is promoted to the result under
//!   its own name.
//!
//! A generic alias is instantiated only when its target needs evaluation;
//! otherwise `Foo<Bar>` stays a Rust generic alias. A type parameter met
//! outside the scope of the type being rewritten is read at its bound, so
//! `PluralizeUnit<Units>` inside `RoundingOptions<Units extends DateUnit |
//! TimeUnit>` is every unit: Rust can't vary an enum per instantiation.
//! Anything that can't be evaluated keeps the lowering it had before, with
//! a warning.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::context::{GlobalContext, TypeId};
use crate::ir::{
    GetterMember, InterfaceDecl, MappedType, Member, Modifier, Module, Param, SetterMember,
    StringEnumDecl, StringEnumVariant, Throws, TypeAliasDecl, TypeDeclaration, TypeKind, TypeParam,
    TypeRef,
};
use crate::parse::classify::classify_interface;
use crate::parse::scope::{Binding, ScopeId};
use crate::parse::types::simplify_intersection;
use crate::util::diagnostics::DiagnosticCollector;
use crate::util::naming::{dedup_names, to_enum_variant, to_pascal_case, LocalScope, Suffix};

pub(crate) fn evaluate_types(gctx: &mut GlobalContext, module: &mut Module) {
    let listed: HashSet<TypeId> = module
        .types
        .iter()
        .copied()
        .chain(gctx.listed_types())
        .filter(|id| !gctx.builtin_type_ids.contains(id))
        .collect();
    // Aliases first, so one promoted to an enum or dictionary claims its own
    // name before an instantiation elsewhere evaluates to the same type.
    let mut ids: Vec<TypeId> = listed.iter().copied().collect();
    ids.sort_by_key(|id| {
        (
            !matches!(gctx.get_type(*id).kind, TypeKind::TypeAlias(_)),
            id.index(),
        )
    });

    let mut params_of: HashMap<ScopeId, Vec<TypeParam>> = HashMap::new();
    let mut names = Vec::new();
    for (_, decl) in gctx.iter_types() {
        names.extend(super::binding_name(&decl.kind));
        let (scope, params): (ScopeId, &[TypeParam]) = match &decl.kind {
            TypeKind::Class(c) => (c.body_scope, &c.type_params),
            TypeKind::Interface(i) => (i.body_scope, &i.type_params),
            TypeKind::DiscriminatedUnion(d) => (d.body_scope, &d.type_params),
            TypeKind::TypeAlias(a) => (a.body_scope, &a.type_params),
            TypeKind::Function(f) => (f.body_scope, &f.type_params),
            _ => continue,
        };
        params_of.insert(scope, params.to_vec());
        let members = match &decl.kind {
            TypeKind::Class(c) => &c.members,
            TypeKind::Interface(i) => &i.members,
            _ => continue,
        };
        for m in members {
            match m {
                Member::Method(m) => {
                    params_of.insert(m.body_scope, m.type_params.clone());
                }
                Member::StaticMethod(m) => {
                    params_of.insert(m.body_scope, m.type_params.clone());
                }
                _ => {}
            }
        }
    }
    let namespace_of: HashMap<ScopeId, TypeId> = gctx
        .iter_types()
        .filter_map(|(id, decl)| match &decl.kind {
            TypeKind::Namespace(ns) => Some((ns.child_scope, id)),
            _ => None,
        })
        .collect();

    let diag = std::mem::take(&mut gctx.diagnostics);
    let (rewritten, synths, diag) = {
        let mut eval = Eval {
            gctx,
            params_of,
            needs: HashMap::new(),
            synths: Vec::new(),
            enum_names: HashMap::new(),
            dict_names: HashMap::new(),
            done: HashMap::new(),
            used: LocalScope::with_claimed(names),
            diag,
            depth: 0,
        };
        let mut rewritten = Vec::new();
        for id in ids {
            let decl = eval.gctx.get_type(id);
            if is_plain_alias(&decl.kind) {
                let kind = eval.evaluated(id).into_owned();
                rewritten.push((id, kind));
                continue;
            }
            let place = Placement {
                anchor: id,
                scope: decl.scope_id,
            };
            let mut kind = decl.kind.clone();
            if eval.rewrite_kind(&mut kind, place) {
                rewritten.push((id, kind));
            }
        }
        (rewritten, eval.synths, eval.diag)
    };
    gctx.diagnostics = diag;

    for (id, kind) in rewritten {
        gctx.get_type_mut(id).kind = kind;
    }
    for synth in synths {
        let anchor = gctx.get_type(synth.place.anchor);
        let module_context = anchor.module_context.clone();
        let anchor_name = super::binding_name(&anchor.kind);
        // A namespace's members are declared in its block scope and listed
        // under the scope its blocks share.
        let listing = super::binding_scope(gctx, anchor);
        let scope = synth.place.scope;
        let kind = match synth.kind {
            SynthKind::Enum(literals) => TypeKind::StringEnum(string_enum(&synth.name, &literals)),
            SynthKind::Dict { props, type_params } => {
                let body_scope = gctx.scopes.create_child(scope);
                for p in &type_params {
                    gctx.scopes
                        .get_mut(body_scope)
                        .insert_type_param(p.name.clone());
                }
                let members = props_to_members(&props, body_scope);
                TypeKind::Interface(InterfaceDecl {
                    classification: classify_interface(&members),
                    name: synth.name.clone(),
                    js_name: synth.name.clone(),
                    type_params,
                    extends: Vec::new(),
                    members,
                    construct: Vec::new(),
                    body_scope,
                })
            }
        };
        let id = gctx.insert_type(TypeDeclaration {
            kind,
            module_context,
            doc: None,
            scope_id: scope,
        });
        gctx.scopes.insert(scope, synth.name.clone(), id);
        gctx.declare(scope, &synth.name, id);
        let exports = gctx.scopes.get(scope).exports.unwrap_or(scope);
        if anchor_name.is_some_and(|name| gctx.exports.local_name(exports, &name).is_some()) {
            gctx.exports.insert(exports, &synth.name, &synth.name);
        }
        if let Some(&ns) = namespace_of.get(&listing) {
            if let TypeKind::Namespace(ns) = &mut gctx.get_type_mut(ns).kind {
                ns.declarations.push(id);
            }
        } else if !gctx.list_in_module_of(synth.place.anchor, id) {
            module.types.push(id);
        }
    }
}

/// The utilities with an evaluation. The rest of TypeScript's
/// (`ReturnType`, `Awaited`, …) approximate to their first argument at
/// parse time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Utility {
    Partial,
    Required,
    Readonly,
    Pick,
    Omit,
    Exclude,
    Extract,
    NonNullable,
}

impl Utility {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "Partial" => Self::Partial,
            "Required" => Self::Required,
            "Readonly" => Self::Readonly,
            "Pick" => Self::Pick,
            "Omit" => Self::Omit,
            "Exclude" => Self::Exclude,
            "Extract" => Self::Extract,
            "NonNullable" => Self::NonNullable,
            _ => return None,
        })
    }

    fn is_object(self) -> bool {
        matches!(
            self,
            Self::Partial | Self::Required | Self::Readonly | Self::Pick | Self::Omit
        )
    }
}

/// Where a synthesised declaration goes: beside the declaration being
/// rewritten, in its scope, so the rewritten type resolves it.
#[derive(Clone, Copy)]
struct Placement {
    anchor: TypeId,
    scope: ScopeId,
}

enum SynthKind {
    Enum(Vec<String>),
    /// A dictionary is generic over the type parameters its members
    /// capture from the use site.
    Dict {
        props: Vec<Prop>,
        type_params: Vec<TypeParam>,
    },
}

struct Synth {
    name: String,
    place: Placement,
    kind: SynthKind,
}

/// A type parameter bound to an argument, with the scope and environment
/// the argument is written in, since a `TypeRef` carries neither.
struct Bound {
    name: String,
    ty: TypeRef,
    scope: ScopeId,
    env: Env,
}

#[derive(Clone, Default)]
struct Env(Option<Rc<(Bound, Env)>>);

impl Env {
    fn bind(&self, name: String, ty: TypeRef, scope: ScopeId, env: Env) -> Env {
        Env(Some(Rc::new((
            Bound {
                name,
                ty,
                scope,
                env,
            },
            self.clone(),
        ))))
    }

    fn get(&self, name: &str) -> Option<&Bound> {
        let mut env = self;
        while let Some(node) = &env.0 {
            if node.0.name == name {
                return Some(&node.0);
            }
            env = &node.1;
        }
        None
    }
}

/// A property of an evaluated object type, typed in the destination scope.
#[derive(Clone, Debug, PartialEq)]
struct Prop {
    js_name: String,
    type_ref: TypeRef,
    optional: bool,
    readonly: bool,
    doc: Option<String>,
    /// A method: its name is a key, but it has no property type.
    method: bool,
}

struct Eval<'g> {
    gctx: &'g GlobalContext,
    /// Each body scope's type parameters, for a parameter's bound.
    params_of: HashMap<ScopeId, Vec<TypeParam>>,
    /// Whether an alias's target needs evaluation; `None` while computing.
    needs: HashMap<TypeId, Option<bool>>,
    synths: Vec<Synth>,
    enum_names: HashMap<(ScopeId, Vec<String>), String>,
    dict_names: HashMap<(ScopeId, DictKey), String>,
    /// Non-generic aliases rewritten so far. One is evaluated when first
    /// used, so a utility over an alias reads its result, not its target.
    done: HashMap<TypeId, TypeKind>,
    used: LocalScope,
    diag: DiagnosticCollector,
    depth: u32,
}

const MAX_DEPTH: u32 = 64;

impl<'g> Eval<'g> {
    /// The kind of `id`, a non-generic alias's rewritten on demand.
    fn evaluated(&mut self, id: TypeId) -> Cow<'g, TypeKind> {
        if let Some(kind) = self.done.get(&id) {
            return Cow::Owned(kind.clone());
        }
        let decl = self.gctx.get_type(id);
        if !is_plain_alias(&decl.kind) {
            return Cow::Borrowed(&decl.kind);
        }
        // A cycle reads the target as written.
        self.done.insert(id, decl.kind.clone());
        let place = Placement {
            anchor: id,
            scope: decl.scope_id,
        };
        let mut kind = decl.kind.clone();
        let depth = std::mem::replace(&mut self.depth, 0);
        self.rewrite_kind(&mut kind, place);
        self.depth = depth;
        self.done.insert(id, kind.clone());
        Cow::Owned(kind)
    }

    /// Rewrite every type in `kind`; whether anything changed.
    fn rewrite_kind(&mut self, kind: &mut TypeKind, place: Placement) -> bool {
        let before = format!("{kind:?}");
        match kind {
            TypeKind::TypeAlias(alias) => {
                if let Some(promoted) = self.rewrite_alias(alias, place) {
                    *kind = promoted;
                }
            }
            TypeKind::Interface(iface) => {
                let name = iface.name.clone();
                let body_scope = iface.body_scope;
                self.rewrite_type_params(&mut iface.type_params, body_scope, &name, place);
                self.rewrite_members(&mut iface.members, &name, place);
                for c in &mut iface.construct {
                    let scope = c.scope;
                    self.rewrite_params(&mut c.params, scope, &name, place);
                }
                self.flatten_heritage(iface, place);
            }
            TypeKind::Class(class) => {
                let name = class.name.clone();
                let body_scope = class.body_scope;
                self.rewrite_type_params(&mut class.type_params, body_scope, &name, place);
                self.rewrite_members(&mut class.members, &name, place);
            }
            TypeKind::DiscriminatedUnion(du) => {
                let name = du.name.clone();
                let body_scope = du.body_scope;
                self.rewrite_type_params(&mut du.type_params, body_scope, &name, place);
                self.rewrite_members(&mut du.members, &name, place);
                for branch in &mut du.branches {
                    self.rewrite_members(branch, &name, place);
                }
            }
            TypeKind::Function(f) => {
                let name = to_pascal_case(&f.js_name);
                let scope = f.body_scope;
                self.rewrite_type_params(&mut f.type_params, scope, &name, place);
                self.rewrite_params(&mut f.params, scope, &name, place);
                self.rewrite_type(&mut f.return_type, scope, &format!("{name}Return"), place);
                for o in &mut f.overloads {
                    let scope = o.body_scope;
                    self.rewrite_params(&mut o.params, scope, &name, place);
                    self.rewrite_type(&mut o.return_type, scope, &format!("{name}Return"), place);
                }
            }
            TypeKind::Variable(v) => {
                let name = to_pascal_case(&v.js_name);
                self.rewrite_type(&mut v.type_ref, place.scope, &name, place);
            }
            TypeKind::StringEnum(_)
            | TypeKind::NumericEnum(_)
            | TypeKind::TsEnum(_)
            | TypeKind::Namespace(_) => {}
        }
        format!("{kind:?}") != before
    }

    fn rewrite_type_params(
        &mut self,
        params: &mut [TypeParam],
        scope: ScopeId,
        parent: &str,
        place: Placement,
    ) {
        for p in params {
            let hint = format!("{parent}{}", p.name);
            if let Some(c) = &mut p.constraint {
                self.rewrite_type(c, scope, &hint, place);
            }
            if let Some(d) = &mut p.default {
                self.rewrite_type(d, scope, &hint, place);
            }
        }
    }

    fn rewrite_params(
        &mut self,
        params: &mut [Param],
        scope: ScopeId,
        parent: &str,
        place: Placement,
    ) {
        for p in params {
            let hint = format!("{parent}{}", to_pascal_case(&p.name));
            self.rewrite_type(&mut p.type_ref, scope, &hint, place);
        }
    }

    fn rewrite_members(&mut self, members: &mut [Member], parent: &str, place: Placement) {
        for m in members {
            match m {
                Member::Getter(g) => {
                    let hint = format!("{parent}{}", to_pascal_case(&g.js_name));
                    self.rewrite_type(&mut g.type_ref, g.scope, &hint, place);
                }
                Member::Setter(s) => {
                    let hint = format!("{parent}{}", to_pascal_case(&s.js_name));
                    self.rewrite_type(&mut s.type_ref, s.scope, &hint, place);
                }
                Member::StaticGetter(g) => {
                    let hint = format!("{parent}{}", to_pascal_case(&g.js_name));
                    self.rewrite_type(&mut g.type_ref, g.scope, &hint, place);
                }
                Member::StaticSetter(s) => {
                    let hint = format!("{parent}{}", to_pascal_case(&s.js_name));
                    self.rewrite_type(&mut s.type_ref, s.scope, &hint, place);
                }
                Member::Method(m) => {
                    let name = format!("{parent}{}", to_pascal_case(&m.js_name));
                    let scope = m.body_scope;
                    self.rewrite_type_params(&mut m.type_params, scope, &name, place);
                    self.rewrite_params(&mut m.params, scope, &name, place);
                    self.rewrite_type(&mut m.return_type, scope, &format!("{name}Return"), place);
                }
                Member::StaticMethod(m) => {
                    let name = format!("{parent}{}", to_pascal_case(&m.js_name));
                    let scope = m.body_scope;
                    self.rewrite_type_params(&mut m.type_params, scope, &name, place);
                    self.rewrite_params(&mut m.params, scope, &name, place);
                    self.rewrite_type(&mut m.return_type, scope, &format!("{name}Return"), place);
                }
                Member::Constructor(c) => {
                    let scope = c.scope;
                    self.rewrite_params(&mut c.params, scope, parent, place);
                }
                Member::IndexSignature(ix) => {
                    let scope = ix.scope;
                    self.rewrite_type(&mut ix.key_type, scope, parent, place);
                    self.rewrite_type(&mut ix.value_type, scope, parent, place);
                }
            }
        }
    }

    /// Rewrite one type at a use site. The destination scope is the use
    /// site's own, and a result named by a reference is materialised under
    /// that reference's spelling.
    fn rewrite_type(&mut self, ty: &mut TypeRef, scope: ScopeId, hint: &str, place: Placement) {
        if !self.needs_eval(ty, scope) {
            return;
        }
        self.depth = 0;
        match self.eval_named(ty, scope, &Env::default(), scope, hint, place) {
            Some(result) => *ty = result,
            None => {
                let approx = fallback(ty);
                self.diag.warn(format!(
                    "`{}` could not be evaluated, approximating as `{}`",
                    ty.format_ts(),
                    approx.format_ts()
                ));
                *ty = approx;
            }
        }
    }

    /// Evaluate a type at a use site. Only a reference, `keyof` or indexed
    /// access (`undefined` aside) names its result, under its own spelling;
    /// an inline union of literals stays inline for codegen's own
    /// `<Field>Kind` synthesis.
    fn eval_named(
        &mut self,
        ty: &TypeRef,
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        hint: &str,
        place: Placement,
    ) -> Option<TypeRef> {
        let value = match ty.split_empties() {
            Some((value, _)) => value,
            None => ty.clone(),
        };
        let named = matches!(
            value,
            TypeRef::Reference { .. } | TypeRef::KeyOf(_) | TypeRef::IndexedAccess(..)
        );
        let hint = if named {
            spell(&value)
        } else {
            hint.to_string()
        };
        let result = self.eval(ty, scope, env, dest, &hint, place)?;
        Some(if named {
            self.materialize(result, &hint, dest, place)
        } else {
            result
        })
    }

    /// Rewrite an alias's target; an alias to an evaluated object or
    /// string-literal type is promoted to the declaration itself. A generic
    /// alias evaluates at its parameters' bounds, which is the one type its
    /// instantiations share, or else stays as written.
    fn rewrite_alias(&mut self, alias: &mut TypeAliasDecl, place: Placement) -> Option<TypeKind> {
        let scope = alias.body_scope;
        if !self.needs_eval(&alias.target, scope) {
            return None;
        }
        let mut env = Env::default();
        for p in &alias.type_params {
            let Some(bound) = &p.constraint else {
                alias.target = fallback(&alias.target);
                return None;
            };
            env = env.bind(p.name.clone(), bound.clone(), scope, Env::default());
        }
        self.depth = 0;
        if let Some(props) = self.shape(&alias.target, scope, &env, scope, place) {
            if props.iter().any(|p| p.method) {
                return None;
            }
            self.dict_names
                .insert((place.scope, dict_key(&props)), alias.name.clone());
            let members = props_to_members(&props, scope);
            return Some(TypeKind::Interface(InterfaceDecl {
                classification: classify_interface(&members),
                name: alias.name.clone(),
                js_name: alias.name.clone(),
                type_params: Vec::new(),
                extends: Vec::new(),
                members,
                construct: Vec::new(),
                body_scope: scope,
            }));
        }
        self.depth = 0;
        let Some(result) = self.eval(&alias.target, scope, &env, scope, &alias.name, place) else {
            // A generic alias evaluates at each instantiation; its own
            // declaration erasing is expected and codegen reports it.
            let approx = fallback(&alias.target);
            if alias.type_params.is_empty() {
                self.diag.warn(format!(
                    "`{}` could not be evaluated, approximating as `{}`",
                    alias.target.format_ts(),
                    approx.format_ts()
                ));
            }
            alias.target = approx;
            return None;
        };
        if let Some(literals) = self
            .string_literals(&result, scope)
            .filter(|literals| !literals.is_empty())
        {
            self.enum_names
                .insert((place.scope, literals.clone()), alias.name.clone());
            return Some(TypeKind::StringEnum(string_enum(&alias.name, &literals)));
        }
        if alias.type_params.is_empty() {
            alias.target = self.materialize(result, &alias.name.clone(), scope, place);
        }
        None
    }

    /// Replace the `extends` parents that can't be a wasm-bindgen parent, a
    /// utility or an instantiated generic alias, with their members.
    fn flatten_heritage(&mut self, iface: &mut InterfaceDecl, place: Placement) {
        let body_scope = iface.body_scope;
        let mut flattened: Vec<Prop> = Vec::new();
        let mut kept = Vec::new();
        for heritage in std::mem::take(&mut iface.extends) {
            if !self.flattens(&heritage.type_ref, heritage.scope) {
                kept.push(heritage);
                continue;
            }
            self.depth = 0;
            match self.shape(
                &heritage.type_ref,
                heritage.scope,
                &Env::default(),
                body_scope,
                place,
            ) {
                Some(props) => flattened.extend(props),
                None => {
                    self.diag.warn(format!(
                        "`{}` in the extends of `{}` could not be evaluated, ignored",
                        heritage.type_ref.format_ts(),
                        iface.name
                    ));
                }
            }
        }
        iface.extends = kept;
        if flattened.is_empty() {
            return;
        }
        let own: HashSet<String> = iface
            .members
            .iter()
            .filter_map(|m| match m {
                Member::Getter(g) => Some(g.js_name.clone()),
                Member::Setter(s) => Some(s.js_name.clone()),
                Member::Method(m) => Some(m.js_name.clone()),
                _ => None,
            })
            .collect();
        flattened.retain(|p| !own.contains(&p.js_name));
        if flattened.iter().any(|p| p.method) {
            self.diag.warn(format!(
                "methods inherited by `{}` through a utility type are dropped",
                iface.name
            ));
            flattened.retain(|p| !p.method);
        }
        iface
            .members
            .extend(props_to_members(&flattened, body_scope));
        iface.classification = classify_interface(&iface.members);
    }

    /// Whether an `extends` parent flattens: a utility, or a generic alias
    /// whose target needs evaluation. A non-generic alias is promoted in
    /// place and stays a parent.
    fn flattens(&mut self, ty: &TypeRef, scope: ScopeId) -> bool {
        let TypeRef::Reference { segments, .. } = ty else {
            return false;
        };
        if segments.len() != 1 {
            return false;
        }
        match self.gctx.scopes.resolve_binding(scope, &segments[0]) {
            None => Utility::parse(&segments[0]).is_some(),
            Some(Binding::Declared(id)) => match &self.gctx.get_type(id).kind {
                TypeKind::TypeAlias(a) => !a.type_params.is_empty() && self.alias_needs_eval(id),
                _ => false,
            },
            Some(Binding::TypeParam) => false,
        }
    }

    fn needs_eval(&mut self, ty: &TypeRef, scope: ScopeId) -> bool {
        match ty {
            TypeRef::KeyOf(_)
            | TypeRef::IndexedAccess(..)
            | TypeRef::Mapped(_)
            | TypeRef::ObjectLiteral(_) => true,
            TypeRef::Array(e) => self.needs_eval(e, scope),
            TypeRef::Union(u) => u.members().iter().any(|m| self.needs_eval(m, scope)),
            TypeRef::Intersection(v) | TypeRef::Tuple(v) => {
                v.iter().any(|m| self.needs_eval(m, scope))
            }
            TypeRef::Function(sig) => {
                sig.params
                    .iter()
                    .any(|p| self.needs_eval(&p.type_ref, scope))
                    || self.needs_eval(&sig.return_type, scope)
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => {
                if generic_args.iter().any(|a| self.needs_eval(a, scope)) {
                    return true;
                }
                if segments.len() != 1 {
                    return false;
                }
                match self.gctx.scopes.resolve_binding(scope, &segments[0]) {
                    None => Utility::parse(&segments[0]).is_some(),
                    Some(Binding::Declared(id)) => match &self.gctx.get_type(id).kind {
                        TypeKind::TypeAlias(a) => {
                            !a.type_params.is_empty() && self.alias_needs_eval(id)
                        }
                        _ => false,
                    },
                    Some(Binding::TypeParam) => false,
                }
            }
            _ => false,
        }
    }

    fn alias_needs_eval(&mut self, id: TypeId) -> bool {
        match self.needs.get(&id) {
            Some(Some(known)) => return *known,
            // A cycle through aliases is not a valid type.
            Some(None) => return false,
            None => {}
        }
        self.needs.insert(id, None);
        let needs = match &self.gctx.get_type(id).kind {
            TypeKind::TypeAlias(a) => {
                let target = a.target.clone();
                self.needs_eval(&target, a.body_scope)
            }
            _ => false,
        };
        self.needs.insert(id, Some(needs));
        needs
    }

    /// Evaluate `ty`, written in `scope` under `env`, to a type whose names
    /// resolve in `dest`. `None` when some part can't be evaluated.
    fn eval(
        &mut self,
        ty: &TypeRef,
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        hint: &str,
        place: Placement,
    ) -> Option<TypeRef> {
        if self.depth > MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let result = self.eval_inner(ty, scope, env, dest, hint, place);
        self.depth -= 1;
        result
    }

    fn eval_inner(
        &mut self,
        ty: &TypeRef,
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        hint: &str,
        place: Placement,
    ) -> Option<TypeRef> {
        Some(match ty {
            TypeRef::Array(e) => {
                TypeRef::Array(Box::new(self.eval(e, scope, env, dest, hint, place)?))
            }
            TypeRef::Union(u) => {
                let members = u
                    .members()
                    .iter()
                    .map(|m| self.eval(m, scope, env, dest, hint, place))
                    .collect::<Option<Vec<_>>>()?;
                self.literal_union(members, dest)
            }
            TypeRef::Intersection(v) => {
                let parts = v
                    .iter()
                    .map(|m| self.eval(m, scope, env, dest, hint, place))
                    .collect::<Option<Vec<_>>>()?;
                simplify_intersection(parts)
            }
            TypeRef::Tuple(v) => TypeRef::Tuple(
                v.iter()
                    .map(|m| self.eval(m, scope, env, dest, hint, place))
                    .collect::<Option<Vec<_>>>()?,
            ),
            TypeRef::Function(sig) => {
                let mut sig = sig.clone();
                for p in &mut sig.params {
                    p.type_ref = self.eval(&p.type_ref, scope, env, dest, hint, place)?;
                }
                sig.return_type =
                    Box::new(self.eval(&sig.return_type, scope, env, dest, hint, place)?);
                TypeRef::Function(sig)
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => return self.eval_reference(segments, generic_args, scope, env, dest, hint, place),
            TypeRef::KeyOf(t) => {
                let props = self.shape(t, scope, env, dest, place)?;
                TypeRef::union(props.into_iter().map(|p| TypeRef::StringLiteral(p.js_name)))
            }
            TypeRef::IndexedAccess(object, index) => {
                let keys = self.literal_set(index, scope, env, dest, place)?;
                let props = self.shape(object, scope, env, dest, place)?;
                let mut members = Vec::new();
                for key in keys {
                    let prop = props.iter().find(|p| p.js_name == key)?;
                    if prop.method {
                        return None;
                    }
                    members.push(prop.type_ref.clone());
                    if prop.optional {
                        members.push(TypeRef::Undefined);
                    }
                }
                TypeRef::union(members)
            }
            TypeRef::Mapped(m) => {
                let props = self.mapped_props(m, scope, env, dest, place)?;
                return self.synth_dict(props, hint, dest, place);
            }
            TypeRef::ObjectLiteral(_) => TypeRef::Object,
            other => other.clone(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_reference(
        &mut self,
        segments: &[String],
        generic_args: &[TypeRef],
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        hint: &str,
        place: Placement,
    ) -> Option<TypeRef> {
        let eval_args = |this: &mut Self| {
            generic_args
                .iter()
                .map(|a| this.eval(a, scope, env, dest, hint, place))
                .collect::<Option<Vec<_>>>()
        };
        if segments.len() != 1 {
            let path = segments.join(".");
            let from = self.gctx.resolve_path(scope, &path);
            if from.is_some() && from != self.gctx.resolve_path(dest, &path) {
                return None;
            }
            return Some(TypeRef::Reference {
                segments: segments.to_vec(),
                generic_args: eval_args(self)?,
            });
        }
        let name = &segments[0];
        if let Some(bound) = env.get(name) {
            let (ty, bound_scope, bound_env) = (bound.ty.clone(), bound.scope, bound.env.clone());
            return self.eval(&ty, bound_scope, &bound_env, dest, hint, place);
        }
        match self.gctx.scopes.resolve_binding(scope, name) {
            Some(Binding::TypeParam) => {
                let owner = self.param_owner(scope, name)?;
                if self.param_owner(dest, name) == Some(owner) {
                    return Some(TypeRef::ident(name));
                }
                let bound = self.param_bound(owner, name)?;
                self.eval(&bound, owner, &Env::default(), dest, hint, place)
            }
            Some(Binding::Declared(id)) => {
                if let TypeKind::TypeAlias(alias) = &self.gctx.get_type(id).kind {
                    if !alias.type_params.is_empty() && self.alias_needs_eval(id) {
                        let alias = alias.clone();
                        return self.instantiate(&alias, generic_args, scope, env, dest, place);
                    }
                }
                if scope != dest && self.gctx.scopes.resolve(dest, name) != Some(id) {
                    return None;
                }
                Some(TypeRef::Reference {
                    segments: segments.to_vec(),
                    generic_args: eval_args(self)?,
                })
            }
            None => match Utility::parse(name) {
                Some(utility) => {
                    self.eval_utility(utility, generic_args, scope, env, dest, hint, place)
                }
                None => Some(TypeRef::Reference {
                    segments: segments.to_vec(),
                    generic_args: eval_args(self)?,
                }),
            },
        }
    }

    /// Instantiate a generic alias whose target needs evaluation. An
    /// argument that is a type parameter out of scope at the destination is
    /// read at its bound.
    fn instantiate(
        &mut self,
        alias: &TypeAliasDecl,
        generic_args: &[TypeRef],
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        place: Placement,
    ) -> Option<TypeRef> {
        let mut bound_env = Env::default();
        let mut spelled = Vec::new();
        for (i, p) in alias.type_params.iter().enumerate() {
            let mut arg = match (generic_args.get(i), &p.default) {
                (Some(arg), _) => self.eval(arg, scope, env, dest, &alias.name, place)?,
                (None, Some(default)) => self.eval(
                    default,
                    alias.body_scope,
                    &bound_env,
                    dest,
                    &alias.name,
                    place,
                )?,
                (None, None) => return None,
            };
            if let Some(name) = arg.as_ident() {
                if self.gctx.scopes.resolve_binding(dest, name) == Some(Binding::TypeParam) {
                    let owner = self.param_owner(dest, name)?;
                    let bound = self.param_bound(owner, name)?;
                    arg = self.eval(&bound, owner, &Env::default(), dest, &alias.name, place)?;
                }
            }
            spelled.push(spell(&arg));
            bound_env = bound_env.bind(p.name.clone(), arg, dest, Env::default());
        }
        let hint = format!("{}{}", alias.name, spelled.concat());
        self.eval(
            &alias.target,
            alias.body_scope,
            &bound_env,
            dest,
            &hint,
            place,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_utility(
        &mut self,
        utility: Utility,
        args: &[TypeRef],
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        hint: &str,
        place: Placement,
    ) -> Option<TypeRef> {
        let first = args.first()?;
        if utility.is_object() {
            let source = self.shape(first, scope, env, dest, place)?;
            let props = self.utility_props(utility, &source, args, scope, env, dest, place)?;
            if props == source {
                return self.eval(first, scope, env, dest, &spell(first), place);
            }
            return self.synth_dict(props, hint, dest, place);
        }
        let subject = self.eval(first, scope, env, dest, hint, place)?;
        if utility == Utility::NonNullable {
            // `T & {}`: `unknown` narrows to the empty object type.
            return Some(match subject.split_empties() {
                Some((values, _)) => values,
                None if subject == TypeRef::Unknown => TypeRef::Object,
                None => subject,
            });
        }
        let test = self.eval(args.get(1)?, scope, env, dest, hint, place)?;
        // A string enum tests as its literals.
        let mut expanded_test = Vec::new();
        for t in union_members(&test) {
            match self.string_literals(&t, dest) {
                Some(literals) if matches!(t, TypeRef::Reference { .. }) => {
                    expanded_test.extend(literals.into_iter().map(TypeRef::StringLiteral));
                }
                _ => expanded_test.push(t),
            }
        }
        let test = expanded_test;
        let has_literals = test.iter().any(|t| matches!(t, TypeRef::StringLiteral(_)));
        let mut kept = Vec::new();
        for member in union_members(&subject) {
            // Testing an enum's variants against literals reads the enum
            // as its literals.
            let expanded = match (&member, has_literals) {
                (TypeRef::Reference { .. }, true) => self
                    .string_literals(&member, dest)
                    .map(|lits| lits.into_iter().map(TypeRef::StringLiteral).collect()),
                _ => None,
            };
            for m in expanded.unwrap_or_else(|| vec![member.clone()]) {
                let matched = test.iter().any(|t| assignable(&m, t));
                if matched == (utility == Utility::Extract) {
                    kept.push(m);
                }
            }
        }
        Some(if kept.is_empty() {
            TypeRef::Never
        } else {
            TypeRef::union(kept)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn utility_props(
        &mut self,
        utility: Utility,
        source: &[Prop],
        args: &[TypeRef],
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        place: Placement,
    ) -> Option<Vec<Prop>> {
        let mut props = source.to_vec();
        match utility {
            Utility::Partial => props.iter_mut().for_each(|p| p.optional = true),
            Utility::Required => props.iter_mut().for_each(|p| {
                if p.optional {
                    p.type_ref = without_undefined(&p.type_ref);
                }
                p.optional = false;
            }),
            Utility::Readonly => props.iter_mut().for_each(|p| p.readonly = true),
            Utility::Pick | Utility::Omit => {
                let keys = self.literal_set(args.get(1)?, scope, env, dest, place)?;
                props.retain(|p| keys.contains(&p.js_name) == (utility == Utility::Pick));
            }
            Utility::Exclude | Utility::Extract | Utility::NonNullable => unreachable!(),
        }
        Some(props)
    }

    /// The members of an evaluated mapped type.
    fn mapped_props(
        &mut self,
        mapped: &MappedType,
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        place: Placement,
    ) -> Option<Vec<Prop>> {
        let keys = self.literal_set(&mapped.keys, scope, env, dest, place)?;
        // Over `keyof T`, possibly filtered, the type is homomorphic: each
        // property keeps the modifiers (and, without a value, the type) it
        // has in `T`.
        let source = match homomorphic_source(&mapped.keys) {
            Some(t) => self.shape(t, scope, env, dest, place),
            None => None,
        };
        let mut props = Vec::new();
        for key in keys {
            let from = source
                .as_ref()
                .and_then(|props| props.iter().find(|p| p.js_name == key));
            let mut type_ref = match &mapped.value {
                Some(value) => {
                    let env = env.bind(
                        mapped.key.clone(),
                        TypeRef::StringLiteral(key.clone()),
                        dest,
                        Env::default(),
                    );
                    self.eval(value, scope, &env, dest, &key, place)?
                }
                None => {
                    let from = from?;
                    if from.method {
                        return None;
                    }
                    from.type_ref.clone()
                }
            };
            let modifier = |m: Option<Modifier>, inherited: bool| match m {
                Some(Modifier::Add) => true,
                Some(Modifier::Remove) => false,
                None => inherited,
            };
            if mapped.optional == Some(Modifier::Remove) && from.is_some_and(|p| p.optional) {
                type_ref = without_undefined(&type_ref);
            }
            props.push(Prop {
                js_name: key,
                type_ref,
                optional: modifier(mapped.optional, from.is_some_and(|p| p.optional)),
                readonly: modifier(mapped.readonly, from.is_some_and(|p| p.readonly)),
                doc: from.and_then(|p| p.doc.clone()),
                method: false,
            });
        }
        Some(props)
    }

    /// The properties of an object type, typed in `dest`, inherited ones
    /// included; `None` for anything that isn't one or can't be read.
    fn shape(
        &mut self,
        ty: &TypeRef,
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        place: Placement,
    ) -> Option<Vec<Prop>> {
        if self.depth > MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let result = self.shape_inner(ty, scope, env, dest, place);
        self.depth -= 1;
        result
    }

    fn shape_inner(
        &mut self,
        ty: &TypeRef,
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        place: Placement,
    ) -> Option<Vec<Prop>> {
        match ty {
            TypeRef::ObjectLiteral(props) => props
                .iter()
                .map(|p| {
                    Some(Prop {
                        js_name: p.name.clone(),
                        type_ref: self.eval(&p.type_ref, scope, env, dest, &p.name, place)?,
                        optional: p.optional,
                        readonly: p.readonly,
                        doc: None,
                        method: false,
                    })
                })
                .collect(),
            TypeRef::Mapped(m) => self.mapped_props(m, scope, env, dest, place),
            TypeRef::Intersection(parts) => {
                let mut props: Vec<Prop> = Vec::new();
                for part in parts {
                    for p in self.shape(part, scope, env, dest, place)? {
                        if !props.iter().any(|q| q.js_name == p.js_name) {
                            props.push(p);
                        }
                    }
                }
                Some(props)
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => {
                if segments.len() == 1 {
                    if let Some(bound) = env.get(&segments[0]) {
                        let (ty, bound_scope, bound_env) =
                            (bound.ty.clone(), bound.scope, bound.env.clone());
                        return self.shape(&ty, bound_scope, &bound_env, dest, place);
                    }
                    match self.gctx.scopes.resolve_binding(scope, &segments[0]) {
                        Some(Binding::TypeParam) => {
                            let owner = self.param_owner(scope, &segments[0])?;
                            let bound = self.param_bound(owner, &segments[0])?;
                            return self.shape(&bound, owner, &Env::default(), dest, place);
                        }
                        None => {
                            let utility = Utility::parse(&segments[0])?;
                            if !utility.is_object() {
                                return None;
                            }
                            let source =
                                self.shape(generic_args.first()?, scope, env, dest, place)?;
                            return self.utility_props(
                                utility,
                                &source,
                                generic_args,
                                scope,
                                env,
                                dest,
                                place,
                            );
                        }
                        Some(Binding::Declared(_)) => {}
                    }
                }
                let Some(id) = self.gctx.resolve_path(scope, &segments.join(".")) else {
                    return self.synth_props(scope, segments);
                };
                self.shape_of_decl(id, generic_args, scope, env, dest, place)
            }
            _ => None,
        }
    }

    fn shape_of_decl(
        &mut self,
        id: TypeId,
        generic_args: &[TypeRef],
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        place: Placement,
    ) -> Option<Vec<Prop>> {
        let kind = self.evaluated(id);
        let (type_params, body_scope) = match &*kind {
            TypeKind::Interface(i) => (&i.type_params, i.body_scope),
            TypeKind::Class(c) => (&c.type_params, c.body_scope),
            TypeKind::TypeAlias(a) => (&a.type_params, a.body_scope),
            _ => return None,
        };
        let mut bound_env = Env::default();
        for (i, p) in type_params.iter().enumerate() {
            match (generic_args.get(i), &p.default) {
                (Some(arg), _) => {
                    bound_env = bound_env.bind(p.name.clone(), arg.clone(), scope, env.clone());
                }
                (None, Some(default)) => {
                    bound_env = bound_env.bind(
                        p.name.clone(),
                        default.clone(),
                        body_scope,
                        bound_env.clone(),
                    );
                }
                (None, None) => {}
            }
        }
        let (members, extends): (&[Member], Vec<crate::ir::Heritage>) = match &*kind {
            TypeKind::Interface(i) => (&i.members, i.extends.clone()),
            TypeKind::Class(c) => (&c.members, c.extends.iter().cloned().collect()),
            TypeKind::TypeAlias(a) => {
                let target = a.target.clone();
                return self.shape(&target, body_scope, &bound_env, dest, place);
            }
            _ => return None,
        };
        let mut props: Vec<Prop> = Vec::new();
        for m in members {
            let (js_name, type_ref, optional, member_scope, doc) = match m {
                Member::Getter(g) => (&g.js_name, &g.type_ref, g.optional, g.scope, &g.doc),
                Member::Method(m) => {
                    if !props.iter().any(|p| p.js_name == m.js_name) {
                        props.push(Prop {
                            js_name: m.js_name.clone(),
                            type_ref: TypeRef::Unresolved("method".into()),
                            optional: m.optional,
                            readonly: true,
                            doc: m.doc.clone(),
                            method: true,
                        });
                    }
                    continue;
                }
                _ => continue,
            };
            if props.iter().any(|p| &p.js_name == js_name) {
                continue;
            }
            let readonly = !members
                .iter()
                .any(|m| matches!(m, Member::Setter(s) if &s.js_name == js_name));
            props.push(Prop {
                js_name: js_name.clone(),
                type_ref: self.eval_named(
                    type_ref,
                    member_scope,
                    &bound_env,
                    dest,
                    js_name,
                    place,
                )?,
                optional,
                readonly,
                doc: doc.clone(),
                method: false,
            });
        }
        for heritage in extends {
            for p in self.shape(&heritage.type_ref, heritage.scope, &bound_env, dest, place)? {
                if !props.iter().any(|q| q.js_name == p.js_name) {
                    props.push(p);
                }
            }
        }
        Some(props)
    }

    /// The union of `members`. One mixing string literals with string enums
    /// reads the enums as their literals, so the whole is one literal set.
    fn literal_union(&mut self, members: Vec<TypeRef>, dest: ScopeId) -> TypeRef {
        let members: Vec<TypeRef> = members
            .into_iter()
            .flat_map(|m| union_members(&m))
            .collect();
        let has_literal = members
            .iter()
            .any(|m| matches!(m, TypeRef::StringLiteral(_)));
        let mut expanded = Vec::new();
        let mut has_enum = false;
        for m in &members {
            match self.string_literals(m, dest) {
                Some(literals) if matches!(m, TypeRef::Reference { .. }) => {
                    has_enum = true;
                    expanded.extend(literals.into_iter().map(TypeRef::StringLiteral));
                }
                _ => expanded.push(m.clone()),
            }
        }
        if has_literal && has_enum {
            TypeRef::union(expanded)
        } else {
            TypeRef::union(members)
        }
    }

    /// The string literals `ty` evaluates to, in order.
    fn literal_set(
        &mut self,
        ty: &TypeRef,
        scope: ScopeId,
        env: &Env,
        dest: ScopeId,
        place: Placement,
    ) -> Option<Vec<String>> {
        let evaluated = self.eval(ty, scope, env, dest, "", place)?;
        self.string_literals(&evaluated, dest)
    }

    /// The string literals of an evaluated type, reading a string enum
    /// (or an alias of one) as its variants. `None` for anything else.
    fn string_literals(&mut self, ty: &TypeRef, scope: ScopeId) -> Option<Vec<String>> {
        self.string_literals_in(ty, scope, 0)
    }

    fn string_literals_in(
        &mut self,
        ty: &TypeRef,
        scope: ScopeId,
        depth: u32,
    ) -> Option<Vec<String>> {
        if depth > 32 {
            return None;
        }
        let mut out = Vec::new();
        for member in union_members(ty) {
            match member {
                TypeRef::StringLiteral(s) => out.push(s),
                TypeRef::Never => {}
                TypeRef::Reference { segments, .. } => {
                    let Some(id) = self.gctx.resolve_path(scope, &segments.join(".")) else {
                        out.extend(self.synth_literals(scope, &segments)?);
                        continue;
                    };
                    match &*self.evaluated(id) {
                        TypeKind::StringEnum(e) => {
                            out.extend(e.variants.iter().map(|v| v.js_value.clone()));
                        }
                        TypeKind::TypeAlias(a) if a.type_params.is_empty() => {
                            let (target, body_scope) = (a.target.clone(), a.body_scope);
                            out.extend(self.string_literals_in(&target, body_scope, depth + 1)?);
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        let mut seen = HashSet::new();
        out.retain(|s| seen.insert(s.clone()));
        Some(out)
    }

    /// A synthesised declaration named by `segments`, visible from `scope`.
    /// Synths are declared once the pass ends, so until then a reference to
    /// one resolves here.
    fn synth(&self, scope: ScopeId, segments: &[String]) -> Option<&Synth> {
        if segments.len() != 1 {
            return None;
        }
        let chain = self.gctx.scopes.chain(scope);
        self.synths
            .iter()
            .find(|s| s.name == segments[0] && chain.contains(&s.place.scope))
    }

    fn synth_literals(&self, scope: ScopeId, segments: &[String]) -> Option<Vec<String>> {
        match &self.synth(scope, segments)?.kind {
            SynthKind::Enum(literals) => Some(literals.clone()),
            SynthKind::Dict { .. } => None,
        }
    }

    fn synth_props(&self, scope: ScopeId, segments: &[String]) -> Option<Vec<Prop>> {
        match &self.synth(scope, segments)?.kind {
            SynthKind::Dict { props, .. } => Some(props.clone()),
            SynthKind::Enum(_) => None,
        }
    }

    /// The type parameters in scope at `dest` that `props` mention.
    fn captured_params(&self, props: &[Prop], dest: ScopeId) -> Vec<String> {
        let mut names = Vec::new();
        for p in props {
            idents(&p.type_ref, &mut names);
        }
        let mut seen = HashSet::new();
        names.retain(|n| {
            self.gctx.scopes.resolve_binding(dest, n) == Some(Binding::TypeParam)
                && seen.insert(n.clone())
        });
        names
    }

    /// The body scope declaring the type parameter `name` visible from
    /// `scope`, if a declared type doesn't shadow it first.
    fn param_owner(&self, scope: ScopeId, name: &str) -> Option<ScopeId> {
        for s in self.gctx.scopes.chain(scope) {
            match self.gctx.scopes.get(s).get_binding(name) {
                Some(Binding::TypeParam) => return Some(s),
                Some(Binding::Declared(_)) => return None,
                None => {}
            }
        }
        None
    }

    fn param_bound(&self, owner: ScopeId, name: &str) -> Option<TypeRef> {
        self.params_of
            .get(&owner)?
            .iter()
            .find(|p| p.name == name)?
            .constraint
            .clone()
    }

    /// Name an evaluated type that has no name of its own: string literals
    /// become a string enum under `hint`. A result that is a reference,
    /// or has no literal of its own, stays as it is.
    fn materialize(
        &mut self,
        result: TypeRef,
        hint: &str,
        dest: ScopeId,
        place: Placement,
    ) -> TypeRef {
        let (values, empties) = match result.split_empties() {
            Some((values, n)) => (values, Some(n)),
            None => (result, None),
        };
        let has_literal = union_members(&values)
            .iter()
            .any(|m| matches!(m, TypeRef::StringLiteral(_)));
        let values = match self.string_literals(&values, dest) {
            Some(literals) if has_literal && !literals.is_empty() => {
                self.synth_enum(literals, hint, dest, place)
            }
            _ => values,
        };
        match empties {
            Some(n) => values.with_empties(n),
            None => values,
        }
    }

    fn synth_enum(
        &mut self,
        literals: Vec<String>,
        hint: &str,
        dest: ScopeId,
        place: Placement,
    ) -> TypeRef {
        if !self.gctx.scopes.chain(dest).contains(&place.scope) {
            return TypeRef::union(literals.into_iter().map(TypeRef::StringLiteral));
        }
        let key = (place.scope, literals);
        if let Some(name) = self.enum_names.get(&key) {
            return TypeRef::ident(name.clone());
        }
        let name = self.used.declare(hint, Suffix::Pascal);
        self.enum_names.insert(key.clone(), name.clone());
        self.synths.push(Synth {
            name: name.clone(),
            place,
            kind: SynthKind::Enum(key.1),
        });
        TypeRef::ident(name)
    }

    fn synth_dict(
        &mut self,
        props: Vec<Prop>,
        hint: &str,
        dest: ScopeId,
        place: Placement,
    ) -> Option<TypeRef> {
        if props.iter().any(|p| p.method) {
            return None;
        }
        // The use site must see the declaration beside its anchor.
        if !self.gctx.scopes.chain(dest).contains(&place.scope) {
            return None;
        }
        let captured = self.captured_params(&props, dest);
        let reference = |name: String| TypeRef::Reference {
            segments: vec![name],
            generic_args: captured.iter().map(TypeRef::ident).collect(),
        };
        let key = (place.scope, dict_key(&props));
        if let Some(name) = self.dict_names.get(&key) {
            return Some(reference(name.clone()));
        }
        let name = self.used.declare(hint, Suffix::Pascal);
        self.dict_names.insert(key, name.clone());
        let type_params = captured
            .iter()
            .map(|name| TypeParam {
                name: name.clone(),
                constraint: None,
                default: None,
            })
            .collect();
        self.synths.push(Synth {
            name: name.clone(),
            place,
            kind: SynthKind::Dict { props, type_params },
        });
        Some(reference(name))
    }
}

/// The `T` a mapped type over `keyof T`, possibly filtered through
/// `Exclude` / `Extract`, is homomorphic in.
fn homomorphic_source(keys: &TypeRef) -> Option<&TypeRef> {
    match keys {
        TypeRef::KeyOf(t) => Some(t),
        TypeRef::Reference {
            segments,
            generic_args,
        } if segments.len() == 1 && matches!(segments[0].as_str(), "Exclude" | "Extract") => {
            homomorphic_source(generic_args.first()?)
        }
        _ => None,
    }
}

/// The identity of a dictionary's members, so one shape is one type.
type DictKey = Vec<(String, String, bool, bool)>;

fn dict_key(props: &[Prop]) -> DictKey {
    props
        .iter()
        .map(|p| {
            (
                p.js_name.clone(),
                p.type_ref.format_ts(),
                p.optional,
                p.readonly,
            )
        })
        .collect()
}

fn is_plain_alias(kind: &TypeKind) -> bool {
    matches!(kind, TypeKind::TypeAlias(a) if a.type_params.is_empty())
}

/// `-?` on an optional property removes `undefined` from its type as well.
fn without_undefined(ty: &TypeRef) -> TypeRef {
    TypeRef::union(
        union_members(ty)
            .into_iter()
            .filter(|m| !matches!(m, TypeRef::Undefined | TypeRef::Void)),
    )
}

/// Every single-segment name `ty` mentions, type parameters included.
fn idents(ty: &TypeRef, out: &mut Vec<String>) {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            if segments.len() == 1 && generic_args.is_empty() {
                out.push(segments[0].clone());
            }
            generic_args.iter().for_each(|a| idents(a, out));
        }
        TypeRef::Array(e) | TypeRef::KeyOf(e) => idents(e, out),
        TypeRef::Union(u) => u.members().iter().for_each(|m| idents(m, out)),
        TypeRef::Intersection(v) | TypeRef::Tuple(v) => v.iter().for_each(|m| idents(m, out)),
        TypeRef::Function(sig) => {
            sig.params.iter().for_each(|p| idents(&p.type_ref, out));
            idents(&sig.return_type, out);
        }
        TypeRef::IndexedAccess(o, i) => {
            idents(o, out);
            idents(i, out);
        }
        TypeRef::Mapped(m) => {
            idents(&m.keys, out);
            m.value.iter().for_each(|v| idents(v, out));
        }
        TypeRef::ObjectLiteral(props) => props.iter().for_each(|p| idents(&p.type_ref, out)),
        _ => {}
    }
}

fn union_members(ty: &TypeRef) -> Vec<TypeRef> {
    match ty {
        TypeRef::Union(u) => u.members().to_vec(),
        other => vec![other.clone()],
    }
}

/// Whether `ty` is assignable to `to` for the purposes of `Exclude` and
/// `Extract`: equal, or a literal of `to`'s primitive.
fn assignable(ty: &TypeRef, to: &TypeRef) -> bool {
    ty == to
        || matches!(
            (ty, to),
            (TypeRef::StringLiteral(_), TypeRef::String)
                | (TypeRef::NumberLiteral(_), TypeRef::Number)
                | (TypeRef::BooleanLiteral(_), TypeRef::Boolean)
        )
        || matches!(to, TypeRef::Any | TypeRef::Unknown)
}

/// The lowering a type had before evaluation existed, for one that can't
/// be evaluated: a utility is its first argument, the rest erase.
fn fallback(ty: &TypeRef) -> TypeRef {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            if segments.len() == 1 && Utility::parse(&segments[0]).is_some() {
                return generic_args.first().map_or(TypeRef::Object, fallback);
            }
            TypeRef::Reference {
                segments: segments.clone(),
                generic_args: generic_args.iter().map(fallback).collect(),
            }
        }
        TypeRef::Array(e) => TypeRef::Array(Box::new(fallback(e))),
        TypeRef::Union(u) => TypeRef::union(u.members().iter().map(fallback)),
        TypeRef::Intersection(v) => simplify_intersection(v.iter().map(fallback).collect()),
        TypeRef::Tuple(v) => TypeRef::Tuple(v.iter().map(fallback).collect()),
        TypeRef::KeyOf(_) => TypeRef::Unresolved("keyof".into()),
        TypeRef::IndexedAccess(..) => TypeRef::Unresolved("indexed access type".into()),
        TypeRef::Mapped(_) => TypeRef::Unresolved("mapped type".into()),
        TypeRef::ObjectLiteral(_) => TypeRef::Object,
        other => other.clone(),
    }
}

/// A PascalCase spelling of a type for naming what it evaluates to:
/// `PluralizeUnit<"day" | TimeUnit>` is `PluralizeUnitDayOrTimeUnit`.
fn spell(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            let mut out = to_pascal_case(segments.last().unwrap());
            for a in generic_args {
                out.push_str(&spell(a));
            }
            out
        }
        TypeRef::StringLiteral(s) => to_enum_variant(s),
        TypeRef::NumberLiteral(n) => to_enum_variant(&n.to_string()),
        TypeRef::BooleanLiteral(b) => to_pascal_case(&b.to_string()),
        TypeRef::Union(u) => u.members().iter().map(spell).collect::<Vec<_>>().join("Or"),
        TypeRef::Array(e) => format!("{}Array", spell(e)),
        TypeRef::KeyOf(t) => format!("KeyOf{}", spell(t)),
        TypeRef::IndexedAccess(o, i) => format!("{}{}", spell(o), spell(i)),
        other => to_pascal_case(&other.format_ts()),
    }
}

fn string_enum(name: &str, literals: &[String]) -> StringEnumDecl {
    let mut rust_names: Vec<String> = literals.iter().map(|s| to_enum_variant(s)).collect();
    dedup_names(&mut rust_names);
    StringEnumDecl {
        name: name.to_string(),
        variants: literals
            .iter()
            .zip(rust_names)
            .map(|(js_value, rust_name)| StringEnumVariant {
                rust_name,
                js_value: js_value.clone(),
            })
            .collect(),
    }
}

fn props_to_members(props: &[Prop], scope: ScopeId) -> Vec<Member> {
    let mut members = Vec::new();
    for p in props {
        if p.method {
            continue;
        }
        members.push(Member::Getter(GetterMember {
            scope,
            js_name: p.js_name.clone(),
            type_ref: p.type_ref.clone(),
            optional: p.optional,
            doc: p.doc.clone(),
            throws: Throws::None,
        }));
        if !p.readonly {
            members.push(Member::Setter(SetterMember {
                scope,
                js_name: p.js_name.clone(),
                type_ref: p.type_ref.clone(),
                doc: None,
                slice_mode: None,
            }));
        }
    }
    members
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::InterfaceClassification;

    fn parse(src: &str) -> (Module, GlobalContext) {
        super::super::parse_single_source(src, None).unwrap()
    }

    fn kind<'g>(gctx: &'g GlobalContext, module: &Module, name: &str) -> &'g TypeKind {
        let id = gctx
            .scopes
            .resolve(module.file_scopes[0], name)
            .unwrap_or_else(|| panic!("no type named {name}"));
        &gctx.get_type(id).kind
    }

    fn param_type<'g>(
        gctx: &'g GlobalContext,
        module: &Module,
        func: &str,
        i: usize,
    ) -> &'g TypeRef {
        match kind(gctx, module, func) {
            TypeKind::Function(f) => &f.params[i].type_ref,
            other => panic!("{func} is {other:?}"),
        }
    }

    fn variants(kind: &TypeKind) -> Vec<&str> {
        match kind {
            TypeKind::StringEnum(e) => e.variants.iter().map(|v| v.js_value.as_str()).collect(),
            other => panic!("not a string enum: {other:?}"),
        }
    }

    fn props(kind: &TypeKind) -> Vec<(&str, bool, bool)> {
        let TypeKind::Interface(i) = kind else {
            panic!("not an interface: {kind:?}");
        };
        i.members
            .iter()
            .filter_map(|m| match m {
                Member::Getter(g) => Some((
                    g.js_name.as_str(),
                    g.optional,
                    !i.members
                        .iter()
                        .any(|m| matches!(m, Member::Setter(s) if s.js_name == g.js_name)),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn exclude_and_extract_filter_literals_through_enums() {
        let (module, gctx) = parse(
            "type Unit = \"hour\" | \"minute\" | \"second\";
             type NotHour = Exclude<Unit, \"hour\">;
             type Day = Extract<Unit | \"day\", \"day\" | \"week\">;
             type None = Exclude<Unit, Unit>;
             declare function f(a: Exclude<Unit, \"hour\">, b: NonNullable<string | null | undefined>): void;",
        );
        assert_eq!(
            variants(kind(&gctx, &module, "NotHour")),
            ["minute", "second"]
        );
        assert_eq!(variants(kind(&gctx, &module, "Day")), ["day"]);
        let TypeKind::TypeAlias(none) = kind(&gctx, &module, "None") else {
            panic!("None should stay an alias");
        };
        assert_eq!(none.target, TypeRef::Never);
        assert_eq!(
            param_type(&gctx, &module, "f", 0),
            &TypeRef::ident("NotHour")
        );
        assert_eq!(param_type(&gctx, &module, "f", 1), &TypeRef::String);
    }

    #[test]
    fn object_utilities_synthesise_dictionaries_or_keep_the_type() {
        let (module, gctx) = parse(
            "interface Options { name: string; size?: number; readonly fixed: boolean }
             declare function f(
               a: Partial<Options>, b: Required<Options>, c: Readonly<Options>,
               d: Pick<Options, \"name\">, e: Omit<Options, \"name\" | \"fixed\">,
               f: Partial<Pick<Options, \"size\">>, g: Partial<Omit<Options, \"fixed\">>,
             ): void;",
        );
        let at = |i| {
            param_type(&gctx, &module, "f", i)
                .as_ident()
                .unwrap()
                .to_string()
        };
        assert_eq!(at(0), "PartialOptions");
        assert_eq!(
            props(kind(&gctx, &module, "PartialOptions")),
            [
                ("name", true, false),
                ("size", true, false),
                ("fixed", true, true)
            ]
        );
        assert_eq!(
            props(kind(&gctx, &module, &at(1))),
            [
                ("name", false, false),
                ("size", false, false),
                ("fixed", false, true)
            ]
        );
        assert!(props(kind(&gctx, &module, &at(2))).iter().all(|p| p.2));
        assert_eq!(
            props(kind(&gctx, &module, &at(3))),
            [("name", false, false)]
        );
        // `size` is already optional, so `Partial` changes nothing.
        assert_eq!(at(4), at(5));
        assert_eq!(props(kind(&gctx, &module, &at(4))), [("size", true, false)]);
        assert_eq!(
            props(kind(&gctx, &module, &at(6))),
            [("name", true, false), ("size", true, false)]
        );
        assert!(matches!(
            kind(&gctx, &module, "PartialOptions"),
            TypeKind::Interface(i) if i.classification == InterfaceClassification::Dictionary
        ));
    }

    #[test]
    fn utility_parents_flatten_into_the_interface() {
        let (module, gctx) = parse(
            "interface Options { name: string; size?: number }
             interface Named extends Omit<Options, \"size\"> { label?: string }
             interface Plain extends Options {}",
        );
        let TypeKind::Interface(named) = kind(&gctx, &module, "Named") else {
            unreachable!()
        };
        assert!(named.extends.is_empty());
        assert_eq!(
            props(kind(&gctx, &module, "Named")),
            [("label", true, false), ("name", false, false)]
        );
        let TypeKind::Interface(plain) = kind(&gctx, &module, "Plain") else {
            unreachable!()
        };
        assert_eq!(plain.extends.len(), 1);
        assert!(plain.members.is_empty());
    }

    #[test]
    fn mapped_and_indexed_types_instantiate_through_aliases() {
        let (module, gctx) = parse(
            "interface Options { name: string; size?: number }
             type Loose<T> = { [P in keyof T]?: T[P] };
             type Flags<K extends string> = { [P in K]: boolean };
             type Unit = \"hour\" | \"minute\";
             type Plural<T extends Unit> = T | { hour: \"hours\"; minute: \"minutes\" }[T];
             type Keys = keyof Options;
             declare function f(a: Loose<Options>, b: Flags<\"x\" | \"y\">, c: Plural<\"hour\">, d: Plural<Unit>, e: keyof Options): void;",
        );
        let at = |i| {
            param_type(&gctx, &module, "f", i)
                .as_ident()
                .unwrap()
                .to_string()
        };
        assert_eq!(
            props(kind(&gctx, &module, &at(0))),
            [("name", true, false), ("size", true, false)]
        );
        assert_eq!(
            props(kind(&gctx, &module, &at(1))),
            [("x", false, false), ("y", false, false)]
        );
        assert_eq!(variants(kind(&gctx, &module, &at(2))), ["hour", "hours"]);
        // At its bound the alias is its own enum, shared by `Plural<Unit>`.
        assert_eq!(at(3), "Plural");
        assert_eq!(
            variants(kind(&gctx, &module, "Plural")),
            ["hour", "minute", "hours", "minutes"]
        );
        assert_eq!(variants(kind(&gctx, &module, "Keys")), ["name", "size"]);
        // The same literal set as `Keys`, so it is `Keys`.
        assert_eq!(at(4), "Keys");
        // Unbound, `Loose` can't evaluate and erases as before.
        let TypeKind::TypeAlias(loose) = kind(&gctx, &module, "Loose") else {
            panic!("Loose should stay an alias");
        };
        assert!(matches!(loose.target, TypeRef::Unresolved(_)));
    }

    #[test]
    fn out_of_scope_parameters_evaluate_at_their_bound() {
        let (module, gctx) = parse(
            "type Unit = \"hour\" | \"minute\";
             type Plural<T extends Unit> = T | { hour: \"hours\"; minute: \"minutes\" }[T];
             interface Rounding<U extends Unit> { smallestUnit?: Plural<U> }",
        );
        let TypeKind::Interface(rounding) = kind(&gctx, &module, "Rounding") else {
            unreachable!()
        };
        let Member::Getter(g) = &rounding.members[0] else {
            unreachable!()
        };
        assert_eq!(g.type_ref, TypeRef::ident("Plural"));
    }

    #[test]
    fn unevaluable_types_keep_their_old_lowering() {
        let (module, gctx) = parse(
            "declare class Widget { size: number; render(): void }
             declare function f<T>(a: Partial<Widget>, b: keyof T, c: Partial<T>): void;",
        );
        assert_eq!(
            param_type(&gctx, &module, "f", 0),
            &TypeRef::ident("Widget")
        );
        assert!(matches!(
            param_type(&gctx, &module, "f", 1),
            TypeRef::Unresolved(_)
        ));
        assert_eq!(param_type(&gctx, &module, "f", 2), &TypeRef::ident("T"));
        let rendered = gctx.diagnostics.render(false);
        assert_eq!(
            rendered.matches("could not be evaluated").count(),
            3,
            "{rendered}"
        );
    }

    #[test]
    fn utilities_read_an_alias_evaluated() {
        let (module, gctx) = parse(
            "type Unit = \"hour\" | \"minute\" | \"second\";
             type Minute = Exclude<Unit, \"hour\" | \"second\">;
             type NotMinute = Exclude<Unit, Minute>;
             type Keys = Exclude<keyof Options, \"fixed\">;
             interface Options { name: string; size?: number; readonly fixed: boolean }
             declare function f(a: Exclude<Minute, \"minute\">, b: Pick<Options, Keys>): void;",
        );
        assert_eq!(
            variants(kind(&gctx, &module, "NotMinute")),
            ["hour", "second"]
        );
        assert_eq!(param_type(&gctx, &module, "f", 0), &TypeRef::Never);
        assert_eq!(
            props(kind(&gctx, &module, "PickOptionsKeys")),
            [("name", false, false), ("size", true, false)]
        );
    }

    #[test]
    fn dictionaries_capturing_a_type_parameter_are_generic_over_it() {
        let (module, gctx) = parse(
            "interface Box<T> { value: T; label: string }
             interface Holder<T> { partial: Partial<Box<T>> }",
        );
        let TypeKind::Interface(holder) = kind(&gctx, &module, "Holder") else {
            unreachable!()
        };
        let Member::Getter(g) = &holder.members[0] else {
            unreachable!()
        };
        assert_eq!(
            g.type_ref,
            TypeRef::Reference {
                segments: vec!["PartialBoxT".into()],
                generic_args: vec![TypeRef::ident("T")],
            }
        );
        let TypeKind::Interface(dict) = kind(&gctx, &module, "PartialBoxT") else {
            unreachable!()
        };
        assert_eq!(dict.type_params.len(), 1);
        assert_eq!(dict.type_params[0].name, "T");
        assert_eq!(
            gctx.scopes.resolve_binding(dict.body_scope, "T"),
            Some(Binding::TypeParam)
        );
    }

    #[test]
    fn indexed_access_on_an_optional_property_admits_undefined() {
        let (module, gctx) = parse(
            "interface Options { name: string; size?: number; note: string | undefined }
             type Strict = { [P in keyof Options]-?: Options[P] };
             declare function f(a: Options[\"size\"], b: Options[\"name\"], c: Required<Options>): void;",
        );
        assert_eq!(
            param_type(&gctx, &module, "f", 0),
            &TypeRef::union([TypeRef::Number, TypeRef::Undefined])
        );
        assert_eq!(param_type(&gctx, &module, "f", 1), &TypeRef::String);
        let TypeKind::Interface(strict) = kind(&gctx, &module, "Strict") else {
            unreachable!()
        };
        let getter = |name: &str| {
            strict.members.iter().find_map(|m| match m {
                Member::Getter(g) if g.js_name == name => Some((g.type_ref.clone(), g.optional)),
                _ => None,
            })
        };
        assert_eq!(getter("size"), Some((TypeRef::Number, false)));
        assert_eq!(
            getter("note"),
            Some((TypeRef::union([TypeRef::String, TypeRef::Undefined]), false))
        );
        assert_eq!(
            param_type(&gctx, &module, "f", 2),
            &TypeRef::ident("Strict")
        );
    }
}
