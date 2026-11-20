//! Lowering: the parsed `.d.ts` to the ts-gen IR (`IR.md`).
//!
//! The parse is the front end: it resolves names, merges declarations and
//! evaluates type-level constructs. Lowering makes the TypeScript-level
//! decisions the IR leaves out: unions in parameters expand into overloads,
//! constants move into `@js(args = (...))`, other unions become enums, and
//! `null`/`undefined` become `T?`, `@js(null) T?` or `T??`. The result
//! is one canonical IR per input, independent of any language generator.

mod assoc;
mod callables;
mod indices;
mod names;
mod pad;
mod types;
mod walk;

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use ts_gen_abi::ir as abi;
use ts_gen_abi::resolve::Location;

use crate::context::{GlobalContext, TypeId};
use crate::ir::{Module, ModuleContext, TypeKind};
use crate::provides::Provides;

/// A lowered IR, with what lowering couldn't express faithfully.
pub struct Lowered {
    pub ir: abi::Ir,
    pub diagnostics: Vec<String>,
    /// Where each parsed type declaration is in the IR, for generators
    /// that still read the parse alongside it.
    pub locations: HashMap<TypeId, Location>,
}

/// Producer configuration: what the `.d.ts` can't say.
pub struct Options {
    /// Callback parameters called at most once, as `Owner.member.param`
    /// or `function.param` in JS names: they're `@once` (IR.md §14).
    pub once: Vec<String>,
    /// Make a future-returning call cancellable when it takes a dictionary
    /// with an `AbortSignal` `signal` (IR.md §16).
    pub cancel: bool,
    /// What the module provides to JS (`--provides`), as exports (§17).
    pub provides: Provides,
    /// Concrete bounds, as `Decl.Param` and IR types (`Map.K` → `Str`,
    /// `f64`), for generics a language concretizes (§13).
    pub bounds: Vec<(String, Vec<abi::Type>)>,
    /// Produce a lib IR (§5) instead.
    pub lib: Option<LibOptions>,
    /// Lib IRs that declare the lib types the input references, instead
    /// of stubs (`with`, §5).
    pub with: Vec<abi::Ir>,
    /// `T | null | undefined` keeps three states as `T??`. Otherwise it's
    /// `T?`, writing `undefined`, which TS accepts there (§9).
    pub strict_nullable: bool,
}

/// A lib IR to produce: `lib <name>;` with the root declarations, at
/// their global names. The rest of the input reads as lib types, as if it
/// weren't declared, and a root's defaulted type parameters take their
/// defaults, so a reference names only the ones it must.
#[derive(Clone, Debug, Default)]
pub struct LibOptions {
    pub name: String,
    pub roots: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            once: Vec::new(),
            cancel: true,
            provides: Provides::default(),
            bounds: Vec::new(),
            lib: None,
            with: Vec::new(),
            strict_nullable: false,
        }
    }
}

/// A `--bound` spec, `Decl.Param=T,U` (`Map.K=Str,f64`), with the types
/// in IR syntax.
pub fn parse_bound(spec: &str) -> Result<(String, Vec<abi::Type>), String> {
    let (target, types) = spec
        .split_once('=')
        .ok_or_else(|| format!("`{spec}`: expected `Decl.Param=Type,...`"))?;
    let src = format!("ir 1;\nf(x: ({types}));\n");
    let ir = ts_gen_abi::text::parse(&src).map_err(|e| format!("`{spec}`: {e}"))?;
    let types = match ir.items.first() {
        Some(abi::Decl::Member(m)) => match &m.params[0].ty {
            abi::Type::Tuple(ts) => ts.clone(),
            t => vec![t.clone()],
        },
        _ => unreachable!(),
    };
    Ok((target.trim().to_string(), types))
}

/// Lower every declaration of `module` to one IR.
pub fn lower(module: &Module, gctx: &GlobalContext) -> Lowered {
    lower_with(module, gctx, &Options::default())
}

/// [`lower`], configured.
pub fn lower_with(module: &Module, gctx: &GlobalContext, opts: &Options) -> Lowered {
    let mut l = Lowerer::new(module, gctx, opts);
    l.index();
    for &id in &module.types {
        let place = Place::of(&gctx.get_type(id).module_context);
        if !l.in_lib(id, &place) {
            continue;
        }
        l.decl(id, &place);
    }
    l.emit_pulled();
    l.provide();
    l.finish()
}

/// A `union` whose arms the bridge can't tell apart passes its enum as a
/// value. Otherwise an overload that is one of its arm variants, the same
/// call with the same result, is left to derive (`new RegExp(string)`
/// beside `new RegExp(RegExp | string)`).
fn settle_unions(ir: &mut abi::Ir, libs: &[abi::Ir]) {
    use ts_gen_abi::union;
    let libs: Vec<&abi::Ir> = libs.iter().collect();
    let keys = ts_gen_abi::key::Keys::with_libs(ir, &libs);
    let all = keys.members();
    let mut plain: HashSet<*const abi::Param> = HashSet::new();
    for k in &all {
        for p in &k.member.params {
            if p.union && union::invalid(&keys.symbols, &p.ty, &k.cx).is_some() {
                plain.insert(p);
            }
        }
    }
    let module = |k: &ts_gen_abi::key::Keyed<'_>| match &k.role {
        ts_gen_abi::key::Role::Import(m) => Some(m.clone()),
        ts_gen_abi::key::Role::Export => None,
    };
    let by_key: HashMap<(Option<String>, &str), &abi::Member> = all
        .iter()
        .map(|k| ((module(k), k.key.as_str()), k.member))
        .collect();
    let mut derived: HashSet<*const abi::Member> = HashSet::new();
    for k in &all {
        let m = k.member;
        if !m
            .params
            .iter()
            .any(|p| p.union && !plain.contains(&(p as *const _)))
        {
            continue;
        }
        for (key, _) in union::variants(&keys, k) {
            if let Some(&o) = by_key.get(&(module(k), key.as_str())) {
                if o.result == m.result
                    && o.throws == m.throws
                    && o.tparams == m.tparams
                    && union::same_call(o, m)
                {
                    derived.insert(o);
                }
            }
        }
    }
    walk::members(&mut ir.items, &mut |m| {
        for p in &mut m.params {
            if plain.contains(&(p as *const abi::Param)) {
                p.union = false;
            }
        }
    });
    walk::retain_members(&mut ir.items, &mut |m| !derived.contains(&(m as *const _)));
}

/// Each `with` lib type's lib, arity and whether it's an interface, by its
/// dotted path through the lib's namespaces (`Intl.DateTimeFormatOptions`).
fn lib_types(libs: &[abi::Ir]) -> HashMap<String, (String, usize, bool)> {
    fn walk(
        lib: &str,
        prefix: &str,
        items: &[abi::Decl],
        out: &mut HashMap<String, (String, usize, bool)>,
    ) {
        for d in items {
            let (ty, arity, interface) = match d {
                abi::Decl::Class(c) => (
                    &c.name,
                    c.tparams.len(),
                    c.kind == abi::ClassKind::Interface,
                ),
                abi::Decl::Enum(e) => (&e.name, 0, false),
                abi::Decl::Namespace(n) => {
                    walk(lib, &format!("{prefix}{}.", n.name), &n.items, out);
                    continue;
                }
                _ => continue,
            };
            out.entry(format!("{prefix}{ty}"))
                .or_insert((lib.to_string(), arity, interface));
        }
    }
    let mut out = HashMap::new();
    for lib in libs {
        if let Some(name) = &lib.lib {
            walk(name, "", &lib.items, &mut out);
        }
    }
    out
}

/// The string enums of the `with` libs a literal set can be: closed,
/// with no bases, and one literal per arm.
fn lib_literals(libs: &[abi::Ir]) -> Vec<(String, String, Vec<String>)> {
    fn walk(
        lib: &str,
        prefix: &str,
        items: &[abi::Decl],
        out: &mut Vec<(String, String, Vec<String>)>,
    ) {
        for d in items {
            match d {
                abi::Decl::Namespace(n) => {
                    walk(lib, &format!("{prefix}{}.", n.name), &n.items, out)
                }
                abi::Decl::Enum(e) if e.carrier.is_none() && e.extends.is_empty() => {
                    let lits: Option<Vec<String>> = e
                        .arms
                        .iter()
                        .map(|a| {
                            match (
                                a.payload.is_empty(),
                                a.meta.attrs.fallback,
                                &a.meta.attrs.js[..],
                            ) {
                                (true, false, [abi::JsEntry::Literal(s)]) => Some(s.clone()),
                                _ => None,
                            }
                        })
                        .collect();
                    if let Some(mut lits) = lits {
                        lits.sort();
                        out.push((lib.to_string(), format!("{prefix}{}", e.name), lits));
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for lib in libs {
        if let Some(name) = &lib.lib {
            walk(name, "", &lib.items, &mut out);
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Where a declaration lives in the IR: its `module` block, if any, and the
/// namespaces inside it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Place {
    pub module: Option<String>,
    pub ns: Vec<String>,
}

impl Place {
    fn of(cx: &ModuleContext) -> Place {
        Place {
            module: cx.specifier().map(Into::into),
            ns: Vec::new(),
        }
    }

    pub(crate) fn child(&self, name: &str) -> Place {
        let mut p = self.clone();
        p.ns.push(name.into());
        p
    }
}

/// A type declaration's IR identity.
#[derive(Clone, Debug)]
pub(crate) struct DeclInfo {
    pub place: Place,
    /// The IR name: the JS name, escaped when it's reserved.
    pub name: String,
}

#[derive(Default)]
struct Container {
    entries: Vec<Entry>,
    namespaces: IndexMap<String, Container>,
    /// Enums synthesized for uses here, emitted after the declarations.
    synth: Vec<abi::Decl>,
}

enum Entry {
    Decl(Box<abi::Decl>),
    Namespace(String),
}

impl Container {
    fn at(&mut self, ns: &[String]) -> &mut Container {
        match ns.split_first() {
            None => self,
            Some((first, rest)) => {
                if !self.namespaces.contains_key(first) {
                    self.entries.push(Entry::Namespace(first.clone()));
                }
                self.namespaces.entry(first.clone()).or_default().at(rest)
            }
        }
    }
}

pub(crate) struct Lowerer<'a> {
    module: &'a Module,
    gctx: &'a GlobalContext,
    opts: &'a Options,
    /// What the module exports (§17), after the imports.
    exported: Vec<abi::Decl>,
    info: HashMap<TypeId, DeclInfo>,
    /// The full path of every type declaration, by module block.
    declared: HashSet<(Option<String>, Vec<String>)>,
    /// Where each alias is declared: a union alias's enum goes there.
    alias_places: HashMap<TypeId, Place>,
    /// The aliases declared in the IR, each with its IR name, and their
    /// full paths: each is a function type.
    kept: HashMap<TypeId, String>,
    fn_aliases: HashSet<(Option<String>, Vec<String>)>,
    /// The full path of every class, for namespace folding.
    classes: HashSet<(Option<String>, Vec<String>)>,
    /// Every type name used anywhere, so synthesized names stay unique
    /// across the output.
    taken: HashSet<String>,
    globals: Container,
    modules: IndexMap<String, Container>,
    /// Lib types the input references but doesn't declare.
    stubs: IndexMap<String, (abi::ClassKind, usize)>,
    /// The `Record` interfaces referenced, by name.
    records: indexmap::IndexSet<&'static str>,
    /// Each `with` lib type's lib, arity and whether it's an interface.
    lib_types: HashMap<String, (String, usize, bool)>,
    /// Each `with` lib string enum: its lib, dotted path and sorted literals.
    lib_literals: Vec<(String, String, Vec<String>)>,
    /// The `with` libs referenced.
    with_used: HashSet<String>,
    /// Synthesized enums by structural key.
    synth: HashMap<String, (Place, String)>,
    enums: HashSet<(Option<String>, Vec<String>)>,
    /// The synthesized enums with a payload arm, by full path.
    payloads: HashSet<(Option<String>, Vec<String>)>,
    /// In lib mode, the enums outside the roots that the output references,
    /// by full path, emitted at their own place after the roots.
    pulled: indexmap::IndexSet<(Option<String>, Vec<String>)>,
    /// The literals of every string enum, by full path.
    literals: HashMap<(Option<String>, Vec<String>), Vec<String>>,
    /// The synth key of the first enum of each open literal set, by its
    /// sorted literals, which unnamed uses of the set share.
    open_sets: HashMap<Vec<String>, String>,
    /// The anchors using each inline payload enum named by its first, the
    /// name its arms spell, and its synth key: one more than one anchor
    /// shares takes the spelled name once lowering is done.
    anchors: Vec<Shared>,
    made: HashMap<String, usize>,
    /// Each `<Owner><Anchor>Kind` and the `<Anchor>Kind` another enum held
    /// when it was named.
    qualified: Vec<(String, String)>,
    /// The `--bound` keys a type parameter took.
    bounds_used: HashSet<String>,
    pending_assoc: HashMap<TypeId, Vec<assoc::Pending>>,
    /// Each associated type's name, by its declaration and TS spelling.
    assoc_names: HashMap<(TypeId, String), String>,
    assocs: assoc::Assocs,
    /// The enums each literal-only type parameter is instantiated at, by
    /// declaration and parameter index: its `in` bound.
    lit_args: HashMap<(Place, String, usize), Vec<abi::Type>>,
    /// A literal-only parameter passed on to another declaration's
    /// (`extends RoundingOptions<Units>`), whose bound then takes in its.
    passed: Vec<(LitParam, LitParam)>,
    /// Namespace functions and values folded into a same-named class's
    /// statics (IR.md §4).
    folded: HashMap<(Place, String), Vec<callables::Overload>>,
    /// The module and first segment of each value a `@js(new = ...)`
    /// constructor runs, which the module keeps exporting.
    new_values: HashSet<(String, String)>,
    diagnostics: Vec<String>,
}

/// An inline payload enum named by its first anchor.
pub(crate) struct Shared {
    /// Every `(owner, anchor)` using it.
    pub anchors: HashSet<(Option<String>, String)>,
    /// The name its arms spell, if short enough.
    pub spelled: String,
    pub key: String,
}

/// A type parameter by declaration and index.
pub(crate) type LitParam = (Place, String, usize);

impl<'a> Lowerer<'a> {
    fn new(module: &'a Module, gctx: &'a GlobalContext, opts: &'a Options) -> Self {
        Lowerer {
            module,
            gctx,
            opts,
            exported: Vec::new(),
            info: HashMap::new(),
            declared: HashSet::new(),
            alias_places: HashMap::new(),
            kept: HashMap::new(),
            fn_aliases: HashSet::new(),
            classes: HashSet::new(),
            // A synthesized name mustn't shadow a lib's global (`Object`).
            taken: opts
                .with
                .iter()
                .flat_map(|l| l.items.iter().filter_map(abi::Decl::name))
                .map(String::from)
                .collect(),
            globals: Container::default(),
            modules: IndexMap::new(),
            stubs: IndexMap::new(),
            records: Default::default(),
            lib_types: lib_types(&opts.with),
            lib_literals: lib_literals(&opts.with),
            with_used: HashSet::new(),
            synth: HashMap::new(),
            enums: HashSet::new(),
            payloads: HashSet::new(),
            pulled: indexmap::IndexSet::new(),
            literals: HashMap::new(),
            open_sets: HashMap::new(),
            anchors: Vec::new(),
            made: HashMap::new(),
            qualified: Vec::new(),
            bounds_used: HashSet::new(),
            pending_assoc: HashMap::new(),
            assoc_names: HashMap::new(),
            assocs: HashMap::new(),
            lit_args: HashMap::new(),
            passed: Vec::new(),
            folded: HashMap::new(),
            new_values: HashSet::new(),
            diagnostics: Vec::new(),
        }
    }

    /// In lib mode, whether `id` is a root declaration, by its global path.
    pub(crate) fn is_root(&self, id: TypeId) -> bool {
        let Some(lib) = &self.opts.lib else {
            return true;
        };
        self.info.get(&id).is_some_and(|i| {
            let mut path = i.place.ns.clone();
            path.push(i.name.clone());
            i.place.module.is_none() && lib.roots.contains(&path.join("."))
        })
    }

    /// Whether `id`, at `place`, belongs in the output: in lib mode, a
    /// root or a namespace holding one.
    fn in_lib(&self, id: TypeId, place: &Place) -> bool {
        let Some(lib) = &self.opts.lib else {
            return true;
        };
        match &self.gctx.get_type(id).kind {
            TypeKind::Namespace(ns) => {
                let mut path = place.ns.clone();
                path.push(ns.name.clone());
                let prefix = format!("{}.", path.join("."));
                place.module.is_none() && lib.roots.iter().any(|r| r.starts_with(&prefix))
            }
            _ => self.is_root(id),
        }
    }

    /// In lib mode, has the enum at `key` emitted if it's outside the roots.
    pub(crate) fn pull(&mut self, key: (Option<String>, Vec<String>)) {
        if self.opts.lib.is_some() {
            self.pulled.insert(key);
        }
    }

    fn emit_pulled(&mut self) {
        let mut done = 0;
        while done < self.pulled.len() {
            let key = self.pulled[done].clone();
            done += 1;
            let found = self.info.iter().find_map(|(&id, i)| {
                let mut path = i.place.ns.clone();
                path.push(i.name.clone());
                (i.place.module == key.0 && path == key.1 && !self.is_root(id))
                    .then(|| (id, i.place.clone()))
            });
            if let Some((id, place)) = found {
                self.decl(id, &place);
            }
        }
    }

    pub(crate) fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.diagnostics.contains(&message) {
            self.diagnostics.push(message);
        }
    }

    /// Record every type declaration's IR name and place before lowering,
    /// so references resolve to it wherever they appear.
    fn index(&mut self) {
        for c in &self.opts.provides.classes {
            self.taken.insert(c.decl.name.clone());
            self.declared.insert((None, vec![c.decl.name.clone()]));
        }
        let ids = self.module.types.clone();
        for id in ids {
            let place = Place::of(&self.gctx.get_type(id).module_context);
            self.index_decl(id, &place);
        }
    }

    fn index_decl(&mut self, id: TypeId, place: &Place) {
        let decl = self.gctx.get_type(id);
        let js_name = match &decl.kind {
            TypeKind::Namespace(ns) => {
                let inner = place.child(&ns.name);
                for &child in &ns.declarations {
                    self.index_decl(child, &inner);
                }
                return;
            }
            TypeKind::Class(c) => &c.js_name,
            TypeKind::Interface(i) => &i.js_name,
            TypeKind::DiscriminatedUnion(d) => &d.js_name,
            TypeKind::StringEnum(e) => &e.name,
            TypeKind::NumericEnum(e) => &e.name,
            TypeKind::TsEnum(e) => &e.name,
            TypeKind::TypeAlias(_) => {
                self.alias_places.insert(id, place.clone());
                return;
            }
            TypeKind::Function(_) | TypeKind::Variable(_) => return,
        };
        if self.info.contains_key(&id) {
            return;
        }
        // Module blocks keep their own locals, so two declarations of one
        // name can share a module.
        let name = names::unique(&names::escape_type(js_name), |n| {
            let mut path = place.ns.clone();
            path.push(n.to_string());
            self.declared.contains(&(place.module.clone(), path))
        });
        let mut path = place.ns.clone();
        path.push(name.clone());
        match &decl.kind {
            TypeKind::Class(_) => {
                self.classes.insert((place.module.clone(), path.clone()));
            }
            TypeKind::StringEnum(e) => {
                self.enums.insert((place.module.clone(), path.clone()));
                let strs = e.variants.iter().map(|v| v.js_value.clone()).collect();
                self.literals
                    .insert((place.module.clone(), path.clone()), strs);
            }
            TypeKind::NumericEnum(_) | TypeKind::TsEnum(_) => {
                self.enums.insert((place.module.clone(), path.clone()));
            }
            _ => {}
        }
        self.declared.insert((place.module.clone(), path));
        self.taken.insert(name.clone());
        self.info.insert(
            id,
            DeclInfo {
                place: place.clone(),
                name,
            },
        );
    }

    fn container(&mut self, place: &Place) -> &mut Container {
        let root = match &place.module {
            None => &mut self.globals,
            Some(m) => self.modules.entry(m.clone()).or_default(),
        };
        root.at(&place.ns)
    }

    fn push(&mut self, place: &Place, decl: abi::Decl) {
        let entries = &mut self.container(place).entries;
        // A redeclared `var` lowers to the same members again.
        let repeat = matches!(decl, abi::Decl::Member(_))
            && entries
                .iter()
                .any(|e| matches!(e, Entry::Decl(d) if **d == decl));
        if !repeat {
            entries.push(Entry::Decl(Box::new(decl)));
        }
    }

    fn decl(&mut self, id: TypeId, place: &Place) {
        let decl = self.gctx.get_type(id);
        let doc = names::doc(&decl.doc);
        match &decl.kind {
            TypeKind::Namespace(ns) => {
                let inner = place.child(&ns.name);
                let fold = self.folds_into_class(place, &ns.name);
                // Make the namespace's container now, so it keeps its place
                // among its siblings.
                self.container(&inner);
                for &child in &ns.declarations {
                    if fold && self.fold(child, place, &ns.name) {
                        continue;
                    }
                    if !self.in_lib(child, &inner) {
                        continue;
                    }
                    self.decl(child, &inner);
                }
            }
            TypeKind::Class(c) => {
                let class = self.class(id, c, doc);
                self.push(place, abi::Decl::Class(class));
            }
            TypeKind::Interface(i) => {
                let class = self.interface(id, i, doc);
                self.push(place, abi::Decl::Class(class));
            }
            TypeKind::DiscriminatedUnion(d) => {
                let class = self.discriminated_union(id, d, doc);
                self.push(place, abi::Decl::Class(class));
            }
            TypeKind::StringEnum(e) => {
                let e = self.string_enum(id, e, doc);
                self.push(place, abi::Decl::Enum(e));
            }
            TypeKind::NumericEnum(e) => {
                let e = self.numeric_enum(id, e, doc);
                self.push(place, abi::Decl::Enum(e));
            }
            TypeKind::TsEnum(e) => {
                self.warn(format!("enum `{}`: unevaluated TS enum skipped", e.name));
            }
            TypeKind::Function(f) => {
                let members = self.function(f, place, doc);
                for o in members {
                    self.push(place, abi::Decl::Member(o.member));
                }
            }
            TypeKind::Variable(v) => {
                let members = self.variable(v, decl.scope_id, place, doc, false);
                for o in members {
                    self.push(place, abi::Decl::Member(o.member));
                }
            }
            // Aliases have no IR declaration: uses read through them, and a
            // union alias becomes an enum where it's used.
            TypeKind::TypeAlias(_) => {}
        }
    }

    /// Whether `place` declares a class or enum named `name`, whose
    /// same-named namespace folds into its statics.
    fn folds_into_class(&self, place: &Place, name: &str) -> bool {
        let mut path = place.ns.clone();
        path.push(names::escape_type(name));
        let key = (place.module.clone(), path);
        self.classes.contains(&key) || self.enums.contains(&key)
    }

    /// Fold a namespace member into the statics of the class it merges
    /// with. Returns whether `id` was folded.
    fn fold(&mut self, id: TypeId, place: &Place, class: &str) -> bool {
        let decl = self.gctx.get_type(id);
        let doc = names::doc(&decl.doc);
        let members = match &decl.kind {
            TypeKind::Function(f) => self.function(f, place, doc),
            TypeKind::Variable(v) => self.variable(v, decl.scope_id, place, doc, true),
            _ => return false,
        };
        let members = members
            .into_iter()
            .map(|mut o| {
                o.member.is_static = true;
                o
            })
            .collect::<Vec<_>>();
        self.folded
            .entry((place.clone(), names::escape_type(class)))
            .or_default()
            .extend(members);
        true
    }

    /// Lower `--provides` into exports: a top-level function is exported
    /// on its own, and one inside an object into an exported namespace.
    fn provide(&mut self) {
        #[derive(Default)]
        struct Tree {
            members: Vec<callables::Overload>,
            children: IndexMap<String, Tree>,
        }
        let mut root = Tree::default();
        for f in &self.opts.provides.fns {
            let leaf = f.leaf().to_string();
            let mut cx = types::Tcx::new(f.scope, &Place::default(), &leaf);
            cx.export = true;
            let members = self.callable(
                callables::Form::Method(leaf),
                false,
                &[],
                callables::Sig {
                    params: &f.params,
                    scope: f.scope,
                },
                Some(&f.return_type),
                &crate::ir::Throws::None,
                f.doc.clone(),
                &cx,
            );
            let mut node = &mut root;
            for seg in &f.path[..f.path.len() - 1] {
                node = node.children.entry(seg.clone()).or_default();
            }
            node.members.extend(members);
        }
        fn emit(tree: Tree, top: bool, diags: &mut Vec<String>) -> Vec<abi::Decl> {
            let mut members = tree.members;
            callables::resolve_conflicts(&mut members, diags);
            let mut out: Vec<abi::Decl> = members
                .into_iter()
                .map(|mut o| {
                    o.member.export = top;
                    abi::Decl::Member(o.member)
                })
                .collect();
            for (name, child) in tree.children {
                out.push(abi::Decl::Namespace(abi::Namespace {
                    meta: abi::Meta::default(),
                    export: top,
                    name,
                    items: emit(child, false, diags),
                }));
            }
            out
        }
        self.exported = emit(root, true, &mut self.diagnostics);
        for c in &self.opts.provides.classes {
            let class = self.export_class(&c.decl, c.doc.clone());
            self.exported.push(abi::Decl::Class(class));
        }
    }

    fn finish(mut self) -> Lowered {
        self.finish_assoc();
        for (key, _) in &self.opts.bounds {
            if !self.bounds_used.contains(key) {
                self.diagnostics.push(format!(
                    "`--bound {key}` names no type parameter, so it is unused"
                ));
            }
        }
        let mut items = Vec::new();
        let globals = std::mem::take(&mut self.globals);
        self.assemble(globals, &Place::default(), &mut items);
        items.append(&mut self.exported);
        for (spec, c) in std::mem::take(&mut self.modules) {
            let place = Place {
                module: Some(spec.clone()),
                ns: Vec::new(),
            };
            let mut inner = Vec::new();
            self.assemble(c, &place, &mut inner);
            // A declaration that isn't lowered, such as an uncallable
            // function, has nothing to export.
            let exports = self.exports(&spec).and_then(|es| {
                let es: Vec<_> = es
                    .into_iter()
                    .filter(|e| {
                        inner.iter().any(|d| js_names(d).any(|n| n == e.name))
                            || self.new_values.contains(&(spec.clone(), e.name.clone()))
                    })
                    .collect();
                (!es.is_empty()).then_some(es)
            });
            type_only(&mut inner, exports.as_deref().unwrap_or_default());
            items.push(abi::Decl::Module(abi::Module {
                meta: abi::Meta::default(),
                spec,
                items: inner,
                exports,
            }));
        }
        for name in std::mem::take(&mut self.records) {
            let key = if name == "Record" {
                abi::Type::Str
            } else {
                abi::Type::Scalar(abi::Scalar::U32)
            };
            let value = abi::Type::named(["V"]);
            items.push(abi::Decl::Class(abi::Class {
                assoc: Vec::new(),
                meta: abi::Meta::default(),
                kind: abi::ClassKind::Interface,
                export: false,
                name: name.to_string(),
                tparams: vec![abi::TypeParam {
                    name: "V".into(),
                    bound: None,
                    default: None,
                }],
                extends: Vec::new(),
                members: callables::index_members(
                    key,
                    abi::Type::Nullable(Box::new(value.clone()), false),
                    Some(value),
                ),
            }));
        }
        let stubs = std::mem::take(&mut self.stubs);
        let arities: HashMap<&str, usize> =
            stubs.iter().map(|(n, (_, a))| (n.as_str(), *a)).collect();
        let renames = self.shared_renames();
        walk::decls(&mut items, &mut |r| {
            if let [.., last] = r.path.segments.as_mut_slice() {
                if let Some(new) = renames.get(last.as_str()) {
                    *last = new.clone();
                }
            }
            pad::type_ref(r, &arities);
        });
        // An overload named for a renamed enum (`modeWithXOrY`) follows it.
        walk::members(&mut items, &mut |m| {
            let name = match &mut m.kind {
                abi::MemberKind::Method(n) | abi::MemberKind::Get(n) | abi::MemberKind::Set(n) => n,
                abi::MemberKind::New(Some(n)) => n,
                abi::MemberKind::New(None) => return,
            };
            if let Some((at, old)) = renames
                .keys()
                .filter_map(|old| name.rfind(&format!("With{old}")).map(|at| (at, old)))
                .find(|(at, old)| {
                    name[at + 4 + old.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| c.is_uppercase())
                })
            {
                name.replace_range(at + 4..at + 4 + old.len(), &renames[old]);
            }
        });
        walk::enums(&mut items, &mut |e| {
            if let Some(new) = renames.get(e.name.as_str()) {
                e.name = new.clone();
            }
            for a in &mut e.arms {
                if let Some(new) = renames.get(a.name.as_str()) {
                    if matches!(a.payload.as_slice(), [abi::Type::Named(r)] if r.path.segments.last() == Some(new))
                    {
                        a.name = new.clone();
                    }
                }
            }
        });
        for (name, (kind, arity)) in stubs {
            items.push(abi::Decl::Class(abi::Class {
                assoc: Vec::new(),
                meta: abi::Meta::default(),
                kind,
                export: false,
                name,
                tparams: names::stub_tparams(arity),
                extends: Vec::new(),
                members: Vec::new(),
            }));
        }
        let locations = self
            .info
            .iter()
            .map(|(&id, info)| {
                let mut path = info.place.ns.clone();
                path.push(info.name.clone());
                let loc = Location {
                    module: info.place.module.clone(),
                    path,
                };
                (id, loc)
            })
            .collect();
        let mut ir = abi::Ir {
            lib: self.opts.lib.as_ref().map(|l| l.name.clone()),
            with: self
                .opts
                .with
                .iter()
                .filter_map(|l| l.lib.clone())
                .filter(|n| self.with_used.contains(n))
                .collect(),
            items,
        };
        indices::number(&mut ir);
        settle_unions(&mut ir, &self.opts.with);
        Lowered {
            ir,
            diagnostics: self.diagnostics,
            locations,
        }
    }

    /// An enum named by its first anchor's name is unique in the whole
    /// output, so its references rename by their last segment.
    fn shared_renames(&mut self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for shared in std::mem::take(&mut self.anchors) {
            if shared.anchors.len() < 2 || shared.spelled.is_empty() {
                continue;
            }
            let Some((_, old)) = self.synth.get(&shared.key) else {
                continue;
            };
            let new = names::unique(&shared.spelled, |n| self.taken.contains(n));
            self.taken.insert(new.clone());
            out.insert(old.clone(), new);
        }
        // A name a renamed enum gives up goes to the first enum that wanted
        // it, as if it had been free then.
        let mut freed: HashSet<String> = out.keys().cloned().collect();
        for (base, name) in std::mem::take(&mut self.qualified) {
            if freed.remove(&base) {
                out.insert(name, base);
            }
        }
        out
    }

    fn assemble(&mut self, c: Container, place: &Place, out: &mut Vec<abi::Decl>) {
        let Container {
            entries,
            mut namespaces,
            synth,
        } = c;
        for entry in entries {
            match entry {
                Entry::Decl(d) => {
                    let mut d = *d;
                    if let abi::Decl::Class(class) = &mut d {
                        class.assoc = self.take_assoc(place, &class.name);
                        for (k, p) in class.tparams.iter_mut().enumerate() {
                            let key = (place.clone(), class.name.clone(), k);
                            if let Some(args) = self.lit_args.get(&key) {
                                p.bound = Some(abi::Bound::In(args.clone()));
                            }
                        }
                        let key = (place.clone(), class.name.clone());
                        if let Some(statics) = self.folded.remove(&key) {
                            let mut all: Vec<callables::Overload> =
                                std::mem::take(&mut class.members)
                                    .into_iter()
                                    .map(callables::Overload::plain)
                                    .chain(statics)
                                    .collect();
                            callables::name_constructs(&mut all);
                            callables::resolve_conflicts(&mut all, &mut self.diagnostics);
                            class.members = all.into_iter().map(|o| o.member).collect();
                        }
                    }
                    if let abi::Decl::Enum(e) = &mut d {
                        if let Some(mut statics) =
                            self.folded.remove(&(place.clone(), e.name.clone()))
                        {
                            callables::resolve_conflicts(&mut statics, &mut self.diagnostics);
                            e.members = statics.into_iter().map(|o| o.member).collect();
                        }
                    }
                    out.push(d);
                }
                Entry::Namespace(name) => {
                    let Some(child) = namespaces.shift_remove(&name) else {
                        continue;
                    };
                    let mut items = Vec::new();
                    self.assemble(child, &place.child(&name), &mut items);
                    if !items.is_empty() {
                        let ir = names::escape_type(&name);
                        let mut meta = abi::Meta::default();
                        meta.attrs.js.extend(names::rename(&ir, &name));
                        out.push(abi::Decl::Namespace(abi::Namespace {
                            meta,
                            export: false,
                            name: ir,
                            items,
                        }));
                    }
                }
            }
        }
        out.extend(synth);
    }

    /// A module block's export list (IR.md §5): each export names the
    /// declaration slot it resolves to. Only declarations with a runtime
    /// value are listed; types alone have nothing to import.
    fn exports(&self, spec: &str) -> Option<Vec<abi::ExportName>> {
        // Each input file of a module is a root of its own.
        let roots = self.module.module_roots.iter().filter(|(s, _)| s == spec);
        let mut out: Vec<abi::ExportName> = Vec::new();
        for (_, root) in roots {
            for (public, local, scope) in self.gctx.exports.bindings(root.exports) {
                let ids = self.gctx.declarations(scope, local);
                let Some(name) = ids.iter().find_map(|&id| self.value_name(id)) else {
                    continue;
                };
                let export = abi::ExportName {
                    alias: (public != name).then(|| public.to_string()),
                    name,
                };
                if !out.contains(&export) {
                    out.push(export);
                }
            }
        }
        (!out.is_empty()).then_some(out)
    }

    fn value_name(&self, id: TypeId) -> Option<String> {
        match &self.gctx.get_type(id).kind {
            TypeKind::Class(_) => self.info.get(&id).map(|i| i.name.clone()),
            TypeKind::Function(f) => Some(f.js_name.clone()),
            TypeKind::Variable(v) => Some(v.js_name.clone()),
            TypeKind::Namespace(n) => Some(names::escape_type(&n.name)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;

/// A declaration's IR name and its JS name.
fn js_names(d: &abi::Decl) -> impl Iterator<Item = &str> {
    let js = d.meta().attrs.js.iter().find_map(|e| match e {
        abi::JsEntry::Name(n) => Some(n.as_str()),
        _ => None,
    });
    d.name().into_iter().chain(js)
}

/// A module declaration no export reaches has no JS value (IR.md §5): a
/// class lowers as an interface, keeping only what works without the value.
fn type_only(items: &mut Vec<abi::Decl>, exports: &[abi::ExportName]) {
    let reached = |d: &abi::Decl| js_names(d).any(|n| exports.iter().any(|e| e.name == n));
    fn strip(d: &mut abi::Decl) -> bool {
        match d {
            abi::Decl::Class(c) => {
                c.kind = abi::ClassKind::Interface;
                c.members.retain(|m| {
                    !m.is_static
                        && match m.kind {
                            abi::MemberKind::New(_) => {
                                m.meta.attrs.js.contains(&abi::JsEntry::Object)
                                    || abi::JsEntries::new_value(&m.meta.attrs.js[..]).is_some()
                            }
                            _ => true,
                        }
                });
                true
            }
            abi::Decl::Namespace(n) => {
                n.items.retain_mut(strip);
                true
            }
            abi::Decl::Member(_) => false,
            abi::Decl::Enum(_) | abi::Decl::Module(_) | abi::Decl::Alias(_) => true,
        }
    }
    items.retain_mut(|d| reached(d) || strip(d));
}
