//! TypeScript type scope tracking with arena-based ownership.
//!
//! Each scope maps type names to one of two bindings:
//!
//! * [`Binding::Declared`] — a real type declaration, addressable by
//!   `TypeId` in the global type arena.
//! * [`Binding::TypeParam`] — an in-scope generic type parameter (e.g.
//!   the `T` inside `interface Box<T> { ... }` or `method<T>(...)`).
//!   These have no `TypeId` because there is no declaration to point
//!   at; the binding is purely a scope-chain marker that says "this
//!   name resolves to a generic, not a declared type".
//!
//! Scopes form a tree via parent links, mirroring TypeScript's own
//! lexical type-scope model: every type-parameter-bearing construct
//! (class, interface, type alias, method, function) introduces its
//! own child scope where its parameters are visible.
//!
//! Scopes are stored in a flat arena (`ScopeArena`) and referenced by
//! well-typed `ScopeId` indices.
//!
//! Two resolution APIs are provided:
//!
//! * [`ScopeArena::resolve`] returns `Option<TypeId>` — only declared
//!   types resolve. Most callers (codegen typemap, namespace
//!   traversal, import resolution) use this since they care only
//!   about declarations they can lower to Rust.
//! * [`ScopeArena::resolve_binding`] returns `Option<Binding>` —
//!   distinguishes declared types from in-scope type parameters. Used
//!   by the type-conversion pipeline and by codegen sites that need
//!   to emit a bare ident for a generic parameter rather than chasing
//!   it through a declaration table.

use std::collections::HashMap;

use crate::context::TypeId;

/// Index into a `ScopeArena`. Lightweight, Copy, and well-typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScopeId(pub(crate) u32);

impl ScopeId {
    /// Placeholder scope id for IR nodes constructed in unit tests
    /// that don't exercise scope-driven resolution. Real parse paths
    /// always create proper child scopes via [`ScopeArena::create_child`].
    pub const DUMMY: ScopeId = ScopeId(0);
}

/// What a name in a scope resolves to.
///
/// Type parameters carry no payload because they have no global
/// declaration — the *binding's existence* in the scope is the entire
/// signal: "this name is a type parameter introduced by an enclosing
/// generic declaration."
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    /// A real type declaration in the global arena.
    Declared(TypeId),
    /// An in-scope generic type parameter.
    TypeParam,
}

/// A single level of type scope.
#[derive(Clone, Debug)]
pub struct TypeScope {
    /// Optional parent scope — resolution walks up the chain.
    pub parent: Option<ScopeId>,
    /// Types defined or imported in this scope: name → binding.
    names: HashMap<String, Binding>,
}

impl TypeScope {
    fn new(parent: Option<ScopeId>) -> Self {
        Self {
            parent,
            names: HashMap::new(),
        }
    }

    /// Insert a declared-type binding into this scope.
    pub fn insert(&mut self, name: String, type_id: TypeId) {
        self.names.insert(name, Binding::Declared(type_id));
    }

    /// Insert a type-parameter binding into this scope.
    pub fn insert_type_param(&mut self, name: String) {
        self.names.insert(name, Binding::TypeParam);
    }

    /// Look up a declared-type binding in this scope only (not parent
    /// scopes). Returns `None` for type-parameter bindings.
    pub fn get(&self, name: &str) -> Option<TypeId> {
        match self.names.get(name)? {
            Binding::Declared(id) => Some(*id),
            Binding::TypeParam => None,
        }
    }

    /// Look up any binding in this scope only (not parent scopes).
    pub fn get_binding(&self, name: &str) -> Option<Binding> {
        self.names.get(name).copied()
    }

    /// Iterate over declared-type bindings only. Type-parameter
    /// bindings are skipped — most iteration callers (export
    /// resolution, namespace assembly) want declarations only.
    pub fn iter(&self) -> impl Iterator<Item = (&String, TypeId)> {
        self.names.iter().filter_map(|(k, v)| match v {
            Binding::Declared(id) => Some((k, *id)),
            Binding::TypeParam => None,
        })
    }
}

/// Arena that owns all scopes. Scopes are created via `create_root` and
/// `create_child`, and referenced by `ScopeId`.
#[derive(Clone, Debug)]
pub struct ScopeArena {
    scopes: Vec<TypeScope>,
}

impl ScopeArena {
    pub fn new() -> Self {
        Self { scopes: Vec::new() }
    }

    /// Create a root scope (no parent).
    pub fn create_root(&mut self) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(TypeScope::new(None));
        id
    }

    /// Create a child scope with the given parent.
    pub fn create_child(&mut self, parent: ScopeId) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(TypeScope::new(Some(parent)));
        id
    }

    /// Get a reference to a scope by id.
    pub fn get(&self, id: ScopeId) -> &TypeScope {
        &self.scopes[id.0 as usize]
    }

    /// Get a mutable reference to a scope by id.
    pub fn get_mut(&mut self, id: ScopeId) -> &mut TypeScope {
        &mut self.scopes[id.0 as usize]
    }

    /// Insert a declared-type binding into the given scope.
    pub fn insert(&mut self, scope: ScopeId, name: String, type_id: TypeId) {
        self.get_mut(scope).insert(name, type_id);
    }

    /// Insert a type-parameter binding into the given scope.
    pub fn insert_type_param(&mut self, scope: ScopeId, name: String) {
        self.get_mut(scope).insert_type_param(name);
    }

    /// Resolve a simple name to a declared `TypeId` by walking up the
    /// scope chain. Type-parameter bindings *shadow* declarations: if
    /// the closest binding is a type parameter, this returns `None`
    /// (the caller must use [`Self::resolve_binding`] to detect the
    /// type-param case). This matches TypeScript's lexical resolution
    /// — a method's `<T>` shadows an outer `type T = ...`.
    pub fn resolve(&self, scope: ScopeId, name: &str) -> Option<TypeId> {
        match self.resolve_binding(scope, name)? {
            Binding::Declared(id) => Some(id),
            Binding::TypeParam => None,
        }
    }

    /// Resolve a simple name to a [`Binding`] by walking up the scope
    /// chain. Returns the *closest* binding — a method's type
    /// parameter shadows an outer declaration with the same name.
    pub fn resolve_binding(&self, scope: ScopeId, name: &str) -> Option<Binding> {
        let s = self.get(scope);
        if let Some(b) = s.get_binding(name) {
            return Some(b);
        }
        if let Some(parent) = s.parent {
            return self.resolve_binding(parent, name);
        }
        None
    }
}

impl Default for ScopeArena {
    fn default() -> Self {
        Self::new()
    }
}

/// An unresolved import that needs to be resolved during import resolution.
/// Stored in a side table on GlobalContext, not in the scope.
#[derive(Clone, Debug)]
pub struct PendingImport {
    /// The scope that contains this import.
    pub scope: ScopeId,
    /// The local name in the importing scope.
    pub local_name: String,
    /// The module specifier from the import statement.
    pub from_module: String,
    /// The original name in the source module.
    pub original_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::GlobalContext;

    fn make_iface_decl(name: &str, scope: ScopeId) -> crate::ir::TypeDeclaration {
        crate::ir::TypeDeclaration {
            kind: crate::ir::TypeKind::Interface(crate::ir::InterfaceDecl {
                name: name.to_string(),
                js_name: name.to_string(),
                type_params: vec![],
                extends: vec![],
                members: vec![],
                classification: crate::ir::InterfaceClassification::ClassLike,
                body_scope: scope,
            }),
            module_context: crate::ir::ModuleContext::Global,
            doc: None,
            scope_id: scope,
            exported: false,
        }
    }

    #[test]
    fn test_basic_resolution() {
        let mut gctx = GlobalContext::new();
        let root = gctx.create_root_scope();
        let type_id = gctx.insert_type(make_iface_decl("Foo", root));
        gctx.scopes.insert(root, "Foo".to_string(), type_id);

        assert!(gctx.scopes.resolve(root, "Foo").is_some());
        assert!(gctx.scopes.resolve(root, "Bar").is_none());
    }

    #[test]
    fn test_child_scope_shadows_parent() {
        let mut gctx = GlobalContext::new();
        let parent = gctx.create_root_scope();
        let child = gctx.scopes.create_child(parent);

        let id_a = gctx.insert_type(make_iface_decl("Foo", parent));
        let id_b = gctx.insert_type(make_iface_decl("Foo", child));

        gctx.scopes.insert(parent, "Foo".to_string(), id_a);
        gctx.scopes.insert(child, "Foo".to_string(), id_b);

        assert_eq!(gctx.scopes.resolve(child, "Foo"), Some(id_b));
        assert_eq!(gctx.scopes.resolve(parent, "Foo"), Some(id_a));
    }

    #[test]
    fn test_child_inherits_parent() {
        let mut gctx = GlobalContext::new();
        let parent = gctx.create_root_scope();
        let child = gctx.scopes.create_child(parent);

        let id = gctx.insert_type(make_iface_decl("Foo", parent));
        gctx.scopes.insert(parent, "Foo".to_string(), id);

        assert_eq!(gctx.scopes.resolve(child, "Foo"), Some(id));
    }

    /// Type-parameter bindings shadow outer declarations: the closest
    /// `T` wins, mirroring TypeScript's lexical type-scope rules.
    #[test]
    fn test_type_param_shadows_outer_declaration() {
        let mut gctx = GlobalContext::new();
        let parent = gctx.create_root_scope();
        let child = gctx.scopes.create_child(parent);

        // Outer `type T = ...`
        let id = gctx.insert_type(make_iface_decl("T", parent));
        gctx.scopes.insert(parent, "T".to_string(), id);
        // Inner method-scope `<T>` parameter
        gctx.scopes.insert_type_param(child, "T".to_string());

        // From the child, T resolves to the type parameter binding.
        assert_eq!(
            gctx.scopes.resolve_binding(child, "T"),
            Some(Binding::TypeParam),
        );
        // The declared-type API filters TypeParam out — the caller must
        // detect that case via `resolve_binding`.
        assert_eq!(gctx.scopes.resolve(child, "T"), None);

        // From the parent, T still resolves to the outer declaration.
        assert_eq!(gctx.scopes.resolve(parent, "T"), Some(id));
    }
}
