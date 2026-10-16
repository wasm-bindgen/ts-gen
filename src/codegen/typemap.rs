//! TypeRef → syn::Type mapping with unified position-based system.
//!
//! Follows the wasm-bindgen WebIDL approach: a single `to_syn_type` function
//! that uses `TypePosition` to determine how types are lowered to Rust.
//!
//! `TypePosition` is a struct with two fields:
//! - `direction`: `Argument` or `Return` — controls borrowing (e.g., `&str` vs `String`)
//! - `inner`: whether we're nested inside a generic container (e.g., `Promise<T>`)
//!
//! When `inner` is true:
//! - Primitives map to JS wrapper types (`Number`, `JsString`, `Boolean`, `Undefined`)
//! - `Nullable` becomes `JsOption<T>` instead of `Option<T>`
//! - Argument-position types are NOT borrowed (owned `T`, not `&T`)

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use proc_macro2::TokenStream;
use quote::quote;

use crate::context::{GlobalContext, TypeId};
use crate::ir::{self, ModuleContext, TypeKind, TypeRef};
use crate::parse::scope::ScopeId;
use crate::util::diagnostics::DiagnosticCollector;
use crate::util::naming::{LocalScope, Suffix};

/// js_sys type names reserved by the `use js_sys::*` glob import.
/// User-defined types that collide with these will be renamed.
pub const JS_SYS_RESERVED: &[&str] = &[
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "ArrayTuple",
    "AsyncGenerator",
    "AsyncIterator",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "Float32Array",
    "Float64Array",
    "Function",
    "Generator",
    "Global",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Iterator",
    "IteratorNext",
    "JsOption",
    "JsString",
    "Map",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Undefined",
    "UriError",
    "WeakMap",
    "WeakRef",
    "WeakSet",
];

/// Prelude names the generated code spells bare, so a declaration with one
/// of these names is moved aside like a `js_sys` collision. Re-exporting it
/// under its own name would shadow the prelude again, so it keeps the
/// suffixed name.
const RUST_PRELUDE_RESERVED: &[&str] = &["Option", "Result", "String", "Vec"];

pub(crate) fn is_prelude_reserved(name: &str) -> bool {
    RUST_PRELUDE_RESERVED.contains(&name)
}

fn reserved_names() -> impl Iterator<Item = &'static str> {
    JS_SYS_RESERVED.iter().chain(RUST_PRELUDE_RESERVED).copied()
}

/// Direction of data flow at the FFI boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Data flowing from Rust to JS (function arguments).
    Argument,
    /// Data flowing from JS to Rust (function returns).
    Return,
}

/// Position context for type mapping, following the wasm-bindgen WebIDL pattern.
///
/// Combines a direction (Argument/Return) with an inner flag indicating
/// whether we're inside a generic container. When `inner` is true,
/// primitives use their JS wrapper types and nullable uses `JsOption`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypePosition {
    pub direction: Direction,
    /// Whether this type is nested inside a generic or callback.
    /// When true, must use JS-compatible wrapper types.
    pub inner: bool,
}

impl TypePosition {
    /// Top-level function argument position.
    pub const ARGUMENT: Self = Self {
        direction: Direction::Argument,
        inner: false,
    };
    /// Top-level function return position.
    pub const RETURN: Self = Self {
        direction: Direction::Return,
        inner: false,
    };

    /// Convert to inner position (for generic type parameters).
    /// Preserves direction but sets `inner: true`.
    pub fn to_inner(self) -> Self {
        Self {
            direction: self.direction,
            inner: true,
        }
    }

    pub fn is_argument(self) -> bool {
        matches!(self.direction, Direction::Argument)
    }
}

/// Context for codegen that tracks locally-defined types and resolved type aliases.
///
/// This allows `to_syn_type` to distinguish between locally-generated types
/// and types that should resolve via `use js_sys::*`.
pub struct CodegenContext<'a> {
    /// Read-only access to the global context (scopes, modules, external map).
    pub gctx: &'a GlobalContext,
    /// Names of types defined in this codegen unit, used to detect
    /// collisions with the `js_sys` glob and as a fallback when a use
    /// site bypasses scope-chain resolution (tests / direct entry
    /// points). Same-named types from different module contexts collapse
    /// here, which is fine because the value is only used as a fallback —
    /// `local_type_ids` is the authoritative "is this ours to emit?" set.
    pub local_types: HashMap<String, ModuleContext>,
    /// `TypeId`s of declarations this codegen unit will emit. The
    /// authoritative source for "local" — same-name distinct
    /// declarations have distinct `TypeId`s, so this is the lookup key
    /// when scope resolution gives us a hit.
    pub local_type_ids: HashSet<TypeId>,
    /// Every name claimed at the top level of this codegen unit: the
    /// `js_sys` prelude, declared types, their respellings and synthesised
    /// enums. Anything that invents a top-level name declares it here.
    pub names: RefCell<LocalScope>,
    /// Local types that collide with js_sys reserved names — maps original name → renamed name.
    pub renamed_locals: HashMap<String, String>,
    /// How many type parameters each *locally emitted* type accepts, by
    /// name. `is_local_generic` reads arity from the resolved declaration
    /// and only falls back to this for references it cannot resolve.
    pub local_type_param_counts: HashMap<String, usize>,
    /// Member scope of each namespace and declared module → the path of
    /// the `pub mod` it is emitted as.
    pub namespace_paths: HashMap<ScopeId, Vec<String>>,
    /// The `--lib-name` module, which is the output's own and emitted at
    /// the top level.
    lib_name: Option<String>,
    /// Path of the `pub mod` currently being emitted into, empty at the top
    /// level. References are qualified relative to it, not to the scope
    /// they resolve from: a synthesised union resolves from its use site
    /// but is emitted at the top level.
    pub emit_path: RefCell<Vec<String>>,
    /// Builtin (root) scope id.
    pub root_scope: ScopeId,
    /// Per-file scopes (children of root, contain imports + local types).
    pub file_scopes: Vec<ScopeId>,
    /// External type use aliases collected during codegen: (local_name, rust_path).
    pub external_uses: RefCell<HashMap<String, String>>,
    /// Diagnostics collected during code generation.
    pub diagnostics: RefCell<DiagnosticCollector>,
    /// Dynamic-union enums synthesised at codegen time for top-level
    /// return-position unions that would otherwise erase to `JsValue`.
    ///
    /// Identity is keyed by the member set (TS source-order list of
    /// type-ref representations) so two erasing unions with the same
    /// members share a single enum. Callers reach into this through
    /// [`CodegenContext::synthesise_dynamic_union`].
    pub dynamic_unions: RefCell<DynamicUnionRegistry>,
    /// When `true`, the default error type for fallible bindings is
    /// `Error` rather than `JsValue`. See
    /// [`crate::codegen::GenerateOptions::errors_as_error`].
    pub errors_as_error: bool,
    /// Whether generic imports use wasm-bindgen's experimental
    /// per-monomorphization codegen rather than type erasure.
    pub experimental_generic_mono: bool,
    /// JS module specifiers whose declarations are emitted, each in the
    /// `pub mod` of `module_path`.
    pub exported_modules: HashSet<String>,
    /// JS module specifiers whose declarations are fully covered by the
    /// external map. References to their types resolve through
    /// `external_map.resolve` (yielding e.g. `::other::foo::Bar`)
    /// rather than emitting a `foo::` local-mod qualifier. The
    /// corresponding `mod` block is suppressed by `should_emit`.
    pub externalised_modules: HashSet<String>,
    /// Already-reported `(type_name, module_spec)` pairs for the
    /// unresolved-module-ref error. Emitting one diagnostic per
    /// offending pair keeps the output focused when many references
    /// point at the same unconfigured module.
    pub unresolved_module_refs: RefCell<HashSet<(String, String)>>,
}

/// Registry of synthesised dynamic-union enums, plus the bookkeeping
/// for first-seen-wins anchor-name allocation.
#[derive(Debug, Default)]
pub struct DynamicUnionRegistry {
    /// Identity (member-set key) → assigned Rust enum name.
    pub by_key: indexmap::IndexMap<DynamicUnionKey, DynamicUnionInfo>,
}

/// Stable identity key for a dynamic union — the ordered list of
/// member type representations.
///
/// Order matches TS source order so two unions with the same members
/// in different orders are *not* equal. (Order matters at runtime —
/// dispatch tries variants in source order, so reordering changes
/// behaviour.)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DynamicUnionKey(pub Vec<String>);

/// Bookkeeping for a single synthesised dynamic-union enum.
#[derive(Debug, Clone)]
pub struct DynamicUnionInfo {
    /// Final Rust enum name.
    pub rust_name: String,
    /// Members of the union in TS source order — kept here so the
    /// emit pass can render variants without re-running the
    /// member-set computation.
    pub members: Vec<TypeRef>,
    /// Scope and module of the first use site. Members are lowered
    /// against these, since a member can name a declaration that is
    /// private to an ambient module.
    pub scope: ScopeId,
    pub from_module: ModuleContext,
}

/// Claim a fresh Rust enum name for a synthesised dynamic union.
///
/// Preference order: bare anchor → parent-prefixed → numeric
/// suffix on whichever form was tried first. The first call with
/// a given anchor wins the bare form.
///
/// The trailing suffix is `Kind` (rather than `Union`) — Rust
/// idiom is to name discriminated-enum types `<Concept>Kind`,
/// e.g. `EmailAttachmentContentKind`.
fn allocate_dynamic_union_name(
    names: &mut LocalScope,
    anchor: &str,
    parent: Option<&str>,
) -> String {
    let bare = format!("{anchor}Kind");
    if names.claim(bare.clone()) {
        return bare;
    }
    match parent {
        Some(p) => names.declare(&format!("{p}{anchor}Kind"), Suffix::Pascal),
        None => names.declare(&bare, Suffix::Pascal),
    }
}

/// Stable, hash-friendly representation of a `TypeRef` used as the
/// identity component for [`DynamicUnionKey`].
///
/// Walks the structure rather than relying on `Debug` so cosmetic
/// changes to `TypeRef`'s `Debug` impl don't perturb dedup. Generic
/// args, tuple elements, and union members all recurse.
fn format_type_ref_key(ty: &TypeRef) -> String {
    let mut buf = String::new();
    write_type_ref_key(&mut buf, ty);
    buf
}

fn write_type_ref_key(buf: &mut String, ty: &TypeRef) {
    use std::fmt::Write;
    match ty {
        TypeRef::Boolean => buf.push_str("bool"),
        TypeRef::Number => buf.push_str("num"),
        TypeRef::BigInt => buf.push_str("bigint"),
        TypeRef::String => buf.push_str("str"),
        TypeRef::Void => buf.push_str("void"),
        TypeRef::Undefined => buf.push_str("undef"),
        TypeRef::Null => buf.push_str("null"),
        TypeRef::Any => buf.push_str("any"),
        TypeRef::Unknown => buf.push_str("unknown"),
        TypeRef::Object => buf.push_str("Object"),
        TypeRef::Symbol => buf.push_str("Symbol"),
        TypeRef::ArrayBufferView => buf.push_str("ArrayBufferView"),
        TypeRef::PropertyKey => buf.push_str("PropertyKey"),
        TypeRef::StringLiteral(s) => write!(buf, "lit:{s:?}").unwrap(),
        TypeRef::NumberLiteral(n) => write!(buf, "lit:{n}").unwrap(),
        TypeRef::BooleanLiteral(b) => write!(buf, "lit:{b}").unwrap(),
        TypeRef::Array(inner) => {
            buf.push_str("Array<");
            write_type_ref_key(buf, inner);
            buf.push('>');
        }
        TypeRef::Nullable(inner) => {
            buf.push_str("Nullable<");
            write_type_ref_key(buf, inner);
            buf.push('>');
        }
        TypeRef::Tuple(parts) => {
            buf.push_str("Tuple<");
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                write_type_ref_key(buf, p);
            }
            buf.push('>');
        }
        TypeRef::Union(members) => {
            buf.push_str("Union<");
            for (i, m) in members.iter().enumerate() {
                if i > 0 {
                    buf.push('|');
                }
                write_type_ref_key(buf, m);
            }
            buf.push('>');
        }
        TypeRef::Intersection(parts) => {
            buf.push_str("Intersection<");
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    buf.push('&');
                }
                write_type_ref_key(buf, p);
            }
            buf.push('>');
        }
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            buf.push_str(&segments.join("."));
            if !generic_args.is_empty() {
                buf.push('<');
                for (i, a) in generic_args.iter().enumerate() {
                    if i > 0 {
                        buf.push(',');
                    }
                    write_type_ref_key(buf, a);
                }
                buf.push('>');
            }
        }
        TypeRef::Function(sig) => {
            buf.push_str("fn(");
            for (i, p) in sig.params.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                write_type_ref_key(buf, &p.type_ref);
            }
            buf.push_str(")->");
            write_type_ref_key(buf, &sig.return_type);
        }
        TypeRef::Unresolved(s) => {
            buf.push('?');
            buf.push_str(s);
        }
    }
}

impl<'a> CodegenContext<'a> {
    /// Whether per-monomorphization codegen is on. Takes an `Option` because
    /// most emitters carry an optional context; no context means default mode.
    pub(crate) fn generic_mono(ctx: Option<&CodegenContext<'_>>) -> bool {
        ctx.is_some_and(|ctx| ctx.experimental_generic_mono)
    }

    /// One type-parameter declaration: bare `T` under per-monomorphization
    /// (each instantiation picks its own ABI), `T: JsGeneric` otherwise.
    pub(crate) fn type_param_decl(ctx: Option<&CodegenContext<'_>>, name: &str) -> TokenStream {
        let ident = make_ident(name);
        if Self::generic_mono(ctx) {
            quote! { #ident }
        } else {
            quote! { #ident: ::wasm_bindgen::JsGeneric }
        }
    }

    /// Whether `name`, resolved from `scope`, is a locally declared type
    /// with at least one type parameter, i.e. one whose generic arguments
    /// survive into the output. Resolution goes through the scope chain so
    /// a namespace member and a same-named top-level type each answer for
    /// themselves; the by-name arity table is only the fallback for use
    /// sites without a scope chain.
    pub(crate) fn is_local_generic(&self, name: &str, scope: ScopeId) -> bool {
        match self.gctx.resolve_path(scope, name) {
            Some(id) => {
                self.local_type_ids.contains(&id)
                    && declared_arity(&self.gctx.get_type(id).kind).is_some_and(|n| n > 0)
            }
            None => self
                .local_type_param_counts
                .get(name)
                .is_some_and(|count| *count > 0),
        }
    }

    /// Build the attribute for a generated extern block, including the
    /// per-monomorphization opt-in when enabled.
    pub(crate) fn extern_attr(
        ctx: Option<&CodegenContext<'_>>,
        module: Option<&str>,
    ) -> TokenStream {
        match (module, Self::generic_mono(ctx)) {
            (Some(module), true) => {
                quote! { #[wasm_bindgen(module = #module, experimental_generic_mono)] }
            }
            (Some(module), false) => quote! { #[wasm_bindgen(module = #module)] },
            (None, true) => quote! { #[wasm_bindgen(experimental_generic_mono)] },
            (None, false) => quote! { #[wasm_bindgen] },
        }
    }

    /// Build a `CodegenContext` from a parsed IR module + global context.
    /// Used by tests and library callers that don't customise codegen.
    pub fn from_module(module: &ir::Module, gctx: &'a GlobalContext) -> Self {
        Self::from_module_full(module, gctx, false, false, HashSet::new())
    }

    /// Build a `CodegenContext` with the full set of per-codegen options
    /// including the set of emitted JS module specifiers.
    pub fn from_module_full(
        module: &ir::Module,
        gctx: &'a GlobalContext,
        errors_as_error: bool,
        experimental_generic_mono: bool,
        exported_modules: HashSet<String>,
    ) -> Self {
        // Pre-compute which `Module(spec)` groups are fully covered by
        // the external map (Rule 3). References to types from these
        // modules go through `external_map.resolve` in `emit_type_name`
        // instead of producing a local `foo::` qualifier; the
        // corresponding `mod` block is suppressed in `generate_tokens`.
        let mut module_groups: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        for &type_id in &module.types {
            let decl = gctx.get_type(type_id);
            if let ModuleContext::Module(m) = &decl.module_context {
                let name = type_decl_name(&decl.kind);
                if let Some(name) = name {
                    module_groups.entry(m.as_ref()).or_default().push(name);
                }
            }
        }
        let mut externalised_modules = HashSet::new();
        for (spec, names) in &module_groups {
            if names
                .iter()
                .all(|n| gctx.external_map.resolve(n, spec).is_some())
            {
                externalised_modules.insert((*spec).to_string());
            }
        }

        let mut ctx = CodegenContext {
            gctx,
            local_types: HashMap::new(),
            local_type_ids: HashSet::new(),
            names: RefCell::new(LocalScope::with_claimed(reserved_names())),
            renamed_locals: HashMap::new(),
            local_type_param_counts: HashMap::new(),
            namespace_paths: HashMap::new(),
            lib_name: module.lib_name.clone(),
            emit_path: RefCell::new(Vec::new()),
            root_scope: module.builtin_scope,
            file_scopes: module.file_scopes.clone(),
            external_uses: RefCell::new(HashMap::new()),
            diagnostics: RefCell::new(DiagnosticCollector::new()),
            dynamic_unions: RefCell::new(DynamicUnionRegistry::default()),
            errors_as_error,
            experimental_generic_mono,
            exported_modules,
            externalised_modules,
            unresolved_module_refs: RefCell::new(HashSet::new()),
        };
        for (spec, scopes) in &gctx.ambient_module_scopes {
            let path = ctx.specifier_path(spec);
            for &scope in scopes {
                ctx.namespace_paths.insert(scope, path.clone());
            }
        }
        for &type_id in &module.types {
            let path = ctx.module_path(&gctx.get_type(type_id).module_context);
            ctx.collect_declaration(type_id, &path);
        }
        ctx.resolve_collisions();
        ctx
    }

    /// Create an empty context (for tests). Requires a valid root scope.
    pub fn empty(gctx: &'a GlobalContext, root_scope: ScopeId) -> Self {
        CodegenContext {
            gctx,
            local_types: HashMap::new(),
            local_type_ids: HashSet::new(),
            names: RefCell::new(LocalScope::with_claimed(reserved_names())),
            renamed_locals: HashMap::new(),
            local_type_param_counts: HashMap::new(),
            namespace_paths: HashMap::new(),
            lib_name: None,
            emit_path: RefCell::new(Vec::new()),
            root_scope,
            file_scopes: vec![],
            external_uses: RefCell::new(HashMap::new()),
            diagnostics: RefCell::new(DiagnosticCollector::new()),
            dynamic_unions: RefCell::new(DynamicUnionRegistry::default()),
            errors_as_error: false,
            experimental_generic_mono: false,
            exported_modules: HashSet::new(),
            externalised_modules: HashSet::new(),
            unresolved_module_refs: RefCell::new(HashSet::new()),
        }
    }

    /// Register an external type use alias.
    /// Returns the local name to use in generated code.
    fn register_external(&self, local_name: &str, rust_path: &str) {
        self.external_uses
            .borrow_mut()
            .insert(local_name.to_string(), rust_path.to_string());
    }

    /// Look up or synthesise a dynamic-union enum for a top-level
    /// return-position union that would otherwise erase to `JsValue`.
    ///
    /// Identity is the ordered member list (TS source order matters
    /// at runtime — dynamic-union dispatch tries variants in source
    /// order). Two erasing unions with the same member sequence
    /// share a single synthesised enum.
    ///
    /// Naming preference, most-simple to most-qualified:
    ///
    /// 1. The bare anchor (e.g. `ContentKind`).
    /// 2. Parent-prefixed (`EmailAttachmentContentKind`).
    /// 3. Numeric-suffixed (`ContentKind2`, `EmailAttachmentContentKind2`).
    ///
    /// First-seen wins on the bare anchor: subsequent distinct
    /// unions with the same anchor get parent-prefixed (or
    /// numeric-suffixed if that also collides).
    pub fn synthesise_dynamic_union(
        &self,
        members: &[TypeRef],
        anchor: &str,
        parent: Option<&str>,
        scope: ScopeId,
        from_module: &ModuleContext,
    ) -> String {
        let key = DynamicUnionKey(members.iter().map(format_type_ref_key).collect());
        if let Some(info) = self.dynamic_unions.borrow().by_key.get(&key) {
            return info.rust_name.clone();
        }
        let mut reg = self.dynamic_unions.borrow_mut();
        let chosen = allocate_dynamic_union_name(&mut self.names.borrow_mut(), anchor, parent);
        reg.by_key.insert(
            key,
            DynamicUnionInfo {
                rust_name: chosen.clone(),
                members: members.to_vec(),
                scope,
                from_module: from_module.clone(),
            },
        );
        chosen
    }

    /// Whether `name` is a synthesised dynamic-union enum.
    pub(crate) fn is_synthesised_enum(&self, name: &str) -> bool {
        self.dynamic_unions
            .borrow()
            .by_key
            .values()
            .any(|info| info.rust_name == name)
    }

    /// Generate `use` statements for all external type aliases.
    pub fn external_use_tokens(&self) -> TokenStream {
        let uses = self.external_uses.borrow();
        let mut entries: Vec<_> = uses.iter().collect();
        entries.sort_by_key(|(name, _)| (*name).clone());

        let stmts: Vec<TokenStream> = entries
            .into_iter()
            .map(|(local_name, rust_path)| {
                let local_ident = make_ident(local_name);
                // Parse the rust path into tokens
                let path: TokenStream = rust_path.parse().unwrap_or_else(|_| {
                    // Fallback: just use JsValue
                    quote! { JsValue }
                });
                if rust_path == "JsValue" || rust_path.ends_with("::JsValue") {
                    // JsValue fallback: use JsValue as LocalName
                    quote! { #[allow(dead_code)] use JsValue as #local_ident; }
                } else {
                    quote! { #[allow(dead_code)] use #path as #local_ident; }
                }
            })
            .collect();

        quote! { #(#stmts)* }
    }

    /// Resolve an external type through the external map.
    pub fn resolve_external(
        &self,
        type_name: &str,
        from_module: &str,
    ) -> Option<crate::external_map::RustPath> {
        self.gctx.external_map.resolve(type_name, from_module)
    }

    /// Resolve a named type through the scope chain, chasing the full alias chain
    /// until a non-alias terminal type is reached.
    ///
    /// Returns the final `TypeRef` target if the name resolves to a type alias
    /// (or a chain of aliases). Returns `None` if the name resolves to a
    /// non-alias declaration (Class, Interface, Enum, etc.) or is not found.
    ///
    /// Uses a visited set to detect and break circular alias chains.
    pub fn resolve_alias(&self, name: &str, scope: ScopeId) -> Option<&ir::TypeRef> {
        let mut visited = HashSet::new();
        self.resolve_alias_impl(name, scope, &mut visited)
    }

    /// The string or numeric enum declaration `ty` names, through aliases
    /// and qualified paths.
    pub(crate) fn enum_kind(&self, ty: &ir::TypeRef, scope: ScopeId) -> Option<&TypeKind> {
        let ty = ty
            .as_ident()
            .and_then(|name| self.resolve_alias(name, scope))
            .unwrap_or(ty);
        let ir::TypeRef::Reference {
            segments,
            generic_args,
        } = ty
        else {
            return None;
        };
        if !generic_args.is_empty() {
            return None;
        }
        let id = self.gctx.resolve_path(scope, &segments.join("."))?;
        let kind = &self.gctx.get_type(id).kind;
        matches!(kind, TypeKind::StringEnum(_) | TypeKind::NumericEnum(_)).then_some(kind)
    }

    /// The primitive an enum reference carries on the JS side.
    ///
    /// wasm-bindgen enums cross the ABI natively, like primitives, with no
    /// `JsGeneric` or slice support. Wherever a primitive would take its
    /// `js_sys` wrapper, an enum takes the wrapper of this primitive; in a
    /// slice it lowers as this primitive itself.
    pub(crate) fn enum_repr(&self, ty: &ir::TypeRef, scope: ScopeId) -> Option<ir::TypeRef> {
        match self.enum_kind(ty, scope)? {
            TypeKind::StringEnum(_) => Some(ir::TypeRef::String),
            _ => Some(ir::TypeRef::Number),
        }
    }

    fn resolve_alias_impl<'b>(
        &'b self,
        name: &str,
        scope: ScopeId,
        visited: &mut HashSet<String>,
    ) -> Option<&'b ir::TypeRef> {
        if !visited.insert(name.to_string()) {
            return None; // circular alias chain
        }
        if let Some(type_id) = self.gctx.scopes.resolve(scope, name) {
            let decl = self.gctx.get_type(type_id);
            if let TypeKind::TypeAlias(ref alias) = decl.kind {
                // If the target is itself a single-segment path,
                // keep resolving. Generic instantiations and
                // qualified paths stop here.
                if let Some(inner_name) = alias.target.as_ident() {
                    if let Some(resolved) = self.resolve_alias_impl(inner_name, scope, visited) {
                        return Some(resolved);
                    }
                }
                return Some(&alias.target);
            }
        }
        None
    }

    /// Emit an error diagnostic during code generation.
    pub fn error(&self, message: impl Into<String>) {
        self.diagnostics.borrow_mut().error(message);
    }

    /// Emit a warning diagnostic during code generation.
    pub fn warn(&self, message: impl Into<String>) {
        self.diagnostics.borrow_mut().warn(message);
    }

    /// Take ownership of the collected diagnostics.
    pub fn take_diagnostics(&self) -> DiagnosticCollector {
        self.diagnostics.take()
    }

    /// Claim every declared type at the top level, moving the ones that
    /// collide with the `js_sys` glob or the prelude aside under a trailing
    /// underscore.
    fn resolve_collisions(&mut self) {
        let names = self.names.get_mut();
        for name in self.local_types.keys() {
            names.claim(name.clone());
        }
        for name in reserved_names() {
            if self.local_types.contains_key(name) {
                let renamed = names.declare(name, Suffix::Trailing);
                self.renamed_locals.insert(name.to_string(), renamed);
            }
        }
    }

    /// Record a declaration this unit emits. `mod_path` is the `pub mod`
    /// it is emitted in.
    fn collect_declaration(&mut self, type_id: TypeId, mod_path: &[String]) {
        let gctx = self.gctx;
        let decl = gctx.get_type(type_id);
        let mctx = &decl.module_context;
        let (name, arity) = match &decl.kind {
            ir::TypeKind::Class(c) => (&c.name, Some(c.type_params.len())),
            ir::TypeKind::Interface(i) => (&i.name, Some(i.type_params.len())),
            ir::TypeKind::DiscriminatedUnion(d) => (&d.name, Some(d.type_params.len())),
            ir::TypeKind::StringEnum(e) => (&e.name, None),
            ir::TypeKind::NumericEnum(e) => (&e.name, None),
            // Plain references to an alias lower to its target, so only a
            // generic instantiation names the emitted alias.
            ir::TypeKind::TypeAlias(a) => (&a.name, Some(a.type_params.len())),
            ir::TypeKind::Namespace(ns) => {
                let mut path = mod_path.to_vec();
                path.push(crate::util::naming::to_snake_case(&ns.name));
                for &member in &ns.declarations {
                    self.collect_declaration(member, &path);
                }
                self.namespace_paths.insert(ns.child_scope, path);
                return;
            }
            ir::TypeKind::Function(_) | ir::TypeKind::Variable(_) => return,
        };
        self.local_types.insert(name.clone(), mctx.clone());
        self.local_type_ids.insert(type_id);
        if let Some(arity) = arity {
            // Fallback for use sites without a scope chain; resolved
            // references take their arity from the declaration itself, so a
            // top-level declaration keeps this entry over a same-named
            // namespace member.
            if mod_path.is_empty() {
                self.local_type_param_counts.insert(name.clone(), arity);
            } else {
                self.local_type_param_counts
                    .entry(name.clone())
                    .or_insert(arity);
            }
        }
    }

    /// The `pub mod` path declarations of `module` are emitted in: the top
    /// level for globals and the `--lib-name` module, and the specifier's
    /// segments for any other module (`node:fs` → `node::fs`).
    pub(crate) fn module_path(&self, module: &ModuleContext) -> Vec<String> {
        match module {
            ModuleContext::Module(m) => self.specifier_path(m),
            ModuleContext::Global => Vec::new(),
        }
    }

    fn specifier_path(&self, specifier: &str) -> Vec<String> {
        if self.lib_name.as_deref() == Some(specifier) {
            return Vec::new();
        }
        crate::util::naming::module_path(specifier)
    }

    /// The path from the `pub mod` being emitted into to the one `target`
    /// is declared in, or `None` when they are the same.
    pub(crate) fn namespace_qualifier(&self, target: TypeId) -> Option<TokenStream> {
        self.path_qualifier(self.namespace_path(self.gctx.get_type(target).scope_id))
    }

    /// The path from the `pub mod` being emitted into to `target_path`, or
    /// `None` when they are the same.
    fn path_qualifier(&self, target_path: &[String]) -> Option<TokenStream> {
        let here = self.emit_path.borrow();
        let common = here
            .iter()
            .zip(target_path)
            .take_while(|(a, b)| a == b)
            .count();
        if common == here.len() && common == target_path.len() {
            return None;
        }
        let supers = (common..here.len()).map(|_| quote! { super:: });
        let mods = target_path[common..].iter().map(|m| {
            let m = make_ident(m);
            quote! { #m:: }
        });
        Some(quote! { #(#supers)* #(#mods)* })
    }

    /// The `pub mod` path of the innermost emitted namespace enclosing
    /// `scope`.
    fn namespace_path(&self, scope: ScopeId) -> &[String] {
        let mut scope = Some(scope);
        while let Some(id) = scope {
            if let Some(path) = self.namespace_paths.get(&id) {
                return path;
            }
            scope = self.gctx.scopes.get(id).parent;
        }
        &[]
    }

    /// Returns the names of types in `type_ids` whose
    /// `module_context` is `Module(spec)` that do *not* resolve via
    /// the external map. Used by `generate_tokens` to emit a partial-
    /// coverage warning when the user has both `--export <spec>`
    /// (asking for emission) and some but not all external mappings
    /// for the same module.
    pub(crate) fn external_module_unresolved(
        &self,
        spec: &str,
        type_ids: &[crate::context::TypeId],
    ) -> Vec<String> {
        let mut unresolved = Vec::new();
        for &type_id in type_ids {
            let decl = self.gctx.get_type(type_id);
            if let Some(name) = type_decl_name(&decl.kind) {
                if self.gctx.external_map.resolve(name, spec).is_none() {
                    unresolved.push(name.to_string());
                }
            }
        }
        unresolved
    }
}

/// Helper: get the public name of a declaration kind, if it has one.
/// Mirrors the `kind` arms in `populate_builtin_scope::declaration_name`.
fn type_decl_name(kind: &ir::TypeKind) -> Option<&str> {
    match kind {
        ir::TypeKind::Class(c) => Some(&c.name),
        ir::TypeKind::Interface(i) => Some(&i.name),
        ir::TypeKind::DiscriminatedUnion(d) => Some(&d.name),
        ir::TypeKind::TypeAlias(a) => Some(&a.name),
        ir::TypeKind::StringEnum(e) => Some(&e.name),
        ir::TypeKind::NumericEnum(e) => Some(&e.name),
        ir::TypeKind::Function(f) => Some(&f.name),
        ir::TypeKind::Variable(v) => Some(&v.name),
        ir::TypeKind::Namespace(n) => Some(&n.name),
    }
}

/// Map an IR `TypeRef` to a `proc_macro2::TokenStream` representing the Rust type.
///
/// This is the unified type mapping function, following the wasm-bindgen WebIDL
/// `to_syn_type` pattern. A single function handles all positions:
///
/// - When `pos.inner` is true, primitives become JS wrapper types
///   (`Number`, `JsString`, `Boolean`, `Undefined`), nullable becomes `JsOption`,
///   and argument-position types are NOT borrowed.
/// - When `pos.inner` is false, standard Rust types are used (`f64`, `&str`/`String`,
///   `bool`, `()`), nullable becomes `Option<T>`, and argument-position types
///   may be borrowed.
pub fn to_syn_type(
    ty: &TypeRef,
    pos: TypePosition,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    // Only per-mono arguments distinguish the `PropertyKey` spelling (see
    // `signatures::mono_argument_type`); every other position lowers it as
    // the union it abbreviates.
    if matches!(ty, TypeRef::PropertyKey) {
        return to_syn_type(
            &TypeRef::Union(TypeRef::property_key_members()),
            pos,
            ctx,
            scope,
            from_module,
        );
    }

    // When inner, intercept primitives and nullable early to use JS wrapper forms
    if pos.inner {
        match ty {
            TypeRef::Boolean | TypeRef::BooleanLiteral(_) => return quote! { Boolean },
            TypeRef::Number | TypeRef::NumberLiteral(_) => return quote! { Number },
            TypeRef::String | TypeRef::StringLiteral(_) => return quote! { JsString },
            TypeRef::Void | TypeRef::Undefined => return quote! { Undefined },
            TypeRef::Nullable(inner) => {
                let inner_ty = to_syn_type(inner, pos, ctx, scope, from_module);
                return js_option_or_js_value(inner_ty);
            }
            _ => {}
        }
    }

    // Helper: should this type get `&` in argument position?
    // Returns true for all JS/non-Rust types (anything that crosses the FFI boundary
    // as a wasm-bindgen reference). Rust-native primitives (bool, f64, ()) do NOT get `&`.
    let borrow = pos.is_argument() && !pos.inner;

    match ty {
        // === Primitives (outer position only reaches here) ===
        TypeRef::Boolean => quote! { bool },
        TypeRef::Number => quote! { f64 },
        TypeRef::String => {
            if borrow {
                quote! { &str }
            } else {
                quote! { String }
            }
        }
        TypeRef::BigInt => maybe_ref(quote! { BigInt }, borrow),
        TypeRef::Void => quote! { () },
        TypeRef::Undefined => maybe_ref(quote! { Undefined }, borrow),
        TypeRef::Null => maybe_ref(quote! { Null }, borrow),
        TypeRef::Any => maybe_ref(quote! { JsValue }, borrow),
        TypeRef::Unknown => maybe_ref(quote! { JsValue }, borrow),
        TypeRef::Object => maybe_ref(quote! { Object }, borrow),
        TypeRef::Symbol => maybe_ref(quote! { Symbol }, borrow),

        // === TS-only synthetic ===
        // `ArrayBufferView` is a TS-only union alias (typed-array
        // family + DataView), not a JS class. There's no single Rust
        // type that captures the union; we specialise by position:
        //
        // * Return position → `Uint8Array`. The most useful concrete
        //   typed-array view; callers can re-cast to a different
        //   typed-array via `JsCast::dyn_into` if needed.
        // * Argument position → `&Object`. The dictionary-builder
        //   path overrides this with a generic `<T: TypedArray>`
        //   so callers can pass any concrete typed-array.
        TypeRef::ArrayBufferView => {
            if pos.is_argument() {
                maybe_ref(quote! { Object }, borrow)
            } else {
                quote! { Uint8Array }
            }
        }
        TypeRef::PropertyKey => unreachable!("desugared at the top of `to_syn_type`"),

        // === Syntactic constructs ===
        //
        // Top-level `Array<T>` (or `T[]`) lowers to a Rust-idiomatic
        // sequence:
        //
        // * Argument position → `&[T]`, reaching JS as a plain `Array`.
        // * Return position → `Vec<T>` (owned).
        //
        // wasm-bindgen only has slice / `Vec` ABIs for Rust-native
        // `String` and `f64` elements and for JS handles. So an element
        // that resolves (through aliases and unions) to a string or
        // number takes return-position lowering (`&[f64]`,
        // `Vec<String>`; `&[&str]` would needlessly thread a borrow
        // lifetime through the slice). Every other element takes its
        // inner-position handle form: `&[EmailAttachment]`,
        // `Vec<Boolean>`, `Vec<JsOption<JsString>>`, and
        // `Vec<Array<JsString>>` for `string[][]`.
        //
        // A slice of Rust-native elements also needs
        // `#[wasm_bindgen(slice_to_array)]` on its binding to reach JS
        // as an `Array`; see [`needs_slice_to_array`].
        //
        // Inside an actual generic (`pos.inner == true`) we keep the
        // legacy `Array<T'>` form for callers nested in
        // `Promise<Array<T>>`, `Map<K, Array<V>>`, etc.
        TypeRef::Array(inner) => {
            if pos.inner {
                generic_container(quote! { Array }, inner, pos, ctx, scope, from_module)
            } else {
                let repr = ctx.and_then(|c| c.enum_repr(inner, scope));
                let inner = repr.as_ref().unwrap_or(inner);
                let elem_pos = if is_native_array_element(inner, ctx, scope) {
                    TypePosition::RETURN
                } else {
                    TypePosition::RETURN.to_inner()
                };
                let elem = to_syn_type(inner, elem_pos, ctx, scope, from_module);
                if pos.is_argument() {
                    quote! { &[#elem] }
                } else {
                    quote! { Vec<#elem> }
                }
            }
        }

        // === Structural Types ===
        TypeRef::Nullable(inner) => {
            let inner_ty = to_syn_type(inner, pos, ctx, scope, from_module);
            if pos.inner {
                js_option_or_js_value(inner_ty)
            } else if !pos.is_argument() && is_jsvalue_arg(&inner_ty) {
                // In return position, `Option<JsValue>` is redundant —
                // `JsValue` already carries `null`/`undefined` in-band,
                // so collapse to bare `JsValue`. Argument position
                // keeps `Option<&JsValue>` since the caller may want
                // to distinguish "no value" from "JsValue::NULL".
                quote! { JsValue }
            } else {
                quote! { Option<#inner_ty> }
            }
        }
        TypeRef::Union(members) => {
            // Single source of truth for union LUB — handles literal
            // widening, named-type ancestor walking, and the JsValue
            // fallback diagnostic uniformly. See `subtyping::lub_union`.
            if let Some(lub) = crate::codegen::subtyping::lub_union(members, ctx, scope) {
                return to_syn_type(&lub, pos, ctx, scope, from_module);
            }
            if let Some(c) = ctx {
                c.warn(format!(
                    "union `{}` has no common supertype, erased to JsValue",
                    ty.format_ts(),
                ));
            }
            maybe_ref(quote! { JsValue }, borrow)
        }
        TypeRef::Intersection(members) => {
            if let Some(bound) = crate::codegen::subtyping::glb_intersection(members, ctx, scope) {
                return to_syn_type(&bound, pos, ctx, scope, from_module);
            }
            if let Some(c) = ctx {
                c.warn(format!(
                    "intersection `{}` has no member that subtypes the others, erased to JsValue",
                    ty.format_ts(),
                ));
            }
            maybe_ref(quote! { JsValue }, borrow)
        }
        TypeRef::Tuple(elems) => {
            let base = if elems.is_empty() {
                quote! { Array }
            } else {
                let inner_pos = pos.to_inner();
                let elem_types: Vec<TokenStream> = elems
                    .iter()
                    .map(|e| to_syn_type(e, inner_pos, ctx, scope, from_module))
                    .collect();
                quote! { ArrayTuple<(#(#elem_types),*)> }
            };
            maybe_ref(base, borrow)
        }
        TypeRef::Function(sig) => {
            let inner_pos = pos.to_inner();
            let params: Vec<TokenStream> = sig
                .params
                .iter()
                .take(8)
                .map(|p| to_syn_type(&p.type_ref, inner_pos, ctx, scope, from_module))
                .collect();
            let ret = to_syn_type(&sig.return_type, inner_pos, ctx, scope, from_module);
            let base = if params.iter().all(is_jsvalue_arg) && is_jsvalue_arg(&ret) {
                quote! { Function }
            } else {
                quote! { Function<fn(#(#params),*) -> #ret> }
            };
            maybe_ref(base, borrow)
        }

        // === Literal Types ===
        TypeRef::StringLiteral(_) => {
            if borrow {
                quote! { &str }
            } else {
                quote! { String }
            }
        }
        TypeRef::NumberLiteral(_) => quote! { f64 },
        TypeRef::BooleanLiteral(_) => quote! { bool },

        // === Named References ===
        //
        // Bare ident, generic instantiation, qualified path — all
        // flow through one arm. Resolution mirrors TypeScript's name
        // lookup: user scope first (so `interface MyDate {}` shadows
        // `Date`), then well-known JS heads (`js_sys::*` glob), then
        // the external map (`web_sys::*` defaults / user mappings),
        // then `JsValue` fallback with a diagnostic.
        //
        // Two cases short-circuit straight to a recursive
        // `to_syn_type`, bypassing the outer `maybe_ref` wrap so the
        // already-borrowed slice / `Vec<T>` shape isn't double-borrowed:
        //
        // * `Array<T>` / `ReadonlyArray<T>` — lower to the JS-array
        //   reference form (`&Array<U>` / `Array<U>`), distinct from
        //   the syntactic `T[]` which keeps the Rust-slice form.
        //   TS spelling drives the choice: callers who want a JS
        //   array reference (no element-wise materialisation) write
        //   `Array<T>`; callers who want a Rust slice write `T[]`.
        // * Bare references that resolve to a type alias whose target
        //   carries its own borrow shape (e.g. `string`, `T[]`,
        //   `Array<U>`) — recursing into the target re-runs
        //   `to_syn_type` with the same `pos`, which already does the
        //   right thing.
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            if segments.len() == 1 && (segments[0] == "Array" || segments[0] == "ReadonlyArray") {
                let inner = generic_args.first().cloned().unwrap_or(TypeRef::Any);
                let base =
                    generic_container(quote! { Array }, &inner, pos, ctx, scope, from_module);
                return maybe_ref(base, borrow);
            }
            if segments.len() == 1 && generic_args.is_empty() {
                if let Some(c) = ctx {
                    if c.experimental_generic_mono
                        && matches!(
                            c.gctx.scopes.resolve_binding(scope, &segments[0]),
                            Some(crate::parse::scope::Binding::TypeParam),
                        )
                    {
                        return lower_reference(
                            segments,
                            generic_args,
                            pos,
                            ctx,
                            scope,
                            from_module,
                        );
                    }
                    if let Some(target) = c.resolve_alias(&segments[0], scope) {
                        let target = target.clone();
                        return to_syn_type(&target, pos, ctx, scope, from_module);
                    }
                    // Synthesised enum names never collide with declared
                    // types. wasm-bindgen enums cross the boundary by value.
                    if c.is_synthesised_enum(&segments[0]) {
                        let ident = make_ident(&segments[0]);
                        return quote! { #ident };
                    }
                }
            }
            // `Ns.Alias` lowers its target from the alias's own scope, where
            // the target's names are declared.
            if segments.len() > 1 && generic_args.is_empty() {
                if let Some(c) = ctx {
                    let resolved = c.gctx.resolve_path(scope, &segments.join("."));
                    if let Some(TypeKind::TypeAlias(alias)) =
                        resolved.map(|id| &c.gctx.get_type(id).kind)
                    {
                        return to_syn_type(&alias.target, pos, ctx, alias.body_scope, from_module);
                    }
                }
            }
            if let Some(repr) = ctx.and_then(|c| c.enum_repr(ty, scope)) {
                if pos.inner {
                    return to_syn_type(&repr, pos, ctx, scope, from_module);
                }
                return lower_reference(segments, generic_args, pos, ctx, scope, from_module);
            }
            maybe_ref(
                lower_reference(segments, generic_args, pos, ctx, scope, from_module),
                borrow,
            )
        }

        // === Fallback ===
        TypeRef::Unresolved(desc) => {
            if let Some(cgctx) = ctx {
                cgctx.warn(format!("unresolved type `{desc}`, falling back to JsValue"));
            }
            maybe_ref(quote! { JsValue }, borrow)
        }
    }
}

/// Wrap a type in `&` when in argument position (the `externref` pattern from wasm-bindgen WebIDL).
///
/// All JS object types (anything that isn't a Rust `Copy` primitive like `bool`/`f64`)
/// are passed by reference in argument position at the top level.
fn maybe_ref(ty: TokenStream, borrow: bool) -> TokenStream {
    if borrow {
        quote! { &#ty }
    } else {
        ty
    }
}

/// Helper: emit `Base<T'>` or just `Base` if T' is JsValue (the default).
fn generic_container(
    base: TokenStream,
    inner: &TypeRef,
    pos: TypePosition,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    let arg = to_syn_type(inner, pos.to_inner(), ctx, scope, from_module);
    if is_jsvalue_arg(&arg) {
        base
    } else {
        quote! { #base<#arg> }
    }
}

fn js_option_or_js_value(inner_ty: TokenStream) -> TokenStream {
    if is_jsvalue_arg(&inner_ty) {
        quote! { JsValue }
    } else {
        quote! { JsOption<#inner_ty> }
    }
}

/// Check if a generic argument token stream represents `JsValue` (the default).
/// When it is the default, we elide the generic parameter.
fn is_jsvalue_arg(tokens: &TokenStream) -> bool {
    let s = tokens.to_string();
    s == "JsValue"
}

/// Emit a type name as Rust tokens.
///
/// Single unified path for ALL type name emission:
/// 1. Resolve name → TypeId through scope (the resolved declaration tells
///    us where the referenced type actually lives — its `module_context`).
/// 2. Get canonical name (last segment for dotted paths).
/// 3. Resolved to a local type → emit directly, qualified with the path to
///    its namespace `pub mod` (with js_sys collision rename).
/// 4. Resolved-but-non-local with an external mapping → register the use alias.
/// 5. Unresolved → js_sys glob → name directly.
/// 6. Otherwise → error + `use JsValue as Foo;`.
///
/// `use_scope` is the scope the reference appears in, and `from_module` the
/// [`ModuleContext`] of the extern block this name is being emitted into.
///
/// Going through the scope-resolved `TypeId` (rather than a flat name →
/// module map) keeps name collisions like a global `interface Foo` and a
/// namespace member `Foo` correctly distinguished: scope-chain resolution
/// from the use site picks the visible declaration, and that declaration's
/// scope drives qualification.
fn emit_type_name(
    name: &str,
    use_scope: Option<ScopeId>,
    ctx: &CodegenContext<'_>,
    from_module: &ModuleContext,
) -> TokenStream {
    // A use site inside an ambient module also sees that module's private
    // declarations, which live in its body scopes rather than a file scope.
    let module_scopes = match from_module {
        ModuleContext::Module(m) => ctx.gctx.ambient_module_scopes.get(m.as_ref()),
        ModuleContext::Global => None,
    };
    // Names resolve from the use site first: a namespace member sees its
    // siblings over same-named globals, and a qualified path can name a
    // namespace only visible there.
    let resolved = use_scope
        .and_then(|scope| ctx.gctx.resolve_path(scope, name))
        .or_else(|| {
            module_scopes
                .into_iter()
                .flatten()
                .chain(&ctx.file_scopes)
                .find_map(|&scope| {
                    if name.contains('.') {
                        ctx.gctx.resolve_path(scope, name)
                    } else {
                        ctx.gctx.scopes.resolve(scope, name)
                    }
                })
        });

    // Canonical ident name (last segment for dotted paths)
    let ident_name = name.rsplit('.').next().unwrap_or(name);

    // A namespace name on its own (not as a path prefix) isn't a
    // bindable type — wasm-bindgen has no way to express "the namespace
    // value", so erase it to JsValue and warn.
    if let Some(type_id) = resolved {
        if matches!(&ctx.gctx.get_type(type_id).kind, TypeKind::Namespace(_)) {
            ctx.warn(format!(
                "type reference `{name}` resolves to a namespace, not a type — erasing to JsValue"
            ));
            return quote! { JsValue };
        }
    }

    // A scope-registered builtin (`Error`, `Promise`, `ReadableStream`,
    // `Headers`, …) is either provided unprefixed by `use js_sys::*` or
    // expected to land via a `--external` mapping at the top of the
    // generated file. Either way the right token is the bare ident.
    if let Some(type_id) = resolved {
        if ctx.gctx.builtin_type_ids.contains(&type_id) {
            // Names in the js_sys glob need no use alias.
            if !JS_SYS_RESERVED.contains(&ident_name) {
                if let Some(rust_path) = ctx.gctx.external_map.resolve_type(ident_name) {
                    ctx.register_external(ident_name, &rust_path.path);
                } else {
                    ctx.error(format!(
                        "builtin type `{name}` is not in `js_sys` and has no \
                         external mapping. Use --external to map this type."
                    ));
                    ctx.register_external(ident_name, "JsValue");
                }
            }
            let ident = make_ident(ident_name);
            return quote! { #ident };
        }
    }

    // A type is "local" only if codegen will actually emit it as a
    // top-level declaration in this output (tracked in `local_type_ids`).
    // When two declarations share a name (e.g. a global `interface
    // EmailMessage` and a module-scoped `class EmailMessage`), the
    // scope-chain resolution picks the visible one at the use site, and
    // its `module_context` drives qualification across module boundaries.
    let target_module = resolved
        .filter(|id| ctx.local_type_ids.contains(id))
        .map(|type_id| ctx.gctx.get_type(type_id).module_context.clone())
        .or_else(|| {
            // Fallback: tests and entry points that don't construct a
            // full scope chain populate `local_types` by name.
            if resolved.is_none() && !name.contains('.') {
                ctx.local_types.get(ident_name).cloned()
            } else {
                None
            }
        });

    if let Some(target_module) = target_module {
        // Externalised module: references resolve through the external
        // map to the user-supplied crate path.
        if let ModuleContext::Module(m) = &target_module {
            let m_str: &str = m.as_ref();
            if ctx.externalised_modules.contains(m_str) {
                if let Some(rust_path) = ctx.gctx.external_map.resolve(ident_name, m_str) {
                    ctx.register_external(ident_name, &rust_path.path);
                    let ident = make_ident(ident_name);
                    return quote! { #ident };
                }
            }
        }
        // Resolution-only module: the type lives in a `Module(m)` we
        // parsed but the user neither emitted it (`--export m`) nor
        // externalised it (`--external m=...`). The reference can't
        // compile against any emitted declaration, so we error and
        // fall back to a `use JsValue as Name;` alias to keep the
        // output buildable.
        if let ModuleContext::Module(m) = &target_module {
            let m_str: &str = m.as_ref();
            let not_exported = !ctx.exported_modules.contains(m_str);
            let no_external = ctx.gctx.external_map.resolve(ident_name, m_str).is_none();
            if not_exported && no_external {
                let key = (ident_name.to_string(), m_str.to_string());
                if ctx.unresolved_module_refs.borrow_mut().insert(key) {
                    ctx.error(format!(
                        "type `{ident_name}` from module \"{m_str}\" is referenced by exported \
                         declarations but module \"{m_str}\" is not in --export and not in \
                         --external. Add `--export {m_str}` to emit it, or \
                         `--external {m_str}=...` to point at a separately-generated crate."
                    ));
                }
                ctx.register_external(ident_name, "JsValue");
                let ident = make_ident(ident_name);
                return quote! { #ident };
            }
        }
        let rust_name = ctx
            .renamed_locals
            .get(ident_name)
            .map(String::as_str)
            .unwrap_or(ident_name);
        let ident = make_ident(rust_name);
        let qualifier = match resolved {
            Some(id) => ctx.namespace_qualifier(id),
            None => ctx.path_qualifier(&ctx.module_path(&target_module)),
        };
        return match qualifier {
            Some(prefix) => quote! { #prefix #ident },
            None => quote! { #ident },
        };
    }

    // External map
    if let Some(rust_path) = ctx.gctx.external_map.resolve_type(ident_name) {
        ctx.register_external(ident_name, &rust_path.path);
        let ident = make_ident(ident_name);
        return quote! { #ident };
    }

    // Type resolved through scope but is not local and not in external map.
    // It's a dependency type — register as JsValue alias (user needs --external).
    if resolved.is_some() {
        ctx.error(format!(
            "Non-local type `{name}` resolved but has no external mapping. \
             Use --external to map this type."
        ));
        ctx.register_external(ident_name, "JsValue");
        let ident = make_ident(ident_name);
        return quote! { #ident };
    }

    // Truly unresolved — error + JsValue alias. A qualified name's last
    // segment may be a local type's name, so it isn't aliased.
    ctx.error(format!(
        "Unresolved type `{name}`. Use --external to map this type."
    ));
    if name.contains('.') {
        return quote! { JsValue };
    }
    ctx.register_external(ident_name, "JsValue");
    let ident = make_ident(ident_name);
    quote! { #ident }
}

/// Backward-compatible wrapper: calls `emit_type_name` when ctx is available.
fn named_type_to_rust(
    name: &str,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    match ctx {
        Some(ctx) => emit_type_name(name, Some(scope), ctx, from_module),
        // Without a codegen context (e.g. unit tests that exercise
        // type lowering directly) we can still resolve names that
        // appear in the `js_sys::*` glob — emit them as a bare ident.
        // Anything else falls back to `JsValue`.
        None if JS_SYS_RESERVED.contains(&name) => {
            let ident = make_ident(name);
            quote! { #ident }
        }
        None => quote! { JsValue },
    }
}

/// Type-parameter count of a declaration that can be instantiated, or
/// `None` for kinds that cannot carry generic arguments.
fn declared_arity(kind: &TypeKind) -> Option<usize> {
    match kind {
        TypeKind::Class(c) => Some(c.type_params.len()),
        TypeKind::Interface(i) => Some(i.type_params.len()),
        TypeKind::DiscriminatedUnion(d) => Some(d.type_params.len()),
        TypeKind::TypeAlias(a) => Some(a.type_params.len()),
        _ => None,
    }
}

/// Lower a `TypeRef::Reference` to its Rust syntax token. Handles
/// alias resolution, dedicated-codegen heads (`Promise`, `Map`,
/// `Set`, `Record`, `Iterator`, ...), in-scope generic type
/// parameters, and falls through to `emit_type_name` for everything
/// else.
///
/// `segments` is the full path (`["Foo"]` or `["A", "B", "C"]`).
/// `generic_args` is empty for non-generic references and non-empty
/// for instantiations like `Foo<T>` or `A.B.C<X>`.
fn lower_reference(
    segments: &[String],
    generic_args: &[TypeRef],
    pos: TypePosition,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    // Qualified paths resolve through namespace scopes from the use site;
    // `emit_type_name` then consults the external map and falls back to
    // `JsValue` with a diagnostic for anything that doesn't match. They
    // keep their generic arguments like a bare name does.
    if segments.len() > 1 {
        let dotted = segments.join(".");
        let base = match ctx {
            Some(c) => emit_type_name(&dotted, Some(scope), c, from_module),
            None => quote! { JsValue },
        };
        return with_generic_args(base, &dotted, generic_args, pos, ctx, scope, from_module);
    }

    let head = segments.first().map(String::as_str).unwrap_or("");

    // Heads with dedicated codegen rules. `Promise<T>`, `Map<K,V>`,
    // `Set<T>`, `Record<K,V>`, and the iteration protocols share a
    // single dispatch here. The syntactic `T[]` form is handled
    // separately by `TypeRef::Array`; `Array<T>` / `ReadonlyArray<T>`
    // get re-routed through that arm by the caller.
    match head {
        "Promise" | "PromiseLike" => {
            let inner = generic_args.first().cloned().unwrap_or(TypeRef::Any);
            return generic_container(quote! { Promise }, &inner, pos, ctx, scope, from_module);
        }
        "Set" | "ReadonlySet" => {
            let inner = generic_args.first().cloned().unwrap_or(TypeRef::Any);
            return generic_container(quote! { Set }, &inner, pos, ctx, scope, from_module);
        }
        "Map" | "ReadonlyMap" => {
            let inner_pos = pos.to_inner();
            let k = generic_args.first().cloned().unwrap_or(TypeRef::Any);
            let v = generic_args.get(1).cloned().unwrap_or(TypeRef::Any);
            let k_arg = to_syn_type(&k, inner_pos, ctx, scope, from_module);
            let v_arg = to_syn_type(&v, inner_pos, ctx, scope, from_module);
            return if is_jsvalue_arg(&k_arg) && is_jsvalue_arg(&v_arg) {
                quote! { Map }
            } else {
                quote! { Map<#k_arg, #v_arg> }
            };
        }
        "Record" => {
            // `Record<K, V>` desugars to an `Object` with V-typed
            // values. Drop the key type — wasm-bindgen has no Rust
            // representation for "object with arbitrary string keys".
            let v = generic_args.get(1).cloned().unwrap_or(TypeRef::Any);
            return generic_container(quote! { Object }, &v, pos, ctx, scope, from_module);
        }
        // Already-iterator types: `Iterator<T>` and `IterableIterator<T>`
        // both map straight to `js_sys::Iterator<T>`. The async pair
        // maps to `js_sys::AsyncIterator<T>`.
        "Iterator" | "IterableIterator" => {
            let inner = generic_args.first().cloned().unwrap_or(TypeRef::Any);
            return generic_container(quote! { Iterator }, &inner, pos, ctx, scope, from_module);
        }
        "AsyncIterator" | "AsyncIterableIterator" => {
            let inner = generic_args.first().cloned().unwrap_or(TypeRef::Any);
            return generic_container(
                quote! { AsyncIterator },
                &inner,
                pos,
                ctx,
                scope,
                from_module,
            );
        }
        // The iterability protocol — `Iterable<T>` exposes
        // `[Symbol.iterator](): Iterator<T>` rather than being an
        // iterator itself. Top-level occurrences are hoisted by
        // `parse::members` synthesis into a dedicated wrapper before
        // reaching codegen; un-hoisted (nested) occurrences erase to
        // `JsValue` since we can't express the protocol inline.
        "Iterable" | "AsyncIterable" => {
            return quote! { JsValue };
        }
        _ => {}
    }

    // In-scope generic type parameter — lower to a bare Rust ident.
    // The surrounding method/type carries the `<T: JsGeneric>` bound.
    if generic_args.is_empty() {
        if let Some(c) = ctx {
            if matches!(
                c.gctx.scopes.resolve_binding(scope, head),
                Some(crate::parse::scope::Binding::TypeParam),
            ) {
                let ident = make_ident(head);
                return quote! { #ident };
            }
        }
    }

    let base = named_type_to_rust(head, ctx, scope, from_module);
    with_generic_args(base, head, generic_args, pos, ctx, scope, from_module)
}

/// Append `generic_args` to `base` when `name` is a locally emitted generic
/// type. Everything else (web_sys externals, builtins) is treated as
/// non-generic and the args are dropped to keep the output compiling.
#[allow(clippy::too_many_arguments)]
fn with_generic_args(
    base: TokenStream,
    name: &str,
    generic_args: &[TypeRef],
    pos: TypePosition,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    if generic_args.is_empty() {
        return base;
    }
    let accepts_generics = ctx.is_some_and(|c| c.is_local_generic(name, scope));
    if !accepts_generics {
        if let Some(c) = ctx {
            c.warn(format!(
                "generic type arguments on `{name}<...>` are not emitted (target is not a \
                 locally-declared generic type), using bare `{name}`"
            ));
        }
        return base;
    }
    let arg_tokens: Vec<TokenStream> = generic_args
        .iter()
        .map(|a| {
            if CodegenContext::generic_mono(ctx) {
                to_mono_user_generic_arg(a, ctx, scope, from_module)
            } else {
                to_syn_type(a, pos.to_inner(), ctx, scope, from_module)
            }
        })
        .collect();
    quote! { #base<#(#arg_tokens),*> }
}

/// Create a `syn::Ident`, sanitizing invalid characters and escaping keywords.
pub(crate) fn make_ident(name: &str) -> syn::Ident {
    // Strip characters that aren't valid in Rust identifiers
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let sanitized = if sanitized.is_empty() {
        "__unknown__".to_string()
    } else if sanitized.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{sanitized}")
    } else {
        sanitized
    };
    // Try as a normal identifier first.
    if let Ok(ident) = syn::parse_str::<syn::Ident>(&sanitized) {
        return ident;
    }
    // `self`, `Self`, `super`, `crate` cannot be raw identifiers — append `_`.
    match sanitized.as_str() {
        "self" | "Self" | "super" | "crate" => {
            syn::Ident::new(&format!("{sanitized}_"), proc_macro2::Span::call_site())
        }
        // All other keywords can use r# raw identifiers.
        _ => syn::Ident::new_raw(&sanitized, proc_macro2::Span::call_site()),
    }
}

/// Whether a binding needs an explicit `js_name`. wasm-bindgen infers the JS
/// name from the emitted ident, so this must compare against the ident after
/// [`make_ident`] sanitisation (`0` → `_0`, `$foo` → `foo`), not the
/// pre-sanitised Rust name.
pub(crate) fn needs_js_name(ident: &syn::Ident, js_name: &str) -> bool {
    use syn::ext::IdentExt;
    ident.unraw() != js_name
}

/// Map an IR `TypeRef` to the type used in a wasm_bindgen return position,
/// wrapping in `Result<T, ErrTy>` when `catch` is true.
///
/// `error_ty`, when supplied, replaces the default `JsValue` error type —
/// callers pass the simplified throws union here. Resolution goes through
/// the same `to_syn_type` path as any other type, so e.g. `js_sys::TypeError`
/// vs an external mapping is handled uniformly.
#[allow(clippy::too_many_arguments)]
pub fn to_return_type(
    ty: &TypeRef,
    catch: bool,
    is_async: bool,
    js_string_return: bool,
    error_ty: Option<&TypeRef>,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
    anchor: ReturnAnchor<'_>,
) -> TokenStream {
    // Async returns are `Promise<T>` JS-side. `wasm-bindgen` requires
    // `T: JsGeneric` for `Promise<T>` / `JsFuture<T>`, which bare Rust
    // primitives don't satisfy. Routing through `to_inner()` reuses
    // the inside-generic Canon path in `to_syn_type`: primitives lower
    // to `js_sys` wrappers (`Boolean` / `Number` / `JsString` /
    // `Undefined`) and `Nullable<T>` becomes `JsOption<T>`. Callers
    // recover primitives via `.value_of()` (`Boolean` / `Number`) or
    // `String::from(_)` (`JsString`).
    //
    // Top-level (non-`is_async`) returns also participate in
    // dynamic-union synthesis — see [`maybe_synthesise_return_union`].
    let mono_native_return = is_async
        && ctx.is_some_and(|c| {
            c.experimental_generic_mono && is_mono_native_return_shape(ty, c, scope)
        });
    // `js_string_return` is only set by `build_signatures` in per-mono mode,
    // which implies a context; without one, fall through to the plain lowering.
    let js_string_ctx = ctx.filter(|_| js_string_return);
    let inner = if let Some(ctx) = js_string_ctx {
        to_mono_js_string_return_type(ty, ctx, scope, from_module)
    } else if !is_async || mono_native_return {
        match maybe_synthesise_return_union(ty, ctx, scope, from_module, anchor) {
            Some(tokens) => tokens,
            None => to_syn_type(ty, TypePosition::RETURN, ctx, scope, from_module),
        }
    } else {
        to_syn_type(ty, TypePosition::RETURN.to_inner(), ctx, scope, from_module)
    };

    if catch {
        let err = match error_ty {
            Some(ty) => to_syn_type(ty, TypePosition::RETURN, ctx, scope, from_module),
            None => default_error_type(ctx),
        };
        quote! { Result<#inner, #err> }
    } else {
        inner
    }
}

/// Per-monomorphization walkers.
///
/// Each walker first calls [`peel_mono`], which resolves aliases and unions
/// that reduce to one common type. The walkers then only match on primitive
/// leaves, `Nullable`, and locally declared generic instantiations, so
/// `Holder<Id>`, `Holder<"a" | "b">` and `Holder<string>` all get the same
/// answer from every walker.
///
/// `visited` holds the aliases already resolved on the current path, so a
/// self-referential alias ends the walk. When a walker descends into several
/// generic arguments, it gives each argument its own copy of `visited`, so
/// `Pair<Id, Id>` resolves `Id` both times.
fn peel_mono<'t>(
    ty: &'t TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    visited: &mut HashSet<String>,
) -> std::borrow::Cow<'t, TypeRef> {
    let mut current = std::borrow::Cow::Borrowed(ty);
    loop {
        let next = match current.as_ref() {
            TypeRef::Reference {
                segments,
                generic_args,
            } if segments.len() == 1 && generic_args.is_empty() => {
                if !visited.insert(segments[0].clone()) {
                    break;
                }
                ctx.resolve_alias(&segments[0], scope).cloned()
            }
            TypeRef::Union(members) => {
                crate::codegen::subtyping::lub_union(members, Some(ctx), scope)
                    .filter(|lub| !matches!(lub, TypeRef::Union(_)))
            }
            _ => None,
        };
        match next {
            Some(next) => current = std::borrow::Cow::Owned(next),
            None => break,
        }
    }
    current
}

/// Leaves that cross the ABI natively rather than as a JS handle: primitives
/// and enums. A per-mono type parameter instantiated with one keeps it
/// (`Holder<String>`, `Holder<Kind>`), where an erased `T: JsGeneric` would
/// need its `js_sys` wrapper (`Holder<JsString>` for both).
fn is_mono_native_leaf(ty: &TypeRef, ctx: &CodegenContext<'_>, scope: ScopeId) -> bool {
    matches!(
        ty,
        TypeRef::Boolean
            | TypeRef::BooleanLiteral(_)
            | TypeRef::Number
            | TypeRef::NumberLiteral(_)
            | TypeRef::String
            | TypeRef::StringLiteral(_)
    ) || ctx.enum_kind(ty, scope).is_some()
}

/// The generic arguments of `ty` if it instantiates a locally declared
/// generic type, e.g. `Holder<T>`. These are the only generic arguments
/// that per-mono lowering looks inside.
fn local_generic_args<'t>(
    ty: &'t TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
) -> Option<&'t [TypeRef]> {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } if !generic_args.is_empty() && ctx.is_local_generic(&segments.join("."), scope) => {
            Some(generic_args)
        }
        _ => None,
    }
}

/// Lower an argument of a locally declared generic type in per-mono mode.
/// Native leaves (primitives, enums) use their native Rust ABI, while
/// built-in JS containers remain on the established inner-position wrapper
/// mapping.
fn to_mono_user_generic_arg(
    ty: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    let Some(c) = ctx else {
        return to_syn_type(ty, TypePosition::RETURN.to_inner(), ctx, scope, from_module);
    };
    to_mono_user_generic_arg_impl(ty, c, scope, from_module, &mut HashSet::new())
}

fn to_mono_user_generic_arg_impl(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    from_module: &ModuleContext,
    visited: &mut HashSet<String>,
) -> TokenStream {
    let peeled = peel_mono(ty, ctx, scope, visited);
    match peeled.as_ref() {
        t if is_mono_native_leaf(t, ctx, scope) => {
            to_syn_type(t, TypePosition::RETURN, Some(ctx), scope, from_module)
        }
        TypeRef::Nullable(inner) => {
            let inner = to_mono_user_generic_arg_impl(inner, ctx, scope, from_module, visited);
            quote! { Option<#inner> }
        }
        t if local_generic_args(t, ctx, scope).is_some() => {
            to_syn_type(t, TypePosition::RETURN, Some(ctx), scope, from_module)
        }
        // Lower the original type, not the peeled one, so aliases that are
        // emitted under their own name stay named.
        _ => to_syn_type(
            ty,
            TypePosition::RETURN.to_inner(),
            Some(ctx),
            scope,
            from_module,
        ),
    }
}

/// Whether an async per-monomorphization return should keep Rust-native
/// lowering instead of using the wrapper-only mapping required inside
/// built-in JS containers.
fn is_mono_native_return_shape(ty: &TypeRef, ctx: &CodegenContext<'_>, scope: ScopeId) -> bool {
    is_mono_native_return_shape_impl(ty, ctx, scope, &mut HashSet::new())
}

fn is_mono_native_return_shape_impl(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    visited: &mut HashSet<String>,
) -> bool {
    let peeled = peel_mono(ty, ctx, scope, visited);
    match peeled.as_ref() {
        t if is_mono_native_leaf(t, ctx, scope) => true,
        TypeRef::Nullable(inner) => is_mono_native_return_shape_impl(inner, ctx, scope, visited),
        t => local_generic_args(t, ctx, scope).is_some(),
    }
}

/// Whether a return has an eligible string leaf for a `_js_string` variant.
/// Traversal enters aliases, unions with a common string type, locally
/// declared generic types, and nullable wrappers, but intentionally stops at
/// built-in JS containers, callbacks, and tuples.
pub(crate) fn has_mono_js_string_return(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
) -> bool {
    // A top-level union that becomes a dynamic-union enum has no `String`
    // leaf to swap for `JsString`. Getters synthesise under `Nullable` too,
    // so both shapes are excluded.
    let top = match ty {
        TypeRef::Nullable(inner) => inner.as_ref(),
        other => other,
    };
    if let TypeRef::Union(members) = top {
        if dynamic_union_members(members, Some(ctx), scope).is_some() {
            return false;
        }
    }
    has_mono_js_string_return_impl(ty, ctx, scope, &mut HashSet::new())
}

fn has_mono_js_string_return_impl(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    visited: &mut HashSet<String>,
) -> bool {
    let peeled = peel_mono(ty, ctx, scope, visited);
    match peeled.as_ref() {
        TypeRef::String | TypeRef::StringLiteral(_) => true,
        TypeRef::Nullable(inner) => has_mono_js_string_return_impl(inner, ctx, scope, visited),
        t => local_generic_args(t, ctx, scope).is_some_and(|args| {
            args.iter()
                .any(|arg| has_mono_js_string_return_impl(arg, ctx, scope, &mut visited.clone()))
        }),
    }
}

/// Lower the original TypeScript return while replacing only the eligible
/// string leaves with `JsString`. This consumes the explicit signature marker
/// rather than smuggling a synthetic `TypeRef::Reference("JsString")` through
/// normal name resolution, where a user declaration could shadow it.
///
/// Also used directly for additive `_js_string` getter variants.
pub(crate) fn to_mono_js_string_return_type(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    to_mono_js_string_return_type_impl(ty, ctx, scope, from_module, &mut HashSet::new())
}

fn to_mono_js_string_return_type_impl(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    from_module: &ModuleContext,
    visited: &mut HashSet<String>,
) -> TokenStream {
    let peeled = peel_mono(ty, ctx, scope, visited);
    match peeled.as_ref() {
        TypeRef::String | TypeRef::StringLiteral(_) => quote! { JsString },
        TypeRef::Nullable(inner) => {
            let inner = to_mono_js_string_return_type_impl(inner, ctx, scope, from_module, visited);
            quote! { Option<#inner> }
        }
        t @ TypeRef::Reference { segments, .. } if local_generic_args(t, ctx, scope).is_some() => {
            let base = named_type_to_rust(&segments[0], Some(ctx), scope, from_module);
            let args = local_generic_args(t, ctx, scope)
                .unwrap_or_default()
                .iter()
                .map(|arg| {
                    let mut visited = visited.clone();
                    if has_mono_js_string_return_impl(arg, ctx, scope, &mut visited.clone()) {
                        to_mono_js_string_return_type_impl(
                            arg,
                            ctx,
                            scope,
                            from_module,
                            &mut visited,
                        )
                    } else {
                        to_mono_user_generic_arg_impl(arg, ctx, scope, from_module, &mut visited)
                    }
                })
                .collect::<Vec<_>>();
            quote! { #base<#(#args),*> }
        }
        _ => to_syn_type(ty, TypePosition::RETURN, Some(ctx), scope, from_module),
    }
}

/// The default error type used when a fallible binding has no
/// `@throws` annotation. Either `JsValue` (the wasm-bindgen default)
/// or `Error` (when [`GenerateOptions::errors_as_error`][1] is set).
///
/// `Error` is the `js_sys::Error` re-export — already in scope via
/// the codegen preamble's `use js_sys::*` glob, so no extra alias
/// is needed.
///
/// [1]: crate::codegen::GenerateOptions::errors_as_error
fn default_error_type(ctx: Option<&CodegenContext<'_>>) -> TokenStream {
    if ctx.is_some_and(|c| c.errors_as_error) {
        quote! { Error }
    } else {
        quote! { JsValue }
    }
}

/// Lower a getter's return type, going through dynamic-union
/// synthesis when the type is a top-level erasing union.
///
/// Getters never go through the async / catch / Result wrapping that
/// callable returns do, so this helper is a thin wrapper around
/// `maybe_synthesise_return_union` + `to_syn_type` rather than
/// piggy-backing on `to_return_type`.
pub fn to_getter_return_type(
    ty: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
    anchor: ReturnAnchor<'_>,
) -> TokenStream {
    if let Some(tokens) = maybe_synthesise_return_union(ty, ctx, scope, from_module, anchor) {
        return tokens;
    }
    // For optional getters the IR widens to `Nullable<T>`. If the
    // inner `T` is itself a top-level erasing union, we still want
    // the synthesised enum — but wrapped in `Option<…>`.
    if let TypeRef::Nullable(inner) = ty {
        if let Some(inner_tokens) =
            maybe_synthesise_return_union(inner, ctx, scope, from_module, anchor)
        {
            return quote! { Option<#inner_tokens> };
        }
    }
    to_syn_type(ty, TypePosition::RETURN, ctx, scope, from_module)
}

/// Anchor for naming a synthesised dynamic-union enum. Carries the
/// member/function name and its enclosing parent (used as a fallback
/// prefix on name collisions).
///
/// The anchor governs the **base** name only — the trailing `Kind`
/// suffix and any numeric dedup suffix come from the registry side
/// (see [`CodegenContext::synthesise_dynamic_union`]).
#[derive(Clone, Copy, Debug)]
pub struct ReturnAnchor<'a> {
    /// Plain anchor source — getter name (`content`), method name
    /// (`bar`), function name (`fetch`). Will be PascalCased before
    /// being used as the enum-name base.
    pub base: &'a str,
    /// Whether the anchor refers to a getter (where the base is used
    /// as-is) or a callable return (where `Return` is appended before
    /// `Kind`).
    pub kind: ReturnAnchorKind,
    /// Enclosing type / namespace name, used as the prefix on
    /// collision (`EmailAttachmentContentKind`).
    pub parent: Option<&'a str>,
}

/// Distinguishes anchors that already refer to a property (no extra
/// suffix) from those that refer to a callable's return position
/// (which gets a `Return` infix to disambiguate from the callable's
/// own name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnAnchorKind {
    Getter,
    Callable,
}

impl<'a> ReturnAnchor<'a> {
    /// PascalCased base for the enum name, with the `Return` infix
    /// appended for callables.
    fn enum_anchor(self) -> String {
        let pc = crate::util::naming::to_pascal_case(self.base);
        match self.kind {
            ReturnAnchorKind::Getter => pc,
            ReturnAnchorKind::Callable => format!("{pc}Return"),
        }
    }
}

/// If `ty` is a top-level union that would otherwise erase to
/// `JsValue`, synthesise (or reuse) a dynamic-union enum and return
/// the enum's name as a token stream.
///
/// Only fires for the *outer* return position. Inner unions (inside
/// `Promise<T>`, `Map<K, V>`, etc.) keep going through the regular
/// LUB → `JsValue` path because they live inside generic containers
/// where wasm-bindgen needs a `JsGeneric` type and a synthesised
/// enum doesn't qualify.
///
/// Unions with a useful named LUB (`TypeError | RangeError` →
/// `Error`) keep the narrower static type and do not synthesise.
/// Pure-boolean-literal unions (`true | false`) keep `bool` since
/// every member round-trips through it without loss.
fn maybe_synthesise_return_union(
    ty: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
    anchor: ReturnAnchor<'_>,
) -> Option<TokenStream> {
    let property_key_members;
    let members = match ty {
        TypeRef::Union(members) => members,
        TypeRef::PropertyKey => {
            property_key_members = TypeRef::property_key_members();
            &property_key_members
        }
        _ => return None,
    };
    let cgctx = ctx?;

    let members = dynamic_union_members(members, ctx, scope)?;
    let name = cgctx.synthesise_dynamic_union(
        &members,
        &anchor.enum_anchor(),
        anchor.parent,
        scope,
        from_module,
    );
    let ident = make_ident(&name);
    Some(quote! { #ident })
}

/// The getter's synthesised enum, as seen by the matching setter.
pub(crate) struct SetterEnum {
    pub(crate) name: String,
    /// Each literal member the enum keeps as its own variant, paired with
    /// that variant's name.
    pub(crate) variants: Vec<(TypeRef, String)>,
}

impl SetterEnum {
    pub(crate) fn variant(&self, literal: &TypeRef) -> Option<&str> {
        self.variants
            .iter()
            .find(|(l, _)| l == literal)
            .map(|(_, v)| v.as_str())
    }

    /// `ty` with the enum's literal members replaced by the enum, at the
    /// position of the first one.
    pub(crate) fn param_type(&self, ty: &TypeRef) -> TypeRef {
        match ty {
            TypeRef::Nullable(inner) => TypeRef::Nullable(Box::new(self.param_type(inner))),
            TypeRef::Union(members) => {
                let mut out: Vec<TypeRef> = Vec::with_capacity(members.len());
                let mut placed = false;
                for m in members {
                    if self.variant(m).is_none() {
                        out.push(m.clone());
                    } else if !placed {
                        out.push(TypeRef::ident(self.name.clone()));
                        placed = true;
                    }
                }
                if out.len() == 1 {
                    out.pop().unwrap()
                } else {
                    TypeRef::Union(out)
                }
            }
            other => other.clone(),
        }
    }
}

/// Setters route the literal members that the getter's enum keeps as their
/// own variants (string literals, or every member of a C-style integer
/// enum) through that enum, rather than a widened `&str` / `f64` that
/// accepts values outside the union. String literals covered by a `string`
/// member stay on the `&str` setter, which is then exact.
pub(crate) fn setter_enum(
    ty: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
    anchor: ReturnAnchor<'_>,
) -> Option<SetterEnum> {
    let cgctx = ctx?;
    let ty = match ty {
        TypeRef::Nullable(inner) => inner,
        other => other,
    };
    let TypeRef::Union(members) = ty else {
        return None;
    };
    let members = dynamic_union_members(members, ctx, scope)?;
    let integer_enum = is_integer_literal_union(&members);
    let has_string = members.contains(&TypeRef::String);
    let variant_names = crate::codegen::dynamic_union_variant_names(&members);
    let variants: Vec<(TypeRef, String)> = members
        .iter()
        .zip(variant_names)
        .filter(|(m, _)| match m {
            TypeRef::StringLiteral(_) => !has_string,
            TypeRef::NumberLiteral(_) => integer_enum,
            _ => false,
        })
        .map(|(m, v)| (m.clone(), v))
        .collect();
    if variants.is_empty() {
        return None;
    }
    let name = cgctx.synthesise_dynamic_union(
        &members,
        &anchor.enum_anchor(),
        anchor.parent,
        scope,
        from_module,
    );
    Some(SetterEnum { name, variants })
}

/// The members a return-position union synthesises its `<Anchor>Kind`
/// enum from, or `None` when it keeps its regular lowering.
///
/// wasm-bindgen discriminants are either all strings (optionally next to
/// tuple variants) or all 32-bit integers (unit variants only). So number
/// literals stay discriminants only when every member is such an integer
/// literal; otherwise they widen to `number`, and boolean literals widen to
/// `boolean`. After widening:
///
/// 1. **Fewer than two members** (`true | false`, `number | 1`, `0.5 | 1.5`)
///    keep the plain primitive.
/// 2. **Any string literal** ⇒ synthesise a string-discriminant enum, or a
///    dynamic union when non-literal members remain.
/// 3. **Otherwise**, fall back to the named LUB lattice — synthesise only
///    when there's no useful narrowing.
fn dynamic_union_members(
    members: &[TypeRef],
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> Option<Vec<TypeRef>> {
    if is_integer_literal_union(members) {
        return Some(members.to_vec());
    }
    let mut widened: Vec<TypeRef> = Vec::with_capacity(members.len());
    for m in members {
        let primitive = match m {
            TypeRef::NumberLiteral(_) => TypeRef::Number,
            TypeRef::BooleanLiteral(_) => TypeRef::Boolean,
            other => {
                widened.push(other.clone());
                continue;
            }
        };
        if !widened.contains(&primitive) && !members.contains(&primitive) {
            widened.push(primitive);
        }
    }
    if widened.len() < 2 {
        return None;
    }
    if widened
        .iter()
        .any(|m| matches!(m, TypeRef::StringLiteral(_)))
    {
        return Some(widened);
    }
    crate::codegen::subtyping::lub_union(&widened, ctx, scope)
        .is_none()
        .then_some(widened)
}

/// Whether every member is a distinct-valued integer literal that fits the
/// `u32` (or, with any negative, `i32`) repr of a wasm-bindgen C-style enum.
fn is_integer_literal_union(members: &[TypeRef]) -> bool {
    let signed = members
        .iter()
        .any(|m| matches!(m, TypeRef::NumberLiteral(n) if *n < 0.0));
    let range = if signed {
        i32::MIN as f64..=i32::MAX as f64
    } else {
        0.0..=u32::MAX as f64
    };
    let mut seen = HashSet::new();
    !members.is_empty()
        && members.iter().all(|m| {
            matches!(m, TypeRef::NumberLiteral(n)
                if n.fract() == 0.0 && range.contains(n) && seen.insert(*n as i64))
        })
}

/// Whether a function with these parameters should be tagged
/// `#[wasm_bindgen(slice_to_array)]`, so its `&[T]` arguments reach JS as
/// a plain `Array`.
///
/// Only a slice of Rust values (`&[String]`, `&[f64]`) needs it: without
/// the attribute `&[String]` has no ABI and `&[f64]` arrives as a
/// `Float64Array` view of linear memory, which a `number[]` callee may
/// keep past the call. A slice of JS handles already arrives as a fresh
/// `Array`, and the attribute cannot apply to one whose element is a type
/// parameter. The attribute is per function, so one qualifying parameter
/// applies it to every slice parameter.
pub fn needs_slice_to_array(
    params: &[crate::codegen::signatures::ConcreteParam],
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> bool {
    params.iter().any(|p| {
        // A variadic rest lowers to `&[JsValue]` whatever its element.
        if p.variadic {
            return false;
        }
        let ty = match &p.type_ref {
            TypeRef::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        matches!(ty, TypeRef::Array(elem) if is_native_array_element(elem, ctx, scope))
    })
}

/// Whether a top-level `T[]` element keeps its Rust-native form
/// (`String` / `f64`) inside the slice / `Vec`, rather than its handle
/// form. Enums count as the primitive they carry.
fn is_native_array_element(
    elem: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> bool {
    let resolved = match ctx {
        Some(c) if c.enum_repr(elem, scope).is_some() => return true,
        Some(c) => peel_mono(elem, c, scope, &mut HashSet::new()),
        None => std::borrow::Cow::Borrowed(elem),
    };
    matches!(
        resolved.as_ref(),
        TypeRef::String | TypeRef::StringLiteral(_) | TypeRef::Number | TypeRef::NumberLiteral(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::scope::ScopeId;

    #[test]
    fn setter_enum_replaces_its_literals_at_the_first_position() {
        let e = SetterEnum {
            name: "ModeKind".to_string(),
            variants: vec![
                (TypeRef::StringLiteral("x".into()), "X".to_string()),
                (TypeRef::StringLiteral("y".into()), "Y".to_string()),
            ],
        };
        let union = |members: Vec<TypeRef>| TypeRef::Union(members);
        let x = TypeRef::StringLiteral("x".into());
        let y = TypeRef::StringLiteral("y".into());
        assert_eq!(
            e.param_type(&union(vec![
                TypeRef::BooleanLiteral(true),
                x.clone(),
                TypeRef::NumberLiteral(1.0),
                y.clone(),
            ])),
            union(vec![
                TypeRef::BooleanLiteral(true),
                TypeRef::ident("ModeKind"),
                TypeRef::NumberLiteral(1.0),
            ])
        );
        assert_eq!(
            e.param_type(&TypeRef::Nullable(Box::new(union(vec![x.clone(), y])))),
            TypeRef::Nullable(Box::new(TypeRef::ident("ModeKind")))
        );
        assert_eq!(e.variant(&x), Some("X"));
        assert_eq!(e.variant(&TypeRef::String), None);
    }

    // Helper to run to_syn_type with ARGUMENT position
    fn arg_type(ty: &TypeRef) -> String {
        // Scope is unused when ctx is None — use a dummy value.
        to_syn_type(
            ty,
            TypePosition::ARGUMENT,
            None,
            ScopeId(0),
            &ModuleContext::Global,
        )
        .to_string()
    }

    fn ret_type(ty: &TypeRef) -> String {
        to_syn_type(
            ty,
            TypePosition::RETURN,
            None,
            ScopeId(0),
            &ModuleContext::Global,
        )
        .to_string()
    }

    #[test]
    fn needs_js_name_compares_sanitised_ident() {
        let needs = |rust: &str, js: &str| needs_js_name(&make_ident(rust), js);
        assert!(!needs("foo", "foo"));
        assert!(needs("foo_bar", "fooBar"));
        assert!(needs("0", "0"));
        assert!(needs("$foo", "$foo"));
        assert!(needs("@cf/x", "@cf/x"));
        assert!(needs("$", "$"));
        assert!(!needs("set_0", "set_0"));
        assert!(needs("set_$foo", "set_$foo"));
        // Raw idents are unrawed by wasm-bindgen, so `r#type` already binds `type`.
        assert!(!needs_js_name(
            &syn::Ident::new_raw("type", proc_macro2::Span::call_site()),
            "type"
        ));
    }

    fn inner_type(ty: &TypeRef) -> String {
        to_syn_type(
            ty,
            TypePosition::RETURN.to_inner(),
            None,
            ScopeId(0),
            &ModuleContext::Global,
        )
        .to_string()
    }

    #[test]
    fn test_string_positions() {
        assert_eq!(arg_type(&TypeRef::String), "& str");
        assert_eq!(ret_type(&TypeRef::String), "String");
    }

    #[test]
    fn test_string_inner_position() {
        // Inner position: string → JsString
        assert_eq!(inner_type(&TypeRef::String), "JsString");
    }

    #[test]
    fn test_number_inner_position() {
        // Inner position: number → Number
        assert_eq!(inner_type(&TypeRef::Number), "Number");
    }

    #[test]
    fn test_boolean_inner_position() {
        // Inner position: boolean → Boolean
        assert_eq!(inner_type(&TypeRef::Boolean), "Boolean");
    }

    #[test]
    fn test_void_inner_position() {
        // Inner position: void → Undefined
        assert_eq!(inner_type(&TypeRef::Void), "Undefined");
    }

    #[test]
    fn test_nullable() {
        let ty = TypeRef::Nullable(Box::new(TypeRef::String));
        // Option<T> passes through position — Option<String> at return position
        let result = ret_type(&ty);
        assert_eq!(result, "Option < String >");
    }

    #[test]
    fn test_promise_with_named_type_unresolved() {
        // Without ctx, Foo is unresolved → JsValue, so Promise<JsValue> elides to Promise
        let ty = TypeRef::generic("Promise", vec![TypeRef::ident("Foo")]);
        assert_eq!(ret_type(&ty), "Promise");
    }

    #[test]
    fn test_nullable_inner() {
        // Nullable inside generic (inner position) → JsOption
        let ty = TypeRef::Nullable(Box::new(TypeRef::String));
        let result = inner_type(&ty);
        assert_eq!(result, "JsOption < JsString >");
    }

    #[test]
    fn test_nullable_inner_jsvalue_collapses() {
        let ty = TypeRef::Nullable(Box::new(TypeRef::Any));
        assert_eq!(inner_type(&ty), "JsValue");
    }

    #[test]
    fn test_promise_with_string() {
        let ty = TypeRef::generic("Promise", vec![TypeRef::String]);
        let result = ret_type(&ty);
        assert_eq!(result, "Promise < JsString >");
    }

    #[test]
    fn test_promise_with_any_elides_generic() {
        let ty = TypeRef::generic("Promise", vec![TypeRef::Any]);
        let result = ret_type(&ty);
        assert_eq!(result, "Promise");
    }

    #[test]
    fn test_promise_with_void() {
        let ty = TypeRef::generic("Promise", vec![TypeRef::Void]);
        let result = ret_type(&ty);
        assert_eq!(result, "Promise < Undefined >");
    }

    #[test]
    fn test_nullable_named_type_unresolved() {
        // Without ctx, `Foo` is unresolved → `JsValue`. In *return*
        // position the `Option<JsValue>` shape is redundant (JsValue
        // already carries `null`/`undefined` in-band) and collapses
        // to bare `JsValue`. In *argument* position we preserve
        // `Option<&JsValue>` so callers can distinguish "no value"
        // from passing `JsValue::NULL`.
        let ty = TypeRef::Nullable(Box::new(TypeRef::ident("Foo")));
        assert_eq!(arg_type(&ty), "Option < & JsValue >");
        assert_eq!(ret_type(&ty), "JsValue");
    }

    #[test]
    fn test_promise_with_arraybuffer() {
        let ty = TypeRef::generic("Promise", vec![TypeRef::ident("ArrayBuffer")]);
        let result = ret_type(&ty);
        assert_eq!(result, "Promise < ArrayBuffer >");
    }

    #[test]
    fn test_array_with_type() {
        // Top-level `Array<T>` lowers to `Vec<T>` in return position
        // and `&[T]` in argument position. The element uses
        // return-position lowering so primitives stay bare and named
        // types are unborrowed — see the `TypeRef::Array` arm in
        // `to_syn_type`.
        let ty = TypeRef::Array(Box::new(TypeRef::Number));
        assert_eq!(ret_type(&ty), "Vec < f64 >");
        assert_eq!(arg_type(&ty), "& [f64]");
    }

    #[test]
    fn test_array_with_any() {
        // `Array<any>` no longer elides — the slice / `Vec<T>` form
        // always carries the element type.
        let ty = TypeRef::Array(Box::new(TypeRef::Any));
        assert_eq!(ret_type(&ty), "Vec < JsValue >");
        assert_eq!(arg_type(&ty), "& [JsValue]");
    }

    #[test]
    fn test_array_with_string() {
        // Strings are owned inside the slice / Vec — `&str` would
        // need a borrow lifetime, and `str` itself is unsized.
        let ty = TypeRef::Array(Box::new(TypeRef::String));
        assert_eq!(ret_type(&ty), "Vec < String >");
        assert_eq!(arg_type(&ty), "& [String]");
    }

    #[test]
    fn test_array_inside_generic_keeps_array() {
        // Nested inside a generic (`pos.inner == true`) we keep the
        // legacy `Array<T'>` form because callers like
        // `Promise<Array<T>>` need the in-band JS-wrapper element.
        let ty = TypeRef::Array(Box::new(TypeRef::Number));
        assert_eq!(inner_type(&ty), "Array < Number >");
    }

    fn cp(name: &str, ty: TypeRef) -> crate::codegen::signatures::ConcreteParam {
        crate::codegen::signatures::ConcreteParam {
            name: name.to_string(),
            type_ref: ty,
            variadic: false,
        }
    }

    #[test]
    fn slice_to_array_follows_the_lowered_element() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let needs = |elem: &TypeRef| {
                needs_slice_to_array(
                    &[cp("xs", TypeRef::Array(Box::new(elem.clone())))],
                    Some(ctx),
                    scope,
                )
            };
            let numbers = TypeRef::Union(vec![
                TypeRef::NumberLiteral(1.0),
                TypeRef::NumberLiteral(2.0),
            ]);
            for elem in [
                TypeRef::String,
                TypeRef::Number,
                TypeRef::ident("IdChain"),
                TypeRef::ident("Mode"),
                TypeRef::ident("Level"),
                lits(&["a", "b"]),
                numbers,
            ] {
                assert!(needs(&elem), "expected slice_to_array for {elem:?}");
            }
            for elem in [
                TypeRef::Boolean,
                TypeRef::BigInt,
                TypeRef::Any,
                TypeRef::ident("Plain"),
                TypeRef::ident("T"),
                TypeRef::ident("MaybeId"),
                TypeRef::Array(Box::new(TypeRef::String)),
            ] {
                assert!(!needs(&elem), "unexpected slice_to_array for {elem:?}");
            }
        });
    }

    #[test]
    fn slice_to_array_param_shapes() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let needs = |params: &[crate::codegen::signatures::ConcreteParam]| {
                needs_slice_to_array(params, Some(ctx), scope)
            };
            let strings = || TypeRef::Array(Box::new(TypeRef::String));
            assert!(needs(&[cp("xs", TypeRef::Nullable(Box::new(strings())))]));
            assert!(needs(&[
                cp("first", TypeRef::ident("Plain")),
                cp("xs", strings())
            ]));
            assert!(!needs(&[
                cp("x", TypeRef::String),
                cp("y", TypeRef::Number)
            ]));
            assert!(!needs(&[cp(
                "xs",
                TypeRef::generic("Array", vec![TypeRef::String])
            )]));
            let mut rest = cp("rest", strings());
            rest.variadic = true;
            assert!(!needs(&[rest]));
        });
    }

    #[test]
    fn test_set_with_type() {
        let ty = TypeRef::generic("Set", vec![TypeRef::String]);
        let result = ret_type(&ty);
        assert_eq!(result, "Set < JsString >");
    }

    #[test]
    fn test_map_with_types() {
        let ty = TypeRef::generic("Map", vec![TypeRef::String, TypeRef::Number]);
        let result = ret_type(&ty);
        assert_eq!(result, "Map < JsString , Number >");
    }

    #[test]
    fn test_record_erases_key() {
        let ty = TypeRef::generic("Record", vec![TypeRef::String, TypeRef::Number]);
        let result = ret_type(&ty);
        assert_eq!(result, "Object < Number >");
    }

    #[test]
    fn test_record_nullable_jsvalue_elides_generic() {
        let ty = TypeRef::generic(
            "Record",
            vec![
                TypeRef::String,
                TypeRef::Nullable(Box::new(TypeRef::Union(vec![
                    TypeRef::String,
                    TypeRef::Number,
                    TypeRef::Boolean,
                ]))),
            ],
        );
        assert_eq!(ret_type(&ty), "Object");
    }

    #[test]
    fn test_promise_nullable_inner() {
        // Promise<string | null> → Promise<JsOption<JsString>>
        let ty = TypeRef::generic(
            "Promise",
            vec![TypeRef::Nullable(Box::new(TypeRef::String))],
        );
        let result = ret_type(&ty);
        assert_eq!(result, "Promise < JsOption < JsString > >");
    }

    #[test]
    fn test_promise_nullable_jsvalue_elides_generic() {
        let ty = TypeRef::generic("Promise", vec![TypeRef::Nullable(Box::new(TypeRef::Any))]);
        assert_eq!(ret_type(&ty), "Promise");
    }

    #[test]
    fn test_function_typed() {
        let sig = ir::FunctionSig {
            params: vec![ir::Param {
                name: "x".into(),
                type_ref: TypeRef::Number,
                optional: false,
                variadic: false,
            }],
            return_type: Box::new(TypeRef::Boolean),
        };
        let ty = TypeRef::Function(sig);
        let result = ret_type(&ty);
        assert_eq!(result, "Function < fn (Number) -> Boolean >");
    }

    #[test]
    fn test_function_untyped() {
        let sig = ir::FunctionSig {
            params: vec![ir::Param {
                name: "x".into(),
                type_ref: TypeRef::Any,
                optional: false,
                variadic: false,
            }],
            return_type: Box::new(TypeRef::Any),
        };
        let ty = TypeRef::Function(sig);
        let result = ret_type(&ty);
        assert_eq!(result, "Function");
    }

    #[test]
    fn test_named_unresolved_without_ctx() {
        // Without a CodegenContext, unknown types fall back to JsValue
        let ty = TypeRef::ident("Request");
        assert_eq!(ret_type(&ty), "JsValue");
    }

    #[test]
    fn test_named_unknown_without_ctx() {
        let ty = TypeRef::ident("MyCustomType");
        assert_eq!(ret_type(&ty), "JsValue");
    }

    #[test]
    fn test_return_with_catch() {
        let ty = TypeRef::generic("Promise", vec![TypeRef::Void]);
        let result = to_return_type(
            &ty,
            true,
            false,
            false,
            None,
            None,
            ScopeId(0),
            &ModuleContext::Global,
            ReturnAnchor {
                base: "test",
                kind: ReturnAnchorKind::Callable,
                parent: None,
            },
        )
        .to_string();
        assert_eq!(result, "Result < Promise < Undefined > , JsValue >");
    }

    #[test]
    fn test_union_erases() {
        let ty = TypeRef::Union(vec![TypeRef::String, TypeRef::Number]);
        // Unions erase to JsValue, but in argument position they're borrowed
        assert_eq!(arg_type(&ty), "& JsValue");
        assert_eq!(ret_type(&ty), "JsValue");
    }

    fn test_gctx() -> (GlobalContext, ScopeId) {
        let mut gctx = GlobalContext::new();
        let scope = gctx.create_root_scope();
        (gctx, scope)
    }

    #[test]
    fn test_local_type_overrides_web_sys() {
        let (gctx, scope) = test_gctx();
        let mut ctx = CodegenContext::empty(&gctx, scope);
        ctx.local_types
            .insert("Response".into(), ModuleContext::Global);
        let ty = TypeRef::ident("Response");
        let result = to_syn_type(
            &ty,
            TypePosition::RETURN,
            Some(&ctx),
            scope,
            &ModuleContext::Global,
        )
        .to_string();
        assert_eq!(result, "Response");
    }

    #[test]
    fn test_union_alias_resolves_to_jsvalue() {
        // A type alias to a union resolves through the scope and erases to JsValue.
        let (mut gctx, scope) = test_gctx();
        let alias_id = gctx.insert_type(crate::ir::TypeDeclaration {
            kind: crate::ir::TypeKind::TypeAlias(crate::ir::TypeAliasDecl {
                name: "BodyInit".to_string(),
                type_params: vec![],
                target: TypeRef::Union(vec![TypeRef::String, TypeRef::ident("ArrayBuffer")]),
                from_module: None,
                body_scope: scope,
            }),
            module_context: crate::ir::ModuleContext::Global,
            doc: None,
            scope_id: scope,
            exported: false,
        });
        gctx.scopes.insert(scope, "BodyInit".to_string(), alias_id);

        let ctx = CodegenContext::empty(&gctx, scope);
        let ty = TypeRef::ident("BodyInit");
        let result = to_syn_type(
            &ty,
            TypePosition::RETURN,
            Some(&ctx),
            scope,
            &ModuleContext::Global,
        )
        .to_string();
        assert_eq!(result, "JsValue");
    }

    #[test]
    fn test_web_sys_default_resolves_response() {
        // `Response` is in the web platform default mapping, so even
        // without an explicit `--external` flag it resolves to
        // `::web_sys::Response` (rather than erasing to JsValue).
        let (gctx, scope) = test_gctx();
        let ctx = CodegenContext::empty(&gctx, scope);
        let ty = TypeRef::ident("Response");
        let result = to_syn_type(
            &ty,
            TypePosition::RETURN,
            Some(&ctx),
            scope,
            &ModuleContext::Global,
        )
        .to_string();
        assert_eq!(result, "Response");
        let uses = ctx.external_uses.borrow();
        assert_eq!(
            uses.get("Response"),
            Some(&"::web_sys::Response".to_string())
        );
    }

    #[test]
    fn test_truly_unknown_type_aliases_to_jsvalue() {
        // A name with no scope hit, no external mapping, and not in
        // `js_sys::*` or the web_sys defaults falls back to a
        // `use JsValue as Foo;` alias and an error diagnostic.
        let (gctx, scope) = test_gctx();
        let ctx = CodegenContext::empty(&gctx, scope);
        let ty = TypeRef::ident("MyCustomThing");
        let result = to_syn_type(
            &ty,
            TypePosition::RETURN,
            Some(&ctx),
            scope,
            &ModuleContext::Global,
        )
        .to_string();
        assert_eq!(result, "MyCustomThing");
        let uses = ctx.external_uses.borrow();
        assert_eq!(uses.get("MyCustomThing"), Some(&"JsValue".to_string()));
    }

    #[test]
    fn test_local_type_in_promise() {
        let (gctx, scope) = test_gctx();
        let mut ctx = CodegenContext::empty(&gctx, scope);
        ctx.local_types
            .insert("MyThing".into(), ModuleContext::Global);
        let ty = TypeRef::generic("Promise", vec![TypeRef::ident("MyThing")]);
        let result = to_syn_type(
            &ty,
            TypePosition::RETURN,
            Some(&ctx),
            scope,
            &ModuleContext::Global,
        )
        .to_string();
        assert_eq!(result, "Promise < MyThing >");
    }

    #[test]
    fn test_declared_module_is_qualified_from_global() {
        // A declared module is emitted under its specifier's `pub mod`
        // path, so a global reference to one of its types is qualified.
        let (gctx, scope) = test_gctx();
        let mut ctx = CodegenContext::empty(&gctx, scope);
        ctx.exported_modules.insert("cloudflare:email".to_string());
        let module = ModuleContext::Module("cloudflare:email".into());
        ctx.local_types
            .insert("EmailMessage".into(), module.clone());

        let lower = |ctx: &CodegenContext| {
            to_syn_type(
                &TypeRef::ident("EmailMessage"),
                TypePosition::ARGUMENT,
                Some(ctx),
                scope,
                &ModuleContext::Global,
            )
            .to_string()
        };
        assert_eq!(lower(&ctx), "& cloudflare :: email :: EmailMessage");

        // The `--lib-name` module is the output's own, at the top level.
        ctx.lib_name = Some("cloudflare:email".to_string());
        assert_eq!(lower(&ctx), "& EmailMessage");
    }

    #[test]
    fn test_global_to_global_is_bare() {
        // Trivial: same-scope references are bare.
        let (gctx, scope) = test_gctx();
        let mut ctx = CodegenContext::empty(&gctx, scope);
        ctx.local_types
            .insert("EmailSendResult".into(), ModuleContext::Global);

        let ty = TypeRef::ident("EmailSendResult");
        let result = to_syn_type(
            &ty,
            TypePosition::RETURN,
            Some(&ctx),
            scope,
            &ModuleContext::Global,
        )
        .to_string();
        assert_eq!(result, "EmailSendResult");
    }

    // === New tests for the unified approach ===

    #[test]
    fn test_to_inner_preserves_direction() {
        let pos = TypePosition::ARGUMENT.to_inner();
        assert!(pos.is_argument());
        assert!(pos.inner);

        let pos = TypePosition::RETURN.to_inner();
        assert!(!pos.is_argument());
        assert!(pos.inner);
    }

    #[test]
    fn test_inner_position_named_type_unresolved() {
        // Without ctx, unresolved named types → JsValue
        let ty = TypeRef::ident("Response");
        assert_eq!(inner_type(&ty), "JsValue");
        assert_eq!(ret_type(&ty), "JsValue");
    }

    #[test]
    fn test_inner_position_typed_array_unchanged() {
        // Typed arrays pass through in inner position
        let ty = TypeRef::ident("Uint8Array");
        assert_eq!(inner_type(&ty), "Uint8Array");
        assert_eq!(ret_type(&ty), "Uint8Array");
    }

    #[test]
    fn test_tuple_generates_array_tuple() {
        // Without ctx, named types are unresolved → JsValue, so Array<JsValue> elides to Array
        let ty = TypeRef::Tuple(vec![
            TypeRef::Array(Box::new(TypeRef::ident("ImportSpecifier"))),
            TypeRef::Array(Box::new(TypeRef::ident("ExportSpecifier"))),
            TypeRef::Boolean,
            TypeRef::Boolean,
        ]);
        let result = ret_type(&ty);
        assert_eq!(result, "ArrayTuple < (Array , Array , Boolean , Boolean) >");
    }

    #[test]
    fn test_empty_tuple_is_bare_array() {
        let ty = TypeRef::Tuple(vec![]);
        assert_eq!(ret_type(&ty), "Array");
    }

    #[test]
    fn test_type_position_all_variants() {
        // Verify TypePosition constants and to_inner() work correctly
        let ty = TypeRef::String;
        let g = &ModuleContext::Global;
        assert_eq!(
            to_syn_type(&ty, TypePosition::ARGUMENT, None, ScopeId(0), g).to_string(),
            "& str"
        );
        assert_eq!(
            to_syn_type(&ty, TypePosition::RETURN, None, ScopeId(0), g).to_string(),
            "String"
        );
        // to_inner() → inner:true, so should give JsString
        assert_eq!(
            to_syn_type(&ty, TypePosition::RETURN.to_inner(), None, ScopeId(0), g).to_string(),
            "JsString"
        );
        // Argument inner also gives JsString (inner overrides borrowing)
        assert_eq!(
            to_syn_type(&ty, TypePosition::ARGUMENT.to_inner(), None, ScopeId(0), g).to_string(),
            "JsString"
        );
    }

    /// Parse `src` and run `f` against a per-monomorphization context scoped
    /// to the source file, so aliases and local generics resolve.
    fn with_mono_ctx(src: &str, f: impl FnOnce(&CodegenContext<'_>, ScopeId)) {
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let ctx = CodegenContext::from_module_full(&module, &gctx, false, true, HashSet::new());
        let scope = *module.file_scopes.first().expect("file scope");
        f(&ctx, scope);
    }

    const MONO_SRC: &str = r#"
        declare class Holder<T> { constructor(value: T); }
        declare class Pair<A, B> { constructor(a: A, b: B); }
        declare class Plain {}
        type Id = string;
        type IdChain = Id;
        type MaybeId = Id | null;
        type Loop = Loop2;
        type Loop2 = Loop;
        type Mode = "fast" | "slow";
        type ModeAlias = Mode;
        declare enum Level { Low = 1 }
    "#;

    fn holder(arg: TypeRef) -> TypeRef {
        TypeRef::generic("Holder", vec![arg])
    }

    fn lits(values: &[&str]) -> TypeRef {
        TypeRef::Union(
            values
                .iter()
                .map(|v| TypeRef::StringLiteral((*v).into()))
                .collect(),
        )
    }

    fn user_arg(ty: &TypeRef, ctx: &CodegenContext<'_>, scope: ScopeId) -> String {
        to_mono_user_generic_arg(ty, Some(ctx), scope, &ModuleContext::Global).to_string()
    }

    fn js_string_ret(ty: &TypeRef, ctx: &CodegenContext<'_>, scope: ScopeId) -> String {
        to_mono_js_string_return_type(ty, ctx, scope, &ModuleContext::Global).to_string()
    }

    #[test]
    fn mono_context_helpers() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            assert!(CodegenContext::generic_mono(Some(ctx)));
            assert!(!CodegenContext::generic_mono(None));
            assert!(ctx.is_local_generic("Holder", scope));
            assert!(!ctx.is_local_generic("Plain", scope));
            assert!(!ctx.is_local_generic("Array", scope));
            assert_eq!(
                CodegenContext::type_param_decl(Some(ctx), "T").to_string(),
                "T"
            );
            assert_eq!(
                CodegenContext::type_param_decl(None, "T").to_string(),
                "T : :: wasm_bindgen :: JsGeneric"
            );
        });
    }

    #[test]
    fn peel_mono_resolves_aliases_and_reducible_unions() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let peel = |ty: &TypeRef| peel_mono(ty, ctx, scope, &mut HashSet::new()).into_owned();
            assert_eq!(peel(&TypeRef::ident("IdChain")), TypeRef::String);
            assert_eq!(peel(&lits(&["a", "b"])), TypeRef::String);
            assert!(matches!(
                peel(&TypeRef::ident("MaybeId")),
                TypeRef::Nullable(_)
            ));
            // Stops at non-aliases and generic instantiations.
            assert_eq!(peel(&TypeRef::ident("Plain")), TypeRef::ident("Plain"));
            assert_eq!(
                peel(&holder(TypeRef::ident("Id"))),
                holder(TypeRef::ident("Id"))
            );
        });
    }

    #[test]
    fn peel_mono_terminates_on_alias_cycles() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let ty = TypeRef::ident("Loop");
            let peeled = peel_mono(&ty, ctx, scope, &mut HashSet::new());
            assert!(matches!(peeled.as_ref(), TypeRef::Reference { .. }));
        });
    }

    #[test]
    fn array_elements_are_native_only_for_strings_and_numbers() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let ret = |elem: TypeRef| {
                to_syn_type(
                    &TypeRef::Array(Box::new(elem)),
                    TypePosition::RETURN,
                    Some(ctx),
                    scope,
                    &ModuleContext::Global,
                )
                .to_string()
            };
            assert_eq!(ret(TypeRef::ident("IdChain")), "Vec < String >");
            assert_eq!(ret(lits(&["a", "b"])), "Vec < String >");
            assert_eq!(ret(TypeRef::Number), "Vec < f64 >");
            assert_eq!(ret(TypeRef::Boolean), "Vec < Boolean >");
            assert_eq!(
                ret(TypeRef::ident("MaybeId")),
                "Vec < JsOption < JsString > >"
            );
            assert_eq!(
                ret(TypeRef::Array(Box::new(TypeRef::String))),
                "Vec < Array < JsString > >"
            );
        });
    }

    #[test]
    fn mono_user_generic_args_use_native_leaves_consistently() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            assert_eq!(user_arg(&TypeRef::String, ctx, scope), "String");
            assert_eq!(user_arg(&TypeRef::Number, ctx, scope), "f64");
            assert_eq!(user_arg(&TypeRef::Boolean, ctx, scope), "bool");
            assert_eq!(user_arg(&TypeRef::ident("IdChain"), ctx, scope), "String");
            assert_eq!(user_arg(&lits(&["a", "b"]), ctx, scope), "String");
            assert_eq!(
                user_arg(&TypeRef::ident("MaybeId"), ctx, scope),
                "Option < String >"
            );
            // Built-in containers keep wrapper elements.
            assert_eq!(
                user_arg(
                    &TypeRef::generic("Array", vec![TypeRef::String]),
                    ctx,
                    scope
                ),
                "Array < JsString >"
            );
            assert_eq!(
                user_arg(&holder(TypeRef::ident("Id")), ctx, scope),
                "Holder < String >"
            );
            // Enums are native leaves too, so they stay named rather than
            // taking the wrapper of their primitive.
            assert_eq!(user_arg(&TypeRef::ident("ModeAlias"), ctx, scope), "Mode");
            assert_eq!(user_arg(&TypeRef::ident("Level"), ctx, scope), "Level");
            assert_eq!(
                user_arg(&holder(TypeRef::ident("Mode")), ctx, scope),
                "Holder < Mode >"
            );
            assert_eq!(
                user_arg(
                    &TypeRef::Nullable(Box::new(TypeRef::ident("Level"))),
                    ctx,
                    scope
                ),
                "Option < Level >"
            );
            assert_eq!(
                user_arg(
                    &TypeRef::generic("Array", vec![TypeRef::ident("Mode")]),
                    ctx,
                    scope
                ),
                "Array < JsString >"
            );
        });
    }

    #[test]
    fn mono_native_return_shape() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let native = |ty: &TypeRef| is_mono_native_return_shape(ty, ctx, scope);
            assert!(native(&TypeRef::String));
            assert!(native(&TypeRef::ident("MaybeId")));
            assert!(native(&holder(TypeRef::Number)));
            assert!(native(&TypeRef::ident("ModeAlias")));
            assert!(native(&TypeRef::ident("Level")));
            assert!(!native(&TypeRef::generic("Array", vec![TypeRef::String])));
            assert!(!native(&TypeRef::ident("Plain")));
            assert!(!native(&TypeRef::ident("Loop")));
        });
    }

    #[test]
    fn mono_js_string_eligibility() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let eligible = |ty: &TypeRef| has_mono_js_string_return(ty, ctx, scope);
            assert!(eligible(&TypeRef::String));
            assert!(eligible(&TypeRef::ident("IdChain")));
            assert!(eligible(&TypeRef::ident("MaybeId")));
            assert!(eligible(&holder(lits(&["a", "b"]))));
            assert!(eligible(&TypeRef::generic(
                "Pair",
                vec![TypeRef::Number, TypeRef::ident("Id")]
            )));
            assert!(!eligible(&TypeRef::ident("Mode")));
            assert!(!eligible(&holder(TypeRef::ident("Mode"))));
            // Top-level literal unions become enums, not `String`.
            assert!(!eligible(&lits(&["a", "b"])));
            assert!(!eligible(&TypeRef::Nullable(Box::new(lits(&["a", "b"])))));
            // Built-in containers and non-string leaves are excluded.
            assert!(!eligible(&TypeRef::generic("Array", vec![TypeRef::String])));
            assert!(!eligible(&holder(TypeRef::Number)));
            assert!(!eligible(&TypeRef::ident("Loop")));
        });
    }

    #[test]
    fn mono_js_string_return_replaces_only_string_leaves() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            assert_eq!(
                js_string_ret(&TypeRef::ident("IdChain"), ctx, scope),
                "JsString"
            );
            assert_eq!(
                js_string_ret(&TypeRef::ident("MaybeId"), ctx, scope),
                "Option < JsString >"
            );
            assert_eq!(
                js_string_ret(&holder(lits(&["a", "b"])), ctx, scope),
                "Holder < JsString >"
            );
            // Each argument resolves `Id` independently.
            assert_eq!(
                js_string_ret(
                    &TypeRef::generic("Pair", vec![TypeRef::ident("Id"), TypeRef::ident("Id")]),
                    ctx,
                    scope
                ),
                "Pair < JsString , JsString >"
            );
            assert_eq!(
                js_string_ret(
                    &TypeRef::generic("Pair", vec![TypeRef::Number, TypeRef::String]),
                    ctx,
                    scope
                ),
                "Pair < f64 , JsString >"
            );
        });
    }

    #[test]
    fn extern_attr_adds_mono_opt_in() {
        with_mono_ctx(MONO_SRC, |ctx, _| {
            assert_eq!(
                CodegenContext::extern_attr(Some(ctx), Some("m")).to_string(),
                "# [wasm_bindgen (module = \"m\" , experimental_generic_mono)]"
            );
            assert_eq!(
                CodegenContext::extern_attr(Some(ctx), None).to_string(),
                "# [wasm_bindgen (experimental_generic_mono)]"
            );
        });
        assert_eq!(
            CodegenContext::extern_attr(None, Some("m")).to_string(),
            "# [wasm_bindgen (module = \"m\")]"
        );
        assert_eq!(
            CodegenContext::extern_attr(None, None).to_string(),
            "# [wasm_bindgen]"
        );
    }

    #[test]
    fn dynamic_union_members_literal_kinds() {
        use TypeRef::{
            Boolean, BooleanLiteral as B, Number, NumberLiteral as N, StringLiteral as S,
        };
        let members = |ms: &[TypeRef]| dynamic_union_members(ms, None, ScopeId(0));
        let ab = || TypeRef::ident("ArrayBuffer");

        // 32-bit integer literals stay discriminants of a C-style enum.
        assert_eq!(
            members(&[N(0.0), N(1.0), N(2.0)]),
            Some(vec![N(0.0), N(1.0), N(2.0)])
        );
        assert_eq!(members(&[N(-1.0), N(1.0)]), Some(vec![N(-1.0), N(1.0)]));
        assert_eq!(
            members(&[N(0.0), N(u32::MAX as f64)]),
            Some(vec![N(0.0), N(u32::MAX as f64)])
        );

        // Otherwise they widen to `number`, collapsing to a plain `f64`.
        assert_eq!(members(&[N(0.5), N(1.0)]), None);
        assert_eq!(members(&[Number, N(0.0)]), None);
        assert_eq!(members(&[N(0.0), Number]), None);
        assert_eq!(members(&[N(0.0), N(4294967296.0)]), None);
        assert_eq!(members(&[N(-1.0), N(u32::MAX as f64)]), None);
        assert_eq!(members(&[N(1.0), N(1.0)]), None);

        // Boolean literals widen to `boolean` beside other members.
        assert_eq!(members(&[B(true), B(false)]), None);
        assert_eq!(
            members(&[B(true), S("x".into()), S("y".into())]),
            Some(vec![Boolean, S("x".into()), S("y".into())])
        );
        assert_eq!(members(&[B(false), ab()]), Some(vec![Boolean, ab()]));
        assert_eq!(
            members(&[S("auto".into()), N(0.0), N(1.0)]),
            Some(vec![S("auto".into()), Number])
        );
    }

    #[test]
    fn enums_are_named_only_at_the_top_level() {
        let src = r#"
            type Mode = "fast" | "slow";
            type ModeAlias = Mode;
            declare enum Level { Low = 1 }
            declare namespace Codes { type Region = "eu" | "us"; }
        "#;
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let ctx = CodegenContext::from_module_full(&module, &gctx, false, false, HashSet::new());
        let scope = *module.file_scopes.first().expect("file scope");
        let lower = |ty: &TypeRef, pos: TypePosition| {
            to_syn_type(ty, pos, Some(&ctx), scope, &ModuleContext::Global).to_string()
        };
        let region = TypeRef::Reference {
            segments: vec!["Codes".into(), "Region".into()],
            generic_args: vec![],
        };
        let arg = TypePosition::ARGUMENT;
        let ret = TypePosition::RETURN;

        assert_eq!(lower(&TypeRef::ident("Mode"), arg), "Mode");
        assert_eq!(lower(&TypeRef::ident("ModeAlias"), arg), "Mode");
        assert_eq!(lower(&TypeRef::ident("Level"), arg), "Level");
        assert_eq!(lower(&region, arg), "codes :: Region");

        let modes = TypeRef::Array(Box::new(TypeRef::ident("ModeAlias")));
        let levels = TypeRef::Array(Box::new(TypeRef::ident("Level")));
        assert_eq!(lower(&modes, arg), "& [String]");
        assert_eq!(lower(&levels, ret), "Vec < f64 >");
        assert_eq!(
            lower(&TypeRef::generic("Promise", vec![region]), ret),
            "Promise < JsString >"
        );
        assert_eq!(
            lower(
                &TypeRef::generic("Array", vec![TypeRef::ident("Level")]),
                arg
            ),
            "& Array < Number >"
        );

        assert_eq!(
            ctx.enum_repr(&TypeRef::ident("ModeAlias"), scope),
            Some(TypeRef::String)
        );
        assert_eq!(ctx.enum_repr(&TypeRef::ident("Codes"), scope), None);
    }

    #[test]
    fn qualified_names_resolve_through_module_namespaces() {
        let src = r#"
            export type Kind = "text" | "bytes";
            export declare namespace Outer {
                namespace Inner { type Deep = "a" | "b"; }
            }
        "#;
        let (module, gctx) = crate::parse_source(src, Some("m")).unwrap();
        let ctx = CodegenContext::from_module_full(&module, &gctx, false, false, HashSet::new());
        let scope = *module.file_scopes.first().expect("file scope");
        let outer = match &gctx
            .get_type(gctx.resolve_path(scope, "Outer").unwrap())
            .kind
        {
            TypeKind::Namespace(ns) => ns.child_scope,
            _ => unreachable!(),
        };
        let path = |p: &str| TypeRef::Reference {
            segments: p.split('.').map(String::from).collect(),
            generic_args: vec![],
        };
        let lower = |ty: &TypeRef, scope: ScopeId| {
            let from = ModuleContext::Module("m".into());
            to_syn_type(ty, TypePosition::ARGUMENT, Some(&ctx), scope, &from).to_string()
        };

        assert_eq!(lower(&path("Outer.Inner.Deep"), scope), "Deep");
        assert_eq!(lower(&path("Inner.Deep"), outer), "Deep");
        // Only the last segment matches the local `Kind`.
        assert_eq!(lower(&path("Other.Kind"), scope), "& JsValue");
    }

    #[test]
    fn namespace_members_resolve_from_their_use_site() {
        let src = r#"
            interface Emitter {}
            declare namespace Events {
                interface Emitter {}
                interface Options { emitter: Emitter }
                namespace Inner { interface Handle {} }
            }
        "#;
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let ctx = CodegenContext::from_module_full(&module, &gctx, false, false, HashSet::new());
        let file = module.file_scopes[0];
        let events = gctx.scopes.get(file).namespace("Events").unwrap();
        let inner = gctx.scopes.get(events).namespace("Inner").unwrap();
        let lower = |p: &str, scope: ScopeId, here: &[&str]| {
            *ctx.emit_path.borrow_mut() = here.iter().map(|s| s.to_string()).collect();
            let ty = TypeRef::Reference {
                segments: p.split('.').map(String::from).collect(),
                generic_args: vec![],
            };
            to_syn_type(
                &ty,
                TypePosition::RETURN,
                Some(&ctx),
                scope,
                &ModuleContext::Global,
            )
            .to_string()
        };

        assert_eq!(lower("Emitter", file, &[]), "Emitter");
        assert_eq!(lower("Events.Emitter", file, &[]), "events :: Emitter");
        assert_eq!(lower("Emitter", events, &["events"]), "Emitter");
        assert_eq!(
            lower("Emitter", inner, &["events", "inner"]),
            "super :: Emitter"
        );
        assert_eq!(
            lower("Events.Inner.Handle", file, &["other"]),
            "super :: events :: inner :: Handle"
        );
    }
}
