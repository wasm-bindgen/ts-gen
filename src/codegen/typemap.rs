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

use crate::codegen::GenerateOptions;
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
    "JsNullable",
    "JsOption",
    "JsString",
    "Map",
    "Nullable",
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
    /// collisions with the `js_sys` glob. Never used to resolve a
    /// reference: same-named types from different scopes collapse here.
    pub local_types: HashSet<String>,
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
    /// Types the surface emits under another name than their own (see
    /// [`crate::codegen::surface::Surface::type_names`]).
    renamed_types: HashMap<TypeId, String>,
    /// Types a constructor-shaped variable already binds `new` on, which
    /// a second such variable would redefine.
    pub constructed: RefCell<HashSet<TypeId>>,
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
    /// The statics the module being emitted sees, which a parameter or
    /// local can't shadow (E0530).
    pub statics_in_scope: RefCell<HashSet<String>>,
    /// Builtin (root) scope id.
    pub root_scope: ScopeId,
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
    /// Default error type for fallible bindings without `@throws`;
    /// `None` is `JsValue`. See
    /// [`crate::codegen::GenerateOptions::error_type`].
    pub error_type: Option<syn::Type>,
    /// Whether sync callables emit one catching `Result` form under the
    /// primary name instead of a trapping primary plus `try_<name>`. See
    /// [`crate::codegen::GenerateOptions::fallible`].
    pub fallible: bool,
    /// Name of the generated `@throws` enum; every `@throws`-typed
    /// binding returns it. See
    /// [`crate::codegen::GenerateOptions::error_enum`].
    pub error_enum: Option<String>,
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
    /// surface leaves out the corresponding `mod` block.
    pub externalised_modules: HashSet<String>,
    /// Already-reported `(type_name, module_spec)` pairs for the
    /// unresolved-module-ref error. Emitting one diagnostic per
    /// offending pair keeps the output focused when many references
    /// point at the same unconfigured module.
    pub unresolved_module_refs: RefCell<HashSet<(String, String)>>,
    /// [`Self::alias_params`], by the alias's body scope.
    alias_params: RefCell<HashMap<ScopeId, AliasParams>>,
    /// The type parameters lowered while [`Self::alias_params`] probes an
    /// alias, innermost probe last.
    alias_param_mentions: RefCell<Vec<HashSet<String>>>,
    /// [`Self::is_recursive_alias`], by the alias's id.
    recursive_aliases: RefCell<HashMap<TypeId, bool>>,
    /// The types [`Self::trim_default_args`] is reading the defaults of.
    trimming: RefCell<HashSet<TypeId>>,
    /// Arguments standing in for type parameters while a default is read
    /// against them: the scope declaring the parameters, and each one's
    /// argument tokens. Innermost last.
    param_args: RefCell<Vec<(ScopeId, HashMap<String, TokenStream>)>>,
}

/// How a local generic alias's type parameters carry over to the Rust
/// alias; see [`CodegenContext::alias_params`].
#[derive(Clone)]
pub(crate) struct AliasParams {
    /// Whether the Rust alias declares each parameter.
    pub kept: Vec<bool>,
    /// Whether each kept parameter has its default on the Rust alias.
    pub defaulted: Vec<bool>,
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
        TypeRef::TypedSlice(e, _) => write!(buf, "slice:{}", e.typed_array()).unwrap(),
        TypeRef::StringLiteral(s) => write!(buf, "lit:{s:?}").unwrap(),
        TypeRef::NumberLiteral(n) => write!(buf, "lit:{n}").unwrap(),
        TypeRef::BooleanLiteral(b) => write!(buf, "lit:{b}").unwrap(),
        TypeRef::Array(inner) => {
            buf.push_str("Array<");
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
        TypeRef::Union(u) => {
            buf.push_str("Union<");
            for (i, m) in u.members().iter().enumerate() {
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
        // Unevaluated constructs erase, so their spelling is their key.
        TypeRef::Never
        | TypeRef::KeyOf(_)
        | TypeRef::IndexedAccess(..)
        | TypeRef::Mapped(_)
        | TypeRef::ObjectLiteral(_) => {
            buf.push('?');
            buf.push_str(&ty.format_ts());
        }
    }
}

/// What a name binds in a scope.
#[derive(Clone, Copy, PartialEq)]
enum Bound {
    Type(crate::context::TypeId),
    Namespace(ScopeId),
}

/// How a reference's first segment, written in an alias target, reads
/// from the scope referencing the alias.
enum Qualify {
    /// It binds the same there.
    Keep,
    /// It needs this qualified path instead.
    As(Vec<String>),
    /// No path from there reaches it.
    Unreachable,
}

/// `ty` with the first segment of each reference replaced as `qualify`
/// says, given the segment and whether it heads a path.
fn qualify_references(
    ty: &ir::TypeRef,
    qualify: &dyn Fn(&str, bool) -> Qualify,
) -> Option<ir::TypeRef> {
    let q = |t: &ir::TypeRef| qualify_references(t, qualify);
    let all = |items: &[ir::TypeRef]| items.iter().map(q).collect::<Option<Vec<_>>>();
    Some(match ty {
        ir::TypeRef::Reference {
            segments,
            generic_args,
        } => ir::TypeRef::Reference {
            segments: match qualify(&segments[0], segments.len() > 1) {
                Qualify::Keep => segments.clone(),
                Qualify::As(path) => path
                    .into_iter()
                    .chain(segments[1..].iter().cloned())
                    .collect(),
                Qualify::Unreachable => return None,
            },
            generic_args: all(generic_args)?,
        },
        ir::TypeRef::Array(inner) => ir::TypeRef::Array(Box::new(q(inner)?)),
        ir::TypeRef::Union(u) => ir::TypeRef::union(all(u.members())?),
        ir::TypeRef::Intersection(items) => ir::TypeRef::Intersection(all(items)?),
        ir::TypeRef::Tuple(items) => ir::TypeRef::Tuple(all(items)?),
        ir::TypeRef::Function(sig) => ir::TypeRef::Function(ir::FunctionSig {
            params: sig
                .params
                .iter()
                .map(|p| {
                    Some(ir::Param {
                        type_ref: q(&p.type_ref)?,
                        ..p.clone()
                    })
                })
                .collect::<Option<_>>()?,
            return_type: Box::new(q(&sig.return_type)?),
        }),
        other => other.clone(),
    })
}

/// `alias`'s target with `generic_args` substituted for its parameters;
/// a missing argument takes the parameter's default, else `any`.
fn alias_instance(alias: &ir::TypeAliasDecl, generic_args: &[ir::TypeRef]) -> ir::TypeRef {
    // A default may name an earlier parameter (`<A, B = A>`), so each
    // one is substituted with the map built so far.
    let mut map: HashMap<&str, ir::TypeRef> = HashMap::new();
    for (i, tp) in alias.type_params.iter().enumerate() {
        let arg = generic_args
            .get(i)
            .cloned()
            .or_else(|| tp.default.as_ref().map(|d| d.substitute(&map)))
            .unwrap_or(ir::TypeRef::Any);
        map.insert(&tp.name, arg);
    }
    alias.target.substitute(&map)
}

/// The Rust default of a generic type's parameter, as its `pub type`
/// declaration spells it: the TS default lowered like a generic
/// argument in the type's `scope`, `JsValue` when there is none. When
/// the default's target itself erases to `JsValue` (an alias of a union,
/// an unresolved name) its type arguments go with it:
/// `CfProperties<CfHostMetadata>` is just `JsValue`.
pub(crate) fn type_param_default_tokens(
    default: Option<&TypeRef>,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    let Some(default) = default else {
        return quote! { JsValue };
    };
    let tokens = to_syn_type(
        default,
        TypePosition::ARGUMENT.to_inner(),
        ctx,
        scope,
        from_module,
    );
    if let (TypeRef::Reference { segments, .. }, Some(ctx)) = (default, ctx) {
        let uses = ctx.external_uses.borrow();
        if uses.get(&segments[0]).is_some_and(|v| v == "JsValue") {
            return quote! { JsValue };
        }
    }
    tokens
}

/// The `Module(spec)` groups fully covered by the external map (Rule 3).
/// References to types from these modules go through
/// `external_map.resolve` in `emit_type_name` instead of producing a local
/// `foo::` qualifier, and their declarations are not emitted.
pub(crate) fn externalised_modules(module: &ir::Module, gctx: &GlobalContext) -> HashSet<String> {
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
    externalised_modules
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
    /// themselves.
    pub(crate) fn is_local_generic(&self, name: &str, scope: ScopeId) -> bool {
        self.gctx.resolve_path(scope, name).is_some_and(|id| {
            self.local_type_ids.contains(&id)
                && declared_arity(&self.gctx.get_type(id).kind).is_some_and(|n| n > 0)
        })
    }

    /// The Rust target of a local type alias, lowered in the alias's own
    /// scope so its type parameters bind. An enum target has no inner form,
    /// so it is named directly.
    pub(crate) fn alias_target(
        &self,
        alias: &ir::TypeAliasDecl,
        from_module: &ModuleContext,
    ) -> TokenStream {
        // Under per-monomorphization a non-generic alias admitting empties
        // takes its carrier (`Nullable<String>`), as its uses inline the
        // target rather than name it. A generic one is named inside
        // containers, which need the `JsGeneric` handle.
        if Self::generic_mono(Some(self))
            && alias.type_params.is_empty()
            && alias.target.split_empties().is_some()
        {
            return to_mono_user_generic_arg(
                &alias.target,
                Some(self),
                alias.body_scope,
                from_module,
            );
        }
        let pos = if self.enum_kind(&alias.target, alias.body_scope).is_some() {
            TypePosition::RETURN
        } else {
            TypePosition::ARGUMENT.to_inner()
        };
        to_syn_type(
            &alias.target,
            pos,
            Some(self),
            alias.body_scope,
            from_module,
        )
    }

    /// The Rust default of a type parameter of `alias`, lowered in the
    /// alias's scope like a generic argument at a use site.
    pub(crate) fn alias_param_default(
        &self,
        default: &TypeRef,
        alias: &ir::TypeAliasDecl,
        from_module: &ModuleContext,
    ) -> TokenStream {
        if Self::generic_mono(Some(self)) {
            to_mono_user_generic_arg(default, Some(self), alias.body_scope, from_module)
        } else {
            to_syn_type(
                default,
                TypePosition::ARGUMENT.to_inner(),
                Some(self),
                alias.body_scope,
                from_module,
            )
        }
    }

    /// The type parameters of the alias being probed that `lower` emits.
    fn mentioned_params(&self, lower: impl FnOnce()) -> HashSet<String> {
        self.alias_param_mentions.borrow_mut().push(HashSet::new());
        lower();
        self.alias_param_mentions
            .borrow_mut()
            .pop()
            .unwrap_or_default()
    }

    /// Which of a local generic alias's type parameters the Rust alias
    /// keeps, and which keep their defaults.
    ///
    /// A Rust alias can't declare a parameter its target doesn't use
    /// (E0091), and lowering can erase one (`Missing<T>` → `JsValue`,
    /// `T & { b: B }` → `T`), so those are dropped from the declaration and
    /// from every instantiation. The trailing kept parameters keep their
    /// defaults, lowered in the alias's scope, unless one names a dropped
    /// parameter.
    pub(crate) fn alias_params(
        &self,
        alias: &ir::TypeAliasDecl,
        from_module: &ModuleContext,
    ) -> Option<AliasParams> {
        if alias.type_params.is_empty() || alias.from_module.is_some() {
            return None;
        }
        if let Some(params) = self.alias_params.borrow().get(&alias.body_scope) {
            return Some(params.clone());
        }
        let n = alias.type_params.len();
        // A recursive alias sees every parameter as kept, and passes every
        // argument, while its own target is lowered.
        self.alias_params.borrow_mut().insert(
            alias.body_scope,
            AliasParams {
                kept: vec![true; n],
                defaulted: vec![false; n],
            },
        );
        // The declaration reports these lowerings' diagnostics when it's
        // emitted.
        let diagnostics = self.diagnostics.take();
        let reported = self.unresolved_module_refs.borrow().clone();
        let in_target = self.mentioned_params(|| {
            self.alias_target(alias, from_module);
        });
        let in_defaults: Vec<Option<HashSet<String>>> = alias
            .type_params
            .iter()
            .map(|tp| {
                let default = tp.default.as_ref()?;
                Some(self.mentioned_params(|| {
                    self.alias_param_default(default, alias, from_module);
                }))
            })
            .collect();
        self.diagnostics.replace(diagnostics);
        self.unresolved_module_refs.replace(reported);
        let kept: Vec<bool> = alias
            .type_params
            .iter()
            .map(|tp| in_target.contains(&tp.name))
            .collect();
        let mut defaulted = vec![false; n];
        for i in (0..n).rev().filter(|&i| kept[i]) {
            let Some(mentions) = &in_defaults[i] else {
                break;
            };
            let names_dropped = alias
                .type_params
                .iter()
                .zip(&kept)
                .any(|(tp, &kept)| !kept && mentions.contains(&tp.name));
            if names_dropped {
                break;
            }
            defaulted[i] = true;
        }
        let params = AliasParams { kept, defaulted };
        self.alias_params
            .borrow_mut()
            .insert(alias.body_scope, params.clone());
        Some(params)
    }

    /// The arguments an instantiation of the generic alias `name` resolves
    /// to passes to the Rust alias. One the Rust alias has a default for is
    /// left to it when omitted, along with every omitted one after it; TS's
    /// defaults fill the others (`U = T` with `T` dropped), then the
    /// parameters the alias drops are skipped.
    fn alias_args(&self, name: &str, scope: ScopeId, args: &[TypeRef]) -> Option<Vec<TypeRef>> {
        let id = self.gctx.resolve_path(scope, name)?;
        let decl = self.gctx.get_type(id);
        let TypeKind::TypeAlias(alias) = &decl.kind else {
            return None;
        };
        let params = self.alias_params(alias, &decl.module_context)?;
        let mut given: HashMap<&str, TypeRef> = HashMap::new();
        let mut out = Vec::new();
        for (i, tp) in alias.type_params.iter().enumerate() {
            let arg = match (args.get(i), &tp.default) {
                (Some(arg), _) => arg.clone(),
                _ if params.defaulted[i] => break,
                (None, Some(default)) => default.substitute(&given),
                (None, None) => TypeRef::Any,
            };
            if params.kept[i] {
                out.push(arg.clone());
            }
            given.insert(&tp.name, arg);
        }
        Some(out)
    }

    /// `args` of a reference to the local generic class, interface or
    /// discriminated union `id`, without the trailing ones its `pub type`
    /// declaration defaults to (see [`type_param_default_tokens`]):
    /// `Request<JsValue, JsValue>` is `Request`, and `Foo<JsString,
    /// JsValue>` with `B = JsValue` is `Foo<JsString>`. A default naming an
    /// earlier parameter is read with that parameter's argument, so
    /// `Holder<JsValue, Box>` with `U = Box<T>` is `Holder`.
    pub(crate) fn trim_default_args(
        &self,
        id: TypeId,
        mut args: Vec<TokenStream>,
    ) -> Vec<TokenStream> {
        let decl = self.gctx.get_type(id);
        let (type_params, scope) = match &decl.kind {
            TypeKind::Class(c) => (&c.type_params, c.body_scope),
            TypeKind::Interface(i) => (&i.type_params, i.body_scope),
            TypeKind::DiscriminatedUnion(d) => (&d.type_params, d.body_scope),
            _ => return args,
        };
        // A default naming its own type (`Node<C = Node<…>>`) keeps that
        // reference's arguments.
        if !self.local_type_ids.contains(&id) || !self.trimming.borrow_mut().insert(id) {
            return args;
        }
        // The declaration reports the defaults' diagnostics when it's
        // emitted, and an alias probe in progress mustn't see their
        // parameters as its own.
        let diagnostics = self.diagnostics.take();
        let reported = self.unresolved_module_refs.borrow().clone();
        self.alias_param_mentions.borrow_mut().push(HashSet::new());
        while let Some(last) = args.last() {
            let i = args.len() - 1;
            let Some(tp) = type_params.get(i) else {
                break;
            };
            let earlier = type_params[..i]
                .iter()
                .map(|tp| tp.name.clone())
                .zip(args[..i].iter().cloned())
                .collect();
            self.param_args.borrow_mut().push((scope, earlier));
            let default = type_param_default_tokens(
                tp.default.as_ref(),
                Some(self),
                scope,
                &decl.module_context,
            );
            self.param_args.borrow_mut().pop();
            if default.to_string() != last.to_string() {
                break;
            }
            args.pop();
        }
        self.alias_param_mentions.borrow_mut().pop();
        self.diagnostics.replace(diagnostics);
        self.unresolved_module_refs.replace(reported);
        self.trimming.borrow_mut().remove(&id);
        args
    }

    /// The argument standing in for the type parameter `name`, used in
    /// `scope`, while a default is read against the arguments chosen for
    /// its earlier parameters.
    fn param_arg(&self, scope: ScopeId, name: &str) -> Option<TokenStream> {
        let param_args = self.param_args.borrow();
        let (declaring, args) = param_args.last()?;
        let scopes = &self.gctx.scopes;
        let binds = scopes
            .chain(scope)
            .into_iter()
            .find(|&s| scopes.get(s).get_binding(name).is_some())?;
        (binds == *declaring).then(|| args.get(name).cloned())?
    }

    /// A parameter named `name`, or `name_` when a static in scope has its
    /// name, since a parameter can't shadow a static (E0530).
    pub(crate) fn param_name(ctx: Option<&Self>, name: &str) -> String {
        if ctx.is_some_and(|c| c.statics_in_scope.borrow().contains(name)) {
            format!("{name}_")
        } else {
            name.to_string()
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
        Self::from_module_full(module, gctx, &GenerateOptions::default(), None)
            .expect("default options are valid")
    }

    /// Build a `CodegenContext` from the full set of per-codegen options
    /// and the surface being emitted, which names the emitted modules and
    /// namespaces. Fails when an option carries an unparseable Rust path.
    pub fn from_module_full(
        module: &ir::Module,
        gctx: &'a GlobalContext,
        options: &GenerateOptions,
        surface: Option<&crate::codegen::surface::Surface<'_>>,
    ) -> anyhow::Result<Self> {
        let error_type = options
            .error_type
            .as_deref()
            .map(|path| {
                syn::parse_str::<syn::Type>(path)
                    .map_err(|e| anyhow::anyhow!("invalid --error-type `{path}`: {e}"))
            })
            .transpose()?;
        let externalised_modules = externalised_modules(module, gctx);
        let exported_modules = surface.map(|s| s.modules()).unwrap_or_default();

        let mut ctx = CodegenContext {
            gctx,
            local_types: HashSet::new(),
            local_type_ids: HashSet::new(),
            names: RefCell::new(LocalScope::with_claimed(reserved_names())),
            renamed_locals: HashMap::new(),
            renamed_types: surface.map(|s| s.type_names().clone()).unwrap_or_default(),
            constructed: RefCell::default(),
            namespace_paths: HashMap::new(),
            lib_name: module.lib_name.clone(),
            emit_path: RefCell::new(Vec::new()),
            statics_in_scope: RefCell::new(HashSet::new()),
            root_scope: module.global_scope,
            external_uses: RefCell::new(HashMap::new()),
            diagnostics: RefCell::new(DiagnosticCollector::new()),
            dynamic_unions: RefCell::new(DynamicUnionRegistry::default()),
            error_type,
            fallible: options.fallible,
            error_enum: options.error_enum.clone(),
            experimental_generic_mono: options.experimental_generic_mono,
            exported_modules,
            externalised_modules,
            unresolved_module_refs: RefCell::new(HashSet::new()),
            alias_params: RefCell::new(HashMap::new()),
            alias_param_mentions: RefCell::new(Vec::new()),
            recursive_aliases: RefCell::new(HashMap::new()),
            trimming: RefCell::new(HashSet::new()),
            param_args: RefCell::new(Vec::new()),
        };
        for (spec, scopes) in &gctx.ambient_module_scopes {
            let path = ctx.specifier_path(spec);
            for &scope in scopes {
                ctx.namespace_paths.insert(scope, path.clone());
            }
        }
        for &type_id in &module.types {
            let path = ctx.module_path(&gctx.get_type(type_id).module_context);
            ctx.collect_declaration(type_id, &path, surface);
        }
        ctx.resolve_collisions();
        Ok(ctx)
    }

    /// Create an empty context (for tests). Requires a valid root scope.
    pub fn empty(gctx: &'a GlobalContext, root_scope: ScopeId) -> Self {
        CodegenContext {
            gctx,
            local_types: HashSet::new(),
            local_type_ids: HashSet::new(),
            names: RefCell::new(LocalScope::with_claimed(reserved_names())),
            renamed_locals: HashMap::new(),
            renamed_types: HashMap::new(),
            constructed: RefCell::default(),
            namespace_paths: HashMap::new(),
            lib_name: None,
            emit_path: RefCell::new(Vec::new()),
            statics_in_scope: RefCell::new(HashSet::new()),
            root_scope,
            external_uses: RefCell::new(HashMap::new()),
            diagnostics: RefCell::new(DiagnosticCollector::new()),
            dynamic_unions: RefCell::new(DynamicUnionRegistry::default()),
            error_type: None,
            fallible: false,
            error_enum: None,
            experimental_generic_mono: false,
            exported_modules: HashSet::new(),
            externalised_modules: HashSet::new(),
            unresolved_module_refs: RefCell::new(HashSet::new()),
            alias_params: RefCell::new(HashMap::new()),
            alias_param_mentions: RefCell::new(Vec::new()),
            recursive_aliases: RefCell::new(HashMap::new()),
            trimming: RefCell::new(HashSet::new()),
            param_args: RefCell::new(Vec::new()),
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
    /// Returns the final target if the name resolves to a type alias (or a
    /// chain of aliases), along with the scope its names resolve in: the
    /// body scope of the last alias in the chain. Each alias's target is
    /// read in that alias's own scope, never the use site's, so a name the
    /// use site shadows still means what the alias declared. Returns `None`
    /// if the name resolves to a non-alias declaration (Class, Interface,
    /// Enum, etc.) or is not found.
    ///
    /// Uses a visited set to detect and break circular alias chains.
    ///
    /// A generic alias named bare means its defaults (`Fetcher` is
    /// `Fetcher<undefined>`), so they replace its parameters in the target.
    pub fn resolve_alias(
        &self,
        name: &str,
        scope: ScopeId,
    ) -> Option<(std::borrow::Cow<'_, ir::TypeRef>, ScopeId)> {
        let mut visited = HashSet::new();
        self.resolve_alias_impl(name, scope, &mut visited)
    }

    /// Whether every name in `ty` resolves to the same declaration from
    /// `a` as from `b`, so `ty` lowers alike from either scope.
    pub(crate) fn resolves_alike(&self, ty: &ir::TypeRef, a: ScopeId, b: ScopeId) -> bool {
        if a == b {
            return true;
        }
        let alike = |t: &ir::TypeRef| self.resolves_alike(t, a, b);
        match ty {
            TypeRef::Reference {
                segments,
                generic_args,
            } => {
                let head_alike = match segments.as_slice() {
                    [name] => {
                        self.gctx.scopes.resolve_binding(a, name)
                            == self.gctx.scopes.resolve_binding(b, name)
                    }
                    _ => {
                        let path = segments.join(".");
                        self.gctx.scopes.resolve_namespace(a, &segments[0])
                            == self.gctx.scopes.resolve_namespace(b, &segments[0])
                            && self.gctx.resolve_path(a, &path) == self.gctx.resolve_path(b, &path)
                    }
                };
                head_alike && generic_args.iter().all(alike)
            }
            TypeRef::Array(t) => alike(t),
            TypeRef::Union(u) => u.members().iter().all(alike),
            TypeRef::Intersection(ts) | TypeRef::Tuple(ts) => ts.iter().all(alike),
            TypeRef::Function(sig) => {
                sig.params.iter().all(|p| alike(&p.type_ref)) && alike(&sig.return_type)
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
            | TypeRef::Unresolved(_) => true,
        }
    }

    /// The string or numeric enum declaration `ty` names, through aliases
    /// and qualified paths.
    pub(crate) fn enum_kind(&self, ty: &ir::TypeRef, scope: ScopeId) -> Option<&TypeKind> {
        let mut ty = std::borrow::Cow::Borrowed(ty);
        let mut scope = scope;
        // Bounded, since an alias chain can be circular.
        for _ in 0..32 {
            let ir::TypeRef::Reference {
                segments,
                generic_args,
            } = ty.as_ref()
            else {
                return None;
            };
            let id = self.gctx.resolve_path(scope, &segments.join("."))?;
            let kind = &self.gctx.get_type(id).kind;
            let alias = match kind {
                // An enum takes no arguments, so any given don't matter.
                TypeKind::StringEnum(_) | TypeKind::NumericEnum(_) => return Some(kind),
                TypeKind::TypeAlias(alias) => alias,
                _ => return None,
            };
            // A target that is one of the alias's parameters is the argument
            // given for it, resolved where it's given, or its default.
            let param = alias
                .target
                .as_ident()
                .and_then(|name| alias.type_params.iter().position(|tp| tp.name == name));
            match param {
                Some(i) => match (generic_args.get(i), &alias.type_params[i].default) {
                    (Some(arg), _) => ty = std::borrow::Cow::Owned(arg.clone()),
                    (None, Some(default)) => {
                        ty = std::borrow::Cow::Borrowed(default);
                        scope = alias.body_scope;
                    }
                    (None, None) => return None,
                },
                None => {
                    ty = std::borrow::Cow::Borrowed(&alias.target);
                    scope = alias.body_scope;
                }
            }
        }
        None
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
        visited: &mut HashSet<TypeId>,
    ) -> Option<(std::borrow::Cow<'b, ir::TypeRef>, ScopeId)> {
        let type_id = self.gctx.scopes.resolve(scope, name)?;
        if !visited.insert(type_id) {
            return None; // circular alias chain
        }
        let TypeKind::TypeAlias(ref alias) = self.gctx.get_type(type_id).kind else {
            return None;
        };
        // A self-referential alias is named rather than chased.
        if self.is_recursive_alias(type_id) {
            return None;
        }
        let target = alias_target_with_defaults(alias);
        // If the target is itself a single-segment path, keep resolving.
        // Generic instantiations and qualified paths stop here.
        if let Some(inner_name) = target.as_ident() {
            if let Some(resolved) = self.resolve_alias_impl(inner_name, alias.body_scope, visited) {
                return Some(resolved);
            }
        }
        Some((target, alias.body_scope))
    }

    /// Whether the alias `id` reaches itself through the aliases its
    /// target (or a parameter default) names, directly (`type Next<T> =
    /// (n: Next<T>) => void`) or through other aliases. Expanding such an
    /// alias never ends, so its references keep the alias name.
    pub(crate) fn is_recursive_alias(&self, id: TypeId) -> bool {
        if let Some(&recursive) = self.recursive_aliases.borrow().get(&id) {
            return recursive;
        }
        let recursive = self.alias_reaches(id, id, &mut HashSet::new());
        self.recursive_aliases.borrow_mut().insert(id, recursive);
        recursive
    }

    /// Whether the alias `from` names `target`, or an alias that does.
    fn alias_reaches(&self, from: TypeId, target: TypeId, visited: &mut HashSet<TypeId>) -> bool {
        if !visited.insert(from) {
            return false;
        }
        let TypeKind::TypeAlias(alias) = &self.gctx.get_type(from).kind else {
            return false;
        };
        let scope = alias.body_scope;
        std::iter::once(&alias.target)
            .chain(
                alias
                    .type_params
                    .iter()
                    .filter_map(|tp| tp.default.as_ref()),
            )
            .any(|ty| self.type_reaches_alias(ty, scope, target, visited))
    }

    /// Whether `ty`, read in `scope`, names the alias `target` or an
    /// alias that does.
    fn type_reaches_alias(
        &self,
        ty: &ir::TypeRef,
        scope: ScopeId,
        target: TypeId,
        visited: &mut HashSet<TypeId>,
    ) -> bool {
        let children: Vec<&ir::TypeRef> = match ty {
            ir::TypeRef::Reference {
                segments,
                generic_args,
            } => {
                if let Some(id) = self.gctx.resolve_path(scope, &segments.join(".")) {
                    if id == target || self.alias_reaches(id, target, visited) {
                        return true;
                    }
                }
                generic_args.iter().collect()
            }
            ir::TypeRef::Array(inner) => vec![&**inner],
            ir::TypeRef::Union(u) => u.members().iter().collect(),
            ir::TypeRef::Intersection(items) | ir::TypeRef::Tuple(items) => items.iter().collect(),
            ir::TypeRef::Function(sig) => sig
                .params
                .iter()
                .map(|p| &p.type_ref)
                .chain(std::iter::once(&*sig.return_type))
                .collect(),
            _ => return false,
        };
        children
            .into_iter()
            .any(|t| self.type_reaches_alias(t, scope, target, visited))
    }

    /// Chase a generic alias instantiation: `Foo<A, B>` where `Foo` is
    /// `type Foo<T, U> = …` yields the target with `T`/`U` substituted.
    /// Missing arguments take the parameter default, else `any`. Qualified
    /// references go through [`Self::resolve_qualified_alias_instance`];
    /// non-alias heads and aliases without parameters yield `None`, as
    /// [`Self::resolve_alias`] chases those from the alias's own scope.
    ///
    /// The result lowers in the referencing scope, where the arguments were
    /// written, so a bare reference is chased only where the target (and
    /// any default standing in for an omitted argument) reads alike from
    /// there and from the alias's body scope.
    pub fn resolve_alias_instance(&self, ty: &ir::TypeRef, scope: ScopeId) -> Option<ir::TypeRef> {
        let ir::TypeRef::Reference {
            segments,
            generic_args,
        } = ty
        else {
            return None;
        };
        if segments.len() != 1 {
            return self.resolve_qualified_alias_instance(segments, generic_args, scope);
        }
        let type_id = self.gctx.scopes.resolve(scope, &segments[0])?;
        let TypeKind::TypeAlias(alias) = &self.gctx.get_type(type_id).kind else {
            return None;
        };
        if alias.type_params.is_empty() || self.is_recursive_alias(type_id) {
            return None;
        }
        // The given arguments are the use site's own, so only the target and
        // the defaults filling omitted ones need to read alike.
        let given = vec![ir::TypeRef::Any; generic_args.len().min(alias.type_params.len())];
        if !self.resolves_alike(&alias_instance(alias, &given), alias.body_scope, scope) {
            return None;
        }
        Some(alias_instance(alias, generic_args))
    }

    /// [`Self::resolve_alias_instance`] for `Ns.Alias<A, B>`. The result
    /// lowers in the referencing scope, where the arguments were written,
    /// so each name in the target that the referencing scope would bind
    /// differently is first qualified through the namespaces of the path
    /// (`Ns.X` for a sibling, the shorter `Outer.X` for a name from an
    /// enclosing namespace). A target naming something no such path
    /// reaches, such as an unexported member, is not chased. An alias
    /// without parameters is not either: with no arguments to keep,
    /// `to_syn_type` lowers its target where it was declared.
    fn resolve_qualified_alias_instance(
        &self,
        segments: &[String],
        generic_args: &[ir::TypeRef],
        scope: ScopeId,
    ) -> Option<ir::TypeRef> {
        let type_id = self.gctx.resolve_path(scope, &segments.join("."))?;
        let decl = self.gctx.get_type(type_id);
        let TypeKind::TypeAlias(alias) = &decl.kind else {
            return None;
        };
        if alias.type_params.is_empty() || self.is_recursive_alias(type_id) {
            return None;
        }
        let scopes = &self.gctx.scopes;
        // The namespace each proper prefix of the path reaches.
        let mut chain = vec![scopes.resolve_namespace(scope, &segments[0])?];
        for name in &segments[1..segments.len() - 1] {
            let ns = *chain.last()?;
            let (local, bound) = self.gctx.exports.binding(ns, name)?;
            chain.push(scopes.get(bound).namespace(local)?);
        }
        // A name heading a path binds a namespace, otherwise a type.
        let resolve = |s: ScopeId, name: &str, path: bool| {
            if path {
                scopes.resolve_namespace(s, name).map(Bound::Namespace)
            } else {
                scopes.resolve(s, name).map(Bound::Type)
            }
        };
        let member = |ns: ScopeId, name: &str, path: bool| {
            let members = scopes.get(ns);
            if path {
                members.namespace(name).map(Bound::Namespace)
            } else {
                members.get(name).map(Bound::Type)
            }
        };
        let qualify = |name: &str, path: bool| {
            if !path && alias.type_params.iter().any(|tp| tp.name == name) {
                return Qualify::Keep;
            }
            let meant = resolve(decl.scope_id, name, path);
            if resolve(scope, name, path) == meant {
                return Qualify::Keep;
            }
            if meant.is_none() {
                return Qualify::Unreachable;
            }
            // Each namespace's export map, read forward: any public name
            // whose local binding is the one meant reaches it, the name's
            // own first, so a declaration exported under several names is
            // found by each.
            for (depth, &ns) in chain.iter().enumerate().rev() {
                let public = self
                    .gctx
                    .exports
                    .bindings(ns)
                    .filter(|&(_, local, bound)| member(bound, local, path) == meant)
                    .min_by_key(|&(public, _, _)| public != name)
                    .map(|(public, _, _)| public);
                if let Some(public) = public {
                    return Qualify::As(
                        segments[..=depth]
                            .iter()
                            .cloned()
                            .chain(std::iter::once(public.to_string()))
                            .collect(),
                    );
                }
            }
            Qualify::Unreachable
        };
        // A default stands in for an omitted argument, so it is read where
        // it is written, like the target.
        let type_params = alias
            .type_params
            .iter()
            .enumerate()
            .map(|(i, tp)| {
                let default = match &tp.default {
                    Some(d) if i >= generic_args.len() => Some(qualify_references(d, &qualify)?),
                    _ => None,
                };
                Some(ir::TypeParam {
                    default,
                    ..tp.clone()
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let alias = ir::TypeAliasDecl {
            type_params,
            target: qualify_references(&alias.target, &qualify)?,
            ..alias.clone()
        };
        Some(alias_instance(&alias, generic_args))
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
        for name in &self.local_types {
            names.claim(name.clone());
        }
        for name in reserved_names() {
            if self.local_types.contains(name) {
                let renamed = names.declare(name, Suffix::Trailing);
                self.renamed_locals.insert(name.to_string(), renamed);
            }
        }
    }

    /// Record a declaration this unit emits. `mod_path` is the `pub mod`
    /// it is emitted in.
    fn collect_declaration(
        &mut self,
        type_id: TypeId,
        mod_path: &[String],
        surface: Option<&crate::codegen::surface::Surface<'_>>,
    ) {
        let gctx = self.gctx;
        let decl = gctx.get_type(type_id);
        let name = match &decl.kind {
            ir::TypeKind::Class(c) => &c.name,
            ir::TypeKind::Interface(i) => &i.name,
            ir::TypeKind::DiscriminatedUnion(d) => &d.name,
            ir::TypeKind::StringEnum(e) => &e.name,
            ir::TypeKind::NumericEnum(e) => &e.name,
            ir::TypeKind::TsEnum(e) => &e.name,
            ir::TypeKind::TypeAlias(a) => &a.name,
            ir::TypeKind::Namespace(ns) => {
                let mut path = mod_path.to_vec();
                path.push(match surface {
                    Some(surface) => surface.mod_name(type_id, &ns.name),
                    None => crate::util::naming::to_snake_case(&ns.name),
                });
                for &member in &ns.declarations {
                    self.collect_declaration(member, &path, surface);
                }
                self.namespace_paths.insert(ns.child_scope, path);
                return;
            }
            ir::TypeKind::Function(_) | ir::TypeKind::Variable(_) => return,
        };
        let name = self.renamed_types.get(&type_id).unwrap_or(name);
        self.local_types.insert(name.clone());
        self.local_type_ids.insert(type_id);
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
    pub(crate) fn namespace_path(&self, scope: ScopeId) -> &[String] {
        self.gctx
            .scopes
            .chain(scope)
            .into_iter()
            .find_map(|id| self.namespace_paths.get(&id))
            .map_or(&[], Vec::as_slice)
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
pub(crate) fn type_decl_name(kind: &ir::TypeKind) -> Option<&str> {
    match kind {
        ir::TypeKind::Class(c) => Some(&c.name),
        ir::TypeKind::Interface(i) => Some(&i.name),
        ir::TypeKind::DiscriminatedUnion(d) => Some(&d.name),
        ir::TypeKind::TypeAlias(a) => Some(&a.name),
        ir::TypeKind::StringEnum(e) => Some(&e.name),
        ir::TypeKind::NumericEnum(e) => Some(&e.name),
        ir::TypeKind::TsEnum(e) => Some(&e.name),
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
    // `PropertyKey` lowers as the union it abbreviates.
    if matches!(ty, TypeRef::PropertyKey) {
        return to_syn_type(
            &TypeRef::union(TypeRef::property_key_members()),
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
            _ => {}
        }
        if let Some((core, n, scope)) = split_empties_in(ty, ctx, scope) {
            let inner_ty = to_syn_type(&core, pos, ctx, scope, from_module);
            return js_nullable_or_js_value(inner_ty, n);
        }
    }

    // `Option<T>` is `T | undefined` and `Nullable<T>` is `T | null`; a
    // position admitting both nests them. A type parameter is taken by
    // value, as the macro cannot borrow a generic (`Option<&T>`).
    if let Some((core, n, scope)) = split_empties_in(ty, ctx, scope) {
        let core_pos = if pos.is_argument() && is_type_param(&core, ctx, scope) {
            TypePosition::RETURN
        } else {
            pos
        };
        let inner_ty = to_syn_type(&core, core_pos, ctx, scope, from_module);
        // `JsValue` carries both empties in-band.
        let bare = inner_ty.to_string();
        if bare == "JsValue" || (bare == "& JsValue" && n.admits_null()) {
            return inner_ty;
        }
        return nullable_carrier(inner_ty, n);
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
        TypeRef::TypedSlice(elem, _) => {
            if pos.inner {
                let ident = make_ident(elem.typed_array());
                quote! { #ident }
            } else {
                let scalar = make_ident(elem.rust());
                let seq = if pos.is_argument() {
                    quote! { &[#scalar] }
                } else {
                    quote! { Vec<#scalar> }
                };
                if elem.is_clamped() {
                    quote! { ::wasm_bindgen::Clamped<#seq> }
                } else {
                    seq
                }
            }
        }

        // === Syntactic constructs ===
        //
        // Top-level `Array<T>` (or `T[]`) lowers to a Rust-idiomatic
        // sequence:
        //
        // * Argument position → `&[T]`, reaching JS as a plain `Array`.
        // * Return position → `Vec<T>` (owned).
        //
        // wasm-bindgen has both a slice and a `Vec` ABI for Rust-native
        // `String` and `f64` elements and for JS handles. So an element
        // that resolves (through aliases and unions) to a string or
        // number takes return-position lowering (`&[f64]`,
        // `Vec<String>`; `&[&str]` would needlessly thread a borrow
        // lifetime through the slice). Every other element takes its
        // inner-position handle form: `&[EmailAttachment]`,
        // `Vec<Boolean>`, `Vec<JsOption<JsString>>`, and
        // `Vec<Array<JsString>>` for `string[][]`.
        //
        // A slice of numbers also needs `#[wasm_bindgen(slice = "array")]`
        // on its argument to reach JS as an `Array`; see
        // [`param_slice_mode`].
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
        TypeRef::Union(u) => {
            // Single source of truth for union LUB — handles literal
            // widening, named-type ancestor walking, and the JsValue
            // fallback diagnostic uniformly. See `subtyping::lub_union`.
            if let Some(lub) = crate::codegen::subtyping::lub_union(u.members(), ctx, scope) {
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
            // `Function<fn() -> Promise<T>>` with a type parameter `T`
            // does not satisfy wasm-bindgen's `JsFunction` (the nested
            // `ErasableGeneric::Repr` projection fails to normalise), so
            // the promised value erases inside callback signatures.
            let promised_param = sig.return_type.as_promise_inner().is_some_and(|inner| {
                inner.as_ident().is_some_and(|name| {
                    ctx.is_some_and(|c| {
                        matches!(
                            c.gctx.scopes.resolve_binding(scope, name),
                            Some(crate::parse::scope::Binding::TypeParam)
                        )
                    })
                })
            });
            let ret = if promised_param {
                quote! { Promise }
            } else {
                to_syn_type(&sig.return_type, inner_pos, ctx, scope, from_module)
            };
            // A `fn` pointer type has fixed arity, so a rest parameter
            // cannot be spelled; the untyped `Function` accepts any
            // closure, which is what `(...args: any[]) => void` means.
            let variadic = sig.params.iter().any(|p| p.variadic);
            let base = if variadic || params.iter().all(is_jsvalue_arg) && is_jsvalue_arg(&ret) {
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
        // * `Array<T>` / `ReadonlyArray<T>` — parse normalises these to
        //   `TypeRef::Array`, so a reference here is synthesised; it
        //   delegates to the same `T[]` lowering. `ArrayLike<T>` is the
        //   distinct TS type that lowers to the JS-array reference form
        //   (`&Array<U>` / `Array<U>`), for callers who want to pass an
        //   array handle with no element-wise materialisation.
        // * Bare references that resolve to a type alias whose target
        //   carries its own borrow shape (e.g. `string`, `T[]`,
        //   `Array<U>`) — recursing into the target re-runs
        //   `to_syn_type` with the same `pos`, which already does the
        //   right thing.
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            if segments.len() == 1 && !generic_args.is_empty() {
                match segments[0].as_str() {
                    "Array" | "ReadonlyArray" => {
                        let inner = generic_args.first().cloned().unwrap();
                        return to_syn_type(
                            &TypeRef::Array(Box::new(inner)),
                            pos,
                            ctx,
                            scope,
                            from_module,
                        );
                    }
                    "ArrayLike" => {
                        let inner = generic_args.first().cloned().unwrap();
                        let base = generic_container(
                            quote! { Array },
                            &inner,
                            pos,
                            ctx,
                            scope,
                            from_module,
                        );
                        return maybe_ref(base, borrow);
                    }
                    _ => {}
                }
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
                    // The target lowers from the alias's own scope, where
                    // its names are declared.
                    if let Some((target, alias_scope)) = c.resolve_alias(&segments[0], scope) {
                        let target = target.into_owned();
                        return to_syn_type(&target, pos, ctx, alias_scope, from_module);
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
                    let resolved = resolved.filter(|&id| !c.is_recursive_alias(id));
                    if let Some(TypeKind::TypeAlias(alias)) =
                        resolved.map(|id| &c.gctx.get_type(id).kind)
                    {
                        let target = alias_target_with_defaults(alias);
                        return to_syn_type(&target, pos, ctx, alias.body_scope, from_module);
                    }
                }
            }
            if let Some(target) = ctx.and_then(|c| c.resolve_alias_instance(ty, scope)) {
                return to_syn_type(&target, pos, ctx, scope, from_module);
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
        // `never` has no value to lower; the rest reach codegen only when
        // `parse::eval` could not evaluate them.
        TypeRef::Never => maybe_ref(quote! { JsValue }, borrow),
        TypeRef::KeyOf(_)
        | TypeRef::IndexedAccess(..)
        | TypeRef::Mapped(_)
        | TypeRef::ObjectLiteral(_) => {
            if let Some(cgctx) = ctx {
                cgctx.warn(format!(
                    "unevaluated type `{}`, falling back to JsValue",
                    ty.format_ts()
                ));
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

/// A bare reference to an in-scope type parameter. Without a context (unit
/// tests) nothing is a type parameter.
fn is_type_param(ty: &TypeRef, ctx: Option<&CodegenContext<'_>>, scope: ScopeId) -> bool {
    match (ty.as_ident(), ctx) {
        (Some(name), Some(c)) => matches!(
            c.gctx.scopes.resolve_binding(scope, name),
            Some(crate::parse::scope::Binding::TypeParam)
        ),
        _ => false,
    }
}

/// `ty` without its empties and which it admits, with the scope the rest
/// resolves in; see [`TypeRef::split_empties`]. When the rest is an alias
/// admitting empties of its own (`MaybeStr | undefined` with `type MaybeStr
/// = string | null`), they merge, so each empty is admitted once:
/// `Option<Nullable<String>>`, never `Option<Option<..>>`.
pub(crate) fn split_empties_in(
    ty: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> Option<(TypeRef, ir::Nullability, ScopeId)> {
    let (core, n) = ty.split_empties()?;
    let alias = core
        .as_ident()
        .and_then(|name| ctx?.resolve_alias(name, scope));
    if let Some((target, target_scope)) = alias {
        if let Some((inner, m, s)) = split_empties_in(&target, ctx, target_scope) {
            return Some((inner, n.with(m), s));
        }
    }
    Some((core, n, scope))
}

/// The read carrier: `Option<T>` is `T | undefined`, `Nullable<T>` is
/// `T | null`, and `Option<Nullable<T>>` admits both.
pub(crate) fn nullable_carrier(inner: TokenStream, n: ir::Nullability) -> TokenStream {
    match n {
        ir::Nullability::Both => quote! { Option<Nullable<#inner>> },
        ir::Nullability::Null => quote! { Nullable<#inner> },
        ir::Nullability::Undefined => quote! { Option<#inner> },
    }
}

/// Inner-position nullability follows wasm-bindgen's `sys` split:
/// `JsOption<T>` is `T | undefined` only, so anything admitting `null`
/// is `JsNullable<T>` (`null | undefined`, `is_empty` covers both).
fn js_nullable_or_js_value(inner_ty: TokenStream, n: ir::Nullability) -> TokenStream {
    if is_jsvalue_arg(&inner_ty) {
        quote! { JsValue }
    } else if n.admits_null() {
        quote! { JsNullable<#inner_ty> }
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
/// `use_scope` is the scope the reference appears in, and the only one it
/// is resolved from. As in TypeScript, a name not visible there is not
/// found, even if another file or module declares it: a module's locals
/// are reachable only through an import, and a script's top-level
/// declarations are already in the global scope every chain ends in.
///
/// Going through the scope-resolved `TypeId` (rather than a flat name →
/// module map) keeps name collisions like a global `interface Foo` and a
/// namespace member `Foo` correctly distinguished: scope-chain resolution
/// from the use site picks the visible declaration, and that declaration's
/// scope drives qualification.
fn emit_type_name(name: &str, use_scope: ScopeId, ctx: &CodegenContext<'_>) -> TokenStream {
    let resolved = ctx.gctx.resolve_path(use_scope, name);

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
    if let Some(type_id) = resolved.filter(|id| ctx.local_type_ids.contains(id)) {
        let target_module = &ctx.gctx.get_type(type_id).module_context;
        // Externalised module: references resolve through the external
        // map to the user-supplied crate path.
        if let ModuleContext::Module(m) = target_module {
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
        if let ModuleContext::Module(m) = target_module {
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
        // The Rust ident is the declaration's own name: an import bound
        // through `export { _Foo as Foo }` is referred to as `Foo` but
        // emitted as `_Foo`.
        let declared = ctx
            .renamed_types
            .get(&type_id)
            .map(String::as_str)
            .or_else(|| type_decl_name(&ctx.gctx.get_type(type_id).kind))
            .unwrap_or(ident_name);
        let rust_name = ctx
            .renamed_locals
            .get(declared)
            .map(String::as_str)
            .unwrap_or(declared);
        let ident = make_ident(rust_name);
        return match ctx.namespace_qualifier(type_id) {
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
fn named_type_to_rust(name: &str, ctx: Option<&CodegenContext<'_>>, scope: ScopeId) -> TokenStream {
    match ctx {
        Some(ctx) => emit_type_name(name, scope, ctx),
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
            Some(c) => emit_type_name(&dotted, scope, c),
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
            if let Some(arg) = mono_promise_carrier(&inner, pos, ctx, scope, from_module) {
                return quote! { Promise<#arg> };
            }
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
                if let Some(arg) = c.param_arg(scope, head) {
                    return arg;
                }
                if let Some(mentions) = c.alias_param_mentions.borrow_mut().last_mut() {
                    mentions.insert(head.to_string());
                }
                let ident = make_ident(head);
                return quote! { #ident };
            }
        }
    }

    let base = named_type_to_rust(head, ctx, scope);
    with_generic_args(base, head, generic_args, pos, ctx, scope, from_module)
}

/// Under per-monomorphization, a top-level `Promise<T>` the caller receives
/// takes the carrier of a `T` admitting empties (`Promise<Nullable<JsString>>`),
/// as awaiting it converts through `JsFuture<T: FromWasmAbi>`. One the caller
/// passes, or nested in another type, keeps the `JsGeneric` handle that the
/// `js_sys` containers and constructors (`Promise::resolve`) require.
fn mono_promise_carrier(
    inner: &TypeRef,
    pos: TypePosition,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> Option<TokenStream> {
    if !CodegenContext::generic_mono(ctx) || pos.inner || pos.is_argument() {
        return None;
    }
    let (core, n, scope) = split_empties_in(inner, ctx, scope)?;
    let inner_ty = to_syn_type(&core, pos.to_inner(), ctx, scope, from_module);
    (!is_jsvalue_arg(&inner_ty)).then(|| nullable_carrier(inner_ty, n))
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
        if let Some(c) = ctx.filter(|_| !is_buffer_generic(name)) {
            c.warn(format!(
                "generic type arguments on `{name}<...>` are not emitted (target is not a \
                 locally-declared generic type), using bare `{name}`"
            ));
        }
        return base;
    }
    let generic_args = match ctx.and_then(|c| c.alias_args(name, scope, generic_args)) {
        Some(args) => args,
        None => generic_args.to_vec(),
    };
    if generic_args.is_empty() {
        return base;
    }
    let arg_tokens: Vec<TokenStream> = generic_args
        .iter()
        .map(|a| generic_arg_tokens(a, pos, ctx, scope, from_module))
        .collect();
    let arg_tokens = match ctx.and_then(|c| Some((c, c.gctx.resolve_path(scope, name)?))) {
        Some((c, id)) => c.trim_default_args(id, arg_tokens),
        None => arg_tokens,
    };
    if arg_tokens.is_empty() {
        return base;
    }
    quote! { #base<#(#arg_tokens),*> }
}

/// One generic argument of a locally declared generic type.
fn generic_arg_tokens(
    arg: &TypeRef,
    pos: TypePosition,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    if CodegenContext::generic_mono(ctx) {
        to_mono_user_generic_arg(arg, ctx, scope, from_module)
    } else {
        to_syn_type(arg, pos.to_inner(), ctx, scope, from_module)
    }
}

/// TS 5.7+ makes the typed arrays and `DataView` generic over their buffer
/// (`Uint8Array<ArrayBuffer>`). The js-sys types aren't, and dropping the
/// argument is the intended lowering, so it isn't warned about.
fn is_buffer_generic(name: &str) -> bool {
    name == "DataView" || ir::TypedArrayElement::of_typed_array(name).is_some()
}

/// `alias`'s target with each type parameter replaced by its default, in
/// order so a default can name an earlier parameter (`Cf = CfProperties<M>`).
/// A parameter without one, which TS only allows when the alias isn't named
/// bare, stands for `any`.
fn alias_target_with_defaults(alias: &ir::TypeAliasDecl) -> std::borrow::Cow<'_, TypeRef> {
    if alias.type_params.is_empty() {
        return std::borrow::Cow::Borrowed(&alias.target);
    }
    std::borrow::Cow::Owned(alias_instance(alias, &[]))
}

/// Create a `syn::Ident`, sanitizing invalid characters and escaping keywords.
pub(crate) fn make_ident(name: &str) -> syn::Ident {
    // Strip characters that aren't valid in Rust identifiers
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    // Value names arrive snake-cased, already spelled out by `to_snake_case`,
    // so a name with nothing left here is a type declaration or reference.
    let sanitized = if !sanitized.chars().any(char::is_alphanumeric) {
        match crate::util::naming::symbol_words(name) {
            Some(_) => crate::util::naming::to_pascal_case(name),
            None => "__unknown__".to_string(),
        }
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
/// The awaited value of an `async fn` import crosses the boundary through
/// `JsFuture<T>`'s resolve closure, whose argument only needs
/// `FromWasmAbi`, so async returns lower exactly like sync ones: bare
/// primitives, the read carriers (`Option<T>`, `Nullable<T>`,
/// `Option<Nullable<T>>`), `Vec<T>` and synthesized return unions. A list
/// over a type parameter is the exception (see [`is_generic_rust_list`]).
/// Only the un-awaited `Promise<T>` form keeps the inside-generic lowering.
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
    // `js_string_return` is only set by `build_signatures` in per-mono mode,
    // which implies a context; without one, fall through to the plain lowering.
    let js_string_ctx = ctx.filter(|_| js_string_return);
    let generic_list = is_async && ctx.is_some_and(|c| is_generic_rust_list(ty, c, scope));
    let inner = if let Some(ctx) = js_string_ctx {
        to_mono_js_string_return_type(ty, ctx, scope, from_module)
    } else if generic_list {
        // The list keeps its `Array<T>` handle; the awaited value's own
        // empties still take the read carriers around it.
        match split_empties_in(ty, ctx, scope) {
            Some((core, n, scope)) => nullable_carrier(
                to_syn_type(
                    &core,
                    TypePosition::RETURN.to_inner(),
                    ctx,
                    scope,
                    from_module,
                ),
                n,
            ),
            None => to_syn_type(ty, TypePosition::RETURN.to_inner(), ctx, scope, from_module),
        }
    } else {
        match maybe_synthesise_return_union(ty, ctx, scope, from_module, anchor) {
            Some(tokens) => tokens,
            None => to_syn_type(ty, TypePosition::RETURN, ctx, scope, from_module),
        }
    };

    if catch {
        let err = error_type_tokens(error_ty, ctx, scope, from_module);
        quote! { Result<#inner, #err> }
    } else {
        inner
    }
}

/// The `E` of a fallible binding's `Result<T, E>`: the `@throws` type
/// when one is given (or the `--error-enum` that stands in for every
/// `@throws` type), else the configured default.
pub fn error_type_tokens(
    error_ty: Option<&TypeRef>,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    match error_ty {
        Some(_) if ctx.is_some_and(|c| c.error_enum.is_some()) => {
            let name = ctx
                .and_then(|c| c.error_enum.as_deref())
                .unwrap_or_default();
            let ident = make_ident(name);
            quote! { #ident }
        }
        Some(ty) => to_syn_type(ty, TypePosition::RETURN, ctx, scope, from_module),
        None => default_error_type(ctx),
    }
}

/// Whether an awaited value is a list (after its empties) whose element
/// mentions an in-scope type parameter. A sync import erases such a `T`
/// to its concrete representation, but an `async` import converts the
/// awaited value through `<Vec<T> as FromWasmAbi>`, which `T: JsGeneric`
/// does not supply, so it keeps the `Array<T>` handle.
fn is_generic_rust_list(ty: &TypeRef, ctx: &CodegenContext<'_>, scope: ScopeId) -> bool {
    let (core, scope) = match split_empties_in(ty, Some(ctx), scope) {
        Some((core, _, scope)) => (core, scope),
        None => (ty.clone(), scope),
    };
    let TypeRef::Array(element) = &core else {
        return false;
    };
    let mut names = Vec::new();
    crate::codegen::signatures::collect_type_params(element, ctx, scope, &mut names);
    !names.is_empty()
}

/// Per-monomorphization walkers.
///
/// Each walker first calls [`peel_mono`], which resolves aliases and unions
/// that reduce to one common type. The walkers then only match on primitive
/// leaves, `Nullable`, and locally declared generic instantiations, so
/// `Holder<Id>`, `Holder<"a" | "b">` and `Holder<string>` all get the same
/// answer from every walker. The peeled type comes with the scope its names
/// resolve in, which is the last alias's own scope once one is chased.
///
/// `visited` holds the aliases already resolved on the current path, so a
/// self-referential alias ends the walk. When a walker descends into several
/// generic arguments, it gives each argument its own copy of `visited`, so
/// `Pair<Id, Id>` resolves `Id` both times.
pub(crate) fn peel_mono<'t>(
    ty: &'t TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    visited: &mut HashSet<TypeId>,
) -> (std::borrow::Cow<'t, TypeRef>, ScopeId) {
    let mut current = std::borrow::Cow::Borrowed(ty);
    let mut scope = scope;
    loop {
        let next = match current.as_ref() {
            TypeRef::Reference {
                segments,
                generic_args,
            } if segments.len() == 1 && generic_args.is_empty() => {
                match ctx.gctx.scopes.resolve(scope, &segments[0]) {
                    Some(id) if !visited.insert(id) => break,
                    _ => {}
                }
                ctx.resolve_alias(&segments[0], scope)
                    .map(|(target, alias_scope)| (target.into_owned(), alias_scope))
            }
            // A union admitting an empty stops here, for the walker to
            // take its carrier.
            TypeRef::Union(u) if u.nullability().is_none() => {
                crate::codegen::subtyping::lub_union(u.members(), Some(ctx), scope)
                    .filter(|lub| !matches!(lub, TypeRef::Union(_)))
                    .map(|lub| (lub, scope))
            }
            _ => None,
        };
        match next {
            Some((next, next_scope)) => {
                current = std::borrow::Cow::Owned(next);
                scope = next_scope;
            }
            None => break,
        }
    }
    (current, scope)
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
) -> Option<std::borrow::Cow<'t, [TypeRef]>> {
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } if !generic_args.is_empty() && ctx.is_local_generic(&segments.join("."), scope) => Some(
            match ctx.alias_args(&segments.join("."), scope, generic_args) {
                Some(args) => std::borrow::Cow::Owned(args),
                None => std::borrow::Cow::Borrowed(generic_args),
            },
        ),
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
    visited: &mut HashSet<TypeId>,
) -> TokenStream {
    let (peeled, peeled_scope) = peel_mono(ty, ctx, scope, visited);
    match peeled.as_ref() {
        t if is_mono_native_leaf(t, ctx, peeled_scope) => to_syn_type(
            t,
            TypePosition::RETURN,
            Some(ctx),
            peeled_scope,
            from_module,
        ),
        t if let Some((inner, n, s)) = split_empties_in(t, Some(ctx), peeled_scope) => {
            let inner = to_mono_user_generic_arg_impl(&inner, ctx, s, from_module, visited);
            nullable_carrier(inner, n)
        }
        t if local_generic_args(t, ctx, peeled_scope).is_some() => to_syn_type(
            t,
            TypePosition::RETURN,
            Some(ctx),
            peeled_scope,
            from_module,
        ),
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
    // leaf to swap for `JsString`. Getters synthesise under a carrier too,
    // so both shapes are excluded.
    let top = ty
        .split_empties()
        .map_or_else(|| ty.clone(), |(core, _)| core);
    if let TypeRef::Union(u) = &top {
        if dynamic_union_members(u.members(), Some(ctx), scope).is_some() {
            return false;
        }
    }
    has_mono_js_string_return_impl(ty, ctx, scope, &mut HashSet::new())
}

fn has_mono_js_string_return_impl(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    visited: &mut HashSet<TypeId>,
) -> bool {
    let (peeled, peeled_scope) = peel_mono(ty, ctx, scope, visited);
    match peeled.as_ref() {
        TypeRef::String | TypeRef::StringLiteral(_) => true,
        t if let Some((inner, _, s)) = split_empties_in(t, Some(ctx), peeled_scope) => {
            has_mono_js_string_return_impl(&inner, ctx, s, visited)
        }
        t => local_generic_args(t, ctx, peeled_scope).is_some_and(|args| {
            args.iter().any(|arg| {
                has_mono_js_string_return_impl(arg, ctx, peeled_scope, &mut visited.clone())
            })
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
    visited: &mut HashSet<TypeId>,
) -> TokenStream {
    let (peeled, peeled_scope) = peel_mono(ty, ctx, scope, visited);
    match peeled.as_ref() {
        TypeRef::String | TypeRef::StringLiteral(_) => quote! { JsString },
        TypeRef::Array(elem) if is_string_array_element(elem, ctx, peeled_scope) => {
            quote! { Array<JsString> }
        }
        t if let Some((inner, n, s)) = split_empties_in(t, Some(ctx), peeled_scope) => {
            let inner = to_mono_js_string_return_type_impl(&inner, ctx, s, from_module, visited);
            nullable_carrier(inner, n)
        }
        t @ TypeRef::Reference { segments, .. }
            if local_generic_args(t, ctx, peeled_scope).is_some() =>
        {
            let base = named_type_to_rust(&segments.join("."), Some(ctx), peeled_scope);
            let args = local_generic_args(t, ctx, peeled_scope).unwrap_or_default();
            if args.is_empty() {
                return base;
            }
            let args = args
                .iter()
                .map(|arg| {
                    let mut visited = visited.clone();
                    if has_mono_js_string_return_impl(arg, ctx, peeled_scope, &mut visited.clone())
                    {
                        to_mono_js_string_return_type_impl(
                            arg,
                            ctx,
                            peeled_scope,
                            from_module,
                            &mut visited,
                        )
                    } else {
                        to_mono_user_generic_arg_impl(
                            arg,
                            ctx,
                            peeled_scope,
                            from_module,
                            &mut visited,
                        )
                    }
                })
                .collect::<Vec<_>>();
            let args = match ctx.gctx.resolve_path(peeled_scope, &segments.join(".")) {
                Some(id) => ctx.trim_default_args(id, args),
                None => args,
            };
            if args.is_empty() {
                return base;
            }
            quote! { #base<#(#args),*> }
        }
        _ => to_syn_type(ty, TypePosition::RETURN, Some(ctx), scope, from_module),
    }
}

/// The default error type used when a fallible binding has no
/// `@throws` annotation: `JsValue` unless
/// [`GenerateOptions::error_type`][1] names a path.
///
/// [1]: crate::codegen::GenerateOptions::error_type
fn default_error_type(ctx: Option<&CodegenContext<'_>>) -> TokenStream {
    match ctx.and_then(|c| c.error_type.as_ref()) {
        Some(ty) => quote! { #ty },
        None => quote! { JsValue },
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
    // A union admitting empties still synthesises the enum for its other
    // members, wrapped in its carrier.
    if let Some((core, n)) = ty.split_empties() {
        if let Some(inner_tokens) =
            maybe_synthesise_return_union(&core, ctx, scope, from_module, anchor)
        {
            return nullable_carrier(inner_tokens, n);
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
        TypeRef::Union(u) if u.nullability().is_none() => u.members(),
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
            TypeRef::Union(u) => {
                let mut out: Vec<TypeRef> = Vec::with_capacity(u.members().len());
                let mut placed = false;
                for m in u.members() {
                    if self.variant(m).is_none() {
                        out.push(m.clone());
                    } else if !placed {
                        out.push(TypeRef::ident(self.name.clone()));
                        placed = true;
                    }
                }
                TypeRef::union(out)
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
    let core = ty
        .split_empties()
        .map_or_else(|| ty.clone(), |(core, _)| core);
    let TypeRef::Union(u) = &core else {
        return None;
    };
    let members = dynamic_union_members(u.members(), ctx, scope)?;
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

/// The non-default wasm-bindgen slice mode for a parameter, emitted as a
/// per-argument `#[wasm_bindgen(slice = ..)]` attribute, or `None` for the
/// default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliceMode {
    /// A typed-array view of wasm linear memory — zero-copy, and sound
    /// only when the callee neither retains the buffer past the call nor
    /// re-enters wasm. Never inferred; only a `{@slice view}` JSDoc hint
    /// produces it, and it is emitted explicitly so the assertion is
    /// greppable in generated code.
    View,
    /// [`Self::View`] of a typed array whose callee rejects a view of shared
    /// memory ([`crate::ir::SharedMemory::Rejected`]). Atomics builds use
    /// shared memory, so the function is emitted twice: a view without
    /// `target_feature = "atomics"` and a copy with it.
    UnsharedView,
    /// A fresh plain `Array`. A `T[]` parameter is a plain JS `Array` the
    /// callee may keep, so a slice of numbers takes this rather than
    /// arriving as a `Float64Array` view of linear memory.
    Array,
    /// A fresh typed array (`Uint8Array`, `BigInt64Array`, …). A typed
    /// slice from a `TypedArray | number[]` union takes this, so the
    /// callee owns the array outright rather than a view that dangles if
    /// it's retained or memory grows during the call.
    Copy,
}

/// The slice mode for this parameter, or `None` when the default is
/// right: modes only act on numeric elements, every other slice already
/// arrives as a fresh `Array`, and a variadic rest lowers to `&[JsValue]`
/// whatever its element. The attribute goes on the argument rather than
/// the function: a fn-level mode is inherited by every slice parameter,
/// which would also turn a `TypedSlice`'s typed copy into a plain `Array`.
pub fn param_slice_mode(
    p: &crate::codegen::signatures::ConcreteParam,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> Option<SliceMode> {
    if p.variadic {
        return None;
    }
    let core = p.type_ref.split_empties().map(|(core, _)| core);
    let ty = core.as_ref().unwrap_or(&p.type_ref);
    let inferred = match ty {
        TypeRef::Array(elem)
            if is_numeric_array_element(elem, ctx, scope)
                || is_mono_type_param(elem, ctx, scope) =>
        {
            Some(SliceMode::Array)
        }
        TypeRef::TypedSlice(..) => Some(SliceMode::Copy),
        _ => None,
    };
    // A `{@slice mode}` JSDoc hint overrides the inferred mode when the
    // lowering is slice-shaped, and is dropped by shape otherwise — the
    // same rule wasm-bindgen applies to an inherited mode, which is what
    // lets one hint cover a union whose other alternatives (the
    // typed-array handle overload) aren't slices.
    match (p.slice_mode, inferred) {
        (Some(hint), Some(_)) => Some(match hint {
            crate::ir::SliceModeHint::View
                if matches!(
                    ty,
                    TypeRef::TypedSlice(_, crate::ir::SharedMemory::Rejected)
                ) =>
            {
                SliceMode::UnsharedView
            }
            crate::ir::SliceModeHint::View => SliceMode::View,
            crate::ir::SliceModeHint::Array => SliceMode::Array,
            crate::ir::SliceModeHint::Copy => SliceMode::Copy,
        }),
        (_, inferred) => inferred,
    }
}

/// Whether `ty` is a type parameter that per-monomorphization keeps as a
/// bare `T`. wasm-bindgen resolves a slice mode on `&[T]` per
/// instantiation, so `slice = "array"` turns `T = f64`'s view into an
/// `Array` and is a no-op for every non-numeric `T`. Erased `T: JsGeneric`
/// is always a handle and needs no mode.
fn is_mono_type_param(ty: &TypeRef, ctx: Option<&CodegenContext<'_>>, scope: ScopeId) -> bool {
    let Some(c) = ctx.filter(|c| c.experimental_generic_mono) else {
        return false;
    };
    matches!(
        ty,
        TypeRef::Reference { segments, generic_args }
            if segments.len() == 1
                && generic_args.is_empty()
                && matches!(
                    c.gctx.scopes.resolve_binding(scope, &segments[0]),
                    Some(crate::parse::scope::Binding::TypeParam),
                )
    )
}

/// Whether a top-level `T[]` element keeps its Rust-native form
/// (`String` / `f64`) inside the slice / `Vec`, rather than its handle
/// form. Enums count as the primitive they carry.
fn is_native_array_element(
    elem: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> bool {
    matches!(
        native_array_leaf(elem, ctx, scope).as_ref(),
        TypeRef::String
            | TypeRef::StringLiteral(_)
            | TypeRef::Number
            | TypeRef::NumberLiteral(_)
            | TypeRef::Boolean
            | TypeRef::BooleanLiteral(_)
    )
}

/// Whether a top-level `T[]` element lowers to `String`.
fn is_string_array_element(elem: &TypeRef, ctx: &CodegenContext<'_>, scope: ScopeId) -> bool {
    matches!(
        native_array_leaf(elem, Some(ctx), scope).as_ref(),
        TypeRef::String | TypeRef::StringLiteral(_)
    )
}

/// Per-monomorphization string leaves of a slice or built-in container
/// argument. Every string leaf reached through slices and built-in containers
/// takes `impl JsStringLike`, which wasm-bindgen lowers per instantiation:
/// `string[]` accepts `&[String]`, `&[&str]` or `&[JsString]`, and
/// `Set<string>` accepts `&Set<JsString>` or, under `js_sys_unstable_apis`,
/// `&Set<String>`. `None` when there is no such leaf; a scalar string is
/// handled by `signatures::mono_argument_type`.
pub(crate) fn mono_string_like_argument(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> Option<TokenStream> {
    mono_string_like(ty, true, ctx, scope, from_module)
}

fn mono_string_like(
    ty: &TypeRef,
    top: bool,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> Option<TokenStream> {
    if !top && is_string_array_element(ty, ctx, scope) {
        return Some(quote! { impl ::wasm_bindgen::JsStringLike });
    }
    let borrow = top.then(|| quote! { & });
    let (peeled, scope) = peel_mono(ty, ctx, scope, &mut HashSet::new());
    match peeled.as_ref() {
        TypeRef::Array(elem) => {
            let elem = mono_string_like(elem, false, ctx, scope, from_module)?;
            Some(if top {
                quote! { &[#elem] }
            } else {
                quote! { Array<#elem> }
            })
        }
        TypeRef::Reference {
            segments,
            generic_args,
        } if segments.len() == 1 => {
            let (base, arity) = match segments[0].as_str() {
                "ArrayLike" => (quote! { Array }, 1),
                "Promise" | "PromiseLike" => (quote! { Promise }, 1),
                "Set" | "ReadonlySet" => (quote! { Set }, 1),
                "Map" | "ReadonlyMap" => (quote! { Map }, 2),
                "Iterator" | "IterableIterator" => (quote! { Iterator }, 1),
                "AsyncIterator" | "AsyncIterableIterator" => (quote! { AsyncIterator }, 1),
                _ => return None,
            };
            if generic_args.len() < arity {
                return None;
            }
            let args = &generic_args[..arity];
            let leaves: Vec<_> = args
                .iter()
                .map(|arg| mono_string_like(arg, false, ctx, scope, from_module))
                .collect();
            if leaves.iter().all(Option::is_none) {
                return None;
            }
            let args = args.iter().zip(leaves).map(|(arg, leaf)| {
                leaf.unwrap_or_else(|| {
                    to_syn_type(
                        arg,
                        TypePosition::ARGUMENT.to_inner(),
                        Some(ctx),
                        scope,
                        from_module,
                    )
                })
            });
            Some(quote! { #borrow #base<#(#args),*> })
        }
        _ => None,
    }
}

/// Whether a return (sync or awaited) is a string list, which has both a
/// list-of-elements form (`Vec<String>`) and a list-as-handle form
/// (`Array<JsString>`), so it gets a `_js_string` variant like a string
/// return.
pub(crate) fn is_mono_string_list_return(
    ty: &TypeRef,
    ctx: &CodegenContext<'_>,
    scope: ScopeId,
) -> bool {
    let (list, scope) = match split_empties_in(ty, Some(ctx), scope) {
        Some((inner, _, scope)) => (inner, scope),
        None => (ty.clone(), scope),
    };
    let (peeled, scope) = peel_mono(&list, ctx, scope, &mut HashSet::new());
    matches!(
        peeled.as_ref(),
        TypeRef::Array(elem) if is_string_array_element(elem, ctx, scope)
    )
}

/// Whether a top-level `T[]` element lowers to `f64`.
fn is_numeric_array_element(
    elem: &TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> bool {
    matches!(
        native_array_leaf(elem, ctx, scope).as_ref(),
        TypeRef::Number | TypeRef::NumberLiteral(_)
    )
}

fn native_array_leaf<'t>(
    elem: &'t TypeRef,
    ctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> std::borrow::Cow<'t, TypeRef> {
    match ctx {
        Some(c) => match c.enum_repr(elem, scope) {
            Some(repr) => std::borrow::Cow::Owned(repr),
            None => peel_mono(elem, c, scope, &mut HashSet::new()).0,
        },
        None => std::borrow::Cow::Borrowed(elem),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ir::Nullability;
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
        let union = |members: Vec<TypeRef>| TypeRef::union(members);
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
            e.param_type(&union(vec![x.clone(), y]).with_empties(Nullability::Undefined)),
            TypeRef::ident("ModeKind").with_empties(Nullability::Undefined)
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
    fn make_ident_spells_out_symbol_only_type_names() {
        assert_eq!(make_ident("$$").to_string(), "DollarDollar");
        assert_eq!(make_ident("_").to_string(), "Underscore");
        assert_eq!(make_ident("☃").to_string(), "__unknown__");
    }

    #[test]
    fn alias_target_with_defaults_substitutes_in_order() {
        let ident = |name: &str| TypeRef::Reference {
            segments: vec![name.to_string()],
            generic_args: vec![],
        };
        let param = |name: &str, default: Option<TypeRef>| ir::TypeParam {
            name: name.to_string(),
            constraint: None,
            default,
        };
        // `<T, U = T[], V>`: `U` sees `T`'s default, `V` has none.
        let alias = ir::TypeAliasDecl {
            name: "A".to_string(),
            type_params: vec![
                param("T", Some(TypeRef::String)),
                param("U", Some(TypeRef::Array(Box::new(ident("T"))))),
                param("V", None),
            ],
            target: TypeRef::union([ident("U"), ident("V"), ident("W")]),
            from_module: None,
            body_scope: ScopeId(0),
        };
        assert_eq!(
            *alias_target_with_defaults(&alias),
            TypeRef::union([
                TypeRef::Array(Box::new(TypeRef::String)),
                TypeRef::Any,
                ident("W"),
            ])
        );
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
        let ty = TypeRef::String.with_empties(Nullability::Null);
        assert_eq!(ret_type(&ty), "Nullable < String >");
    }

    #[test]
    fn test_promise_with_named_type_unresolved() {
        // Without ctx, Foo is unresolved → JsValue, so Promise<JsValue> elides to Promise
        let ty = TypeRef::generic("Promise", vec![TypeRef::ident("Foo")]);
        assert_eq!(ret_type(&ty), "Promise");
    }

    #[test]
    fn test_nullable_inner() {
        let ty = TypeRef::String.with_empties(Nullability::Null);
        assert_eq!(inner_type(&ty), "JsNullable < JsString >");
        let ty = TypeRef::String.with_empties(Nullability::Both);
        assert_eq!(inner_type(&ty), "JsNullable < JsString >");
        // Undefined-only is wasm-bindgen's JsOption.
        let ty = TypeRef::String.with_empties(Nullability::Undefined);
        assert_eq!(inner_type(&ty), "JsOption < JsString >");
    }

    #[test]
    fn test_nullable_inner_jsvalue_collapses() {
        let ty = TypeRef::Any.with_empties(Nullability::Null);
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
        // Without ctx, `Foo` is unresolved → `JsValue`, which carries
        // `null` in-band: a `T | null` argument is `&JsValue` (pass
        // `JsValue::NULL`), a return collapses to bare `JsValue`.
        let ty = TypeRef::ident("Foo").with_empties(Nullability::Null);
        assert_eq!(arg_type(&ty), "& JsValue");
        assert_eq!(ret_type(&ty), "JsValue");
        // `T | undefined` keeps `Option` so `None` can cross as `undefined`.
        let ty = TypeRef::ident("Foo").with_empties(Nullability::Undefined);
        assert_eq!(arg_type(&ty), "Option < & JsValue >");
    }

    #[test]
    fn argument_nullability_picks_the_absent_value() {
        let nullable = |n| TypeRef::String.with_empties(n);
        assert_eq!(
            arg_type(&nullable(Nullability::Undefined)),
            "Option < & str >"
        );
        assert_eq!(arg_type(&nullable(Nullability::Null)), "Nullable < & str >");
        assert_eq!(
            arg_type(&nullable(Nullability::Both)),
            "Option < Nullable < & str >>"
        );
        let num = TypeRef::Number.with_empties(Nullability::Null);
        assert_eq!(arg_type(&num), "Nullable < f64 >");
        assert_eq!(
            ret_type(&nullable(Nullability::Undefined)),
            "Option < String >"
        );
        assert_eq!(
            ret_type(&nullable(Nullability::Null)),
            "Nullable < String >"
        );
        assert_eq!(
            ret_type(&nullable(Nullability::Both)),
            "Option < Nullable < String >>"
        );
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
            slice_mode: None,
        }
    }

    #[test]
    fn array_mode_follows_the_lowered_element() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let mode = |elem: &TypeRef| {
                param_slice_mode(
                    &cp("xs", TypeRef::Array(Box::new(elem.clone()))),
                    Some(ctx),
                    scope,
                )
            };
            let numbers =
                TypeRef::union([TypeRef::NumberLiteral(1.0), TypeRef::NumberLiteral(2.0)]);
            for elem in [TypeRef::Number, TypeRef::ident("Level"), numbers] {
                assert_eq!(mode(&elem), Some(SliceMode::Array), "for {elem:?}");
            }
            for elem in [
                TypeRef::String,
                TypeRef::ident("IdChain"),
                TypeRef::ident("Mode"),
                lits(&["a", "b"]),
                TypeRef::Boolean,
                TypeRef::BigInt,
                TypeRef::Any,
                TypeRef::ident("Plain"),
                TypeRef::ident("T"),
                TypeRef::ident("MaybeId"),
                TypeRef::Array(Box::new(TypeRef::String)),
            ] {
                assert_eq!(mode(&elem), None, "for {elem:?}");
            }
        });
    }

    #[test]
    fn mono_type_param_slices_take_array_mode() {
        let src = "declare class Holder<T> { constructor(value: T); }";
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let scope = *module.file_scopes.first().expect("file scope");
        let TypeKind::Class(holder) = &gctx
            .get_type(gctx.resolve_path(scope, "Holder").unwrap())
            .kind
        else {
            panic!("expected class");
        };
        let ts = TypeRef::Array(Box::new(TypeRef::ident("T")));
        let modes = |mono: bool| {
            let options = GenerateOptions::new().experimental_generic_mono(mono);
            let ctx = CodegenContext::from_module_full(&module, &gctx, &options, None).unwrap();
            [
                param_slice_mode(&cp("xs", ts.clone()), Some(&ctx), holder.body_scope),
                param_slice_mode(
                    &cp("xs", ts.clone().with_empties(Nullability::Undefined)),
                    Some(&ctx),
                    holder.body_scope,
                ),
                // Outside its declaring scope `T` is no type parameter.
                param_slice_mode(&cp("xs", ts.clone()), Some(&ctx), scope),
            ]
        };
        assert_eq!(
            modes(true),
            [Some(SliceMode::Array), Some(SliceMode::Array), None]
        );
        assert_eq!(modes(false), [None, None, None]);
    }

    #[test]
    fn slice_mode_param_shapes() {
        use crate::ir::TypedArrayElement as E;
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let mode = |p: &crate::codegen::signatures::ConcreteParam| {
                param_slice_mode(p, Some(ctx), scope)
            };
            let numbers = || TypeRef::Array(Box::new(TypeRef::Number));
            let typed = || TypeRef::TypedSlice(E::U8, crate::ir::SharedMemory::Accepted);
            assert_eq!(
                mode(&cp("xs", numbers().with_empties(Nullability::Undefined))),
                Some(SliceMode::Array)
            );
            for n in [Nullability::Null, Nullability::Both] {
                assert_eq!(
                    mode(&cp("xs", numbers().with_empties(n))),
                    Some(SliceMode::Array)
                );
            }
            assert_eq!(mode(&cp("xs", numbers())), Some(SliceMode::Array));
            assert_eq!(mode(&cp("xs", typed())), Some(SliceMode::Copy));
            assert_eq!(
                mode(&cp("xs", typed().with_empties(Nullability::Undefined))),
                Some(SliceMode::Copy)
            );
            assert_eq!(mode(&cp("first", TypeRef::ident("Plain"))), None);
            assert_eq!(mode(&cp("x", TypeRef::String)), None);
            assert_eq!(mode(&cp("xs", TypeRef::ident("Uint8Array"))), None);
            let mut rest = cp("rest", numbers());
            rest.variadic = true;
            assert_eq!(mode(&rest), None);
            let mut typed_rest = cp("rest", typed());
            typed_rest.variadic = true;
            assert_eq!(mode(&typed_rest), None);

            // A `{@slice mode}` hint overrides where the lowering is
            // slice-shaped, and is dropped by shape otherwise.
            use crate::ir::SliceModeHint as H;
            let hinted = |ty: TypeRef, h: H| {
                let mut p = cp("xs", ty);
                p.slice_mode = Some(h);
                p
            };
            assert_eq!(mode(&hinted(typed(), H::View)), Some(SliceMode::View));
            let unshared = TypeRef::TypedSlice(E::U8, crate::ir::SharedMemory::Rejected);
            assert_eq!(
                mode(&hinted(unshared.clone(), H::View)),
                Some(SliceMode::UnsharedView)
            );
            assert_eq!(
                mode(&hinted(
                    unshared.clone().with_empties(Nullability::Undefined),
                    H::View
                )),
                Some(SliceMode::UnsharedView)
            );
            assert_eq!(
                mode(&hinted(unshared.clone(), H::Copy)),
                Some(SliceMode::Copy)
            );
            assert_eq!(mode(&cp("xs", unshared)), Some(SliceMode::Copy));
            assert_eq!(mode(&hinted(typed(), H::Array)), Some(SliceMode::Array));
            assert_eq!(mode(&hinted(numbers(), H::View)), Some(SliceMode::View));
            // A string slice has no mode to select (its only shape is a
            // fresh `Array`), so a hint on it is dropped by shape.
            let string_slice = TypeRef::Array(Box::new(TypeRef::String));
            assert_eq!(mode(&hinted(string_slice, H::View)), None);
            assert_eq!(mode(&hinted(TypeRef::ident("Plain"), H::View)), None);
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
                TypeRef::union([TypeRef::String, TypeRef::Number, TypeRef::Boolean])
                    .with_empties(Nullability::Null),
            ],
        );
        assert_eq!(ret_type(&ty), "Object");
    }

    #[test]
    fn test_promise_nullable_inner() {
        // Promise<string | null> → Promise<JsNullable<JsString>>
        let ty = TypeRef::generic(
            "Promise",
            vec![TypeRef::String.with_empties(Nullability::Null)],
        );
        let result = ret_type(&ty);
        assert_eq!(result, "Promise < JsNullable < JsString > >");
    }

    #[test]
    fn test_promise_nullable_jsvalue_elides_generic() {
        let ty = TypeRef::generic(
            "Promise",
            vec![TypeRef::Any.with_empties(Nullability::Null)],
        );
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
                slice_mode: None,
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
                slice_mode: None,
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
        let ty = TypeRef::union([TypeRef::String, TypeRef::Number]);
        // Unions erase to JsValue, but in argument position they're borrowed
        assert_eq!(arg_type(&ty), "& JsValue");
        assert_eq!(ret_type(&ty), "JsValue");
    }

    fn test_gctx() -> (GlobalContext, ScopeId) {
        let mut gctx = GlobalContext::new();
        let scope = gctx.create_root_scope();
        (gctx, scope)
    }

    /// Lower `name` as a reference written in `scope`, at the top level of
    /// the output, returning the tokens and the use aliases registered.
    fn lower_name(
        ctx: &CodegenContext<'_>,
        name: &str,
        pos: TypePosition,
        scope: ScopeId,
    ) -> (String, HashMap<String, String>) {
        let tokens = to_syn_type(
            &TypeRef::ident(name),
            pos,
            Some(ctx),
            scope,
            &ModuleContext::Global,
        )
        .to_string();
        (tokens, ctx.external_uses.borrow().clone())
    }

    /// The scope the members of the interface named `name` resolve from.
    fn body_scope(gctx: &GlobalContext, name: &str) -> ScopeId {
        gctx.iter_types()
            .find_map(|(_, d)| match &d.kind {
                TypeKind::Interface(i) if i.name == name => Some(i.body_scope),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no interface {name}"))
    }

    #[test]
    fn test_local_type_overrides_web_sys() {
        let (module, gctx) =
            crate::parse_source("interface Response { ok: boolean; }", None).unwrap();
        let ctx = CodegenContext::from_module(&module, &gctx);
        let (tokens, uses) = lower_name(
            &ctx,
            "Response",
            TypePosition::RETURN,
            module.file_scopes[0],
        );
        assert_eq!(tokens, "Response");
        assert!(uses.is_empty(), "{uses:?}");
    }

    #[test]
    fn test_union_alias_resolves_to_jsvalue() {
        // A type alias to a union resolves through the scope and erases to JsValue.
        let (mut gctx, scope) = test_gctx();
        let alias_id = gctx.insert_type(crate::ir::TypeDeclaration {
            kind: crate::ir::TypeKind::TypeAlias(crate::ir::TypeAliasDecl {
                name: "BodyInit".to_string(),
                type_params: vec![],
                target: TypeRef::union([TypeRef::String, TypeRef::ident("ArrayBuffer")]),
                from_module: None,
                body_scope: scope,
            }),
            module_context: crate::ir::ModuleContext::Global,
            doc: None,
            scope_id: scope,
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
        let (module, gctx) = crate::parse_source("interface MyThing { a: number; }", None).unwrap();
        let ctx = CodegenContext::from_module(&module, &gctx);
        let ty = TypeRef::generic("Promise", vec![TypeRef::ident("MyThing")]);
        let result = to_syn_type(
            &ty,
            TypePosition::RETURN,
            Some(&ctx),
            module.file_scopes[0],
            &ModuleContext::Global,
        )
        .to_string();
        assert_eq!(result, "Promise < MyThing >");
        assert!(ctx.external_uses.borrow().is_empty());
    }

    /// Parse `files` as separate inputs, which, unlike a single source,
    /// also resolves their imports.
    fn parse_files(
        tag: &str,
        files: &[(&str, &str)],
        lib_name: Option<&str>,
    ) -> (crate::ir::Module, GlobalContext) {
        let dir = std::env::temp_dir().join(format!("ts-gen-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<_> = files
            .iter()
            .map(|(name, src)| {
                let path = dir.join(name);
                std::fs::write(&path, src).unwrap();
                path
            })
            .collect();
        let result = crate::parse::parse_dts_files(&paths, lib_name);
        std::fs::remove_dir_all(&dir).unwrap();
        result.unwrap()
    }

    const EMAIL_SRC: &str = r#"
        declare module "cloudflare:email" {
            export class EmailMessage {}
        }
        declare module "user" {
            import { EmailMessage } from "cloudflare:email";
            export interface User { message: EmailMessage; }
        }
    "#;

    #[test]
    fn test_declared_module_is_qualified_from_global() {
        // A declared module is emitted under its specifier's `pub mod`
        // path, so a reference emitted at the top level is qualified.
        let (module, gctx) = parse_files("email", &[("email.d.ts", EMAIL_SRC)], None);
        let mut ctx = CodegenContext::from_module(&module, &gctx);
        ctx.exported_modules.insert("cloudflare:email".to_string());
        let scope = body_scope(&gctx, "User");
        let (tokens, _) = lower_name(&ctx, "EmailMessage", TypePosition::ARGUMENT, scope);
        assert_eq!(tokens, "& cloudflare :: email :: EmailMessage");

        // The `--lib-name` module is the output's own, at the top level.
        let (module, gctx) = parse_files(
            "email-lib",
            &[("email.d.ts", EMAIL_SRC)],
            Some("cloudflare:email"),
        );
        let ctx = CodegenContext::from_module(&module, &gctx);
        let scope = body_scope(&gctx, "User");
        let (tokens, _) = lower_name(&ctx, "EmailMessage", TypePosition::ARGUMENT, scope);
        assert_eq!(tokens, "& EmailMessage");
    }

    #[test]
    fn test_global_to_global_is_bare() {
        let (module, gctx) =
            crate::parse_source("interface EmailSendResult { id: string; }", None).unwrap();
        let ctx = CodegenContext::from_module(&module, &gctx);
        let (tokens, uses) = lower_name(
            &ctx,
            "EmailSendResult",
            TypePosition::RETURN,
            module.file_scopes[0],
        );
        assert_eq!(tokens, "EmailSendResult");
        assert!(uses.is_empty(), "{uses:?}");
    }

    fn assert_unresolved(ctx: &CodegenContext<'_>, name: &str, scope: ScopeId) {
        let (tokens, uses) = lower_name(ctx, name, TypePosition::RETURN, scope);
        assert_eq!(tokens, name);
        assert_eq!(uses.get(name).map(String::as_str), Some("JsValue"));
        let unresolved = format!("Unresolved type `{name}`");
        assert!(
            ctx.diagnostics
                .borrow()
                .diagnostics
                .iter()
                .any(|d| d.message.starts_with(&unresolved)),
            "no `{unresolved}` error"
        );
    }

    #[test]
    fn names_resolve_only_from_the_use_site() {
        // A name another file or module declares but the use site neither
        // sees nor imports is not found, as TypeScript's "Cannot find
        // name": a block's unexported declaration from the module's other
        // block, a module's locals from another module, and a module's
        // exports from another module or a script.
        let src = r#"
            declare module "a" {
                interface Secret { a: number; }
                export {};
            }
            declare module "a" {
                export interface InA { s: Secret; }
            }
            declare module "b" {
                export interface InB { s: Secret; e: Exported; }
            }
            declare module "c" {
                export interface Exported { c: number; }
            }
        "#;
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let ctx = CodegenContext::from_module(&module, &gctx);
        assert_unresolved(&ctx, "Secret", body_scope(&gctx, "InA"));
        assert_unresolved(&ctx, "Secret", body_scope(&gctx, "InB"));
        assert_unresolved(&ctx, "Exported", body_scope(&gctx, "InB"));
        assert_unresolved(&ctx, "Exported", module.file_scopes[0]);
        assert!(!ctx.is_local_generic("Secret", module.file_scopes[0]));
    }

    #[test]
    fn module_file_locals_stay_in_their_file() {
        // Every module file of the `--lib-name` library is its own module:
        // its unexported declarations are visible to no other file, even
        // one that declares a local of the same name.
        let (module, gctx) = parse_files(
            "module-file-locals",
            &[
                ("main.d.ts", "export declare function f(): void;"),
                (
                    "a.d.ts",
                    "interface Secret<T> { a: T; } interface Shared { a: number; } \
                     export declare function a(s: Shared): Secret<string>;",
                ),
                (
                    "b.d.ts",
                    "interface Shared { b: number; } export declare function b(s: Shared): void;",
                ),
            ],
            Some("lib"),
        );
        let ctx = CodegenContext::from_module(&module, &gctx);
        let (main, a, b) = (
            module.file_scopes[0],
            module.file_scopes[1],
            module.file_scopes[2],
        );
        assert!(ctx.is_local_generic("Secret", a));
        assert!(!ctx.is_local_generic("Secret", main));
        assert_unresolved(&ctx, "Secret", main);
        assert_unresolved(&ctx, "Shared", main);
        let shared = |scope| gctx.resolve_path(scope, "Shared").unwrap();
        assert_ne!(shared(a), shared(b));
    }

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
        let options = GenerateOptions::new().experimental_generic_mono(true);
        let ctx = CodegenContext::from_module_full(&module, &gctx, &options, None).unwrap();
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
        TypeRef::union(values.iter().map(|v| TypeRef::StringLiteral((*v).into())))
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
            let peel = |ty: &TypeRef| {
                peel_mono(ty, ctx, scope, &mut HashSet::new())
                    .0
                    .into_owned()
            };
            assert_eq!(peel(&TypeRef::ident("IdChain")), TypeRef::String);
            assert_eq!(peel(&lits(&["a", "b"])), TypeRef::String);
            assert!(matches!(
                peel(&TypeRef::ident("MaybeId")),
                TypeRef::Union(_)
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
            let (peeled, _) = peel_mono(&ty, ctx, scope, &mut HashSet::new());
            assert!(matches!(peeled.as_ref(), TypeRef::Reference { .. }));
        });
    }

    const SHADOW_SRC: &str = r#"
        interface Pair<A, B> { a: A; b: B; }
        interface Inner { top: string; }
        type TopX = Pair<Inner, Inner>;
        type TopChain = TopX;
        type Mode = "fast" | "slow";
        type TopMode = Mode;
        declare namespace Shadow {
            interface Inner { shadow: boolean; }
            interface Mode { shadow: boolean; }
            type Local = Inner;
        }
    "#;

    const REEXPORT_SRC: &str = r#"
        declare class Position { lineNumber: number; }
        type editor_main_Position = Position;
        declare namespace editor_main {
            export { editor_main_Position as Position };
        }
        export { editor_main as m };
    "#;

    fn namespace_scope(ctx: &CodegenContext<'_>, file: ScopeId, name: &str) -> ScopeId {
        ctx.gctx
            .scopes
            .resolve_namespace(file, name)
            .expect("namespace scope")
    }

    /// Whether `name` means the same declaration from `a` as from `b`.
    fn same_decl(ctx: &CodegenContext<'_>, name: &str, a: ScopeId, b: ScopeId) -> bool {
        let id = ctx.gctx.scopes.resolve(a, name);
        id.is_some() && id == ctx.gctx.scopes.resolve(b, name)
    }

    #[test]
    fn resolve_alias_reads_the_target_in_the_alias_scope() {
        with_mono_ctx(SHADOW_SRC, |ctx, file| {
            let shadow = namespace_scope(ctx, file, "Shadow");
            let pair = TypeRef::generic("Pair", vec![TypeRef::ident("Inner"); 2]);
            // A top-level alias used in `Shadow` reads its target at the top
            // level, through every link of the chain.
            for name in ["TopX", "TopChain"] {
                let (target, scope) = ctx.resolve_alias(name, shadow).unwrap();
                assert_eq!(*target, pair);
                assert!(same_decl(ctx, "Inner", scope, file));
            }
            // The namespace's own alias reads it in the namespace.
            let (target, scope) = ctx.resolve_alias("Local", shadow).unwrap();
            assert_eq!(*target, TypeRef::ident("Inner"));
            assert!(same_decl(ctx, "Inner", scope, shadow));
        });
    }

    #[test]
    fn resolve_alias_follows_a_reexport_under_the_target_name() {
        let (module, gctx) = crate::parse_source(REEXPORT_SRC, Some("lib")).unwrap();
        let ctx = CodegenContext::from_module(&module, &gctx);
        let file = *module.file_scopes.first().expect("file scope");
        // The namespace's body, where the re-export binds `Position`.
        let ns = gctx
            .iter_types()
            .find_map(|(_, decl)| match &decl.kind {
                TypeKind::TypeAlias(a) if a.name == "Position" => Some(a.body_scope),
                _ => None,
            })
            .expect("re-export alias");
        // `Position` in the namespace is the re-export, whose alias's target
        // is the top-level class rather than the re-export again.
        let (target, scope) = ctx.resolve_alias("Position", ns).unwrap();
        assert_eq!(*target, TypeRef::ident("Position"));
        assert!(same_decl(&ctx, "Position", scope, file));
        assert!(ctx.resolve_alias("Position", scope).is_none());
    }

    #[test]
    fn resolves_alike_compares_every_name() {
        with_mono_ctx(SHADOW_SRC, |ctx, file| {
            let shadow = namespace_scope(ctx, file, "Shadow");
            let alike = |ty: &TypeRef| ctx.resolves_alike(ty, file, shadow);
            assert!(alike(&TypeRef::String));
            assert!(alike(&TypeRef::ident("Pair")));
            assert!(alike(&TypeRef::ident("Undeclared")));
            assert!(!alike(&TypeRef::ident("Inner")));
            assert!(!alike(&TypeRef::generic(
                "Pair",
                vec![TypeRef::ident("Inner")]
            )));
            assert!(!alike(&TypeRef::Array(Box::new(TypeRef::ident("Inner")))));
            assert!(!alike(&TypeRef::union([
                TypeRef::String,
                TypeRef::ident("Inner"),
            ])));
        });
    }

    #[test]
    fn peel_mono_keeps_the_alias_scope() {
        with_mono_ctx(SHADOW_SRC, |ctx, file| {
            let shadow = namespace_scope(ctx, file, "Shadow");
            let ty = TypeRef::ident("TopMode");
            let (peeled, scope) = peel_mono(&ty, ctx, shadow, &mut HashSet::new());
            assert_eq!(*peeled, TypeRef::ident("Mode"));
            assert!(same_decl(ctx, "Mode", scope, file));
            // `Mode` is the enum where the alias reads it, not the
            // namespace's interface.
            assert!(is_mono_native_leaf(&peeled, ctx, scope));
            assert!(!is_mono_native_leaf(&peeled, ctx, shadow));
        });
    }

    #[test]
    fn mono_string_leaves_accept_any_string_shape() {
        with_mono_ctx(MONO_SRC, |ctx, scope| {
            let arg = |ty: TypeRef| {
                mono_string_like_argument(&ty, ctx, scope, &ModuleContext::Global)
                    .map(|t| t.to_string())
            };
            let list = |elem: TypeRef| TypeRef::Array(Box::new(elem));
            let like = "impl :: wasm_bindgen :: JsStringLike";
            let slice = format!("& [{like}]");
            assert_eq!(arg(list(TypeRef::String)), Some(slice.clone()));
            assert_eq!(arg(list(TypeRef::ident("IdChain"))), Some(slice.clone()));
            assert_eq!(arg(list(TypeRef::ident("ModeAlias"))), Some(slice));
            assert_eq!(
                arg(TypeRef::generic("ArrayLike", vec![TypeRef::String])),
                Some(format!("& Array < {like} >"))
            );
            assert_eq!(
                arg(list(list(TypeRef::String))),
                Some(format!("& [Array < {like} >]"))
            );
            assert_eq!(
                arg(TypeRef::generic("ReadonlySet", vec![TypeRef::ident("Id")])),
                Some(format!("& Set < {like} >"))
            );
            assert_eq!(
                arg(TypeRef::generic(
                    "Map",
                    vec![TypeRef::String, list(TypeRef::String)]
                )),
                Some(format!("& Map < {like} , Array < {like} > >"))
            );
            assert_eq!(arg(list(TypeRef::Number)), None);
            assert_eq!(arg(list(TypeRef::ident("MaybeId"))), None);
            assert_eq!(arg(TypeRef::generic("Set", vec![TypeRef::Number])), None);
            assert_eq!(arg(TypeRef::String), None);

            assert!(is_mono_string_list_return(
                &list(TypeRef::ident("Id")),
                ctx,
                scope
            ));
            assert!(is_mono_string_list_return(
                &list(TypeRef::String).with_empties(crate::ir::Nullability::Null),
                ctx,
                scope
            ));
            assert!(!is_mono_string_list_return(
                &list(TypeRef::Boolean),
                ctx,
                scope
            ));
            assert!(!is_mono_string_list_return(
                &holder(list(TypeRef::String)),
                ctx,
                scope
            ));
        });
    }

    #[test]
    fn array_elements_are_native_only_for_primitives() {
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
            assert_eq!(ret(TypeRef::Boolean), "Vec < bool >");
            assert_eq!(
                ret(TypeRef::ident("MaybeId")),
                "Vec < JsNullable < JsString > >"
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
                "Nullable < String >"
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
                    &TypeRef::ident("Level").with_empties(Nullability::Undefined),
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
            assert!(!eligible(
                &(lits(&["a", "b"])).with_empties(Nullability::Null)
            ));
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
                "Nullable < JsString >"
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
        let ctx = CodegenContext::from_module_full(&module, &gctx, &GenerateOptions::new(), None)
            .unwrap();
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
        // A synthesised `Array<T>` reference lowers like the
        // parse-normalised `T[]`.
        assert_eq!(
            lower(
                &TypeRef::generic("Array", vec![TypeRef::ident("Level")]),
                arg
            ),
            "& [f64]"
        );
        assert_eq!(
            lower(
                &TypeRef::generic("ArrayLike", vec![TypeRef::ident("Level")]),
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
        let ctx = CodegenContext::from_module_full(&module, &gctx, &GenerateOptions::new(), None)
            .unwrap();
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
        let ctx = CodegenContext::from_module_full(&module, &gctx, &GenerateOptions::new(), None)
            .unwrap();
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

    #[test]
    fn error_type_option_is_validated() {
        let (module, gctx) =
            crate::parse_source("declare function f(): Promise<void>;", None).unwrap();
        let bad = GenerateOptions::new().error_type("not a type");
        let err = CodegenContext::from_module_full(&module, &gctx, &bad, None)
            .err()
            .expect("unparseable path is rejected");
        assert!(err.to_string().contains("invalid --error-type"));

        let good = GenerateOptions::new().error_type("crate::Error");
        let ctx = CodegenContext::from_module_full(&module, &gctx, &good, None).unwrap();
        assert_eq!(default_error_type(Some(&ctx)).to_string(), "crate :: Error");
        assert_eq!(default_error_type(None).to_string(), "JsValue");
    }
}
