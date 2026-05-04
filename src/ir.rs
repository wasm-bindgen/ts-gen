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
#[derive(Clone, Debug, PartialEq)]

pub enum TypeRef {
    // === Primitives ===
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
    Symbol,

    // === Typed Arrays ===
    Int8Array,
    Uint8Array,
    Uint8ClampedArray,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Float32Array,
    Float64Array,
    BigInt64Array,
    BigUint64Array,
    ArrayBuffer,
    ArrayBufferView,
    DataView,

    // === Built-in Generic Containers ===
    Promise(Box<TypeRef>),
    Array(Box<TypeRef>),
    Record(Box<TypeRef>, Box<TypeRef>),
    Map(Box<TypeRef>, Box<TypeRef>),
    Set(Box<TypeRef>),

    // === Structural Types ===
    Nullable(Box<TypeRef>),
    Union(Vec<TypeRef>),
    Intersection(Vec<TypeRef>),
    Tuple(Vec<TypeRef>),
    Function(FunctionSig),

    // === Literal Types ===
    StringLiteral(String),
    NumberLiteral(f64),
    BooleanLiteral(bool),

    // === Named References ===
    /// Reference to a type by name. Resolved during first pass.
    Named(String),
    /// Generic instantiation: `Named<T1, T2, ...>`
    GenericInstantiation(String, Vec<TypeRef>),

    // === Special ===
    Date,
    RegExp,
    Error,

    // === Fallback ===
    /// For TS constructs we can't represent (conditional types, mapped types,
    /// template literals, `keyof`, etc.) — erased to `JsValue`.
    Unresolved(String),
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

// ─── Type Alias ──────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct TypeAliasDecl {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub target: TypeRef,
    /// If this alias is a re-export from an external module, the module specifier.
    /// Used by codegen to emit `pub use <external>::Foo;` instead of `pub type`.
    pub from_module: Option<String>,
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
