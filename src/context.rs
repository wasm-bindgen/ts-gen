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
    /// Body scopes of every `declare module "x"`, by specifier, one per
    /// declaring file.
    pub ambient_module_scopes: std::collections::HashMap<String, Vec<ScopeId>>,
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
    /// The static side of a global constructor-shaped `var` (see
    /// [`crate::parse::first_pass::StaticSides`]), by the var's id, for an
    /// instance interface a later file declares.
    pub(crate) static_sides: std::collections::HashMap<TypeId, TypeDeclaration>,
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
/// Each entry maps a public name to the local name it is bound to in the
/// scope; the two differ for `export { local as public }`.
#[derive(Clone, Debug, Default)]
pub struct ModuleExports {
    by_scope: std::collections::HashMap<ScopeId, indexmap::IndexMap<String, String>>,
}

impl ModuleExports {
    /// Record that `scope` exports its binding `local` as `public`.
    pub fn insert(&mut self, scope: ScopeId, public: impl Into<String>, local: impl Into<String>) {
        self.by_scope
            .entry(scope)
            .or_default()
            .insert(public.into(), local.into());
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
        self.by_scope.get(&scope)?.get(public).map(String::as_str)
    }

    /// The name `scope` exports its binding `local` as: `local` itself
    /// when exported under its own name, else its first public name.
    pub fn public_name<'a>(&'a self, scope: ScopeId, local: &'a str) -> Option<&'a str> {
        let m = self.by_scope.get(&scope)?;
        if m.get(local).is_some_and(|l| l == local) {
            return Some(local);
        }
        m.iter()
            .find(|(_, l)| *l == local)
            .map(|(public, _)| public.as_str())
    }

    /// Every export of `scope` as `(public, local)`, in declaration order.
    pub fn entries(&self, scope: ScopeId) -> impl Iterator<Item = (&str, &str)> {
        self.by_scope
            .get(&scope)
            .into_iter()
            .flatten()
            .map(|(public, local)| (public.as_str(), local.as_str()))
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
            assigned_namespaces: std::collections::HashMap::new(),
            exports: ModuleExports::default(),
            declarations: std::collections::HashMap::new(),
            static_sides: std::collections::HashMap::new(),
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
            self.scopes.get(ns).namespace(self.member_name(ns, name)?)
        })?;
        self.scopes
            .get(members)
            .get(self.member_name(members, member)?)
    }

    /// The local name a member `public` of the block scope `ns` is bound
    /// to: `A.Foo` through `import * as A`, or `NS.Foo`, names the export
    /// `Foo` of that block.
    fn member_name<'n>(&'n self, ns: ScopeId, public: &'n str) -> Option<&'n str> {
        self.exports.local_name(ns, public)
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
