//! Intermediate Representation for TypeScript declarations.
//!
//! This module defines the IR that sits between the parsed TypeScript AST
//! and the generated Rust code. The pipeline is:
//!
//! ```text
//! .d.ts → oxc_parser AST → First Pass (collect + populate) → IR → Codegen → .rs
//! ```

use std::collections::HashMap;

use crate::context::TypeId;
use crate::parse::scope::ScopeId;

// ─── Module Context ──────────────────────────────────────────────────

/// Where a declaration lives — either as a global ambient declaration
/// or inside a specific JS module.
///
/// Uses `Rc<str>` for the module name to make cloning cheap (ref count bump
/// instead of heap allocation), since `ModuleContext` is cloned extensively
/// during IR construction.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModuleContext {
    /// No `module=` attribute. Accessed as a global.
    Global,
    /// From a specific JS module specifier (e.g., `"cloudflare:sockets"`).
    Module(std::rc::Rc<str>),
}

// ─── Type References ─────────────────────────────────────────────────

/// A reference to a TypeScript type, resolved from the AST.
///
/// The IR mirrors TypeScript's own type system: only the syntactic
/// constructs from the grammar get dedicated variants. Anything that
/// would resolve through TS's name lookup (built-in JS classes,
/// `Promise<T>`, `Map<K,V>`, user-declared types, qualified paths)
/// flows through [`TypeRef::Reference`] and is resolved at codegen
/// time against the scope chain. This means user-declared types
/// shadow built-ins exactly the way they do in TypeScript.
///
/// Note that `T[]` and `Array<T>` are **distinct** at the IR level
/// (matching TS): `T[]` is the syntactic [`TypeRef::Array`] shortcut
/// that never goes through name resolution, while `Array<T>` is a
/// `Reference { head: "Array", ... }` that *does* go through the
/// scope chain and is therefore affected by local shadowing.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeRef {
    // === Primitives ===
    //
    // Lower to a Rust-side type at the FFI boundary, not a JS class —
    // e.g. `String` → `String`/`&str`, `Number` → `f64`. Each variant
    // carries its own non-trivial lowering rule.
    Boolean,
    Number,
    String,
    BigInt,
    Void,
    Undefined,
    Null,
    Any,
    Unknown,
    Object,
    /// JS `symbol` primitive — there is no Rust-side equivalent at the
    /// FFI boundary, so it lowers to `JsValue`.
    Symbol,

    // === TS-only synthetic types ===
    /// `ArrayBufferView` is a TS-only union alias for the typed-array
    /// family + `DataView`. There is no JS class with this name, so it
    /// can't lower to a `js_sys` ident — codegen erases it to `Object`.
    ArrayBufferView,

    // === Syntactic constructs ===
    /// `T[]` — the syntactic array shortcut. Distinct from
    /// `Array<T>` (which is a [`TypeRef::Reference`] to the global
    /// `Array` symbol and goes through name resolution). `T[]` is
    /// unaffected by local shadowing of `Array`.
    Array(Box<TypeRef>),
    /// `T | null` / `T | undefined` / `T?` — wraps the inner in
    /// `Option<T>` at lowering. Coalesces `null`/`undefined` arms.
    Nullable(Box<TypeRef>),
    Union(Vec<TypeRef>),
    Intersection(Vec<TypeRef>),
    Tuple(Vec<TypeRef>),
    /// `(args) => ret` — function type literal.
    Function(FunctionSig),

    // === Literal Types ===
    StringLiteral(String),
    NumberLiteral(f64),
    BooleanLiteral(bool),

    // === Named References ===
    /// Nominal type reference: a (possibly qualified) name with an
    /// (possibly empty) generic type argument list.
    ///
    /// * `Foo` → `Reference { segments: vec!["Foo"], generic_args: vec![] }`
    /// * `Foo<T>` → `Reference { segments: vec!["Foo"], generic_args: vec![T] }`
    /// * `A.B.C` → `Reference { segments: vec!["A","B","C"], generic_args: vec![] }`
    /// * `A.B.C<T>` → `Reference { segments: vec!["A","B","C"], generic_args: vec![T] }`
    ///
    /// All scope-resolved names use this variant: user-declared
    /// types, built-in JS classes (`Date`, `Error`, `TypeError`,
    /// `Uint8Array`, ...), and `Promise`/`Map`/`Set`/`Record`. Codegen
    /// resolves these at emit time via scope chain → `JS_SYS_RESERVED`
    /// (built-ins) → external map → `JsValue` fallback. Heads with
    /// special codegen rules (e.g. `Promise<T>` → `async fn`) are
    /// recognized by name at the relevant emitter.
    ///
    /// Single-segment references participate in alias chasing and
    /// shadowing (mirroring TS scope-driven resolution); multi-
    /// segment references are not yet fully resolved through the
    /// scope chain — they fall back to the external map or to
    /// `JsValue` with a diagnostic. Proper namespace traversal is a
    /// future extension.
    Reference {
        segments: Vec<String>,
        generic_args: Vec<TypeRef>,
    },

    // === Fallback ===
    /// For TS constructs we can't represent (conditional types, mapped types,
    /// template literals, `keyof`, etc.) — erased to `JsValue`.
    Unresolved(String),
}

impl TypeRef {
    /// Construct a single-segment, non-generic reference: `Foo`.
    /// Convenience for the common case of a bare ident.
    pub fn ident(name: impl Into<String>) -> Self {
        TypeRef::Reference {
            segments: vec![name.into()],
            generic_args: Vec::new(),
        }
    }

    /// Construct a single-segment generic instantiation:
    /// `head<T1, T2, ...>`. Convenience for `Promise<T>`,
    /// `Map<K, V>`, `Array<T>`, etc.
    pub fn generic(head: impl Into<String>, generic_args: Vec<TypeRef>) -> Self {
        TypeRef::Reference {
            segments: vec![head.into()],
            generic_args,
        }
    }

    /// If `self` is a bare reference (single-segment, no generic
    /// arguments), return the ident. `Foo` → `Some("Foo")`,
    /// `A.B` → `None`, `Foo<T>` → `None`.
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            TypeRef::Reference {
                segments,
                generic_args,
            } if segments.len() == 1 && generic_args.is_empty() => Some(segments[0].as_str()),
            _ => None,
        }
    }

    /// If `self` is a generic instantiation of a single-segment head
    /// matching `name` (e.g. `"Promise"`, `"Map"`), return its type
    /// arguments. Used by codegen sites that recognize specific
    /// generic shapes for dedicated lowering.
    pub fn as_generic_head(&self, name: &str) -> Option<&[TypeRef]> {
        match self {
            TypeRef::Reference {
                segments,
                generic_args,
            } if segments.len() == 1 && segments[0] == name && !generic_args.is_empty() => {
                Some(generic_args)
            }
            _ => None,
        }
    }

    /// `Promise<T>` → `Some(&T)`. Convenience over [`Self::as_generic_head`]
    /// for the common Promise-unwrap case (signature collapse for
    /// async returns).
    pub fn as_promise_inner(&self) -> Option<&TypeRef> {
        let args = self.as_generic_head("Promise")?;
        args.first()
    }

    /// Render this `TypeRef` back into a TypeScript-like string.
    ///
    /// Pretty-prints structurally faithful to the source — primitives
    /// as their lowercase keywords, generics as `Foo<T1, T2>`, unions
    /// as `A | B`, etc. Used to surface the original TS shape of a
    /// return type in `Returns:` doc comments when the static Rust
    /// type loses information (e.g. an erased union).
    pub fn format_ts(&self) -> String {
        match self {
            TypeRef::Boolean => "boolean".into(),
            TypeRef::Number => "number".into(),
            TypeRef::String => "string".into(),
            TypeRef::BigInt => "bigint".into(),
            TypeRef::Void => "void".into(),
            TypeRef::Undefined => "undefined".into(),
            TypeRef::Null => "null".into(),
            TypeRef::Any => "any".into(),
            TypeRef::Unknown => "unknown".into(),
            TypeRef::Object => "object".into(),
            TypeRef::Symbol => "symbol".into(),
            TypeRef::ArrayBufferView => "ArrayBufferView".into(),
            TypeRef::Array(inner) => {
                // Wrap composite inner types in parens so e.g.
                // `(A | B)[]` doesn't render as the wrong-precedence
                // `A | B[]`.
                let needs_parens = matches!(
                    inner.as_ref(),
                    TypeRef::Union(_) | TypeRef::Intersection(_) | TypeRef::Nullable(_)
                );
                if needs_parens {
                    format!("({})[]", inner.format_ts())
                } else {
                    format!("{}[]", inner.format_ts())
                }
            }
            TypeRef::Nullable(inner) => format!("{} | null", inner.format_ts()),
            TypeRef::Union(members) => members
                .iter()
                .map(Self::format_ts)
                .collect::<Vec<_>>()
                .join(" | "),
            TypeRef::Intersection(parts) => parts
                .iter()
                .map(Self::format_ts)
                .collect::<Vec<_>>()
                .join(" & "),
            TypeRef::Tuple(elems) => format!(
                "[{}]",
                elems
                    .iter()
                    .map(Self::format_ts)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            TypeRef::Function(sig) => {
                let params = sig
                    .params
                    .iter()
                    .map(|p| {
                        let opt = if p.optional { "?" } else { "" };
                        let dots = if p.variadic { "..." } else { "" };
                        format!("{dots}{}{opt}: {}", p.name, p.type_ref.format_ts())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({params}) => {}", sig.return_type.format_ts())
            }
            TypeRef::StringLiteral(s) => format!("\"{s}\""),
            TypeRef::NumberLiteral(n) => n.to_string(),
            TypeRef::BooleanLiteral(b) => b.to_string(),
            TypeRef::Reference {
                segments,
                generic_args,
            } => {
                let name = segments.join(".");
                if generic_args.is_empty() {
                    name
                } else {
                    let args = generic_args
                        .iter()
                        .map(Self::format_ts)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name}<{args}>")
                }
            }
            TypeRef::Unresolved(s) => s.clone(),
        }
    }
}

// ─── Function Signatures ─────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]

pub struct FunctionSig {
    pub params: Vec<Param>,
    pub return_type: Box<TypeRef>,
}

#[derive(Clone, Debug, PartialEq)]

pub struct Param {
    pub name: String,
    pub type_ref: TypeRef,
    pub optional: bool,
    pub variadic: bool,
}

// ─── Type Parameters (Generics) ──────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]

pub struct TypeParam {
    pub name: String,
    /// `T extends Foo` — used for classification only.
    pub constraint: Option<TypeRef>,
    /// `T = Bar` — used for default instantiation.
    pub default: Option<TypeRef>,
}

// ─── Top-level Module ────────────────────────────────────────────────

/// A parsed module — references types and scopes in the `GlobalContext`.
#[derive(Clone, Debug)]

pub struct Module {
    /// Type ids for declarations from the input files (what to generate code for).
    pub types: Vec<TypeId>,
    /// Optional library name from `--lib-name`.
    pub lib_name: Option<String>,
    /// Builtin (root) scope id.
    pub builtin_scope: ScopeId,
    /// Input file scope ids (for scope-based codegen iteration).
    pub file_scopes: Vec<ScopeId>,
    /// Canonical source-file path for each input file scope. Used by
    /// codegen to decide whether a declaration's owning file is in the
    /// `--export` set when filtering the output surface.
    pub file_scope_paths: HashMap<ScopeId, std::path::PathBuf>,
}

#[derive(Clone, Debug)]

pub struct TypeDeclaration {
    pub kind: TypeKind,
    pub module_context: ModuleContext,
    pub doc: Option<String>,
    /// The scope this declaration was defined in (for type reference resolution).
    pub scope_id: crate::parse::scope::ScopeId,
    /// Whether this declaration was explicitly exported.
    /// In script mode, all declarations are implicitly public.
    /// In module mode, only exported declarations are public.
    pub exported: bool,
}

#[derive(Clone, Debug)]

pub enum TypeKind {
    Class(ClassDecl),
    Interface(InterfaceDecl),
    /// A `type X = A | B | ...` whose branches share a string-literal
    /// discriminant property — see [`DiscriminatedUnionDecl`].
    DiscriminatedUnion(DiscriminatedUnionDecl),
    TypeAlias(TypeAliasDecl),
    StringEnum(StringEnumDecl),
    NumericEnum(NumericEnumDecl),
    Function(FunctionDecl),
    Variable(VariableDecl),
    Namespace(NamespaceDecl),
}

// ─── Class ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct ClassDecl {
    pub name: String,
    /// Original JS name (may differ from Rust name after case conversion).
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    /// Immediate parent class.
    pub extends: Option<TypeRef>,
    pub implements: Vec<TypeRef>,
    pub is_abstract: bool,
    pub members: Vec<Member>,
    /// Where the type declaration itself should live.
    pub type_module_context: ModuleContext,
    /// Scope inside this class's body — child of the enclosing scope,
    /// holding [`Binding::TypeParam`] entries for the class's own
    /// `<T, ...>`. Methods chain off this scope; codegen resolves
    /// names against it.
    pub body_scope: crate::parse::scope::ScopeId,
}

// ─── Interface ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct InterfaceDecl {
    pub name: String,
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    pub extends: Vec<TypeRef>,
    pub members: Vec<Member>,
    /// Classification determined during assembly.
    pub classification: InterfaceClassification,
    /// Scope inside this interface's body — see
    /// [`ClassDecl::body_scope`] for the contract.
    pub body_scope: crate::parse::scope::ScopeId,
}

#[derive(Clone, Debug, PartialEq, Eq)]

pub enum InterfaceClassification {
    /// Has methods, used as a class-like type.
    ClassLike,
    /// All optional properties, no methods → options bag / dictionary.
    Dictionary,
    /// Mixed or unclear — treat as class-like.
    Unclassified,
}

// ─── Discriminated Union ─────────────────────────────────────────────

/// A type alias `type Foo = A | B | ...` whose branches share at least
/// one **required** property typed as a string literal — that property
/// is the discriminator.
///
/// Modeled as its own kind (separate from `Interface`) because a
/// faithful binding must respect per-branch shape rather than the
/// merged-shape required-vs-optional split. For example
///
/// ```ts
/// type EmailAttachment =
///   | { disposition: "inline";     contentId: string;     filename: string; ... }
///   | { disposition: "attachment"; contentId?: undefined; filename: string; ... };
/// ```
///
/// requires `contentId` in the `inline` branch but not in `attachment`.
/// Treating the merge as a flat interface erases that distinction —
/// codegen would mark `contentId` optional everywhere, then `new_inline`
/// wouldn't take it as a parameter even though the source contract
/// requires it.
#[derive(Clone, Debug)]

pub struct DiscriminatedUnionDecl {
    pub name: String,
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    /// Pre-merge per-branch member sets. Order matches source order;
    /// every branch contributes a (possibly empty) set of members.
    pub branches: Vec<Vec<Member>>,
    /// Merged member view — useful for the regular extern-block
    /// emission (one `pub type Foo;` plus a getter/setter per property
    /// across all branches). The branch-specific factories use
    /// [`branches`] instead.
    pub members: Vec<Member>,
    /// JS names of properties that act as discriminators — present and
    /// required in every branch with a string-literal type.
    pub discriminators: Vec<String>,
    /// Scope inside this declaration's body — see
    /// [`ClassDecl::body_scope`] for the contract.
    pub body_scope: crate::parse::scope::ScopeId,
}

// ─── Type Alias ──────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct TypeAliasDecl {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub target: TypeRef,
    /// If this alias is a re-export from an external module, the module specifier.
    /// Used by codegen to emit `pub use <external>::Foo;` instead of `pub type`.
    pub from_module: Option<String>,
    /// Scope inside this alias's body — see [`ClassDecl::body_scope`].
    pub body_scope: crate::parse::scope::ScopeId,
}

// ─── String Enum ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct StringEnumDecl {
    pub name: String,
    pub variants: Vec<StringEnumVariant>,
}

#[derive(Clone, Debug)]

pub struct StringEnumVariant {
    /// PascalCase variant name.
    pub rust_name: String,
    /// Original string value.
    pub js_value: String,
}

// ─── Numeric Enum ────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct NumericEnumDecl {
    pub name: String,
    pub variants: Vec<NumericEnumVariant>,
}

#[derive(Clone, Debug)]

pub struct NumericEnumVariant {
    /// PascalCase variant name.
    pub rust_name: String,
    /// Original JS member name.
    pub js_name: String,
    /// Numeric discriminant value.
    pub value: i64,
    /// Doc comment for this variant.
    pub doc: Option<String>,
}

// ─── Function ────────────────────────────────────────────────────────

/// What `@throws` JSDoc says about a callable's failure modes.
///
/// The three states are mutually exclusive and drive different codegen
/// decisions:
///
/// * [`Throws::None`] — no `@throws` annotation. Sync callables get a
///   `try_<name>` companion returning `Result<T, JsValue>`; async ones
///   wrap as `Result<T, JsValue>` directly.
/// * [`Throws::Type`] — typed throws. Sync gets `try_<name>` returning
///   `Result<T, ErrTy>`; async wraps as `Result<T, ErrTy>`. The carried
///   `TypeRef` runs through the regular type pipeline, so subtyping
///   LUB rules (see `lub_types`) apply when multiple types are listed.
/// * [`Throws::Never`] — `@throws {never}`. The callable is declared
///   never to throw. Sync gets *no* `try_` companion; async drops
///   `catch` and returns `T` directly (no `Result` wrapper).
///
/// `Throws::Never` only fires when `never` is the *sole* type listed
/// across all `@throws` lines — `@throws {never | OtherError}` is
/// treated as `Throws::Type(OtherError)` (the `never` is ignored).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Throws {
    #[default]
    None,
    Type(TypeRef),
    Never,
}

impl Throws {
    /// Returns the error `TypeRef` for codegen, if any. `Never` and
    /// `None` both yield `None` here — callers separately consult
    /// `is_never()` to decide whether to suppress catching entirely.
    pub fn as_type(&self) -> Option<&TypeRef> {
        match self {
            Throws::Type(t) => Some(t),
            _ => None,
        }
    }

    /// Whether this is the explicit "never throws" annotation.
    pub fn is_never(&self) -> bool {
        matches!(self, Throws::Never)
    }
}

#[derive(Clone, Debug)]

pub struct FunctionDecl {
    pub name: String,
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub overloads: Vec<FunctionOverload>,
    /// Failure-mode info from `@throws` JSDoc. See [`Throws`].
    pub throws: Throws,
    /// Scope inside this function's body — see [`ClassDecl::body_scope`].
    pub body_scope: crate::parse::scope::ScopeId,
}

#[derive(Clone, Debug)]

pub struct FunctionOverload {
    pub params: Vec<Param>,
    pub return_type: TypeRef,
}

// ─── Variable ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct VariableDecl {
    pub name: String,
    pub js_name: String,
    pub type_ref: TypeRef,
    pub is_const: bool,
}

// ─── Namespace ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct NamespaceDecl {
    pub name: String,
    pub declarations: Vec<TypeDeclaration>,
    /// The child scope containing this namespace's member types.
    pub child_scope: crate::parse::scope::ScopeId,
}

// ─── Members ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub enum Member {
    Getter(GetterMember),
    Setter(SetterMember),
    Method(MethodMember),
    Constructor(ConstructorMember),
    IndexSignature(IndexSigMember),
    StaticGetter(StaticGetterMember),
    StaticSetter(StaticSetterMember),
    StaticMethod(StaticMethodMember),
}

/// Instance property getter binding.
///
/// Emits: `fn <name>(this: &T) -> R;` with `#[wasm_bindgen(method, getter)]`.
#[derive(Clone, Debug)]

pub struct GetterMember {
    /// Original JS property name (source of truth for naming).
    pub js_name: String,
    /// Return type of the getter.
    pub type_ref: TypeRef,
    /// Whether declared with `?:` syntax — wraps return in `Option`.
    pub optional: bool,
    pub doc: Option<String>,
}

/// Instance property setter binding.
///
/// Emits: `fn set_<name>(this: &T, val: V);` with `#[wasm_bindgen(method, setter)]`.
#[derive(Clone, Debug)]

pub struct SetterMember {
    /// Original JS property name (source of truth for naming).
    pub js_name: String,
    /// Parameter type of the setter.
    pub type_ref: TypeRef,
    pub doc: Option<String>,
}

#[derive(Clone, Debug)]

pub struct MethodMember {
    pub name: String,
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub optional: bool,
    pub doc: Option<String>,
    /// Failure-mode info from `@throws` — see [`Throws`].
    pub throws: Throws,
    /// Scope inside this method's body — child of the enclosing
    /// type's body scope, holding [`Binding::TypeParam`] entries for
    /// the method's own `<T, ...>`. See [`ClassDecl::body_scope`].
    pub body_scope: crate::parse::scope::ScopeId,
}

#[derive(Clone, Debug)]

pub struct ConstructorMember {
    pub params: Vec<Param>,
    pub doc: Option<String>,
    /// Failure-mode info from `@throws` — see [`Throws`].
    ///
    /// Note: constructors always emit `catch` per JS `new` semantics, so
    /// `Throws::Never` here is effectively a no-op for codegen — only
    /// `Throws::Type` (custom error type) changes behavior.
    pub throws: Throws,
}

#[derive(Clone, Debug)]

pub struct IndexSigMember {
    pub key_type: TypeRef,
    pub value_type: TypeRef,
    pub readonly: bool,
}

/// Static property getter binding.
///
/// Emits: `fn <name>() -> R;` with `#[wasm_bindgen(static_method_of = T, getter)]`.
#[derive(Clone, Debug)]

pub struct StaticGetterMember {
    /// Original JS property name (source of truth for naming).
    pub js_name: String,
    pub type_ref: TypeRef,
    pub doc: Option<String>,
}

/// Static property setter binding.
///
/// Emits: `fn set_<name>(val: V);` with `#[wasm_bindgen(static_method_of = T, setter)]`.
#[derive(Clone, Debug)]

pub struct StaticSetterMember {
    /// Original JS property name (source of truth for naming).
    pub js_name: String,
    pub type_ref: TypeRef,
    pub doc: Option<String>,
}

#[derive(Clone, Debug)]

pub struct StaticMethodMember {
    pub name: String,
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub doc: Option<String>,
    /// Failure-mode info from `@throws` — see [`Throws`].
    pub throws: Throws,
    /// Scope inside this static method's body — see
    /// [`MethodMember::body_scope`].
    pub body_scope: crate::parse::scope::ScopeId,
}

// ─── Type Registry (First Pass) ──────────────────────────────────────

/// Registry built during Phase 1 of the first pass, mapping type names
/// to their kind and primary declaration context.
#[derive(Clone, Debug, Default)]

pub struct TypeRegistry {
    pub types: HashMap<String, TypeInfo>,
    /// Local-name → public export name for `export { local as exported }` forms
    /// (the source-less rename pattern, e.g. `export { _EmailMessage as EmailMessage }`).
    ///
    /// Phase 2 consults this to give renamed declarations their public-facing
    /// type name and to suppress redundant alias emission.
    pub export_renames: HashMap<String, String>,
}

#[derive(Clone, Debug)]

pub struct TypeInfo {
    pub kind: RegisteredKind,
    /// Where is this type's "primary" declaration?
    pub primary_context: ModuleContext,
}

#[derive(Clone, Debug, PartialEq, Eq)]

pub enum RegisteredKind {
    Class,
    Interface,
    /// `var` + `interface` pattern merged together.
    MergedClassLike,
    TypeAlias,
    /// Detected when a type alias is a union of string literals.
    StringEnum,
    /// A TS enum with numeric (or auto-incrementing) values.
    NumericEnum,
    Function,
    Variable,
    Namespace,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ts_primitives_use_lowercase_keywords() {
        assert_eq!(TypeRef::String.format_ts(), "string");
        assert_eq!(TypeRef::Number.format_ts(), "number");
        assert_eq!(TypeRef::Boolean.format_ts(), "boolean");
        assert_eq!(TypeRef::Null.format_ts(), "null");
        assert_eq!(TypeRef::Undefined.format_ts(), "undefined");
    }

    #[test]
    fn format_ts_literals_use_their_textual_form() {
        assert_eq!(TypeRef::StringLiteral("foo".into()).format_ts(), "\"foo\"");
        assert_eq!(TypeRef::NumberLiteral(32.0).format_ts(), "32");
        assert_eq!(TypeRef::BooleanLiteral(true).format_ts(), "true");
    }

    #[test]
    fn format_ts_union_renders_with_pipes() {
        let ty = TypeRef::Union(vec![
            TypeRef::String,
            TypeRef::ident("ArrayBuffer"),
            TypeRef::ArrayBufferView,
        ]);
        assert_eq!(ty.format_ts(), "string | ArrayBuffer | ArrayBufferView");
    }

    #[test]
    fn format_ts_array_wraps_composite_inner_in_parens() {
        // `(string | number)[]` — without parens this would mean
        // `string | (number[])`, which is wrong precedence.
        let ty = TypeRef::Array(Box::new(TypeRef::Union(vec![
            TypeRef::String,
            TypeRef::Number,
        ])));
        assert_eq!(ty.format_ts(), "(string | number)[]");
    }

    #[test]
    fn format_ts_array_simple_inner_no_parens() {
        let ty = TypeRef::Array(Box::new(TypeRef::String));
        assert_eq!(ty.format_ts(), "string[]");
    }

    #[test]
    fn format_ts_generic_with_inner_union() {
        let ty = TypeRef::generic(
            "Array",
            vec![TypeRef::Union(vec![
                TypeRef::NumberLiteral(32.0),
                TypeRef::StringLiteral("foo".into()),
            ])],
        );
        assert_eq!(ty.format_ts(), "Array<32 | \"foo\">");
    }

    #[test]
    fn format_ts_qualified_path() {
        let ty = TypeRef::Reference {
            segments: vec!["A".into(), "B".into(), "C".into()],
            generic_args: vec![],
        };
        assert_eq!(ty.format_ts(), "A.B.C");
    }

    #[test]
    fn format_ts_qualified_generic() {
        let ty = TypeRef::Reference {
            segments: vec!["Rpc".into(), "Stub".into()],
            generic_args: vec![TypeRef::ident("Foo")],
        };
        assert_eq!(ty.format_ts(), "Rpc.Stub<Foo>");
    }

    #[test]
    fn format_ts_nullable() {
        let ty = TypeRef::Nullable(Box::new(TypeRef::String));
        assert_eq!(ty.format_ts(), "string | null");
    }
}
