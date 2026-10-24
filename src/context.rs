//! Global context: owns all arenas and provides the central data store
//! for the entire parse → codegen pipeline.
//!
//! The `GlobalContext` is created once, mutated during parsing (Phase 1+2),
//! then borrowed immutably for codegen.

use crate::external_map::ExternalMap;
use crate::ir::TypeDeclaration;
use crate::parse::scope::{PendingImport, ScopeArena, ScopeId};
use crate::util::diagnostics::DiagnosticCollector;

/// Well-typed index into the type arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeId(u32);

impl TypeId {
    /// Convert to a `usize` for indexing into slices.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Well-typed index into the module registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModuleId(u32);

/// A parsed source file / module.
#[derive(Clone, Debug)]
pub struct ParsedModule {
    /// The module specifier (e.g. `"node:buffer"`, `"es-module-lexer"`).
    pub specifier: String,
    /// The file scope in the arena.
    pub scope: ScopeId,
    /// Type ids owned by this module.
    pub types: Vec<TypeId>,
}

/// The global data store shared across parsing and codegen.
///
/// Owns:
/// - `ScopeArena`: type scopes (file, namespace, module, builtin)
/// - Type arena: type definitions
/// - Module registry: parsed source files
/// - `ExternalMap`: external type mappings
/// - `DiagnosticCollector`: warnings and info messages
///
/// Arenas are append-only during parsing and read-only during codegen.
#[derive(Clone, Debug)]
pub struct GlobalContext {
    pub scopes: ScopeArena,
    pub diagnostics: DiagnosticCollector,
    pub external_map: ExternalMap,
    /// Pending imports that need to be resolved during import resolution.
    pub pending_imports: Vec<PendingImport>,
    /// `TypeId`s synthesised by `populate_builtin_scope` for global
    /// intrinsics (`Error`, `Promise`, …) and web-platform types
    /// (`ReadableStream`, `Headers`, …). Codegen short-circuits these
    /// to bare-ident emission — they're either covered by `use js_sys::*`
    /// or expected to come from a `--external` mapping at the top of the
    /// generated file. Treating them as "local" would mark them for
    /// emission; treating them as "unresolved" would alias them to
    /// `JsValue` even though `js_sys` already provides them.
    pub builtin_type_ids: std::collections::HashSet<TypeId>,
    /// Scopes of every `declare module "x"`, by specifier: the module's
    /// exports scope and each block's own.
    pub ambient_module_scopes: std::collections::HashMap<String, Vec<ScopeId>>,
    /// The exports scope of each ambient module, by specifier. As in
    /// TypeScript, an ambient module is one global symbol: what each of its
    /// `declare module "x"` blocks exports, in any file and in a module
    /// augmentation, merges here, while each block keeps its own locals.
    pub(crate) ambient_modules: std::collections::HashMap<String, ScopeId>,
    /// The names each `declare module` block or namespace declaration, by
    /// its scope, exports into the exports its module's or namespace's
    /// blocks share (see `block_module_exports`).
    pub(crate) block_exports: std::collections::HashMap<ScopeId, std::collections::HashSet<String>>,
    /// Member scope of each global namespace a module assigns with
    /// `export =`, → the module's specifier. A file reopening the namespace
    /// populates it into the module too.
    pub assigned_namespaces: std::collections::HashMap<ScopeId, String>,
    /// What each module scope exports, by public name.
    pub exports: ModuleExports,
    /// Each block scope's declarations, by the local name they bind: what
    /// an export map's local names refer to. The global scope holds every
    /// global, with the declarations of all files merged.
    declarations: std::collections::HashMap<ScopeId, indexmap::IndexMap<String, Vec<TypeId>>>,
    /// The static side of a constructor-shaped `var` (see
    /// [`crate::parse::first_pass::StaticSides`]), by the id of the var or
    /// its class, for an instance interface another file declares.
    pub(crate) static_sides: std::collections::HashMap<TypeId, TypeDeclaration>,
    /// Each `var` typed by a named type, with the scope it is declared in,
    /// whose type `bind_constructor_vars` resolves once every file is
    /// inserted.
    pub(crate) ctor_vars: Vec<(TypeId, ScopeId)>,
    /// The kinds of type declaration each name has, by the scope its
    /// declarations merge in, over every file collected so far: what a
    /// later declaration of the name may merge with.
    pub(crate) declared: std::collections::HashMap<(ScopeId, String), crate::ir::DeclFlags>,
    /// The class of each constructor-shaped `var` nothing else describes
    /// the instance of, whose construct signature's return type
    /// `bind_constructor_vars` may find to be a global instance interface.
    pub(crate) constructor_classes: Vec<TypeId>,
    /// Aliases type-level evaluation reads through, so a use names
    /// something else: generic aliases instantiated at every use
    /// (`PartialLike<T> = { [P in keyof T]?: T[P] }`), and literal aliases
    /// that only constrain literal-only type parameters (`Units extends
    /// DateUnit | TimeUnit`). The output surface emits one only when a
    /// signature still reaches it.
    pub(crate) type_level_only: std::collections::HashSet<TypeId>,
    /// String enums type-level evaluation synthesised for a result with no
    /// name of its own (`PluralizeUnit<TimeUnit>`), which convert into an
    /// enum of the same module with more values.
    pub(crate) synthesised_enums: std::collections::HashSet<TypeId>,
    /// Dictionaries type-level evaluation synthesised for an instantiation
    /// that narrows a literal-only parameter (`RoundingOptions<TimeUnit>`),
    /// with the generic base each extends, which names them in a
    /// variant's `_with_` suffix.
    pub(crate) specialisations: std::collections::HashMap<TypeId, Vec<String>>,
    /// Whether the parse evaluated for per-monomorphization (see
    /// [`crate::parse::ParseOptions::experimental_generic_mono`]).
    pub experimental_generic_mono: bool,
    /// Under per-monomorphization, the traits bounding interfaces'
    /// literal-only type parameters, by the interface emitting each.
    pub(crate) param_traits: std::collections::HashMap<TypeId, Vec<crate::ir::ParamTrait>>,
    types: Vec<TypeDeclaration>,
    modules: Vec<ParsedModule>,
    /// Map from module specifier to ModuleId for fast lookup.
    module_index: std::collections::HashMap<String, ModuleId>,
}

/// The export map of every declaration block scope: a module file, a
/// `declare module "x"` body or a namespace body. Like ECMA-262's
/// ResolveExport over a fully known graph, it is built up front, so an
/// import is a lookup by public name and a value is emitted only when its
/// block exports it. TypeScript's reading of the `.d.ts` defines the
/// surface: a block exports every declaration unless it decides its
/// exports explicitly (see `parse::first_pass::block_exports_explicitly`).
///
/// Each entry maps a public name to the local name it is bound to, and the
/// scope binding it: the exporting scope's own, except for an ambient
/// module's exports, where each export resolves in the block declaring it,
/// as a TypeScript export specifier resolves in its block's locals. The
/// names differ for `export { local as public }`.
#[derive(Clone, Debug, Default)]
pub struct ModuleExports {
    by_scope: std::collections::HashMap<ScopeId, indexmap::IndexMap<String, (String, ScopeId)>>,
}

impl ModuleExports {
    /// Record that `scope` exports its binding `local` as `public`.
    pub fn insert(&mut self, scope: ScopeId, public: impl Into<String>, local: impl Into<String>) {
        self.by_scope
            .entry(scope)
            .or_default()
            .insert(public.into(), (local.into(), scope));
    }

    /// Make every export of the block scope `block` an export of `scope`,
    /// still bound in the scope it was.
    pub fn move_into(&mut self, block: ScopeId, scope: ScopeId) {
        let Some(entries) = self.by_scope.remove(&block) else {
            return;
        };
        self.by_scope.entry(scope).or_default().extend(entries);
    }

    /// Drop the export `public` of `scope`.
    pub fn remove(&mut self, scope: ScopeId, public: &str) {
        if let Some(m) = self.by_scope.get_mut(&scope) {
            m.shift_remove(public);
        }
    }

    /// Re-export everything `from` exports as exports of `scope`, under the
    /// same public and local names. For `export = X`, the members of the
    /// namespace merged with `X` are the module's named exports, and phase
    /// 2 binds them in the module scope as well as in `X`'s.
    pub fn extend_from(&mut self, scope: ScopeId, from: ScopeId) {
        let Some(entries) = self.by_scope.get(&from).cloned() else {
            return;
        };
        self.by_scope.entry(scope).or_default().extend(entries);
    }

    /// The local name `scope` binds its export `public` to.
    pub fn local_name(&self, scope: ScopeId, public: &str) -> Option<&str> {
        self.binding(scope, public).map(|(local, _)| local)
    }

    /// The local name the export `public` of `scope` is bound to, and the
    /// scope binding it.
    pub fn binding(&self, scope: ScopeId, public: &str) -> Option<(&str, ScopeId)> {
        let (local, bindings) = self.by_scope.get(&scope)?.get(public)?;
        Some((local.as_str(), *bindings))
    }

    /// Every export of `scope` as `(public, local)`, in declaration order.
    pub fn entries(&self, scope: ScopeId) -> impl Iterator<Item = (&str, &str)> {
        self.bindings(scope)
            .map(|(public, local, _)| (public, local))
    }

    /// Every export of `scope` as `(public, local, the scope binding it)`,
    /// in declaration order.
    pub fn bindings(&self, scope: ScopeId) -> impl Iterator<Item = (&str, &str, ScopeId)> {
        self.by_scope
            .get(&scope)
            .into_iter()
            .flatten()
            .map(|(public, (local, bindings))| (public.as_str(), local.as_str(), *bindings))
    }
}

impl GlobalContext {
    pub fn new() -> Self {
        Self {
            scopes: ScopeArena::new(),
            diagnostics: DiagnosticCollector::new(),
            external_map: ExternalMap::new(),
            pending_imports: Vec::new(),
            builtin_type_ids: std::collections::HashSet::new(),
            ambient_module_scopes: std::collections::HashMap::new(),
            ambient_modules: std::collections::HashMap::new(),
            block_exports: std::collections::HashMap::new(),
            assigned_namespaces: std::collections::HashMap::new(),
            exports: ModuleExports::default(),
            declarations: std::collections::HashMap::new(),
            static_sides: std::collections::HashMap::new(),
            ctor_vars: Vec::new(),
            declared: std::collections::HashMap::new(),
            constructor_classes: Vec::new(),
            type_level_only: std::collections::HashSet::new(),
            synthesised_enums: std::collections::HashSet::new(),
            specialisations: std::collections::HashMap::new(),
            experimental_generic_mono: false,
            param_traits: std::collections::HashMap::new(),
            types: Vec::new(),
            modules: Vec::new(),
            module_index: std::collections::HashMap::new(),
        }
    }

    /// Emit a warning diagnostic.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.diagnostics.warn(message);
    }

    /// Emit an info diagnostic.
    pub fn info(&mut self, message: impl Into<String>) {
        self.diagnostics.info(message);
    }

    // ─── Scope operations (delegate to ScopeArena) ───────────────────

    /// Create the root (builtin) scope.
    pub fn create_root_scope(&mut self) -> ScopeId {
        self.scopes.create_root()
    }

    /// Create a child scope.
    pub fn create_child_scope(&mut self, parent: ScopeId) -> ScopeId {
        self.scopes.create_child(parent)
    }

    // ─── Module registry operations ────────────────────────────────

    /// Register a parsed module, returning its `ModuleId`.
    pub fn register_module(&mut self, specifier: String, scope: ScopeId) -> ModuleId {
        let id = ModuleId(self.modules.len() as u32);
        self.module_index.insert(specifier.clone(), id);
        self.modules.push(ParsedModule {
            specifier,
            scope,
            types: Vec::new(),
        });
        id
    }

    /// Look up a module by specifier.
    pub fn find_module(&self, specifier: &str) -> Option<ModuleId> {
        self.module_index.get(specifier).copied()
    }

    /// Get a reference to a parsed module by `ModuleId`.
    pub fn get_module(&self, id: ModuleId) -> &ParsedModule {
        &self.modules[id.0 as usize]
    }

    /// Get a mutable reference to a parsed module by `ModuleId`.
    pub fn get_module_mut(&mut self, id: ModuleId) -> &mut ParsedModule {
        &mut self.modules[id.0 as usize]
    }

    // ─── Type arena operations ───────────────────────────────────────

    /// Insert a declaration into the type arena, returning its `TypeId`.
    pub fn insert_type(&mut self, decl: TypeDeclaration) -> TypeId {
        let id = TypeId(self.types.len() as u32);
        self.types.push(decl);
        id
    }

    /// Get a reference to a declaration by `TypeId`.
    pub fn get_type(&self, id: TypeId) -> &TypeDeclaration {
        &self.types[id.index()]
    }

    pub fn get_type_mut(&mut self, id: TypeId) -> &mut TypeDeclaration {
        &mut self.types[id.index()]
    }

    /// The declarations every module and namespace lists.
    pub(crate) fn listed_types(&self) -> impl Iterator<Item = TypeId> + '_ {
        let modules = self.modules.iter().flat_map(|m| m.types.iter().copied());
        let namespaces = self.types.iter().flat_map(|decl| match &decl.kind {
            crate::ir::TypeKind::Namespace(ns) => ns.declarations.as_slice(),
            _ => &[],
        });
        modules.chain(namespaces.copied())
    }

    /// List `new` in the module that lists `beside`, if one does.
    pub(crate) fn list_in_module_of(&mut self, beside: TypeId, new: TypeId) -> bool {
        match self.modules.iter_mut().find(|m| m.types.contains(&beside)) {
            Some(module) => {
                module.types.push(new);
                true
            }
            None => false,
        }
    }

    /// Remove `dropped` from every module's, namespace's and block's
    /// declarations.
    pub(crate) fn drop_types(&mut self, dropped: &std::collections::HashSet<TypeId>) {
        for module in &mut self.modules {
            module.types.retain(|id| !dropped.contains(id));
        }
        for decl in &mut self.types {
            if let crate::ir::TypeKind::Namespace(ns) = &mut decl.kind {
                ns.declarations.retain(|id| !dropped.contains(id));
            }
        }
        for ids in self.declarations.values_mut().flat_map(|m| m.values_mut()) {
            ids.retain(|id| !dropped.contains(id));
        }
    }

    /// Read-only access to the full type arena slice.
    pub fn type_arena(&self) -> &[TypeDeclaration] {
        &self.types
    }

    /// Resolve a dotted type path like `"NodeJS.TypedArray"` through namespace scopes.
    ///
    /// The first namespace resolves from `scope` up the chain; each later
    /// segment must be declared directly in the namespace before it.
    pub fn resolve_path(&self, scope: ScopeId, path: &str) -> Option<TypeId> {
        let Some((namespaces, member)) = path.rsplit_once('.') else {
            return self.scopes.resolve(scope, path);
        };
        let mut namespaces = namespaces.split('.');
        let first = self.scopes.resolve_namespace(scope, namespaces.next()?)?;
        let members = namespaces.try_fold(first, |ns, name| {
            let (local, bindings) = self.exports.binding(ns, name)?;
            self.scopes.get(bindings).namespace(local)
        })?;
        let (local, bindings) = self.exports.binding(members, member)?;
        let id = self.scopes.get(bindings).get(local)?;
        // A builtin member (the lib's `Intl.LocalesArgument`) gives way to an
        // `--external` mapping of its path, as a bare builtin does to a
        // mapping of its name. It is replaced before codegen chases it as
        // an alias, so the path is left to the external map.
        let mapped = || self.external_map.resolve_qualified(path).is_some();
        (!self.builtin_type_ids.contains(&id) || !mapped()).then_some(id)
    }

    /// The scope a declaration of `name` in `scope` binds in: the exports a
    /// `declare module` block or namespace declaration shares with the
    /// others of its module or namespace, where it exports the name, as
    /// TypeScript's binder puts it there rather than in the block's locals;
    /// else `scope`.
    pub(crate) fn binding_scope(&self, scope: ScopeId, name: &str) -> ScopeId {
        let exported = self
            .block_exports
            .get(&scope)
            .is_some_and(|names| names.contains(name));
        match self.scopes.get(scope).exports {
            Some(exports) if exported => exports,
            _ => scope,
        }
    }

    /// Record that `type_id` is a declaration of `scope` binding `name`.
    pub fn declare(&mut self, scope: ScopeId, name: &str, type_id: TypeId) {
        let ids = self
            .declarations
            .entry(scope)
            .or_default()
            .entry(name.to_string())
            .or_default();
        if !ids.contains(&type_id) {
            ids.push(type_id);
        }
    }

    /// The declarations of `scope` binding `name`: a class and a merged
    /// namespace, say.
    pub fn declarations(&self, scope: ScopeId, name: &str) -> &[TypeId] {
        self.declarations
            .get(&scope)
            .and_then(|m| m.get(name))
            .map_or(&[], Vec::as_slice)
    }

    /// Iterate all types in the arena.
    pub fn iter_types(&self) -> impl Iterator<Item = (TypeId, &TypeDeclaration)> {
        self.types
            .iter()
            .enumerate()
            .map(|(i, d)| (TypeId(i as u32), d))
    }
}

impl Default for GlobalContext {
    fn default() -> Self {
        Self::new()
    }
}
