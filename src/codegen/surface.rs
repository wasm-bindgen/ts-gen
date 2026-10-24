//! The output surface: what the generated bindings expose.
//!
//! It is built by iterating export maps, never by asking whether a
//! declaration is exported. The roots are the selected files' globals and
//! the selected modules; each export of a root (public name → local
//! binding → declaration) is emitted under its public name. A declaration
//! a signature reaches but no root exports is emitted as a type only, so
//! every reference in the output has a declaration to name.
//!
//! A declaration root (`--export Request`) selects one export of every
//! root, so a consumer can take part of a single large file.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::context::{GlobalContext, TypeId};
use crate::ir::{Member, Module, ModuleContext, Root, TypeDeclaration, TypeKind, TypeRef};
use crate::parse::scope::{Binding, ScopeId};

use super::{ExportSpec, GenerateOptions};

/// The declarations emitted, by id, each in the forms it is emitted in: a
/// function or variable once per public name, anything else once.
pub struct Surface<'d> {
    items: HashMap<TypeId, Vec<Cow<'d, TypeDeclaration>>>,
    names: HashMap<TypeId, Vec<String>>,
    mod_names: HashMap<TypeId, String>,
    type_names: HashMap<TypeId, String>,
    /// Declaration roots no root exports.
    pub missing: Vec<String>,
}

impl<'d> Surface<'d> {
    /// Select the roots from `options` and build their surface. No
    /// selection means every input file's globals.
    pub fn build(module: &Module, gctx: &'d GlobalContext, options: &GenerateOptions) -> Self {
        let externalised = super::typemap::externalised_modules(module, gctx);
        let mut builder = Builder {
            gctx,
            module,
            externalised: &externalised,
            universe: HashSet::new(),
            parents: HashMap::new(),
            items: HashMap::new(),
            names: HashMap::new(),
            queue: Vec::new(),
        };
        builder.index(&module.types, None);
        let all = options.exports.is_empty();
        for (path, root) in &module.file_roots {
            if all || options.exports.contains(&ExportSpec::File(path.clone())) {
                builder.root(*root);
            }
        }
        for (specifier, root) in &module.module_roots {
            if !externalised.contains(specifier)
                && options
                    .exports
                    .contains(&ExportSpec::Module(specifier.clone()))
            {
                builder.root(*root);
            }
        }
        let mut missing = Vec::new();
        for spec in &options.exports {
            if let ExportSpec::Decl(path) = spec {
                if !builder.select(path) {
                    missing.push(path.clone());
                }
            }
        }
        builder.close();
        let mod_names = builder.name_namespaces();
        let type_names = builder.name_types();
        Surface {
            items: builder.items,
            names: builder.names,
            mod_names,
            type_names,
            missing,
        }
    }

    /// The types emitted under another Rust name than their own: one its
    /// container doesn't export takes a suffix where an exported one has
    /// its name, as a namespace does (see [`Surface::mod_name`]).
    pub fn type_names(&self) -> &HashMap<TypeId, String> {
        &self.type_names
    }

    /// The forms `id` is emitted in, empty if it is not emitted.
    pub fn get(&self, id: TypeId) -> &[Cow<'d, TypeDeclaration>] {
        self.items.get(&id).map_or(&[], Vec::as_slice)
    }

    /// The public names a type is exported under, in export order; empty
    /// for a value, which has a form per name instead, and for a type only
    /// a signature reaches. A namespace's are those its own container
    /// exports it under, whether or not that container is a root.
    pub fn names(&self, id: TypeId) -> &[String] {
        self.names.get(&id).map_or(&[], Vec::as_slice)
    }

    /// The name the type `id`, whose own name is `own`, binds by in JS: its
    /// own when exported under it or not exported, else the first it is
    /// exported under (`export { make as create }` makes `create.helper`).
    pub fn js_name<'n>(&'n self, id: TypeId, own: &'n str) -> &'n str {
        match self.names(id) {
            [first, ..] if !self.names(id).iter().any(|n| n == own) => first,
            _ => own,
        }
    }

    /// The `pub mod` name of the namespace `id`, whose own name is `own`:
    /// its own when exported under it or not exported by name, else the
    /// first name it is exported under (`export { N as M }` makes `m`).
    /// Each other public name aliases it. A namespace its container doesn't
    /// export by name takes a suffix where an exported one has its name.
    pub fn mod_name(&self, id: TypeId, own: &str) -> String {
        self.mod_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| exported_mod_name(&[], own))
    }

    /// The JS module specifiers with an emitted declaration.
    pub fn modules(&self) -> HashSet<String> {
        self.items
            .values()
            .flatten()
            .filter_map(|decl| decl.module_context.specifier())
            .map(|m| m.to_string())
            .collect()
    }
}

struct Builder<'a, 'd> {
    gctx: &'d GlobalContext,
    module: &'a Module,
    externalised: &'a HashSet<String>,
    /// The input declarations, namespace members included: nothing else is
    /// emitted.
    universe: HashSet<TypeId>,
    /// Each namespace member's namespace.
    parents: HashMap<TypeId, TypeId>,
    items: HashMap<TypeId, Vec<Cow<'d, TypeDeclaration>>>,
    names: HashMap<TypeId, Vec<String>>,
    /// Emitted declarations whose signatures are still to be followed.
    queue: Vec<TypeId>,
}

impl<'d> Builder<'_, 'd> {
    fn index(&mut self, ids: &[TypeId], parent: Option<TypeId>) {
        for &id in ids {
            self.universe.insert(id);
            if let Some(parent) = parent {
                self.parents.insert(id, parent);
            }
            if let TypeKind::Namespace(ns) = &self.gctx.get_type(id).kind {
                self.index(&ns.declarations, Some(id));
            }
        }
    }

    fn emittable(&self, id: TypeId) -> bool {
        self.universe.contains(&id)
            && self
                .gctx
                .get_type(id)
                .module_context
                .specifier()
                .is_none_or(|m| !self.externalised.contains(m))
    }

    /// Emit every export of `root` under its public name. The declarations
    /// an export refers to are those binding its local name; a binding from
    /// outside the block, such as an import, is re-exported through the
    /// alias the export declares.
    fn root(&mut self, root: Root) {
        let gctx = self.gctx;
        let mut entries: Vec<_> = gctx.exports.bindings(root.exports).collect();
        // A default export takes the local's Rust name only when no named
        // export of the same declaration does.
        entries.sort_by_key(|&(public, _, _)| public == "default");
        for (public, local, bindings) in entries {
            // An alias evaluation reads through (instantiated at every use,
            // or a literal alias only constraining literal-only parameters)
            // is named by no use; a signature that still names it reaches
            // it, and `--export` can select it.
            let ids = gctx.declarations(bound_in(root, bindings), local);
            if !ids.is_empty() && ids.iter().all(|id| gctx.type_level_only.contains(id)) {
                continue;
            }
            self.entry(root, public, local, bindings);
        }
    }

    /// Emit the export `public` of `root`, bound to `local` in `bindings`.
    fn entry(&mut self, root: Root, public: &str, local: &str, bindings: ScopeId) {
        let gctx = self.gctx;
        let bindings = bound_in(root, bindings);
        let ids = gctx.declarations(bindings, local);
        if ids.is_empty() {
            let target = TypeRef::ident(local.to_string());
            for &id in gctx.declarations(bindings, public) {
                if matches!(&gctx.get_type(id).kind, TypeKind::TypeAlias(a) if a.target == target) {
                    self.export(id, public, public);
                }
            }
        }
        for &id in ids {
            self.export(id, public, local);
        }
    }

    /// Emit the export `path` of every root that is not externalised,
    /// selected or not, as if its root were selected with only that export
    /// in it. A dotted path goes through a namespace's exports, and the
    /// namespaces come along as the `pub mod` path to it. Returns whether
    /// any root exports it.
    fn select(&mut self, path: &str) -> bool {
        let roots: Vec<Root> = self
            .module
            .file_roots
            .iter()
            .map(|(_, root)| *root)
            .chain(
                self.module
                    .module_roots
                    .iter()
                    .filter(|(specifier, _)| !self.externalised.contains(specifier))
                    .map(|(_, root)| *root),
            )
            .collect();
        let mut found = false;
        for root in roots {
            found |= self.select_in(root, path);
        }
        found
    }

    fn select_in(&mut self, root: Root, path: &str) -> bool {
        let gctx = self.gctx;
        let (public, rest) = match path.split_once('.') {
            Some((head, rest)) => (head, Some(rest)),
            None => (path, None),
        };
        let Some((local, bindings)) = gctx.exports.binding(root.exports, public) else {
            return false;
        };
        let Some(rest) = rest else {
            self.entry(root, public, local, bindings);
            for &id in gctx.declarations(bound_in(root, bindings), local) {
                self.contain(id);
            }
            return true;
        };
        let mut found = false;
        for &id in gctx.declarations(bound_in(root, bindings), local) {
            if let TypeKind::Namespace(ns) = &gctx.get_type(id).kind {
                let members = Root {
                    exports: ns.child_scope,
                    bindings: ns.child_scope,
                };
                found |= self.select_in(members, rest);
            }
        }
        found
    }

    /// Emit the namespaces enclosing the emitted `id` as the `pub mod` path
    /// to it.
    fn contain(&mut self, id: TypeId) {
        if !self.items.contains_key(&id) {
            return;
        }
        let mut parent = self.parents.get(&id).copied();
        while let Some(ns) = parent {
            self.items
                .entry(ns)
                .or_insert_with(|| vec![Cow::Borrowed(self.gctx.get_type(ns))]);
            parent = self.parents.get(&ns).copied();
        }
    }

    /// Emit `id`, the declaration binding `local`, as the export `public`.
    fn export(&mut self, id: TypeId, public: &str, local: &str) {
        if !self.emittable(id) {
            return;
        }
        let decl = self.gctx.get_type(id);
        let first = !self.items.contains_key(&id);
        match &decl.kind {
            TypeKind::Function(_) | TypeKind::Variable(_) => self.export_value(id, decl, public),
            _ => self.export_type(id, decl, public, local),
        }
        if first {
            self.queue.push(id);
            if let TypeKind::Namespace(ns) = &decl.kind {
                self.root(Root {
                    exports: ns.child_scope,
                    bindings: ns.child_scope,
                });
            }
        }
    }

    /// A function or variable has a form per public name, bound by it.
    fn export_value(&mut self, id: TypeId, decl: &'d TypeDeclaration, public: &str) {
        let forms = self.items.entry(id).or_default();
        let rust_name = |local: &str| {
            let name = public_rust_name(local, public);
            let taken = forms.iter().any(|d| match &d.kind {
                TypeKind::Function(f) => f.name == name,
                TypeKind::Variable(v) => v.name == name,
                _ => false,
            });
            if taken {
                crate::util::naming::to_snake_case(public)
            } else {
                name
            }
        };
        let form = match &decl.kind {
            TypeKind::Function(f) if f.js_name != public => Cow::Owned(TypeDeclaration {
                kind: TypeKind::Function(crate::ir::FunctionDecl {
                    name: rust_name(&f.js_name),
                    js_name: public.to_string(),
                    ..f.clone()
                }),
                ..decl.clone()
            }),
            TypeKind::Variable(v) if v.js_name != public => Cow::Owned(TypeDeclaration {
                kind: TypeKind::Variable(crate::ir::VariableDecl {
                    name: rust_name(&v.js_name),
                    js_name: public.to_string(),
                    ..v.clone()
                }),
                ..decl.clone()
            }),
            _ => Cow::Borrowed(decl),
        };
        let js_name = |d: &TypeDeclaration| match &d.kind {
            TypeKind::Function(f) => Some(f.js_name.clone()),
            TypeKind::Variable(v) => Some(v.js_name.clone()),
            _ => None,
        };
        if forms.iter().all(|f| js_name(f) != js_name(&form)) {
            forms.push(form);
        }
    }

    /// A type is emitted once, under its own Rust name. A class binds by
    /// its own name when exported under it, else by its first public name;
    /// a namespace likewise (see [`Surface::js_name`]). Each other public
    /// name but `default`, which has no name of its own, aliases it.
    fn export_type(&mut self, id: TypeId, decl: &'d TypeDeclaration, public: &str, local: &str) {
        let names = self.names.entry(id).or_default();
        if names.iter().any(|n| n == public) {
            return;
        }
        names.push(public.to_string());
        let first_name = names.len() == 1;
        let forms = self.items.entry(id).or_default();
        if forms.is_empty() {
            forms.push(Cow::Borrowed(decl));
        }
        if let TypeKind::Class(c) = &decl.kind {
            let bound = matches!(&forms[0].kind, TypeKind::Class(f) if f.js_name == public);
            if (first_name || public == c.name) && !bound {
                if let TypeKind::Class(f) = &mut forms[0].to_mut().kind {
                    f.js_name = public.to_string();
                }
            }
        }
        let own = super::typemap::type_decl_name(&decl.kind);
        if matches!(decl.kind, TypeKind::Namespace(_)) || public == "default" || own == Some(public)
        {
            return;
        }
        forms.push(Cow::Owned(TypeDeclaration {
            kind: TypeKind::TypeAlias(crate::ir::TypeAliasDecl {
                name: public.to_string(),
                type_params: vec![],
                target: TypeRef::ident(local.to_string()),
                from_module: None,
                body_scope: decl.scope_id,
            }),
            module_context: decl.module_context.clone(),
            doc: None,
            scope_id: decl.scope_id,
        }));
    }

    /// Emit `id`, reached through a signature, as a type: a class without
    /// its constructor and statics, which bind through the class's import.
    /// Values never enter this way. Its namespaces come along as `pub mod`
    /// paths holding only what is emitted.
    fn reach(&mut self, id: TypeId) {
        if self.items.contains_key(&id) || !self.emittable(id) {
            return;
        }
        let decl = self.gctx.get_type(id);
        let form = match &decl.kind {
            TypeKind::Class(c) => {
                let mut c = c.clone();
                c.members.retain(|m| {
                    !matches!(
                        m,
                        Member::Constructor(_)
                            | Member::StaticGetter(_)
                            | Member::StaticSetter(_)
                            | Member::StaticMethod(_)
                    )
                });
                Cow::Owned(TypeDeclaration {
                    kind: TypeKind::Class(c),
                    ..decl.clone()
                })
            }
            TypeKind::Interface(_)
            | TypeKind::TypeAlias(_)
            | TypeKind::DiscriminatedUnion(_)
            | TypeKind::StringEnum(_)
            | TypeKind::NumericEnum(_)
            | TypeKind::TsEnum(_) => Cow::Borrowed(decl),
            TypeKind::Function(_) | TypeKind::Variable(_) | TypeKind::Namespace(_) => return,
        };
        self.items.insert(id, vec![form]);
        self.queue.push(id);
        self.contain(id);
    }

    /// Name each emitted namespace by the exports of its own container, a
    /// module or namespace, whichever roots are selected, and give each its
    /// `pub mod` name. As in TypeScript a container's exports and locals
    /// are separate, but Rust has one namespace: the exported namespaces
    /// and their aliases take their names, and one not exported by name (a
    /// local or the default export) takes a suffix where its name is taken.
    fn name_namespaces(&mut self) -> HashMap<TypeId, String> {
        let gctx = self.gctx;
        let mut namespaces: Vec<TypeId> = self
            .items
            .keys()
            .copied()
            .filter(|&id| matches!(gctx.get_type(id).kind, TypeKind::Namespace(_)))
            .collect();
        namespaces.sort_by_key(|id| id.index());
        // A namespace's container: its namespace, or its module's top level.
        type Container = (Option<TypeId>, ModuleContext);
        let mut containers: Vec<(Container, Vec<TypeId>)> = Vec::new();
        for &id in &namespaces {
            let parent = self.parents.get(&id).copied();
            let context = gctx.get_type(id).module_context.clone();
            let roots: Vec<Root> = match parent.map(|p| &gctx.get_type(p).kind) {
                Some(TypeKind::Namespace(ns)) => vec![Root {
                    exports: ns.child_scope,
                    bindings: ns.child_scope,
                }],
                Some(_) => vec![],
                None => self
                    .module
                    .module_roots
                    .iter()
                    .filter(|(specifier, _)| context.specifier() == Some(specifier.as_str()))
                    .map(|(_, root)| *root)
                    .collect(),
            };
            if parent.is_some() || context.specifier().is_some() {
                let mut names: Vec<String> = Vec::new();
                for root in roots {
                    for (public, local, bindings) in gctx.exports.bindings(root.exports) {
                        let bindings = if bindings == root.exports {
                            root.bindings
                        } else {
                            bindings
                        };
                        if gctx.declarations(bindings, local).contains(&id)
                            && !names.iter().any(|n| n == public)
                        {
                            names.push(public.to_string());
                        }
                    }
                }
                names.sort_by_key(|n| n == "default");
                self.names.insert(id, names);
            }
            let key = (parent, context);
            match containers.iter_mut().find(|(k, _)| *k == key) {
                Some((_, ids)) => ids.push(id),
                None => containers.push((key, vec![id])),
            }
        }
        let mut mod_names = HashMap::new();
        for (_, ids) in containers {
            let mut taken = crate::util::naming::LocalScope::default();
            let own = |id: TypeId| match &gctx.get_type(id).kind {
                TypeKind::Namespace(ns) => ns.name.clone(),
                _ => unreachable!(),
            };
            let (exported, local): (Vec<_>, Vec<_>) = ids.into_iter().partition(|id| {
                self.names
                    .get(id)
                    .is_some_and(|names| names.iter().any(|n| n != "default"))
            });
            // Exported namespaces whose Rust names coincide (`Ns` and `ns`)
            // share a module, which holds their distinct bindings.
            for id in exported {
                let names = &self.names[&id];
                let name = exported_mod_name(names, &own(id));
                taken.claim(name.clone());
                for public in names.iter().filter(|&p| p != "default") {
                    taken.claim(crate::util::naming::to_snake_case(public));
                }
                mod_names.insert(id, name);
            }
            for id in local {
                let name = taken.declare(
                    &crate::util::naming::to_snake_case(&own(id)),
                    crate::util::naming::Suffix::Snake,
                );
                mod_names.insert(id, name);
            }
        }
        mod_names
    }

    /// Rename each emitted type its container doesn't export, which only a
    /// signature reaches, whose name an exported type of the container has
    /// (a block's local `interface H` beside another block's exported `H`).
    fn name_types(&mut self) -> HashMap<TypeId, String> {
        let gctx = self.gctx;
        let mut types: Vec<TypeId> = self
            .items
            .keys()
            .copied()
            .filter(|&id| {
                !matches!(
                    gctx.get_type(id).kind,
                    TypeKind::Namespace(_) | TypeKind::Function(_) | TypeKind::Variable(_)
                )
            })
            .collect();
        types.sort_by_key(|id| id.index());
        let mut containers: HashMap<(Option<TypeId>, ModuleContext), Vec<TypeId>> = HashMap::new();
        for id in types {
            let key = (
                self.parents.get(&id).copied(),
                gctx.get_type(id).module_context.clone(),
            );
            containers.entry(key).or_default().push(id);
        }
        let own = |id: TypeId| super::typemap::type_decl_name(&gctx.get_type(id).kind).unwrap();
        let mut renamed = HashMap::new();
        for ids in containers.into_values() {
            let mut taken = crate::util::naming::LocalScope::default();
            let (exported, local): (Vec<_>, Vec<_>) = ids
                .into_iter()
                .partition(|id| self.names.get(id).is_some_and(|n| !n.is_empty()));
            for id in exported {
                taken.claim(own(id));
                for public in self.names[&id].iter().filter(|&p| p != "default") {
                    taken.claim(public.clone());
                }
            }
            for id in local {
                let name = taken.declare(own(id), crate::util::naming::Suffix::Pascal);
                if name != own(id) {
                    if let Some(form) = self.items.get_mut(&id).and_then(|f| f.first_mut()) {
                        rename_type(&mut form.to_mut().kind, &name);
                    }
                    renamed.insert(id, name);
                }
            }
        }
        renamed
    }

    /// Follow the signatures of everything emitted to the declarations
    /// they name.
    fn close(&mut self) {
        while let Some(id) = self.queue.pop() {
            let decl = self.items[&id][0].clone();
            let mut refs = Vec::new();
            declaration_refs(&decl, &mut refs);
            for (name, scope) in refs {
                if let Some(target) = self.resolve(&name, scope) {
                    self.reach(target);
                }
            }
        }
    }

    /// Resolve a type name as codegen does, from its use site only.
    fn resolve(&self, name: &str, scope: ScopeId) -> Option<TypeId> {
        let gctx = self.gctx;
        let head = name.split('.').next().unwrap_or(name);
        if matches!(
            gctx.scopes.resolve_binding(scope, head),
            Some(Binding::TypeParam)
        ) {
            return None;
        }
        gctx.resolve_path(scope, name)
    }
}

/// The `pub mod` name of a namespace whose own name is `own`, exported as
/// `names`: see [`Surface::mod_name`].
fn exported_mod_name(names: &[String], own: &str) -> String {
    let name = match names.iter().find(|&n| n != "default") {
        Some(first) if !names.iter().any(|n| n == own) => first,
        _ => own,
    };
    crate::util::naming::to_snake_case(name)
}

/// The Rust name of a function or variable `local` exported as `public`.
fn public_rust_name(local: &str, public: &str) -> String {
    crate::util::naming::to_snake_case(crate::parse::first_pass::rust_name(local, public))
}

/// Every type name `decl` refers to, with the scope it is resolved from.
fn rename_type(kind: &mut TypeKind, name: &str) {
    let own = match kind {
        TypeKind::Class(c) => &mut c.name,
        TypeKind::Interface(i) => &mut i.name,
        TypeKind::DiscriminatedUnion(d) => &mut d.name,
        TypeKind::TypeAlias(a) => &mut a.name,
        TypeKind::StringEnum(e) => &mut e.name,
        TypeKind::NumericEnum(e) => &mut e.name,
        TypeKind::TsEnum(e) => &mut e.name,
        TypeKind::Function(_) | TypeKind::Variable(_) | TypeKind::Namespace(_) => return,
    };
    *own = name.to_string();
}

/// The scope an export of `root` bound in `bindings` resolves its local in:
/// the root's own bindings for an entry of its export map's own scope.
fn bound_in(root: Root, bindings: ScopeId) -> ScopeId {
    if bindings == root.exports {
        root.bindings
    } else {
        bindings
    }
}

fn declaration_refs(decl: &TypeDeclaration, out: &mut Vec<(String, ScopeId)>) {
    let mut walk = |ty: &TypeRef, scope: ScopeId| type_refs(ty, scope, out);
    match &decl.kind {
        TypeKind::Class(c) => {
            type_params(&c.type_params, c.body_scope, &mut walk);
            if let Some(parent) = &c.extends {
                walk(&parent.type_ref, parent.scope);
            }
            for ty in &c.implements {
                walk(ty, c.body_scope);
            }
            members(&c.members, &mut walk);
        }
        TypeKind::Interface(i) => {
            type_params(&i.type_params, i.body_scope, &mut walk);
            for parent in &i.extends {
                walk(&parent.type_ref, parent.scope);
            }
            members(&i.members, &mut walk);
        }
        TypeKind::DiscriminatedUnion(d) => {
            type_params(&d.type_params, d.body_scope, &mut walk);
            members(&d.members, &mut walk);
            for branch in &d.branches {
                members(branch, &mut walk);
            }
        }
        TypeKind::TypeAlias(a) => {
            type_params(&a.type_params, a.body_scope, &mut walk);
            walk(&a.target, a.body_scope);
        }
        TypeKind::Function(f) => function(f, &mut walk),
        TypeKind::Variable(v) => {
            walk(&v.type_ref, decl.scope_id);
            for f in &v.constructs {
                function(f, &mut walk);
            }
        }
        TypeKind::StringEnum(_)
        | TypeKind::NumericEnum(_)
        | TypeKind::TsEnum(_)
        | TypeKind::Namespace(_) => {}
    }
}

fn function(f: &crate::ir::FunctionDecl, walk: &mut impl FnMut(&TypeRef, ScopeId)) {
    type_params(&f.type_params, f.body_scope, walk);
    for p in &f.params {
        walk(&p.type_ref, f.body_scope);
    }
    walk(&f.return_type, f.body_scope);
    for o in &f.overloads {
        type_params(&o.type_params, o.body_scope, walk);
        for p in &o.params {
            walk(&p.type_ref, o.body_scope);
        }
        walk(&o.return_type, o.body_scope);
    }
    if let Some(ty) = f.throws.as_type() {
        walk(ty, f.body_scope);
    }
}

fn type_params(
    params: &[crate::ir::TypeParam],
    scope: ScopeId,
    walk: &mut impl FnMut(&TypeRef, ScopeId),
) {
    for p in params {
        for ty in p.constraint.iter().chain(&p.default) {
            walk(ty, scope);
        }
    }
}

fn members(members: &[Member], walk: &mut impl FnMut(&TypeRef, ScopeId)) {
    for m in members {
        match m {
            Member::Getter(g) => walk(&g.type_ref, g.scope),
            Member::Setter(s) => walk(&s.type_ref, s.scope),
            Member::StaticGetter(g) => walk(&g.type_ref, g.scope),
            Member::StaticSetter(s) => walk(&s.type_ref, s.scope),
            Member::IndexSignature(i) => {
                walk(&i.key_type, i.scope);
                walk(&i.value_type, i.scope);
            }
            Member::Constructor(c) => {
                for p in &c.params {
                    walk(&p.type_ref, c.scope);
                }
                if let Some(ty) = c.throws.as_type() {
                    walk(ty, c.scope);
                }
            }
            Member::Method(m) => {
                type_params(&m.type_params, m.body_scope, walk);
                for p in &m.params {
                    walk(&p.type_ref, m.body_scope);
                }
                walk(&m.return_type, m.body_scope);
                if let Some(ty) = m.throws.as_type() {
                    walk(ty, m.body_scope);
                }
            }
            Member::StaticMethod(m) => {
                type_params(&m.type_params, m.body_scope, walk);
                for p in &m.params {
                    walk(&p.type_ref, m.body_scope);
                }
                walk(&m.return_type, m.body_scope);
                if let Some(ty) = m.throws.as_type() {
                    walk(ty, m.body_scope);
                }
            }
        }
    }
}

fn type_refs(ty: &TypeRef, scope: ScopeId, out: &mut Vec<(String, ScopeId)>) {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            out.push((segments.join("."), scope));
            for arg in generic_args {
                type_refs(arg, scope, out);
            }
        }
        TypeRef::Array(inner) => type_refs(inner, scope, out),
        TypeRef::Union(u) => {
            for m in u.members() {
                type_refs(m, scope, out);
            }
        }
        TypeRef::Intersection(members) | TypeRef::Tuple(members) => {
            for m in members {
                type_refs(m, scope, out);
            }
        }
        TypeRef::Function(sig) => {
            for p in &sig.params {
                type_refs(&p.type_ref, scope, out);
            }
            type_refs(&sig.return_type, scope, out);
        }
        TypeRef::Boolean
        | TypeRef::Number
        | TypeRef::String
        | TypeRef::BigInt
        | TypeRef::Void
        | TypeRef::Undefined
        | TypeRef::Null
        | TypeRef::Any
        | TypeRef::Unknown
        | TypeRef::Object
        | TypeRef::Symbol
        | TypeRef::ArrayBufferView
        | TypeRef::PropertyKey
        | TypeRef::TypedSlice(..)
        | TypeRef::StringLiteral(_)
        | TypeRef::NumberLiteral(_)
        | TypeRef::BooleanLiteral(_)
        | TypeRef::Never
        | TypeRef::KeyOf(_)
        | TypeRef::IndexedAccess(..)
        | TypeRef::Mapped(_)
        | TypeRef::ObjectLiteral(_)
        | TypeRef::Unresolved(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface_of<'d>(module: &Module, gctx: &'d GlobalContext, specifier: &str) -> Surface<'d> {
        let exports = HashSet::from([ExportSpec::Module(specifier.to_string())]);
        Surface::build(module, gctx, &GenerateOptions::new().exports(exports))
    }

    /// Every declaration of the input, namespace members included, by name.
    fn find(module: &Module, gctx: &GlobalContext, name: &str) -> TypeId {
        let mut stack = module.types.clone();
        while let Some(id) = stack.pop() {
            let kind = &gctx.get_type(id).kind;
            if crate::parse::binding_name(kind).as_deref() == Some(name) {
                return id;
            }
            if let TypeKind::Namespace(ns) = kind {
                stack.extend(&ns.declarations);
            }
        }
        panic!("no declaration {name}");
    }

    #[test]
    fn signatures_reach_types_but_not_values() {
        let src = r#"
            declare class K { constructor(); static make(): K; k(): void; }
            interface Opts { o: string }
            type Alias = Opts;
            interface Unused {}
            type UnusedAlias = string;
            declare function hidden(): void;
            declare const v: number;
            declare namespace NS { interface Inner {} function nf(): void; }
            export declare function f(k: K, a: Alias, i: NS.Inner): void;
            export {};
        "#;
        let (module, gctx) = crate::parse_source(src, Some("m")).unwrap();
        let surface = surface_of(&module, &gctx, "m");
        let emitted = |name| !surface.get(find(&module, &gctx, name)).is_empty();
        for name in ["f", "K", "Alias", "Opts", "NS", "Inner"] {
            assert!(emitted(name), "{name}");
        }
        for name in ["Unused", "UnusedAlias", "hidden", "v", "nf"] {
            assert!(!emitted(name), "{name}");
        }
        let TypeKind::Class(k) = &surface.get(find(&module, &gctx, "K"))[0].kind else {
            panic!("K is a class");
        };
        assert!(k.members.iter().all(|m| matches!(m, Member::Method(_))));
    }

    #[test]
    fn declaration_roots_select_one_export_of_any_root() {
        let src = r#"
            interface Opts { o: string }
            interface Unused {}
            declare function f(o: Opts): void;
            declare function g(): void;
            declare namespace NS { interface Inner {} interface Other {} }
            declare module "m" {
                function connect(): void;
                function other(): void;
            }
        "#;
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let exports = ["f", "NS.Inner", "connect", "Nope"]
            .map(|name| ExportSpec::Decl(name.to_string()))
            .into_iter()
            .collect();
        let surface = Surface::build(&module, &gctx, &GenerateOptions::new().exports(exports));
        let emitted = |name| !surface.get(find(&module, &gctx, name)).is_empty();
        for name in ["f", "Opts", "NS", "Inner", "connect"] {
            assert!(emitted(name), "{name}");
        }
        for name in ["Unused", "g", "Other", "other"] {
            assert!(!emitted(name), "{name}");
        }
        assert_eq!(surface.missing, ["Nope"]);
    }

    #[test]
    fn types_bind_by_their_export() {
        let src = r#"
            interface Foo { internal: number }
            interface _Bar { b: number }
            declare namespace _N { function f(): void; }
            declare class _C {}
            declare class _D {}
            export { _Bar as Foo, _N as N1, _N as N2, _C as C1, _C, _C as C2, _D as D };
        "#;
        let (module, gctx) = crate::parse_source(src, Some("m")).unwrap();
        let surface = surface_of(&module, &gctx, "m");
        let id = |name| find(&module, &gctx, name);
        // A local declaration sharing a public name is not that export.
        assert!(surface.get(id("Foo")).is_empty());
        assert_eq!(surface.names(id("_Bar")), ["Foo"]);
        assert_eq!(surface.js_name(id("_N"), "_N"), "N1");
        assert_eq!(surface.mod_name(id("_N"), "_N"), "n1");
        // A class binds by its own name when exported under it, and each
        // other public name aliases it.
        let class_and_aliases = |name| -> Vec<String> {
            surface
                .get(id(name))
                .iter()
                .map(|d| match &d.kind {
                    TypeKind::Class(c) => c.js_name.clone(),
                    TypeKind::TypeAlias(a) => format!("type {}", a.name),
                    _ => panic!("a class or alias"),
                })
                .collect()
        };
        assert_eq!(class_and_aliases("_C"), ["_C", "type C1", "type C2"]);
        assert_eq!(class_and_aliases("_D"), ["D", "type D"]);
    }

    #[test]
    fn namespaces_take_their_module_name_from_the_export() {
        let src = r#"
            declare function make(): void;
            declare namespace make { function helper(): void; }
            declare function own(): void;
            declare namespace own { function helper(): void; }
            declare namespace D { function f(): void; }
            declare namespace Hidden { interface T {} }
            export declare function f(t: Hidden.T): void;
            export { make as create, own, own as other, D as default };
        "#;
        let (module, gctx) = crate::parse_source(src, Some("m")).unwrap();
        let surface = surface_of(&module, &gctx, "m");
        let id = |name| {
            let mut stack = module.types.clone();
            while let Some(id) = stack.pop() {
                if let TypeKind::Namespace(ns) = &gctx.get_type(id).kind {
                    if ns.name == name {
                        return id;
                    }
                    stack.extend(&ns.declarations);
                }
            }
            panic!("no namespace {name}");
        };
        assert_eq!(surface.mod_name(id("make"), "make"), "create");
        assert_eq!(surface.mod_name(id("own"), "own"), "own");
        // `default` is no name of its own, and a namespace only a signature
        // reaches has no public name.
        assert_eq!(surface.mod_name(id("D"), "D"), "d");
        assert_eq!(surface.mod_name(id("Hidden"), "Hidden"), "hidden");
    }

    #[test]
    fn values_are_emitted_under_each_public_name() {
        /// The (Rust name, JS name) pairs `name` is bound under.
        fn forms(src: &str, name: &str) -> Vec<(String, String)> {
            let (module, gctx) = crate::parse_source(src, Some("m")).unwrap();
            let surface = surface_of(&module, &gctx, "m");
            surface
                .get(find(&module, &gctx, name))
                .iter()
                .map(|d| match &d.kind {
                    TypeKind::Function(f) => (f.name.clone(), f.js_name.clone()),
                    _ => panic!("a function"),
                })
                .collect()
        }
        let pairs = |p: &[(&str, &str)]| -> Vec<(String, String)> {
            p.iter().map(|&(a, b)| (a.into(), b.into())).collect()
        };
        let src = r#"
            declare function _helper(): void;
            export { _helper as a, _helper as b, _helper as default };
        "#;
        assert_eq!(
            forms(src, "_helper"),
            pairs(&[("a", "a"), ("b", "b"), ("helper", "default")])
        );
        // A default export has no name of its own, so it keeps the local
        // one, and its local name is not exported.
        assert_eq!(
            forms("export default function f(): void;", "f"),
            pairs(&[("f", "default")])
        );
    }
}
