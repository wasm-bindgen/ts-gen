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
use crate::util::diagnostics::Span;

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

    /// Whether a binding here belongs to the host: the global object, or a
    /// builtin module, which is any specifier with a scheme (`node:fs`,
    /// `cloudflare:sockets`, `bun:sqlite`) or a bare Node builtin (`fs`).
    /// The host's async functions reject rather than throw, so calling one
    /// never throws synchronously.
    pub(crate) fn is_builtin_or_global(&self) -> bool {
        match self {
            ModuleContext::Global => true,
            ModuleContext::Module(module) => {
                has_builtin_scheme(module) || NODE_BUILTIN_MODULES.contains(&&**module)
            }
        }
    }
}

/// Whether `specifier` has a builtin's `scheme:` prefix. A URL
/// (`https://…`, `file:///…`) is not one, and neither is a package
/// registry's scheme (`npm:lodash`, `jsr:@std/path`) or an inline or
/// object URL's (`data:…`, `blob:…`).
fn has_builtin_scheme(specifier: &str) -> bool {
    match specifier.split_once(':') {
        Some((scheme, rest)) => {
            !scheme.is_empty()
                && !rest.starts_with("//")
                && !matches!(scheme, "npm" | "jsr" | "data" | "blob")
        }
        None => false,
    }
}

/// Node's `require("module").builtinModules` (Node 22), the bare
/// specifiers of its builtins. The `node:`-only ones (`node:test`,
/// `node:sqlite`, …) are not listed there and carry a colon anyway.
const NODE_BUILTIN_MODULES: &[&str] = &[
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_stream_duplex",
    "_stream_passthrough",
    "_stream_readable",
    "_stream_transform",
    "_stream_wrap",
    "_stream_writable",
    "_tls_common",
    "_tls_wrap",
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

// ─── Type References ─────────────────────────────────────────────────

/// The empties a type admits, `null`, `undefined` or both, as lowering
/// reads them off a union with [`TypeRef::split_empties`]. Never stored in
/// the IR, where the empties are union members.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nullability {
    Null,
    Undefined,
    Both,
}

impl Nullability {
    fn of(null: bool, undefined: bool) -> Option<Self> {
        match (null, undefined) {
            (true, true) => Some(Self::Both),
            (true, false) => Some(Self::Null),
            (false, true) => Some(Self::Undefined),
            (false, false) => None,
        }
    }

    pub fn admits_null(self) -> bool {
        matches!(self, Self::Null | Self::Both)
    }

    pub fn admits_undefined(self) -> bool {
        matches!(self, Self::Undefined | Self::Both)
    }

    pub fn with(self, other: Self) -> Self {
        Self::of(
            self.admits_null() || other.admits_null(),
            self.admits_undefined() || other.admits_undefined(),
        )
        .unwrap()
    }

    /// `null` and `undefined` members for these empties, in union order.
    fn members(self) -> impl Iterator<Item = TypeRef> {
        let null = self.admits_null().then_some(TypeRef::Null);
        let undefined = self.admits_undefined().then_some(TypeRef::Undefined);
        null.into_iter().chain(undefined)
    }
}

/// The members of a [`TypeRef::Union`], in normal form: nested unions
/// flattened, each member once, the value members in source order and the
/// empties last (`null` before `undefined`), with two or more members. A
/// `void` member is `undefined`. Built only by [`TypeRef::union`], so equal
/// unions compare equal and no empty is held twice.
#[derive(Clone, Debug, PartialEq)]
pub struct Union(Vec<TypeRef>);

impl Union {
    /// Every member, the empties last.
    pub fn members(&self) -> &[TypeRef] {
        &self.0
    }

    /// The members other than `null` and `undefined`.
    pub fn values(&self) -> &[TypeRef] {
        let n = self.0.iter().filter(|m| m.is_empty_type()).count();
        &self.0[..self.0.len() - n]
    }

    /// The empties among the members.
    pub fn nullability(&self) -> Option<Nullability> {
        Nullability::of(
            self.0.contains(&TypeRef::Null),
            self.0.contains(&TypeRef::Undefined),
        )
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

    // === Syntactic constructs ===
    /// `T[]` — the syntactic array shortcut. Distinct from
    /// `Array<T>` (which is a [`TypeRef::Reference`] to the global
    /// `Array` symbol and goes through name resolution). `T[]` is
    /// unaffected by local shadowing of `Array`.
    Array(Box<TypeRef>),
    /// `A | B`, in normal form (see [`Union`]). `null` and `undefined`
    /// are ordinary members: `T | null` is `Union[T, Null]`.
    Union(Union),
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

    // === Type-level constructs ===
    //
    // Evaluated over concrete operands by `parse::eval` once every file is
    // parsed; one that reaches codegen could not be evaluated and erases to
    // `JsValue`.
    /// `never`. A union drops it; alone it erases.
    Never,
    /// `keyof T`.
    KeyOf(Box<TypeRef>),
    /// `T[K]`.
    IndexedAccess(Box<TypeRef>, Box<TypeRef>),
    /// `{ [P in K]?: V }`.
    Mapped(Box<MappedType>),
    /// An inline `{ … }` whose members evaluation reads: the object of an
    /// indexed access. Elsewhere an inline object type is [`TypeRef::Object`]
    /// or hoists to an interface.
    ObjectLiteral(Vec<ObjectProp>),

    // === Fallback ===
    /// For TS constructs we can't represent (conditional types, template
    /// literals, etc.) — erased to `JsValue`.
    Unresolved(String),
}

/// `{ [P in K]?: V }`. Without `V` the type is homomorphic: each property
/// keeps its source type.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedType {
    /// The key type parameter, `P`.
    pub key: String,
    /// The key set, `K`.
    pub keys: TypeRef,
    pub value: Option<TypeRef>,
    pub optional: Option<Modifier>,
    pub readonly: Option<Modifier>,
}

/// A mapped type's `?` / `+?` / `-?` (or `readonly`) modifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Add,
    Remove,
}

/// A property of an inline object type.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectProp {
    pub name: String,
    pub type_ref: TypeRef,
    pub optional: bool,
    pub readonly: bool,
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

impl TypeRef {
    /// The union of `members` in normal form (see [`Union`]). A single
    /// surviving member is returned bare, so this is also how a union is
    /// widened (`TypeRef::union([ty, TypeRef::Undefined])`).
    pub fn union(members: impl IntoIterator<Item = TypeRef>) -> TypeRef {
        fn add(ty: TypeRef, values: &mut Vec<TypeRef>, null: &mut bool, undefined: &mut bool) {
            match ty {
                TypeRef::Union(Union(members)) => {
                    for m in members {
                        add(m, values, null, undefined);
                    }
                }
                TypeRef::Null => *null = true,
                TypeRef::Undefined | TypeRef::Void => *undefined = true,
                TypeRef::Never => {}
                other if values.contains(&other) => {}
                other => values.push(other),
            }
        }
        let (mut values, mut null, mut undefined) = (Vec::new(), false, false);
        for m in members {
            add(m, &mut values, &mut null, &mut undefined);
        }
        values.extend(
            Nullability::of(null, undefined)
                .into_iter()
                .flat_map(Nullability::members),
        );
        match values.len() {
            0 => TypeRef::Any,
            1 => values.pop().unwrap(),
            _ => TypeRef::Union(Union(values)),
        }
    }

    /// `self` without its empties, and which it admits; `None` when it
    /// admits neither, or is itself `null` or `undefined`. With nothing
    /// else, `null | undefined` is `null` admitting `undefined`.
    pub fn split_empties(&self) -> Option<(TypeRef, Nullability)> {
        let TypeRef::Union(u) = self else {
            return None;
        };
        let n = u.nullability()?;
        Some(match u.values() {
            [] => (TypeRef::Null, Nullability::Undefined),
            [one] => (one.clone(), n),
            values => (TypeRef::Union(Union(values.to_vec())), n),
        })
    }

    /// `self` admitting `n` as well.
    pub fn with_empties(self, n: Nullability) -> TypeRef {
        TypeRef::union(std::iter::once(self).chain(n.members()))
    }

    /// Whether `self` is a union admitting `undefined`.
    pub fn admits_undefined(&self) -> bool {
        self.split_empties()
            .is_some_and(|(_, n)| n.admits_undefined())
    }

    /// `self` without `undefined`.
    pub fn without_undefined(&self) -> TypeRef {
        match self.split_empties() {
            Some((core, Nullability::Both)) => core.with_empties(Nullability::Null),
            Some((core, Nullability::Undefined)) => core,
            _ => self.clone(),
        }
    }

    fn is_empty_type(&self) -> bool {
        matches!(self, TypeRef::Null | TypeRef::Undefined)
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
            } => match (segments.as_slice(), generic_args.is_empty()) {
                ([name], true) if map.contains_key(name.as_str()) => map[name.as_str()].clone(),
                _ => TypeRef::Reference {
                    segments: segments.clone(),
                    generic_args: generic_args.iter().map(sub).collect(),
                },
            },
            TypeRef::Array(t) => TypeRef::Array(Box::new(sub(t))),
            TypeRef::Union(u) => TypeRef::union(u.members().iter().map(sub)),
            TypeRef::Intersection(ts) => TypeRef::Intersection(ts.iter().map(sub).collect()),
            TypeRef::Tuple(ts) => TypeRef::Tuple(ts.iter().map(sub).collect()),
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
            TypeRef::Array(inner) => {
                // Wrap composite inner types in parens so e.g.
                // `(A | B)[]` doesn't render as the wrong-precedence
                // `A | B[]`.
                let needs_parens =
                    matches!(inner.as_ref(), TypeRef::Union(_) | TypeRef::Intersection(_));
                if needs_parens {
                    format!("({})[]", inner.format_ts())
                } else {
                    format!("{}[]", inner.format_ts())
                }
            }
            TypeRef::Union(u) => u
                .members()
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
            TypeRef::Never => "never".into(),
            TypeRef::KeyOf(t) => format!("keyof {}", t.format_ts()),
            TypeRef::IndexedAccess(o, i) => format!("{}[{}]", o.format_ts(), i.format_ts()),
            TypeRef::Mapped(m) => format!(
                "{{ [{} in {}]{}: {} }}",
                m.key,
                m.keys.format_ts(),
                match m.optional {
                    Some(Modifier::Add) => "?",
                    Some(Modifier::Remove) => "-?",
                    None => "",
                },
                m.value.as_ref().map_or("…".to_string(), TypeRef::format_ts)
            ),
            TypeRef::ObjectLiteral(props) => format!(
                "{{ {} }}",
                props
                    .iter()
                    .map(|p| format!(
                        "{}{}: {}",
                        p.name,
                        if p.optional { "?" } else { "" },
                        p.type_ref.format_ts()
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
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
    /// `x?: T`, or a default initializer: whether the argument may be
    /// left out. Says nothing about the type, which is as written.
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
    /// elements here". The empties are kept beside the union.
    pub fn widen_bare_typed_array(self, ty: &TypeRef) -> Option<TypeRef> {
        if self == Self::Array {
            return None;
        }
        if let Some((core, n)) = ty.split_empties() {
            return self
                .widen_bare_typed_array(&core)
                .map(|t| t.with_empties(n));
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
        Some(TypeRef::union([
            ty.clone(),
            TypeRef::Array(Box::new(array_elem)),
        ]))
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
                    let mut iter = items.members().iter().map(of);
                    let first = iter.next()??;
                    iter.all(|e| e == Some(first)).then_some(first)
                }
                _ => None,
            }
        }
        of(self.constraint.as_ref()?)
    }

    /// Whether the constraint is written as string literals only (`K
    /// extends "a" | "b"`). On an interface such a parameter stays live for
    /// type-level evaluation, which specialises it at each instantiation
    /// rather than reading it as `string`.
    pub fn selects_string_literals(&self) -> bool {
        match &self.constraint {
            Some(TypeRef::StringLiteral(_)) => true,
            Some(TypeRef::Union(items)) => items
                .members()
                .iter()
                .all(|m| matches!(m, TypeRef::StringLiteral(_))),
            _ => false,
        }
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
    /// A `const enum`, whose members without an initialiser are numbered.
    /// In any other ambient enum, which every `.d.ts` enum is, they're
    /// computed: TypeScript gives them no value.
    pub is_const: bool,
    /// The enum's name, in the file its scope is declared in.
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TsEnumMember {
    pub name: String,
    /// `None` when the member has no initialiser.
    pub init: Option<EnumInit>,
    pub doc: Option<String>,
    pub span: Span,
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
    /// Declared `async`, so calling it never throws synchronously.
    pub async_modifier: bool,
    /// Scope inside this function's body — see [`ClassDecl::body_scope`].
    pub body_scope: crate::parse::scope::ScopeId,
}

#[derive(Clone, Debug)]

pub struct FunctionOverload {
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    /// Scope inside this overload's declaration, holding its own type
    /// parameters.
    pub body_scope: crate::parse::scope::ScopeId,
    pub throws: Throws,
    /// See [`FunctionDecl::async_modifier`].
    pub async_modifier: bool,
    pub doc: Option<String>,
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
    /// The property's declared type.
    pub type_ref: TypeRef,
    /// Whether declared with `?:`, so the property may be absent: a read
    /// admits `undefined` whatever the type says.
    pub optional: bool,
    pub doc: Option<String>,
    /// `@throws {X}` makes the getter fallible (`Result<R, X>`); getters
    /// are otherwise infallible under every policy. See [`Throws`].
    pub throws: Throws,
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
    /// See [`FunctionDecl::async_modifier`].
    pub async_modifier: bool,
    /// Scope inside this method's body — child of the enclosing
    /// type's body scope, holding [`Binding::TypeParam`] entries for
    /// the method's own `<T, ...>`. See [`ClassDecl::body_scope`].
    pub body_scope: crate::parse::scope::ScopeId,
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
    /// Whether declared with `?:`; see [`GetterMember::optional`].
    pub optional: bool,
    pub doc: Option<String>,
    /// See [`GetterMember::throws`].
    pub throws: Throws,
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
    /// See [`FunctionDecl::async_modifier`].
    pub async_modifier: bool,
    /// Scope inside this static method's body — see
    /// [`MethodMember::body_scope`].
    pub body_scope: crate::parse::scope::ScopeId,
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
    /// Type declarations TypeScript rejects as duplicates of an earlier
    /// declaration of their name, by the span start of the name. Phase 2
    /// doesn't populate them.
    pub duplicates: std::collections::HashSet<u32>,
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

/// The kinds of type declaration a name has in the container its
/// declarations merge in, as TypeScript's binder records them in a symbol's
/// flags. A declaration whose kind excludes one the name already has is a
/// duplicate (TS's `SymbolFlags*Excludes`). Values and namespaces merge with
/// any type declaration, so only these four are tracked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeclFlags(u8);

impl DeclFlags {
    pub const CLASS: Self = Self(1);
    pub const INTERFACE: Self = Self(1 << 1);
    pub const ENUM: Self = Self(1 << 2);
    pub const TYPE_ALIAS: Self = Self(1 << 3);

    const KINDS: [(Self, &'static str); 4] = [
        (Self::CLASS, "class"),
        (Self::INTERFACE, "interface"),
        (Self::ENUM, "enum"),
        (Self::TYPE_ALIAS, "type alias"),
    ];

    /// What a declaration of each kind in `self` can't merge with: a class
    /// merges only with interfaces, an interface with interfaces and
    /// classes, an enum with enums, and a type alias with none of them.
    fn excludes(self) -> Self {
        let mut excludes = 0;
        for (kind, _) in Self::KINDS {
            if self.0 & kind.0 != 0 {
                excludes |= match kind {
                    Self::CLASS => Self::CLASS.0 | Self::ENUM.0 | Self::TYPE_ALIAS.0,
                    Self::INTERFACE => Self::ENUM.0 | Self::TYPE_ALIAS.0,
                    Self::ENUM => Self::CLASS.0 | Self::INTERFACE.0 | Self::TYPE_ALIAS.0,
                    _ => u8::MAX,
                };
            }
        }
        Self(excludes)
    }

    /// The kinds among `existing` a declaration of this kind is a duplicate
    /// of; empty when it merges with them.
    pub fn conflicts(self, existing: Self) -> Self {
        Self(self.excludes().0 & existing.0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// The kinds by keyword, `"class and interface"`.
    pub fn describe(self) -> String {
        Self::KINDS
            .iter()
            .filter(|(kind, _)| self.0 & kind.0 != 0)
            .map(|&(_, name)| name)
            .collect::<Vec<_>>()
            .join(" and ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decl_flags_follow_typescript_excludes() {
        use DeclFlags as F;
        let allowed = [
            (F::INTERFACE, F::INTERFACE),
            (F::INTERFACE, F::CLASS),
            (F::CLASS, F::INTERFACE),
            (F::ENUM, F::ENUM),
        ];
        for (new, existing) in allowed {
            assert!(
                new.conflicts(existing).is_empty(),
                "{new:?} after {existing:?}"
            );
        }
        let rejected = [
            (F::TYPE_ALIAS, F::TYPE_ALIAS),
            (F::TYPE_ALIAS, F::INTERFACE),
            (F::INTERFACE, F::TYPE_ALIAS),
            (F::TYPE_ALIAS, F::CLASS),
            (F::CLASS, F::TYPE_ALIAS),
            (F::TYPE_ALIAS, F::ENUM),
            (F::ENUM, F::TYPE_ALIAS),
            (F::CLASS, F::CLASS),
            (F::CLASS, F::ENUM),
            (F::ENUM, F::CLASS),
            (F::INTERFACE, F::ENUM),
            (F::ENUM, F::INTERFACE),
        ];
        for (new, existing) in rejected {
            assert_eq!(
                new.conflicts(existing),
                existing,
                "{new:?} after {existing:?}"
            );
        }
        // A second class conflicts with the earlier class, not with the
        // interface it merged with.
        let mut merged = F::INTERFACE;
        merged.insert(F::CLASS);
        assert_eq!(F::CLASS.conflicts(merged), F::CLASS);
        assert!(F::INTERFACE.conflicts(merged).is_empty());
        assert_eq!(
            F::TYPE_ALIAS.conflicts(merged).describe(),
            "class and interface"
        );
    }

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
        let ty = TypeRef::union([
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
        let ty = TypeRef::Array(Box::new(TypeRef::union([TypeRef::String, TypeRef::Number])));
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
            vec![TypeRef::union([
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
    fn format_ts_empties() {
        let ty = TypeRef::union([TypeRef::Undefined, TypeRef::String, TypeRef::Null]);
        assert_eq!(ty.format_ts(), "string | null | undefined");
        assert_eq!(
            TypeRef::String
                .with_empties(Nullability::Undefined)
                .format_ts(),
            "string | undefined"
        );
    }

    #[test]
    fn union_normal_form() {
        let s = || TypeRef::String;
        let n = || TypeRef::Number;
        let members = |ty: TypeRef| match ty {
            TypeRef::Union(u) => u.members().to_vec(),
            other => panic!("expected a union, got {other:?}"),
        };
        // Flattened, each member once, values in source order, empties last.
        assert_eq!(
            members(TypeRef::union([
                TypeRef::Undefined,
                s(),
                TypeRef::union([n(), TypeRef::Null]),
                s(),
                TypeRef::Void,
                TypeRef::Null,
            ])),
            vec![s(), n(), TypeRef::Null, TypeRef::Undefined]
        );
        // Equal types compare equal however they were spelled.
        assert_eq!(
            TypeRef::union([TypeRef::Null, s(), TypeRef::Undefined]),
            TypeRef::union([TypeRef::union([s(), TypeRef::Undefined]), TypeRef::Null])
        );
        // One value with empties is still a union; one member alone is bare.
        assert_eq!(
            members(TypeRef::union([s(), TypeRef::Null])),
            vec![s(), TypeRef::Null]
        );
        assert_eq!(TypeRef::union([s(), s()]), s());
        assert_eq!(
            TypeRef::union([TypeRef::Undefined, TypeRef::Void]),
            TypeRef::Undefined
        );
        assert_eq!(
            members(TypeRef::union([TypeRef::Undefined, TypeRef::Null])),
            vec![TypeRef::Null, TypeRef::Undefined]
        );
    }

    #[test]
    fn split_empties_reads_the_empties_off_the_members() {
        let s = || TypeRef::String;
        let both = TypeRef::union([s(), TypeRef::Null, TypeRef::Undefined]);
        assert_eq!(both.split_empties(), Some((s(), Nullability::Both)));
        assert!(both.admits_undefined());
        assert_eq!(
            both.without_undefined(),
            TypeRef::union([s(), TypeRef::Null])
        );
        let pair = TypeRef::union([s(), TypeRef::Number, TypeRef::Null]);
        assert_eq!(
            pair.split_empties(),
            Some((TypeRef::union([s(), TypeRef::Number]), Nullability::Null))
        );
        assert_eq!(
            TypeRef::union([TypeRef::Null, TypeRef::Undefined]).split_empties(),
            Some((TypeRef::Null, Nullability::Undefined))
        );
        assert_eq!(TypeRef::Null.split_empties(), None);
        assert_eq!(TypeRef::union([s(), TypeRef::Number]).split_empties(), None);
        assert_eq!(
            s().with_empties(Nullability::Null)
                .with_empties(Nullability::Undefined),
            both
        );
    }
}
