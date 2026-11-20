//! The IR model (IR.md §3–§19).
//!
//! The model is syntactic: paths are stored as written and resolved on
//! demand by [`crate::resolve`]. Normalizations the text format defines as
//! equivalent are applied on parse, so equal IRs compare equal: an absent
//! result is [`Type::Unit`], `future<T>` and a bare `throws` name `err`, and
//! several `@js(...)` attributes on one node are one entry list.

use std::fmt;

/// A 1-based source position. Positions never take part in equality, so an
/// IR compares equal to the one read back from its printed text.
#[derive(Clone, Copy, Debug, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl PartialEq for Span {
    fn eq(&self, _: &Span) -> bool {
        true
    }
}

impl Eq for Span {}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Names reserved for built-in types (§2). A user declaration with one of
/// these names is written escaped, with `@js(name = "...")` carrying the JS
/// name.
pub const BUILTIN_TYPE_NAMES: &[&str] = &[
    "unit", "bool", "i8", "u8", "i16", "u16", "i32", "u32", "i53", "u53", "i64", "u64", "f16",
    "f32", "f64", "str", "arr", "future", "fn", "Any", "err", "Bool", "I8", "U8", "I16", "U16",
    "I32", "U32", "I53", "U53", "I64", "U64", "F16", "F32", "F64", "Str", "Unit", "Vec", "Arr",
    "Tuple",
];

/// A name a lexical path can't start with, which reads as a built-in type
/// (IR.md §4).
pub fn is_builtin_name(s: &str) -> bool {
    BUILTIN_TYPE_NAMES.contains(&s) || s == "mut"
}

/// The built-in error enum (§15).
pub const ERR: &str = "err";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ir {
    /// `lib name;`: this IR is a lib other IRs resolve against (§5).
    pub lib: Option<String>,
    /// `with name;`: the libs this IR resolves against, in order.
    pub with: Vec<String>,
    pub items: Vec<Decl>,
}

/// What every declaration, member and arm carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub doc: Option<String>,
    pub attrs: Attrs,
    pub span: Span,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attrs {
    /// Every `@js(...)` entry, in source order.
    pub js: Vec<JsEntry>,
    pub fallback: bool,
    pub once: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decl {
    Namespace(Namespace),
    Class(Class),
    Enum(Enum),
    Module(Module),
    Member(Member),
    Alias(Alias),
}

impl Decl {
    pub fn meta(&self) -> &Meta {
        match self {
            Decl::Namespace(d) => &d.meta,
            Decl::Class(d) => &d.meta,
            Decl::Enum(d) => &d.meta,
            Decl::Module(d) => &d.meta,
            Decl::Member(d) => &d.meta,
            Decl::Alias(d) => &d.meta,
        }
    }

    /// The declared name; `None` for a module block and a constructor.
    pub fn name(&self) -> Option<&str> {
        match self {
            Decl::Namespace(d) => Some(&d.name),
            Decl::Class(d) => Some(&d.name),
            Decl::Enum(d) => Some(&d.name),
            Decl::Module(_) => None,
            Decl::Member(d) => d.kind.name(),
            Decl::Alias(d) => Some(&d.name),
        }
    }
}

/// `type Name<T> = ty;` (§3): another name for `ty`. Keys and protocol
/// names spell its expansion, but for a reference back into the alias
/// from inside it (§6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    pub meta: Meta,
    pub name: String,
    pub tparams: Vec<TypeParam>,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Namespace {
    pub meta: Meta,
    pub export: bool,
    pub name: String,
    pub items: Vec<Decl>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClassKind {
    /// Nominal: gets `("tw:instanceof", path)`.
    Class,
    /// Structural: never checked.
    Interface,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Class {
    pub meta: Meta,
    pub kind: ClassKind,
    /// `export class` (§17). Interfaces are never exported.
    pub export: bool,
    pub name: String,
    pub tparams: Vec<TypeParam>,
    /// Ordered; the first parent is primary.
    pub extends: Vec<TypeRef>,
    pub assoc: Vec<Assoc>,
    pub members: Vec<Member>,
}

/// `type Name = match P { A => T, B => U };` (§13): a type that varies
/// with the argument of `P`, a concretely bounded type parameter of the
/// class, with one row per element of its `in` bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assoc {
    pub meta: Meta,
    pub name: String,
    pub param: String,
    /// `(element, type)`, in the bound's order.
    pub rows: Vec<(Type, Type)>,
}

impl Type {
    /// Visit `self` and every type inside it, outermost first.
    pub fn walk(&self, f: &mut impl FnMut(&Type)) {
        f(self);
        match self {
            Type::Vec(t)
            | Type::VecBox(t)
            | Type::MutVec(t)
            | Type::Array(t)
            | Type::ArrBox(t)
            | Type::Future(t, _)
            | Type::Nullable(t, _)
            | Type::View { elem: t, .. } => t.walk(f),
            Type::Tuple(ts) | Type::TupleBox(ts) => ts.iter().for_each(|t| t.walk(f)),
            Type::Fn(fun) => {
                fun.params.iter().for_each(|t| t.walk(f));
                fun.result.walk(f);
            }
            Type::Named(r) => r.args.iter().for_each(|t| t.walk(f)),
            _ => {}
        }
    }
}

impl Type {
    /// `self` with each named type that `f` maps replaced, innermost
    /// last: a replacement isn't mapped again.
    pub fn map_named(&self, f: &impl Fn(&TypeRef) -> Option<Type>) -> Type {
        let m = |t: &Type| t.map_named(f);
        let mb = |t: &Type| Box::new(m(t));
        match self {
            Type::Vec(t) => Type::Vec(mb(t)),
            Type::VecBox(t) => Type::VecBox(mb(t)),
            Type::MutVec(t) => Type::MutVec(mb(t)),
            Type::Array(t) => Type::Array(mb(t)),
            Type::ArrBox(t) => Type::ArrBox(mb(t)),
            Type::Tuple(ts) => Type::Tuple(ts.iter().map(m).collect()),
            Type::TupleBox(ts) => Type::TupleBox(ts.iter().map(m).collect()),
            Type::Future(t, e) => Type::Future(mb(t), e.clone()),
            Type::Nullable(t, js_null) => Type::Nullable(mb(t), *js_null),
            Type::View { mutable, elem } => Type::View {
                mutable: *mutable,
                elem: mb(elem),
            },
            Type::Fn(fun) => Type::Fn(Box::new(FnType {
                params: fun.params.iter().map(m).collect(),
                rest: fun.rest,
                result: m(&fun.result),
                throws: fun.throws.clone(),
            })),
            Type::Named(r) => f(r).unwrap_or_else(|| {
                Type::Named(TypeRef {
                    path: r.path.clone(),
                    args: r.args.iter().map(m).collect(),
                })
            }),
            t => t.clone(),
        }
    }
}

impl Assoc {
    /// The type for the argument `arg`, written as the bound's element.
    pub fn row(&self, arg: &Type) -> Option<&Type> {
        self.rows.iter().find(|(k, _)| k == arg).map(|(_, t)| t)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeParam {
    pub name: String,
    pub bound: Option<Bound>,
    /// `T = D`: the type an erasing reading gives the parameter, and a
    /// mono one gives an argument the language leaves out (IR.md §13).
    pub default: Option<Type>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    /// `T: C`.
    Subtype(Type),
    /// `T in (A, B)`.
    In(Vec<Type>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enum {
    pub meta: Meta,
    pub name: String,
    /// `enum E: <int>`. Exclusive with `extends` in the text; a child
    /// inherits its bases' carrier.
    pub carrier: Option<Scalar>,
    pub extends: Vec<Path>,
    pub arms: Vec<Arm>,
    /// `static` members, folded in from a same-named namespace (§4).
    pub members: Vec<Member>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    pub meta: Meta,
    pub name: String,
    /// Empty for a unit arm.
    pub payload: Vec<Type>,
    pub value: Option<ArmValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArmValue {
    /// `Name = 1;`
    Int(i64),
    /// `Name = .Other;`
    Alias(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub meta: Meta,
    pub spec: String,
    pub items: Vec<Decl>,
    /// `export { A, A as default };`. `None` when the block has no list.
    pub exports: Option<Vec<ExportName>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportName {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub meta: Meta,
    /// `export f()`: a top-level export (§17).
    pub export: bool,
    pub is_static: bool,
    pub kind: MemberKind,
    /// Method type parameters; empty for the other forms.
    pub tparams: Vec<TypeParam>,
    pub params: Vec<Param>,
    /// [`Type::Unit`] when absent. A constructor's result is the enclosing
    /// type, and is stored as `Unit`.
    pub result: Type,
    pub throws: Option<Path>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Method(String),
    Get(String),
    Set(String),
    /// `new(...)`, or `new name(...)`.
    New(Option<String>),
}

impl MemberKind {
    pub fn name(&self) -> Option<&str> {
        match self {
            MemberKind::Method(n) | MemberKind::Get(n) | MemberKind::Set(n) => Some(n),
            MemberKind::New(n) => n.as_deref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    /// Inline `@once`.
    pub once: bool,
    /// Inline `@js(unshared)`: the callee rejects a view of shared memory.
    pub unshared: bool,
    /// `x: union E`: each arm of the payload enum `E` makes the same JS
    /// call, so a generator may expand it (§7).
    pub union: bool,
    pub span: Span,
}

/// A dotted path. Segments are unquoted; a segment that isn't an
/// identifier is printed as a string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Path {
    pub base: Base,
    pub segments: Vec<String>,
}

/// Where a path resolves from (IR.md §4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Base {
    /// `A.B`: the enclosing containers, innermost first.
    #[default]
    Lexical,
    /// `.A.B`: the IR's top level, past any nearer declaration.
    Root,
    /// `"spec".A.B`: that `module` block's top level.
    Module(String),
}

impl Path {
    pub fn new<S: Into<String>>(segments: impl IntoIterator<Item = S>) -> Path {
        Path {
            base: Base::Lexical,
            segments: segments.into_iter().map(Into::into).collect(),
        }
    }

    pub fn rooted<S: Into<String>>(segments: impl IntoIterator<Item = S>) -> Path {
        Path {
            base: Base::Root,
            ..Path::new(segments)
        }
    }

    pub fn in_module<S: Into<String>>(spec: &str, segments: impl IntoIterator<Item = S>) -> Path {
        Path {
            base: Base::Module(spec.to_string()),
            ..Path::new(segments)
        }
    }

    pub fn err() -> Path {
        Path::new([ERR])
    }

    pub fn is_err(&self) -> bool {
        self.base == Base::Lexical && self.segments.len() == 1 && self.segments[0] == ERR
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeRef {
    pub path: Path,
    pub args: Vec<Type>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scalar {
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I53,
    U53,
    I64,
    U64,
    F16,
    F32,
    F64,
}

impl Scalar {
    pub const ALL: [Scalar; 14] = [
        Scalar::Bool,
        Scalar::I8,
        Scalar::U8,
        Scalar::I16,
        Scalar::U16,
        Scalar::I32,
        Scalar::U32,
        Scalar::I53,
        Scalar::U53,
        Scalar::I64,
        Scalar::U64,
        Scalar::F16,
        Scalar::F32,
        Scalar::F64,
    ];

    /// The unboxed spelling, `u32`.
    pub fn name(self) -> &'static str {
        match self {
            Scalar::Bool => "bool",
            Scalar::I8 => "i8",
            Scalar::U8 => "u8",
            Scalar::I16 => "i16",
            Scalar::U16 => "u16",
            Scalar::I32 => "i32",
            Scalar::U32 => "u32",
            Scalar::I53 => "i53",
            Scalar::U53 => "u53",
            Scalar::I64 => "i64",
            Scalar::U64 => "u64",
            Scalar::F16 => "f16",
            Scalar::F32 => "f32",
            Scalar::F64 => "f64",
        }
    }

    /// The box spelling, `U32`.
    pub fn box_name(self) -> &'static str {
        match self {
            Scalar::Bool => "Bool",
            Scalar::I8 => "I8",
            Scalar::U8 => "U8",
            Scalar::I16 => "I16",
            Scalar::U16 => "U16",
            Scalar::I32 => "I32",
            Scalar::U32 => "U32",
            Scalar::I53 => "I53",
            Scalar::U53 => "U53",
            Scalar::I64 => "I64",
            Scalar::U64 => "U64",
            Scalar::F16 => "F16",
            Scalar::F32 => "F32",
            Scalar::F64 => "F64",
        }
    }

    pub fn from_name(s: &str) -> Option<Scalar> {
        Scalar::ALL.into_iter().find(|t| t.name() == s)
    }

    pub fn from_box_name(s: &str) -> Option<Scalar> {
        Scalar::ALL.into_iter().find(|t| t.box_name() == s)
    }

    /// Valid as an `enum E: <int>` carrier.
    pub fn is_int_carrier(self) -> bool {
        matches!(
            self,
            Scalar::I8
                | Scalar::U8
                | Scalar::I16
                | Scalar::U16
                | Scalar::I32
                | Scalar::U32
                | Scalar::I64
                | Scalar::U64
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Unit,
    /// `never`: a result that never returns normally. Results only.
    Never,
    Scalar(Scalar),
    Str,
    Any,
    /// `Bool`, `I8` … `F64`.
    ScalarBox(Scalar),
    StrBox,
    /// `bigint`: an arbitrary-precision integer.
    BigInt,
    /// `BigInt`, the box of `bigint`: the host bigint.
    BigIntBox,
    /// `Unit`, the box of `unit`.
    UnitBox,
    /// `T[]`, the typed array.
    Vec(Box<Type>),
    VecBox(Box<Type>),
    /// `mut T[]`: writeback, parameters only.
    MutVec(Box<Type>),
    /// `arr<T>`, the JS `Array`.
    Array(Box<Type>),
    ArrBox(Box<Type>),
    /// `(A, B)`; never empty.
    Tuple(Vec<Type>),
    TupleBox(Vec<Type>),
    /// `future<T, E>`; `E` is [`Path::err`] for `future<T>`.
    Future(Box<Type>, Path),
    Fn(Box<FnType>),
    /// `*T[]` / `*mut T[]`: a view of the language's memory, import
    /// parameters only.
    View {
        mutable: bool,
        elem: Box<Type>,
    },
    /// `T?`, and `T??` nested (§9). With `@js(null)`, its absence crosses
    /// Wasm → JS as `null`.
    Nullable(Box<Type>, bool),
    /// A class, interface, enum or type parameter, resolved by
    /// [`crate::resolve`].
    Named(TypeRef),
}

impl Type {
    pub fn named<S: Into<String>>(segments: impl IntoIterator<Item = S>) -> Type {
        Type::Named(TypeRef {
            path: Path::new(segments),
            args: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FnType {
    pub params: Vec<Type>,
    /// The last parameter is a rest parameter (`fn(str, ...arr<f64>)`): JS
    /// passes its elements as the remaining arguments.
    pub rest: bool,
    /// [`Type::Unit`] when there's no `->`.
    pub result: Type,
    pub throws: Option<Path>,
}

/// One entry of `@js(...)` (§7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsEntry {
    /// `@js("GET")`: an enum arm's JS literal.
    Literal(String),
    /// `@js(TypeError | DOMException[name = "NotFoundError"])`: an error
    /// arm's class match.
    Class(Vec<ClassPred>),
    /// `name = "..."`: a rename, and only a rename.
    Name(String),
    Object,
    Index,
    /// `symbol = iterator`: a well-known symbol.
    Symbol(String),
    /// `this = p`.
    This(String),
    Args(Vec<Telem>),
    Super(Vec<Telem>),
    Let(String, Expr),
    Cancel(Expr),
    /// `new = a.b`: a constructor runs `new a.b(...)`, the JS value at that
    /// path of JS names, instead of `new` on its class (§6).
    New(Path),
}

/// A declaration's `@js` entries by kind.
pub trait JsEntries {
    fn rename(&self) -> Option<&str>;
    fn new_value(&self) -> Option<&Path>;
    fn symbol(&self) -> Option<&str>;
    fn args(&self) -> Option<&[Telem]>;
    fn super_args(&self) -> Option<&[Telem]>;
    fn cancel(&self) -> Option<&Expr>;
    fn literal(&self) -> Option<&str>;
    fn classes(&self) -> Option<&[ClassPred]>;
}

impl JsEntries for [JsEntry] {
    fn new_value(&self) -> Option<&Path> {
        self.iter().find_map(|j| match j {
            JsEntry::New(p) => Some(p),
            _ => None,
        })
    }
    fn rename(&self) -> Option<&str> {
        self.iter().find_map(|j| match j {
            JsEntry::Name(n) => Some(n.as_str()),
            _ => None,
        })
    }
    fn symbol(&self) -> Option<&str> {
        self.iter().find_map(|j| match j {
            JsEntry::Symbol(s) => Some(s.as_str()),
            _ => None,
        })
    }
    fn args(&self) -> Option<&[Telem]> {
        self.iter().find_map(|j| match j {
            JsEntry::Args(t) => Some(t.as_slice()),
            _ => None,
        })
    }
    fn super_args(&self) -> Option<&[Telem]> {
        self.iter().find_map(|j| match j {
            JsEntry::Super(t) => Some(t.as_slice()),
            _ => None,
        })
    }
    fn cancel(&self) -> Option<&Expr> {
        self.iter().find_map(|j| match j {
            JsEntry::Cancel(e) => Some(e),
            _ => None,
        })
    }
    fn literal(&self) -> Option<&str> {
        self.iter().find_map(|j| match j {
            JsEntry::Literal(l) => Some(l.as_str()),
            _ => None,
        })
    }
    fn classes(&self) -> Option<&[ClassPred]> {
        self.iter().find_map(|j| match j {
            JsEntry::Class(p) => Some(p.as_slice()),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassPred {
    pub class: Path,
    pub props: Vec<(String, Literal)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Literal {
    Str(String),
    Int(i64),
    Bool(bool),
    Null,
}

/// An element of an `args = (...)` or `super = (...)` template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Telem {
    Param(String),
    Lit(Literal),
    Undefined,
    /// `...rest`.
    Spread(String),
    /// `p? { field = <expr>, ... }`.
    Fill {
        param: String,
        fields: Vec<(String, Expr)>,
    },
    Expr(Expr),
}

/// An `@js` expression: a constant, or a path through IR members with an
/// optional call at the end (`AbortController.new()`, `c.signal`,
/// `$result.cancel`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Lit(Literal),
    Ref {
        path: Vec<String>,
        call: Option<Vec<Expr>>,
    },
}
