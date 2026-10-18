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

impl ModuleContext {
    /// The JS module specifier, or `None` for globals.
    pub(crate) fn specifier(&self) -> Option<&str> {
        match self {
            ModuleContext::Global => None,
            ModuleContext::Module(module) => Some(module),
        }
    }
}

// ─── Type References ─────────────────────────────────────────────────

/// Which empty arms a [`TypeRef::Nullable`] coalesced. Both lower to
/// `Option<T>` at top level; inner positions and the Wasm ABI care which.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Nullability {
    pub null: bool,
    pub undefined: bool,
}

impl Nullability {
    pub const NULL: Self = Self {
        null: true,
        undefined: false,
    };
    pub const UNDEFINED: Self = Self {
        null: false,
        undefined: true,
    };
    pub const BOTH: Self = Self {
        null: true,
        undefined: true,
    };

    pub fn union(self, other: Self) -> Self {
        Self {
            null: self.null || other.null,
            undefined: self.undefined || other.undefined,
        }
    }
}

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
/// Note that `T[]`, `Array<T>` and `ReadonlyArray<T>` are the same TS
/// type and normalise to [`TypeRef::Array`] at parse, like the other
/// built-in names (`Boolean`, `Number`, `String`, …) that are matched
/// before name resolution. `ArrayLike<T>` is a distinct type and stays
/// a `Reference`, lowering to the JS array-handle form.
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
    /// TypeScript's built-in `PropertyKey` alias for `string | number |
    /// symbol`. Kept distinct from the equivalent [`TypeRef::Union`] so
    /// per-monomorphization arguments can use the `js_sys::PropertyKey`
    /// spelling; everywhere else it lowers exactly like that union (see
    /// [`TypeRef::property_key_members`]).
    PropertyKey,
    /// A `TypedArray | number[]` (or `bigint[]`) argument's slice of the
    /// typed array's element: `Uint8Array | number[]` is `&[u8]`, passed
    /// to JS as a `Uint8Array` view. Synthesised by codegen's overload
    /// flattening, never parsed.
    TypedSlice(TypedArrayElement, SharedMemory),

    /// A Rust scalar forced by a `@rust <type>` JSDoc tag. TS `number`
    /// is `f64`; declarations that know better (`status: number` is a
    /// `u16`) say so here, and wasm-bindgen converts at the boundary.
    RustScalar(RustScalar),
    /// A 64-bit scalar (`@rust u64` / `i64`) on a `number` leaf. The
    /// extern ABI crosses 64-bit integers as `BigInt`, so the binding
    /// carries `f64` and an inline wrapper converts with `as` (see
    /// `codegen::inline_wrappers`).
    WideScalar(RustScalar),

    // === Syntactic constructs ===
    /// `T[]` — the syntactic array shortcut. Distinct from
    /// `Array<T>` (which is a [`TypeRef::Reference`] to the global
    /// `Array` symbol and goes through name resolution). `T[]` is
    /// unaffected by local shadowing of `Array`.
    Array(Box<TypeRef>),
    /// `T | null` / `T | undefined` / `T?` — wraps the inner in
    /// `Option<T>` at lowering. Coalesces `null`/`undefined` arms,
    /// remembering which were present.
    Nullable(Box<TypeRef>, Nullability),
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
    /// `M[K]` over two type references (`EventMap[Type]`). Erased to
    /// `JsValue` like [`TypeRef::Unresolved`] until a per-mono key
    /// expansion substitutes `K` with a literal key of a concrete `M`.
    IndexedAccess(Box<TypeRef>, Box<TypeRef>),
}

/// Whether a typed-array parameter accepts a view of shared memory. TS 5.7+
/// spells one that doesn't, a WebIDL buffer without `[AllowShared]`, with an
/// `ArrayBuffer` argument (`Uint8Array<ArrayBuffer>`); the bare form defaults
/// to `ArrayBufferLike`, which includes `SharedArrayBuffer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedMemory {
    Accepted,
    Rejected,
}

impl SharedMemory {
    /// Read from a typed array's generic arguments.
    pub fn of_typed_array_args(generic_args: &[TypeRef]) -> Self {
        match generic_args.first() {
            Some(TypeRef::Reference {
                segments,
                generic_args,
            }) if segments == &["ArrayBuffer"] && generic_args.is_empty() => Self::Rejected,
            _ => Self::Accepted,
        }
    }
}

/// The element of a JS typed array with a matching Rust slice ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedArrayElement {
    I8,
    U8,
    /// `Uint8ClampedArray`, whose Rust slice is wrapped in `Clamped`.
    U8Clamped,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
    I64,
    U64,
}

impl TypedArrayElement {
    const ALL: [(Self, &'static str, &'static str); 11] = [
        (Self::I8, "Int8Array", "i8"),
        (Self::U8, "Uint8Array", "u8"),
        (Self::U8Clamped, "Uint8ClampedArray", "u8"),
        (Self::I16, "Int16Array", "i16"),
        (Self::U16, "Uint16Array", "u16"),
        (Self::I32, "Int32Array", "i32"),
        (Self::U32, "Uint32Array", "u32"),
        (Self::F32, "Float32Array", "f32"),
        (Self::F64, "Float64Array", "f64"),
        (Self::I64, "BigInt64Array", "i64"),
        (Self::U64, "BigUint64Array", "u64"),
    ];

    /// The element of the typed array named `name`. `Float16Array` has no
    /// Rust slice and is `None`.
    pub fn of_typed_array(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, n, _)| *n == name)
            .map(|(e, ..)| *e)
    }

    fn entry(self) -> &'static (Self, &'static str, &'static str) {
        Self::ALL.iter().find(|(e, ..)| *e == self).unwrap()
    }

    pub fn typed_array(self) -> &'static str {
        self.entry().1
    }

    /// The Rust scalar, e.g. `u8`.
    pub fn rust(self) -> &'static str {
        self.entry().2
    }

    /// Whether the Rust slice is wrapped in `wasm_bindgen::Clamped`.
    pub fn is_clamped(self) -> bool {
        self == Self::U8Clamped
    }

    /// Whether JS holds the values as `bigint` rather than `number`.
    pub fn is_bigint(self) -> bool {
        matches!(self, Self::I64 | Self::U64)
    }
}

/// The scalar set a `@rust` tag may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RustScalar {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Usize,
}

impl RustScalar {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            "usize" => Self::Usize,
            _ => return None,
        })
    }

    /// How a TS leaf type carries this scalar, if it can. wasm-bindgen
    /// passes `u64`/`i64` as `BigInt`, so on a `bigint` they cross
    /// natively; on a `number` the binding stays `f64` and an inline
    /// wrapper converts. Every other scalar is a JS `number`.
    pub fn apply_to(self, leaf: &TypeRef) -> Option<TypeRef> {
        match (self, leaf) {
            (Self::U64 | Self::I64, TypeRef::BigInt) => Some(TypeRef::RustScalar(self)),
            (Self::U64 | Self::I64, TypeRef::Number) => Some(TypeRef::WideScalar(self)),
            (Self::U64 | Self::I64, _) => None,
            (_, TypeRef::Number) => Some(TypeRef::RustScalar(self)),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Usize => "usize",
        }
    }
}

impl TypeRef {
    /// The type of an optional (`x?: T`) property as read: `T | undefined`,
    /// folded into an existing `Nullable` rather than nesting.
    pub fn or_undefined(self) -> TypeRef {
        match self {
            TypeRef::Nullable(inner, n) => {
                TypeRef::Nullable(inner, n.union(Nullability::UNDEFINED))
            }
            other => TypeRef::Nullable(Box::new(other), Nullability::UNDEFINED),
        }
    }

    /// Replace the scalar leaf of `self` (through `Nullable`, `Promise`,
    /// and `T[]`) with `scalar`, i.e. what a `@rust` tag applies to.
    /// `None` when the leaf cannot carry `scalar` (see
    /// [`RustScalar::apply_to`]).
    pub fn with_rust_scalar(&self, scalar: RustScalar) -> Option<TypeRef> {
        Some(match self {
            TypeRef::Nullable(inner, n) => {
                TypeRef::Nullable(Box::new(inner.with_rust_scalar(scalar)?), *n)
            }
            TypeRef::Array(inner) => TypeRef::Array(Box::new(inner.with_rust_scalar(scalar)?)),
            TypeRef::Reference {
                segments,
                generic_args,
            } if segments.len() == 1 && segments[0] == "Promise" && generic_args.len() == 1 => {
                TypeRef::generic("Promise", vec![generic_args[0].with_rust_scalar(scalar)?])
            }
            leaf => scalar.apply_to(leaf)?,
        })
    }

    /// Whether a `@rust` scalar appears anywhere in `self`.
    pub fn has_rust_scalar(&self) -> bool {
        match self {
            TypeRef::RustScalar(_) | TypeRef::WideScalar(_) => true,
            TypeRef::Nullable(inner, _) | TypeRef::Array(inner) => inner.has_rust_scalar(),
            TypeRef::Reference { generic_args, .. } => {
                generic_args.iter().any(TypeRef::has_rust_scalar)
            }
            TypeRef::Union(items) | TypeRef::Intersection(items) | TypeRef::Tuple(items) => {
                items.iter().any(TypeRef::has_rust_scalar)
            }
            _ => false,
        }
    }

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

    /// `self` with each bare single-segment reference to a name in `map`
    /// (type parameter names) replaced by its substitute, recursively.
    pub fn substitute<K>(&self, map: &HashMap<K, TypeRef>) -> TypeRef
    where
        K: std::borrow::Borrow<str> + Eq + std::hash::Hash,
    {
        let sub = |t: &TypeRef| t.substitute(map);
        match self {
            TypeRef::Reference {
                segments,
                generic_args,
            } => match (segments.as_slice(), map.get(segments[0].as_str())) {
                ([_], Some(arg)) if generic_args.is_empty() => arg.clone(),
                _ => TypeRef::Reference {
                    segments: segments.clone(),
                    generic_args: generic_args.iter().map(sub).collect(),
                },
            },
            TypeRef::Array(t) => TypeRef::Array(Box::new(sub(t))),
            TypeRef::Nullable(t, n) => TypeRef::Nullable(Box::new(sub(t)), *n),
            TypeRef::Union(ts) => TypeRef::Union(ts.iter().map(sub).collect()),
            TypeRef::Intersection(ts) => TypeRef::Intersection(ts.iter().map(sub).collect()),
            TypeRef::Tuple(ts) => TypeRef::Tuple(ts.iter().map(sub).collect()),
            TypeRef::IndexedAccess(o, i) => {
                TypeRef::IndexedAccess(Box::new(sub(o)), Box::new(sub(i)))
            }
            TypeRef::Function(sig) => TypeRef::Function(FunctionSig {
                params: sig
                    .params
                    .iter()
                    .map(|p| Param {
                        type_ref: sub(&p.type_ref),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(sub(&sig.return_type)),
            }),
            other => other.clone(),
        }
    }

    /// Members of the `string | number | symbol` union that
    /// [`TypeRef::PropertyKey`] abbreviates, in `lib.es5.d.ts` order.
    pub fn property_key_members() -> Vec<TypeRef> {
        vec![TypeRef::String, TypeRef::Number, TypeRef::Symbol]
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
            TypeRef::PropertyKey => "PropertyKey".into(),
            TypeRef::TypedSlice(e, _) if e.is_bigint() => "bigint[]".into(),
            TypeRef::TypedSlice(..) => "number[]".into(),
            TypeRef::RustScalar(s) | TypeRef::WideScalar(s) => s.as_str().into(),
            TypeRef::Array(inner) => {
                // Wrap composite inner types in parens so e.g.
                // `(A | B)[]` doesn't render as the wrong-precedence
                // `A | B[]`.
                let needs_parens = matches!(
                    inner.as_ref(),
                    TypeRef::Union(_) | TypeRef::Intersection(_) | TypeRef::Nullable(..)
                );
                if needs_parens {
                    format!("({})[]", inner.format_ts())
                } else {
                    format!("{}[]", inner.format_ts())
                }
            }
            TypeRef::Nullable(inner, n) => {
                let mut out = inner.format_ts();
                if n.null {
                    out.push_str(" | null");
                }
                if n.undefined {
                    out.push_str(" | undefined");
                }
                out
            }
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
            TypeRef::IndexedAccess(..) => "indexed access type".into(),
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
    /// Slice mode requested by a `{@slice mode}` JSDoc inline tag in this
    /// parameter's `@param` description (or the property doc, for a
    /// setter's value). Applied where the parameter's lowering is
    /// slice-shaped; dropped by shape otherwise, mirroring wasm-bindgen.
    pub slice_mode: Option<SliceModeHint>,
}

/// A slice mode named by a `{@slice view|array|copy}` JSDoc inline tag,
/// mapping 1:1 to wasm-bindgen's `#[wasm_bindgen(slice = "..")]`.
///
/// `View` is the author asserting, as a fact about the host API, that the
/// callee neither retains the buffer past the call nor re-enters wasm —
/// ts-gen never infers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliceModeHint {
    View,
    Array,
    Copy,
}

impl SliceModeHint {
    /// Parse the mode name from a `{@slice <mode>}` tag.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "view" => Some(Self::View),
            "array" => Some(Self::Array),
            "copy" => Some(Self::Copy),
            _ => None,
        }
    }

    /// Widen a bare typed-array reference under a `view` / `copy` hint
    /// into the `TypedArray | number[]` (or `bigint[]`) union shape, so
    /// the hinted type takes the element-slice lowering with the typed
    /// array kept as a handle overload — "Rust callers hold native
    /// elements here". `Nullable` is preserved around the union.
    pub fn widen_bare_typed_array(self, ty: &TypeRef) -> Option<TypeRef> {
        if self == Self::Array {
            return None;
        }
        if let TypeRef::Nullable(inner, n) = ty {
            return self
                .widen_bare_typed_array(inner)
                .map(|t| TypeRef::Nullable(Box::new(t), *n));
        }
        let TypeRef::Reference { segments, .. } = ty else {
            return None;
        };
        let elem = (segments.len() == 1)
            .then(|| TypedArrayElement::of_typed_array(&segments[0]))
            .flatten()?;
        let array_elem = if elem.is_bigint() {
            TypeRef::BigInt
        } else {
            TypeRef::Number
        };
        Some(TypeRef::Union(vec![
            ty.clone(),
            TypeRef::Array(Box::new(array_elem)),
        ]))
    }
}

/// Which forms of a string return a `{@string copy|ref|both}` JSDoc inline
/// tag asks for under per-monomorphization, where a string-leaf return
/// otherwise gets both a `String` binding and a `_js_string` twin
/// returning the handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringModeHint {
    /// Only the copied `String` form.
    Copy,
    /// Only the `JsString` handle form, under the base name.
    Ref,
    /// Both forms (the default); lets a member opt back in under a
    /// declaration-level tag.
    Both,
}

impl StringModeHint {
    /// Parse the mode name from a `{@string <mode>}` tag.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "copy" => Some(Self::Copy),
            "ref" => Some(Self::Ref),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
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

impl TypeParam {
    /// A parameter constrained to a primitive (`Key extends string`, or a
    /// union of literals of one primitive) can only be instantiated with
    /// subtypes that share the primitive's ABI, so it has no useful Rust
    /// generic form and reads as the primitive itself.
    pub fn erasure(&self) -> Option<crate::parse::scope::Erasure> {
        use crate::parse::scope::Erasure;
        fn of(ty: &TypeRef) -> Option<Erasure> {
            match ty {
                TypeRef::String | TypeRef::StringLiteral(_) => Some(Erasure::String),
                TypeRef::Number | TypeRef::NumberLiteral(_) => Some(Erasure::Number),
                TypeRef::Boolean | TypeRef::BooleanLiteral(_) => Some(Erasure::Boolean),
                TypeRef::BigInt => Some(Erasure::BigInt),
                TypeRef::Union(items) => {
                    let mut iter = items.iter().map(of);
                    let first = iter.next()??;
                    iter.all(|e| e == Some(first)).then_some(first)
                }
                _ => None,
            }
        }
        of(self.constraint.as_ref()?)
    }
}

// ─── Top-level Module ────────────────────────────────────────────────

/// A parsed module — references types and scopes in the `GlobalContext`.
#[derive(Clone, Debug)]

pub struct Module {
    /// Type ids for declarations from the input files (what to generate code for).
    pub types: Vec<TypeId>,
    /// Optional library name from `--lib-name`.
    pub lib_name: Option<String>,
    /// The global scope: TypeScript's `globals`, the exports of the
    /// `globalThis` namespace. It holds the built-in types and the globals
    /// of every file, merged, and is consulted last in name resolution.
    pub global_scope: ScopeId,
    /// Input file scope ids (for scope-based codegen iteration).
    pub file_scopes: Vec<ScopeId>,
    /// The globals each input file declares, by canonical path: a script's
    /// top-level declarations and a module's `declare global` blocks. A
    /// file without `--lib-name` has no specifier, so it is read as a
    /// script whether or not it has imports or exports.
    pub file_roots: Vec<(std::path::PathBuf, Root)>,
    /// The modules the input files declare, by specifier: a `--lib-name`
    /// module file and each `declare module` body.
    pub module_roots: Vec<(String, Root)>,
}

/// An export map, and the scope whose declarations its local names refer
/// to. A file's globals bind in the global scope, where every file's
/// declarations of a name have merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root {
    pub exports: ScopeId,
    pub bindings: ScopeId,
}

#[derive(Clone, Debug)]

pub struct TypeDeclaration {
    pub kind: TypeKind,
    pub module_context: ModuleContext,
    pub doc: Option<String>,
    /// The scope this declaration was defined in (for type reference resolution).
    pub scope_id: crate::parse::scope::ScopeId,
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
    /// A TS `enum` until parsing ends; see [`TsEnumDecl`].
    TsEnum(TsEnumDecl),
    Function(FunctionDecl),
    Variable(VariableDecl),
    Namespace(NamespaceDecl),
}

// ─── Class ───────────────────────────────────────────────────────────

/// A type an `extends` clause names, with the body scope of the declaration
/// whose clause it is, where it resolves: a merged declaration's parents
/// can come from declarations in different scopes.
#[derive(Clone, Debug, PartialEq)]
pub struct Heritage {
    pub type_ref: TypeRef,
    pub scope: crate::parse::scope::ScopeId,
}

#[derive(Clone, Debug)]

pub struct ClassDecl {
    pub name: String,
    /// Original JS name (may differ from Rust name after case conversion).
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    /// Immediate parent class.
    pub extends: Option<Heritage>,
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
    /// A declaration-level `{@string mode}`, the default for members
    /// carrying none. Parsing applies it to the declaration's own members;
    /// it is kept so `--annotate` can apply a side declaration's default to
    /// the input's members.
    pub string_mode: Option<StringModeHint>,
}

// ─── Interface ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct InterfaceDecl {
    pub name: String,
    pub js_name: String,
    pub type_params: Vec<TypeParam>,
    pub extends: Vec<Heritage>,
    pub members: Vec<Member>,
    /// Its construct signatures (`new (…): T`), overloads in declaration
    /// order. Only a constructor `var` the interface types binds them, as
    /// its class's constructors.
    pub construct: Vec<ConstructorMember>,
    /// Classification determined during assembly.
    pub classification: InterfaceClassification,
    /// Scope inside this interface's body — see
    /// [`ClassDecl::body_scope`] for the contract.
    pub body_scope: crate::parse::scope::ScopeId,
    /// See [`ClassDecl::string_mode`].
    pub string_mode: Option<StringModeHint>,
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

// ─── TS Enum ─────────────────────────────────────────────────────────

/// A TS `enum` declaration with its member initialisers unevaluated, since
/// they can reference enums declared later or in another file. Once every
/// file is parsed, each enum is evaluated across all its declarations and
/// replaced by a [`StringEnumDecl`] or [`NumericEnumDecl`].
#[derive(Clone, Debug)]
pub struct TsEnumDecl {
    pub name: String,
    pub members: Vec<TsEnumMember>,
    /// Declared in the global scope, at a script file's top level or in a
    /// `declare global` block, so it merges with the global declarations of
    /// other files.
    pub global: bool,
}

#[derive(Clone, Debug)]
pub struct TsEnumMember {
    pub name: String,
    /// `None` when the member continues from the previous one.
    pub init: Option<EnumInit>,
    pub doc: Option<String>,
}

/// An enum member initialiser, in the forms TS evaluates at compile time.
#[derive(Clone, Debug)]
pub enum EnumInit {
    Num(f64),
    Str(String),
    /// A member, bare or qualified by its enum's path (`A`, `E.A`,
    /// `Ns.E["A"]`).
    Ref(Vec<String>),
    Unary(String, Box<EnumInit>),
    Binary(String, Box<EnumInit>, Box<EnumInit>),
    /// Anything else, which has no constant value.
    Unknown,
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
    /// See [`StringModeHint`].
    pub string_mode: Option<StringModeHint>,
}

#[derive(Clone, Debug)]

pub struct FunctionOverload {
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    /// Scope inside this overload's declaration, holding its own type
    /// parameters.
    pub body_scope: crate::parse::scope::ScopeId,
    pub throws: Throws,
    pub doc: Option<String>,
    /// See [`StringModeHint`].
    pub string_mode: Option<StringModeHint>,
}

// ─── Variable ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct VariableDecl {
    pub name: String,
    pub js_name: String,
    pub type_ref: TypeRef,
    pub is_const: bool,
    /// The construct signatures of a constructor-shaped variable whose
    /// name a same-scope type alias takes as the type, as with
    /// `type RpcStub<T> = Rpc.Stub<T>` + `const RpcStub: { new <T>(value:
    /// T): Rpc.Stub<T> }`. Each constructs the type it returns, `js_class`
    /// being the variable, when codegen can bind it so.
    pub constructs: Vec<FunctionDecl>,
}

// ─── Namespace ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]

pub struct NamespaceDecl {
    pub name: String,
    /// Member declarations in source order. Members live in the type arena
    /// like top-level declarations, so a scope lookup through the namespace
    /// reaches the same declaration that is emitted.
    pub declarations: Vec<crate::context::TypeId>,
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

impl Member {
    /// The member with the parent's type parameters replaced, for
    /// inheriting it through `extends Parent<Arg>`.
    pub fn substitute(&self, map: &HashMap<String, TypeRef>) -> Member {
        let params = |ps: &[Param]| -> Vec<Param> {
            ps.iter()
                .map(|p| Param {
                    type_ref: p.type_ref.substitute(map),
                    ..p.clone()
                })
                .collect()
        };
        match self {
            Member::Getter(g) => Member::Getter(GetterMember {
                type_ref: g.type_ref.substitute(map),
                ..g.clone()
            }),
            Member::Setter(st) => Member::Setter(SetterMember {
                type_ref: st.type_ref.substitute(map),
                ..st.clone()
            }),
            Member::Method(m) => Member::Method(MethodMember {
                params: params(&m.params),
                return_type: m.return_type.substitute(map),
                ..m.clone()
            }),
            Member::Constructor(c) => Member::Constructor(ConstructorMember {
                params: params(&c.params),
                return_type: c.return_type.as_ref().map(|t| t.substitute(map)),
                ..c.clone()
            }),
            Member::IndexSignature(i) => Member::IndexSignature(IndexSigMember {
                key_type: i.key_type.substitute(map),
                value_type: i.value_type.substitute(map),
                ..i.clone()
            }),
            Member::StaticGetter(g) => Member::StaticGetter(StaticGetterMember {
                type_ref: g.type_ref.substitute(map),
                ..g.clone()
            }),
            Member::StaticSetter(st) => Member::StaticSetter(StaticSetterMember {
                type_ref: st.type_ref.substitute(map),
                ..st.clone()
            }),
            Member::StaticMethod(m) => Member::StaticMethod(StaticMethodMember {
                params: params(&m.params),
                return_type: m.return_type.substitute(map),
                ..m.clone()
            }),
        }
    }
}

/// Instance property getter binding.
///
/// Emits: `fn <name>(this: &T) -> R;` with `#[wasm_bindgen(method, getter)]`.
#[derive(Clone, Debug)]

pub struct GetterMember {
    /// The scope its types resolve in: the body scope of the declaration it
    /// is written in, which a merged declaration's other members needn't
    /// share.
    pub scope: crate::parse::scope::ScopeId,
    /// Original JS property name (source of truth for naming).
    pub js_name: String,
    /// Return type of the getter.
    pub type_ref: TypeRef,
    /// Whether declared with `?:` syntax — wraps return in `Option`.
    pub optional: bool,
    pub doc: Option<String>,
    /// `@throws {X}` makes the getter fallible (`Result<R, X>`); getters
    /// are otherwise infallible under every policy. See [`Throws`].
    pub throws: Throws,
    /// See [`StringModeHint`].
    pub string_mode: Option<StringModeHint>,
}

/// Instance property setter binding.
///
/// Emits: `fn set_<name>(this: &T, val: V);` with `#[wasm_bindgen(method, setter)]`.
#[derive(Clone, Debug)]

pub struct SetterMember {
    /// The scope its types resolve in: the body scope of the declaration it
    /// is written in, which a merged declaration's other members needn't
    /// share.
    pub scope: crate::parse::scope::ScopeId,
    /// Original JS property name (source of truth for naming).
    pub js_name: String,
    /// Parameter type of the setter.
    pub type_ref: TypeRef,
    pub doc: Option<String>,
    /// Slice mode from a `{@slice mode}` inline tag in the property's
    /// JSDoc, applied to the setter's value argument.
    pub slice_mode: Option<SliceModeHint>,
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
    /// See [`StringModeHint`].
    pub string_mode: Option<StringModeHint>,
}

#[derive(Clone, Debug)]

pub struct ConstructorMember {
    /// The scope its types resolve in: the body scope of the declaration it
    /// is written in, which a merged declaration's other members needn't
    /// share, or a construct signature's own scope holding `type_params`.
    pub scope: crate::parse::scope::ScopeId,
    /// Own type parameters of a construct signature (`new <T>(…)`),
    /// as in the `declare var Foo: { new <T>(…): Foo<T> }` pattern.
    /// Class `constructor(…)` members have none.
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub doc: Option<String>,
    /// Failure-mode info from `@throws` — see [`Throws`].
    ///
    /// Note: constructors always emit `catch` per JS `new` semantics, so
    /// `Throws::Never` here is effectively a no-op for codegen — only
    /// `Throws::Type` (custom error type) changes behavior.
    pub throws: Throws,
    /// A construct signature's declared return (`new (…): Foo<Uint8Array>`);
    /// class `constructor(…)` members have none.
    pub return_type: Option<TypeRef>,
}

#[derive(Clone, Debug)]

pub struct IndexSigMember {
    /// The scope its types resolve in: the body scope of the declaration it
    /// is written in, which a merged declaration's other members needn't
    /// share.
    pub scope: crate::parse::scope::ScopeId,
    pub key_type: TypeRef,
    pub value_type: TypeRef,
    pub readonly: bool,
}

/// Static property getter binding.
///
/// Emits: `fn <name>() -> R;` with `#[wasm_bindgen(static_method_of = T, getter)]`.
#[derive(Clone, Debug)]

pub struct StaticGetterMember {
    /// The scope its types resolve in: the body scope of the declaration it
    /// is written in, which a merged declaration's other members needn't
    /// share.
    pub scope: crate::parse::scope::ScopeId,
    /// Original JS property name (source of truth for naming).
    pub js_name: String,
    pub type_ref: TypeRef,
    pub doc: Option<String>,
    /// See [`GetterMember::throws`].
    pub throws: Throws,
    /// See [`StringModeHint`].
    pub string_mode: Option<StringModeHint>,
}

/// Static property setter binding.
///
/// Emits: `fn set_<name>(val: V);` with `#[wasm_bindgen(static_method_of = T, setter)]`.
#[derive(Clone, Debug)]

pub struct StaticSetterMember {
    /// The scope its types resolve in: the body scope of the declaration it
    /// is written in, which a merged declaration's other members needn't
    /// share.
    pub scope: crate::parse::scope::ScopeId,
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
    /// See [`StringModeHint`].
    pub string_mode: Option<StringModeHint>,
}

// ─── Type Registry (First Pass) ──────────────────────────────────────

/// Registry built during Phase 1 of the first pass, mapping type names
/// to their kind and primary declaration context.
#[derive(Clone, Debug, Default)]

pub struct TypeRegistry {
    /// Each declared name by the scope declaring it. Names in different
    /// scopes are different declarations, so a module's `interface C`
    /// doesn't change a global `interface C` + `var C` class merge.
    pub types: HashMap<(crate::parse::scope::ScopeId, String), TypeInfo>,
    /// The exports scope of each module the file has a `declare module "x"`
    /// block of, shared with every other file's blocks of it.
    pub module_scopes: HashMap<String, crate::parse::scope::ScopeId>,
    /// Each `declare module "x"` block's own scope, holding its imports and
    /// locals, with its specifier, by the block's span start.
    pub module_blocks: HashMap<u32, (String, crate::parse::scope::ScopeId)>,
    /// Each namespace declaration's own scope, by the declaration's span
    /// start. Its `exports` is the namespace's member scope.
    pub namespace_blocks: HashMap<u32, crate::parse::scope::ScopeId>,
    /// Member scope of each namespace declared outside the `declare module
    /// "x"` that assigns it with `export =`, → `"x"`. Such a namespace is
    /// the module, so it is populated into the module rather than as a
    /// global.
    pub assigned_namespaces: HashMap<crate::parse::scope::ScopeId, String>,
    /// Constructor-shaped variables whose name a type alias in the same
    /// scope takes as the type. The alias keeps the binding.
    pub alias_constructors: std::collections::HashSet<(crate::parse::scope::ScopeId, String)>,
    /// Key of the export map listing the globals the file declares, if it
    /// declares any (see [`Module::file_roots`]).
    pub globals: Option<crate::parse::scope::ScopeId>,
}

impl TypeRegistry {
    /// The kind registered for `name` declared directly in `scope`.
    pub fn kind(&self, scope: crate::parse::scope::ScopeId, name: &str) -> Option<&RegisteredKind> {
        self.types
            .get(&(scope, name.to_string()))
            .map(|info| &info.kind)
    }

    pub fn kind_mut(
        &mut self,
        scope: crate::parse::scope::ScopeId,
        name: &str,
    ) -> Option<&mut RegisteredKind> {
        self.types
            .get_mut(&(scope, name.to_string()))
            .map(|info| &mut info.kind)
    }
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
    /// A TS `enum`, string or numeric once its members are evaluated.
    TsEnum,
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
        let ty = TypeRef::Nullable(Box::new(TypeRef::String), Nullability::NULL);
        assert_eq!(ty.format_ts(), "string | null");
        let ty = TypeRef::Nullable(Box::new(TypeRef::String), Nullability::UNDEFINED);
        assert_eq!(ty.format_ts(), "string | undefined");
        let ty = TypeRef::Nullable(Box::new(TypeRef::String), Nullability::BOTH);
        assert_eq!(ty.format_ts(), "string | null | undefined");
    }
}
