//! Wire layouts (IR.md §8, §10, §11, §13, §14, §19).
//!
//! Every type flattens, for a [`Profile`], to core Wasm values. The bridge
//! generator and every language generator read layouts from here and
//! nowhere else, so the two sides of a boundary can't disagree.
//!
//! [`Layouts::repr`] lowers an IR type to a [`Repr`]: the type resolved
//! against its declarations and the profile, carrying everything the
//! layout depends on (nullable encodings, enum slot tables, handle
//! carriers, nested element layouts) plus its canonical spelling, which is
//! how protocol names refer to it. [`Layouts::wire`] flattens a `Repr` to
//! its wire values, [`Layouts::elem`] gives a linear `arr<T>` element's
//! memory and table parts, and [`Layouts::member`] gives a member's full
//! signature with every position. The `tw` protocol operations have their
//! signatures here too.
//!
//! The only position-dependent part of a layout is a function type: the
//! language provides it as a handle where it flows to JS, and receives it
//! as an `externref` where it flows from JS ([`Flow`], §18).
//!
//! Flattening orders are normative and shared: a `Repr`'s wire values, and
//! an `arr<T>` element's memory fields and table slots, follow a depth-first
//! walk of the `Repr` in field order.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use crate::ir::*;
use crate::key::{location, Keyed, Keys, Role};
use crate::profile::{BoxNone, Carrier, Encoding, MemoryWidth, Profile, Results, StrCarrier};
use crate::resolve::{Context, Location, Target, TypeDecl};

/// A core Wasm value type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    Ref(RefType),
}

impl ValType {
    pub const EXTERNREF: ValType = ValType::Ref(RefType {
        nullable: true,
        heap: Heap::Extern,
    });
    pub const ANYREF: ValType = ValType::Ref(RefType {
        nullable: true,
        heap: Heap::Any,
    });

    pub fn is_ref(&self) -> bool {
        matches!(self, ValType::Ref(_))
    }

    /// The type with null admitted, for slots that may be left empty.
    pub fn nullable(&self) -> ValType {
        match self {
            ValType::Ref(r) => ValType::Ref(RefType {
                nullable: true,
                heap: r.heap.clone(),
            }),
            v => v.clone(),
        }
    }

    /// Bytes in memory: an enum slot or a descriptor value.
    pub fn size(&self) -> u32 {
        match self {
            ValType::I32 | ValType::F32 => 4,
            ValType::I64 | ValType::F64 => 8,
            ValType::Ref(_) => unreachable!("references never sit in memory"),
        }
    }

    /// Every GC type definition this type uses, inner definitions first.
    pub fn defs(&self, out: &mut Vec<Def>) {
        if let ValType::Ref(RefType {
            heap: Heap::Def(d), ..
        }) = self
        {
            match &**d {
                Def::Array(s) => s.defs(out),
                Def::Struct(fs) => fs.iter().for_each(|f| f.defs(out)),
            }
            if !out.contains(d) {
                out.push((**d).clone());
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RefType {
    pub nullable: bool,
    pub heap: Heap,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Heap {
    Extern,
    Any,
    /// A GC type, each its own singleton rec group (§11), so equal
    /// definitions are the same type on both sides.
    Def(Box<Def>),
}

/// A GC type definition. Arrays are always `(mut E)` and struct fields
/// always immutable (§11).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Def {
    Array(Storage),
    Struct(Vec<Storage>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Storage {
    I8,
    I16,
    Val(ValType),
}

impl Storage {
    fn defs(&self, out: &mut Vec<Def>) {
        if let Storage::Val(v) = self {
            v.defs(out);
        }
    }
}

fn gc_ref(nullable: bool, def: Def) -> ValType {
    ValType::Ref(RefType {
        nullable,
        heap: Heap::Def(Box::new(def)),
    })
}

/// WAT notation, with GC definitions written inline in place of their type
/// index: `(ref (array (mut i8)))`.
impl fmt::Display for ValType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValType::I32 => f.write_str("i32"),
            ValType::I64 => f.write_str("i64"),
            ValType::F32 => f.write_str("f32"),
            ValType::F64 => f.write_str("f64"),
            ValType::Ref(r) => write!(f, "{r}"),
        }
    }
}

impl fmt::Display for RefType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.heap, self.nullable) {
            (Heap::Extern, true) => f.write_str("externref"),
            (Heap::Any, true) => f.write_str("anyref"),
            (Heap::Extern, false) => f.write_str("(ref extern)"),
            (Heap::Any, false) => f.write_str("(ref any)"),
            (Heap::Def(d), true) => write!(f, "(ref null {d})"),
            (Heap::Def(d), false) => write!(f, "(ref {d})"),
        }
    }
}

impl fmt::Display for Def {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Def::Array(s) => write!(f, "(array (mut {s}))"),
            Def::Struct(fs) => {
                f.write_str("(struct")?;
                for s in fs {
                    write!(f, " (field {s})")?;
                }
                f.write_str(")")
            }
        }
    }
}

impl fmt::Display for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Storage::I8 => f.write_str("i8"),
            Storage::I16 => f.write_str("i16"),
            Storage::Val(v) => write!(f, "{v}"),
        }
    }
}

/// A core function type as the language declares it, and the result slots
/// it spills ([`Layouts::lang_core`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LangCore {
    pub ty: FuncType,
    pub spill: Vec<ValType>,
}

/// A spilled value type's `tw:ret` name: numbers, and any extern reference
/// (popped as nullable `externref`).
pub fn spill_name(v: &ValType) -> Option<&'static str> {
    match v {
        ValType::I32 => Some("i32"),
        ValType::I64 => Some("i64"),
        ValType::F32 => Some("f32"),
        ValType::F64 => Some("f64"),
        ValType::Ref(RefType {
            heap: Heap::Extern, ..
        }) => Some("externref"),
        ValType::Ref(_) => None,
    }
}

/// A core function type.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

/// `(externref) -> (i32, i32)`; `(i32)` with no results.
impl fmt::Display for FuncType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |vs: &[ValType]| {
            let vs: Vec<String> = vs.iter().map(ToString::to_string).collect();
            format!("({})", vs.join(", "))
        };
        f.write_str(&list(&self.params))?;
        if !self.results.is_empty() {
            write!(f, " -> {}", list(&self.results))?;
        }
        Ok(())
    }
}

/// An exported global.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    pub ty: ValType,
    pub mutable: bool,
}

/// Which way a value crosses, from the language's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flow {
    /// Language → JS: import parameters, export results.
    ToJs,
    /// JS → language: import results, export parameters.
    ToLang,
}

impl Flow {
    pub fn flip(self) -> Flow {
        match self {
            Flow::ToJs => Flow::ToLang,
            Flow::ToLang => Flow::ToJs,
        }
    }
}

/// A type resolved against its declarations and the profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repr {
    /// The canonical spelling (§3): how protocol names refer to the type.
    pub ty: String,
    pub kind: Kind,
    /// `@js(null)`: a `T?` whose absence crosses Wasm → JS as `null`.
    pub js_null: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// No result: no wire values.
    Unit,
    /// A result that never returns normally (`never`): no wire values, and
    /// the bridge throws a `TypeError` if the call returns.
    Never,
    /// A scalar on its carrier (§10).
    Scalar(Scalar),
    /// A nullable scalar (§10).
    NullScalar(Scalar, ScalarNull),
    /// `str`. Nullable as `len = MAX` (linear) or `ref.null`.
    Str { carrier: StrCarrier, nullable: bool },
    /// `T[]`, or `mut T[]` with `writeback`.
    Vec {
        elem: Scalar,
        carrier: Carrier,
        writeback: bool,
        nullable: bool,
        /// `bigint`, as its `u64` limbs (§10); the host bigint itself under
        /// the `host` carrier.
        bigint: bool,
    },
    /// `arr<T>`.
    Array {
        elem: Box<Repr>,
        carrier: Carrier,
        nullable: bool,
    },
    /// A tuple in a direct position, flattened; nullable adds a leading
    /// `i32 present`.
    Tuple { elems: Vec<Repr>, nullable: bool },
    /// An enum, `err`, or `T??`.
    Enum {
        layout: Rc<EnumLayout>,
        nullable: bool,
    },
    /// A host value on `externref` (§13).
    Host { of: Host, nullable: bool },
    /// An exported class instance: the language's handle (§14).
    Handle {
        /// The class, in canonical spelling.
        class: String,
        carrier: Carrier,
        nullable: bool,
    },
    /// A function type. Flowing to JS the language provides it as a handle
    /// (`provided` is the handle carrier); flowing from JS it's an
    /// `externref` to call through `("tw:call", F)` (§18).
    Fn {
        sig: Rc<FnLayout>,
        provided: Option<Carrier>,
        nullable: bool,
    },
    /// An erased type parameter: `anyref` with GC handles, else `externref`
    /// (§13).
    Erased { nullable: bool },
    /// `*T[]` / `*mut T[]`: `(ptr, len)` in the linear memory, which JS
    /// sees as a typed array over it.
    View { elem: Scalar, mutable: bool },
    /// A reference back into a function alias whose expansion is being
    /// laid out (§3): a [`Kind::Fn`] whose layout is `ty`'s, absolute,
    /// laid out afresh with [`Layouts::rec`].
    Rec {
        ty: Box<Type>,
        flow: Flow,
        provided: Option<Carrier>,
        nullable: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarNull {
    /// The absent value on the scalar's own wire value (`bool?` 2,
    /// `u8?` `i32::MIN`, `i53?` `i64::MIN`).
    Niche(i64),
    /// `(i32 present, T)`.
    Present,
}

/// What a host value is, for the bridge's conversions and checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    /// A class (`instanceof`-checked) or an interface (never checked).
    Class {
        path: String,
        interface: bool,
    },
    Any,
    /// A box (§13), with the layout its `tw:unbox` produces.
    Box(Box<Repr>),
    /// `future<T, E>`: the value `take<T>` produces and the rejection enum.
    Future {
        value: Box<Repr>,
        error: Rc<EnumLayout>,
    },
}

/// std's class of JS symbols, which are primitives: tested with `typeof`,
/// not `instanceof` (§3).
pub const SYMBOL: &str = "Symbol";

impl Host {
    pub fn symbol(&self) -> bool {
        matches!(self, Host::Class { path, .. } if path == SYMBOL)
    }

    /// A `T??` box: the JS value itself, with `null` on `ref.null` and
    /// `undefined` a non-null `externref`.
    pub fn nested(&self) -> bool {
        matches!(self, Host::Box(r) if r.ty.ends_with('?'))
    }
}

/// A function type's positions. Its parameters flow opposite to the
/// function value and its result flows with it: a function the language
/// provides receives its arguments from JS (§18).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnLayout {
    pub params: Vec<Repr>,
    /// The last parameter gathers the remaining JS arguments (§18).
    pub rest: bool,
    pub result: Repr,
    pub throws: Option<Rc<EnumLayout>>,
}

/// An enum's arms and wire (§8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumLayout {
    /// The canonical spelling: the enum's path, `err`, or `T??`.
    pub path: String,
    pub origin: Origin,
    pub shape: Shape,
    /// In index order: base enums first, then declaration order. Aliases
    /// are left out.
    pub arms: Vec<ArmLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Decl(Location),
    /// The built-in `err` (§15).
    Err,
    /// `T??`: `{ Undefined; Value(T?) }` (§9).
    Nested,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// All unit arms, no carrier: an `i32` index.
    Index,
    /// All unit arms with explicit values: the carrier, an open set.
    Carrier(Scalar),
    /// Any payload arm: `(i32 arm, slots…)`. Unused slots are zero or
    /// null, so reference slots are always nullable.
    Payload { slots: Vec<ValType> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmLayout {
    pub name: String,
    /// The index (from 1, `@fallback` -1) or the carrier's value.
    pub index: i64,
    pub fallback: bool,
    /// The arm's `@js` entries (literals, class matches).
    pub js: Vec<JsEntry>,
    pub payload: Vec<Repr>,
    /// For each wire value of the payload, in order, the slot it uses.
    pub slots: Vec<usize>,
}

/// A linear `arr<T>` element (§11): the numeric values packed into memory,
/// and the reference values in the externref table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elem {
    /// The memory stride in bytes, the sum of the field sizes.
    pub size: u32,
    /// The allocation alignment: the largest field's size (1 when empty).
    pub align: u32,
    pub fields: Vec<Field>,
    /// The table stride is `refs.len()`. Each entry is the value's own
    /// wire type: anything other than `externref` is stored externalized
    /// (`extern.convert_any`) and read back with `any.convert_extern` and a
    /// cast.
    pub refs: Vec<ValType>,
}

/// A packed, unaligned memory field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub offset: u32,
    pub size: u32,
    pub kind: FieldKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// A scalar at its native size (`bool`/`u8` 1, `f16` 2, `i53` 8).
    Scalar(Scalar),
    /// The `u8 present` flag of a nullable scalar, tuple or carrier enum.
    Present,
    /// An enum's `i32` index or arm.
    Arm,
    /// A numeric payload slot, at its wire size.
    Slot(ValType),
    Ptr,
    Len,
    /// A table base (always 4 bytes).
    Base,
    /// A linear handle (pointer-sized).
    Handle,
}

/// A linear allocation's unit: `len` counts units of `size` bytes, aligned
/// to `align` (§11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alloc {
    pub size: u32,
    pub align: u32,
}

/// One position of a signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    pub role: Pos,
    pub repr: Repr,
    pub wire: Vec<ValType>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pos {
    /// The JS receiver: an imported instance, or an exported class's JS
    /// instance (borrowed for the call, §17).
    This,
    /// An exported class instance's handle.
    Handle,
    /// A callback's environment handle (trampolines).
    Env,
    /// A JS function to call (`("tw:call", F)`).
    Callee,
    /// The future or resolver of a `tw:future` operation.
    Future,
    /// An index argument (`("tw:arr", "get<T>")`).
    Index,
    /// The `n`th declared parameter.
    Param(usize),
    Result,
}

/// A member's or protocol operation's signature, by position.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signature {
    pub params: Vec<Position>,
    pub results: Vec<Position>,
    /// The tag's enum, for `throws`.
    pub throws: Option<Rc<EnumLayout>>,
}

impl Signature {
    /// The flattened core function type.
    pub fn core(&self) -> FuncType {
        FuncType {
            params: self.params.iter().flat_map(|p| p.wire.clone()).collect(),
            results: self.results.iter().flat_map(|p| p.wire.clone()).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A path that names nothing declared.
    Unknown(String),
    /// Anything else the layout can't be computed for: a validation error
    /// the layout depends on, or a missing linear memory.
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unknown(p) => write!(f, "`{p}` names nothing declared"),
            Error::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(m.into()))
}

/// The carrier of a scalar (§10).
pub fn carrier(s: Scalar) -> ValType {
    match s {
        Scalar::Bool
        | Scalar::I8
        | Scalar::U8
        | Scalar::I16
        | Scalar::U16
        | Scalar::I32
        | Scalar::U32 => ValType::I32,
        Scalar::I53 | Scalar::U53 | Scalar::I64 | Scalar::U64 => ValType::I64,
        Scalar::F16 | Scalar::F32 => ValType::F32,
        Scalar::F64 => ValType::F64,
    }
}

/// A scalar's native size in memory (§11).
pub fn native_size(s: Scalar) -> u32 {
    match s {
        Scalar::Bool | Scalar::I8 | Scalar::U8 => 1,
        Scalar::I16 | Scalar::U16 | Scalar::F16 => 2,
        Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
        Scalar::I53 | Scalar::U53 | Scalar::I64 | Scalar::U64 | Scalar::F64 => 8,
    }
}

/// How a nullable scalar marks absence on the call wire (§10).
pub fn scalar_null(s: Scalar) -> ScalarNull {
    match s {
        Scalar::Bool => ScalarNull::Niche(2),
        Scalar::I8 | Scalar::U8 | Scalar::I16 | Scalar::U16 => ScalarNull::Niche(i32::MIN as i64),
        Scalar::I53 | Scalar::U53 => ScalarNull::Niche(i64::MIN),
        _ => ScalarNull::Present,
    }
}

/// Whether `T[]` admits the element type (§11).
pub fn is_vec_elem(s: Scalar) -> bool {
    !matches!(s, Scalar::Bool | Scalar::I53 | Scalar::U53)
}

/// A scalar's GC array storage, the same for `T[]` and `arr<T>` (§11).
pub fn scalar_storage(s: Scalar) -> Storage {
    match s {
        Scalar::Bool | Scalar::I8 | Scalar::U8 => Storage::I8,
        Scalar::I16 | Scalar::U16 | Scalar::F16 => Storage::I16,
        Scalar::I32 | Scalar::U32 => Storage::Val(ValType::I32),
        Scalar::I53 | Scalar::U53 | Scalar::I64 | Scalar::U64 => Storage::Val(ValType::I64),
        Scalar::F32 => Storage::Val(ValType::F32),
        Scalar::F64 => Storage::Val(ValType::F64),
    }
}

/// The GC string type for an encoding (§11).
pub fn gc_str(enc: Encoding) -> Def {
    match enc {
        Encoding::Utf8 => Def::Array(Storage::I8),
        Encoding::Wtf16 => Def::Array(Storage::I16),
    }
}

/// The built-in error enum (§15).
fn err_decl() -> Enum {
    let class = |n: &str| ClassPred {
        class: Path::new([n]),
        props: Vec::new(),
    };
    let arm = |name: &str, js: Vec<JsEntry>, fallback: bool| Arm {
        meta: Meta {
            attrs: Attrs {
                js,
                fallback,
                once: false,
            },
            ..Meta::default()
        },
        name: name.into(),
        payload: vec![Type::StrBox],
        value: None,
    };
    Enum {
        meta: Meta::default(),
        name: ERR.into(),
        carrier: None,
        extends: Vec::new(),
        members: Vec::new(),
        arms: vec![
            arm(
                "TypeError",
                vec![JsEntry::Class(vec![
                    class("TypeError"),
                    class("RangeError"),
                ])],
                false,
            ),
            arm("Error", Vec::new(), true),
        ],
    }
}

type EnumKey = (Option<Location>, Flow);

/// Layouts of one IR under one profile.
pub struct Layouts<'a> {
    pub keys: Keys<'a>,
    pub profile: &'a Profile,
    /// Exported classes: `export class`, or a class in an exported
    /// namespace.
    exported: HashSet<Location>,
    err: Enum,
    enums: RefCell<HashMap<EnumKey, Rc<EnumLayout>>>,
    pending: RefCell<Vec<EnumKey>>,
    /// The aliases being laid out, each with the function depth it began
    /// at.
    unfolding: RefCell<Vec<(Location, usize)>>,
    fns: Cell<usize>,
    /// Approximations met since the last [`Layouts::take_warnings`].
    warnings: RefCell<Vec<String>>,
}

/// `t` with each associated type replaced by its first row, the bound's
/// union, as erased generics read it (§13).
fn erase_assoc(t: &Type, cx: &Context<'_>) -> Type {
    if cx.assoc.is_empty() {
        return t.clone();
    }
    t.map_named(&|r| {
        let (_, row) = cx.assoc(&r.path)?.rows.first()?;
        Some(row.clone())
    })
}

fn context_of(loc: &Location) -> Context<'_> {
    Context {
        module: loc.module.as_deref(),
        namespace: loc.path[..loc.path.len() - 1]
            .iter()
            .map(String::as_str)
            .collect(),
        ..Context::default()
    }
}

impl<'a> Layouts<'a> {
    pub fn new(ir: &'a Ir, profile: &'a Profile) -> Layouts<'a> {
        Layouts::with_libs(ir, &[], profile)
    }

    /// Layouts of `ir`, with paths resolved against its libs (§5).
    pub fn with_libs(ir: &'a Ir, libs: &[&'a Ir], profile: &'a Profile) -> Layouts<'a> {
        let mut exported = HashSet::new();
        collect_exported(&ir.items, None, &mut Vec::new(), false, &mut exported);
        Layouts {
            keys: Keys::with_libs(ir, libs),
            profile,
            exported,
            err: err_decl(),
            enums: RefCell::new(HashMap::new()),
            pending: RefCell::new(Vec::new()),
            unfolding: RefCell::new(Vec::new()),
            fns: Cell::new(0),
            warnings: RefCell::new(Vec::new()),
        }
    }

    fn width(&self, what: &str) -> Result<MemoryWidth> {
        match self.profile.memory {
            Some(w) => Ok(w),
            None => invalid(format!("{what} needs a linear memory")),
        }
    }

    fn ptr(&self, what: &str) -> Result<ValType> {
        Ok(match self.width(what)? {
            MemoryWidth::M32 => ValType::I32,
            MemoryWidth::M64 => ValType::I64,
        })
    }

    fn ptr_size(&self, what: &str) -> Result<u32> {
        Ok(match self.width(what)? {
            MemoryWidth::M32 => 4,
            MemoryWidth::M64 => 8,
        })
    }

    fn handle_wire(&self, carrier: Carrier) -> Result<ValType> {
        match carrier {
            Carrier::Linear => self.ptr("a linear handle"),
            Carrier::Gc => Ok(ValType::ANYREF),
            Carrier::Host => Ok(ValType::EXTERNREF),
        }
    }

    /// The handle carrier's wire type (§14).
    pub fn handle(&self) -> Result<ValType> {
        self.handle_wire(self.profile.handles)
    }

    /// The approximations met laying out since the last call, for the
    /// caller to locate.
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut self.warnings.borrow_mut())
    }

    /// Lowers `t`, written in `cx`.
    pub fn repr(&self, t: &Type, cx: &Context<'_>, flow: Flow) -> Result<Repr> {
        self.lower(t, cx, flow)
    }

    fn lower(&self, t: &Type, cx: &Context<'_>, flow: Flow) -> Result<Repr> {
        let ty = self.keys.ty(t, cx);
        let kind = self.kind(t, cx, flow, false)?;
        Ok(Repr {
            ty,
            kind,
            js_null: matches!(t, Type::Nullable(_, true)),
        })
    }

    fn kind(&self, t: &Type, cx: &Context<'_>, flow: Flow, nullable: bool) -> Result<Kind> {
        let p = self.profile;
        let host = |of| Kind::Host { of, nullable };
        Ok(match t {
            Type::Unit if nullable => return invalid("`unit?` isn't a type"),
            Type::Unit => Kind::Unit,
            Type::Never if nullable => return invalid("`never?` isn't a type"),
            Type::Never => Kind::Never,
            Type::Scalar(s) if nullable => Kind::NullScalar(*s, scalar_null(*s)),
            Type::Scalar(s) => Kind::Scalar(*s),
            Type::Str => Kind::Str {
                carrier: p.str,
                nullable,
            },
            Type::Vec(e) | Type::MutVec(e) => {
                let elem = match **e {
                    Type::Scalar(s) if is_vec_elem(s) => s,
                    _ => {
                        return invalid(format!(
                            "`{}[]`: the element must be a typed-array element type",
                            self.keys.ty(e, cx)
                        ))
                    }
                };
                Kind::Vec {
                    elem,
                    carrier: p.vec,
                    writeback: matches!(t, Type::MutVec(_)),
                    nullable,
                    bigint: false,
                }
            }
            Type::BigInt => Kind::Vec {
                elem: Scalar::U64,
                carrier: p.vec,
                writeback: false,
                nullable,
                bigint: true,
            },
            Type::Array(e) => Kind::Array {
                elem: Box::new(self.lower(e, cx, flow)?),
                carrier: p.array,
                nullable,
            },
            Type::Tuple(ts) => Kind::Tuple {
                elems: ts
                    .iter()
                    .map(|t| self.lower(t, cx, flow))
                    .collect::<Result<_>>()?,
                nullable,
            },
            Type::Any => host(Host::Any),
            Type::ScalarBox(_)
            | Type::StrBox
            | Type::BigIntBox
            | Type::UnitBox
            | Type::VecBox(_)
            | Type::ArrBox(_)
            | Type::TupleBox(_) => {
                let inner = unboxed(t).unwrap();
                host(Host::Box(Box::new(self.lower(&inner, cx, Flow::ToLang)?)))
            }
            Type::Future(v, e) => host(Host::Future {
                value: Box::new(self.lower(v, cx, flow)?),
                error: self.enum_path(e, cx, flow)?,
            }),
            Type::Fn(f) => Kind::Fn {
                sig: Rc::new(self.fn_layout(f, cx, flow)?),
                provided: (flow == Flow::ToJs).then_some(p.handles),
                nullable,
            },
            Type::View { mutable, elem } if !nullable => match **elem {
                Type::Scalar(s) if is_vec_elem(s) => {
                    self.width("`*T[]`")?;
                    Kind::View {
                        elem: s,
                        mutable: *mutable,
                    }
                }
                _ => return invalid("`*T[]`: the element must be a typed-array element type"),
            },
            Type::View { .. } => return invalid("`?` is invalid on `*T[]`"),
            // `t` is the `T?` of a `T??`.
            Type::Nullable(inner, _) if nullable => match unboxed(inner) {
                // Under `box-none = null`, a box's one none is `ref.null`.
                Some(_) if p.box_none == BoxNone::Null => {
                    let w = format!(
                        "a `{}?` box is `{}` under `box-none = null`: its two nones are one",
                        self.keys.ty(t, cx),
                        self.keys.ty(t, cx)
                    );
                    let mut ws = self.warnings.borrow_mut();
                    if !ws.contains(&w) {
                        ws.push(w);
                    }
                    drop(ws);
                    return self.kind(inner, cx, flow, true);
                }
                Some(inner) => {
                    let inner = Type::Nullable(Box::new(inner), false);
                    host(Host::Box(Box::new(self.lower(&inner, cx, Flow::ToLang)?)))
                }
                None => Kind::Enum {
                    layout: self.nested(t, cx, flow)?,
                    nullable: false,
                },
            },
            Type::Nullable(inner, _) => return self.kind(inner, cx, flow, true),
            Type::Named(r) => match self.keys.symbols.resolve(&r.path, cx) {
                Some(Target::Decl(loc, TypeDecl::Alias(_))) => {
                    let at = self
                        .unfolding
                        .borrow()
                        .iter()
                        .find(|(l, _)| l == loc)
                        .map(|e| e.1);
                    if let Some(at) = at {
                        return self.back_edge(t, cx, flow, nullable, at);
                    }
                    let (loc, u) = self.keys.symbols.unfold(r, cx).unwrap();
                    self.unfolding.borrow_mut().push((loc, self.fns.get()));
                    let kind = self.kind(&u, cx, flow, nullable);
                    self.unfolding.borrow_mut().pop();
                    return kind;
                }
                Some(Target::TypeParam) => Kind::Erased { nullable },
                // Erased, an associated type is its first row's: the
                // bound's union (§13).
                Some(Target::Assoc) => match cx.assoc(&r.path).and_then(|a| a.rows.first()) {
                    Some((_, t)) => return self.kind(t, cx, flow, nullable),
                    None => return invalid(format!("`{}` has no rows", r.path.segments[0])),
                },
                Some(Target::Err) => Kind::Enum {
                    layout: self.enum_layout(None, flow)?,
                    nullable,
                },
                Some(Target::Decl(loc, TypeDecl::Enum(_))) => Kind::Enum {
                    layout: self.enum_layout(Some(loc), flow)?,
                    nullable,
                },
                Some(Target::Decl(loc, TypeDecl::Class(c))) => {
                    if self.exported.contains(loc) {
                        Kind::Handle {
                            class: location(loc),
                            carrier: p.handles,
                            nullable,
                        }
                    } else {
                        host(Host::Class {
                            path: location(loc),
                            interface: c.kind == ClassKind::Interface,
                        })
                    }
                }
                None => return Err(Error::Unknown(self.keys.path(&r.path, cx))),
            },
        })
    }

    fn back_edge(
        &self,
        t: &Type,
        cx: &Context<'_>,
        flow: Flow,
        nullable: bool,
        at: usize,
    ) -> Result<Kind> {
        if self.fns.get() <= at || !matches!(self.keys.symbols.head(t, cx), Type::Fn(_)) {
            return invalid(format!(
                "`{}` reaches itself other than as a function type inside one, which has no layout",
                self.keys.ty(t, cx)
            ));
        }
        let ty = self.keys.symbols.absolute(t, cx, &HashMap::new());
        let mut open = false;
        ty.walk(&mut |t| {
            if let Type::Named(r) = t {
                open |= r.path.base == Base::Lexical
                    && matches!(
                        self.keys.symbols.resolve(&r.path, cx),
                        Some(Target::TypeParam)
                    );
            }
        });
        if open {
            return invalid(format!(
                "`{}`: a recursive alias over a type parameter has no layout",
                self.keys.ty(t, cx)
            ));
        }
        Ok(Kind::Rec {
            ty: Box::new(ty),
            flow,
            provided: (flow == Flow::ToJs).then_some(self.profile.handles),
            nullable,
        })
    }

    /// The layout a [`Kind::Rec`] stands for; any other as it is.
    pub fn rec(&self, r: &Repr) -> Result<Repr> {
        let Kind::Rec {
            ty, flow, nullable, ..
        } = &r.kind
        else {
            return Ok(r.clone());
        };
        let cx = Context::default();
        Ok(Repr {
            kind: self.kind(ty, &cx, *flow, *nullable)?,
            ..r.clone()
        })
    }

    fn fn_layout(&self, f: &FnType, cx: &Context<'_>, flow: Flow) -> Result<FnLayout> {
        self.fns.set(self.fns.get() + 1);
        let l = self.fn_layout_in(f, cx, flow);
        self.fns.set(self.fns.get() - 1);
        l
    }

    fn fn_layout_in(&self, f: &FnType, cx: &Context<'_>, flow: Flow) -> Result<FnLayout> {
        Ok(FnLayout {
            params: f
                .params
                .iter()
                .map(|t| self.lower(t, cx, flow.flip()))
                .collect::<Result<_>>()?,
            rest: f.rest,
            result: self.lower(&f.result, cx, flow)?,
            throws: match &f.throws {
                Some(e) => Some(self.enum_path(e, cx, flow)?),
                None => None,
            },
        })
    }

    fn enum_path(&self, p: &Path, cx: &Context<'_>, flow: Flow) -> Result<Rc<EnumLayout>> {
        match self.keys.symbols.resolve(p, cx) {
            Some(Target::Err) => self.enum_layout(None, flow),
            Some(Target::Decl(loc, TypeDecl::Enum(_))) => self.enum_layout(Some(loc), flow),
            Some(_) => invalid(format!("`{}` isn't an enum", self.keys.path(p, cx))),
            None => Err(Error::Unknown(self.keys.path(p, cx))),
        }
    }

    /// The layout of an enum used in `throws` or `future`, or as a type.
    pub fn enum_of(&self, p: &Path, cx: &Context<'_>, flow: Flow) -> Result<Rc<EnumLayout>> {
        self.enum_path(p, cx, flow)
    }

    fn decl_enum(&self, loc: Option<&Location>) -> Result<&Enum> {
        let Some(loc) = loc else {
            return Ok(&self.err);
        };
        let cx = context_of(loc);
        let last = Path::new([loc.path.last().unwrap().as_str()]);
        match self.keys.symbols.resolve(&last, &cx) {
            Some(Target::Decl(_, TypeDecl::Enum(e))) => Ok(e),
            _ => invalid(format!("`{}` isn't an enum", location(loc))),
        }
    }

    /// An enum's layout (`None` is the built-in `err`), computed once per
    /// flow.
    fn enum_layout(&self, loc: Option<&Location>, flow: Flow) -> Result<Rc<EnumLayout>> {
        let key = (loc.cloned(), flow);
        if let Some(l) = self.enums.borrow().get(&key) {
            return Ok(l.clone());
        }
        let path = loc.map_or_else(|| ERR.to_string(), location);
        if self.pending.borrow().contains(&key) {
            return invalid(format!(
                "`{path}` contains itself: recursive enum layouts are unsupported"
            ));
        }
        self.pending.borrow_mut().push(key.clone());
        let built = self.build_enum(loc, &path, flow);
        self.pending.borrow_mut().pop();
        let l = Rc::new(built?);
        self.enums.borrow_mut().insert(key, l.clone());
        Ok(l)
    }

    fn build_enum(&self, loc: Option<&Location>, path: &str, flow: Flow) -> Result<EnumLayout> {
        // Each enum of the hierarchy once, bases first, with the context
        // its arms are written in.
        let mut order: Vec<Option<Location>> = Vec::new();
        self.hierarchy(loc, &mut order, &mut Vec::new())?;
        let mut carrier = None;
        let mut arms: Vec<(&Arm, Option<&Location>)> = Vec::new();
        for l in &order {
            let e = self.decl_enum(l.as_ref())?;
            carrier = carrier.or(e.carrier);
            for a in &e.arms {
                if !matches!(a.value, Some(ArmValue::Alias(_))) {
                    arms.push((a, l.as_ref()));
                }
            }
        }
        let payload = arms.iter().any(|(a, _)| !a.payload.is_empty());
        let shape = match (payload, carrier) {
            (true, Some(_)) => return invalid(format!("`{path}`: a carrier enum has payloads")),
            (true, None) => Shape::Payload { slots: Vec::new() },
            (false, Some(c)) if c.is_int_carrier() => Shape::Carrier(c),
            (false, Some(c)) => {
                return invalid(format!("`{path}`: `{}` isn't an enum carrier", c.name()))
            }
            (false, None) => Shape::Index,
        };
        // Indices come from the flattened hierarchy, where a base's arms
        // keep their own (IR.md §8).
        let indices: HashMap<&str, i64> = match &shape {
            Shape::Carrier(_) => HashMap::new(),
            _ => {
                let this = crate::enums::EnumRef {
                    loc: loc.cloned(),
                    decl: self.decl_enum(loc)?,
                };
                let flat = crate::enums::flatten(&self.keys.symbols, this).map_err(|es| {
                    Error::Invalid(format!(
                        "`{path}`: {}",
                        es.first().map_or_else(String::new, |e| e.message.clone())
                    ))
                })?;
                flat.arms
                    .iter()
                    .map(|a| (a.arm.name.as_str(), a.value))
                    .collect()
            }
        };
        let mut slots: Vec<ValType> = Vec::new();
        let mut out = Vec::new();
        for (a, l) in arms {
            let fallback = a.meta.attrs.fallback;
            let index = match (&shape, &a.value) {
                (Shape::Carrier(_), Some(ArmValue::Int(v))) => *v,
                (Shape::Carrier(_), _) if fallback => -1,
                (Shape::Carrier(_), _) => {
                    return invalid(format!("`{path}.{}` needs a value", a.name))
                }
                _ if fallback => -1,
                _ => indices[a.name.as_str()],
            };
            let cx = l.map(context_of).unwrap_or_default();
            let payload: Vec<Repr> = a
                .payload
                .iter()
                .map(|t| self.lower(t, &cx, flow))
                .collect::<Result<_>>()?;
            let mut used = Vec::new();
            for v in payload
                .iter()
                .map(|r| self.wire(r))
                .collect::<Result<Vec<_>>>()?
            {
                for v in v {
                    let v = v.nullable();
                    let j = match (0..slots.len()).find(|j| slots[*j] == v && !used.contains(j)) {
                        Some(j) => j,
                        None => {
                            slots.push(v);
                            slots.len() - 1
                        }
                    };
                    used.push(j);
                }
            }
            out.push(ArmLayout {
                name: a.name.clone(),
                index,
                fallback,
                js: a.meta.attrs.js.clone(),
                payload,
                slots: used,
            });
        }
        let shape = match shape {
            Shape::Payload { .. } => Shape::Payload { slots },
            s => s,
        };
        Ok(EnumLayout {
            path: path.into(),
            origin: match loc {
                Some(l) => Origin::Decl(l.clone()),
                None => Origin::Err,
            },
            shape,
            arms: out,
        })
    }

    fn hierarchy(
        &self,
        loc: Option<&Location>,
        out: &mut Vec<Option<Location>>,
        stack: &mut Vec<Option<Location>>,
    ) -> Result<()> {
        let key = loc.cloned();
        if out.contains(&key) {
            return Ok(());
        }
        if stack.contains(&key) {
            return invalid(format!(
                "`{}` extends itself",
                loc.map_or_else(|| ERR.into(), location)
            ));
        }
        stack.push(key.clone());
        let e = self.decl_enum(loc)?;
        let cx = loc.map(context_of).unwrap_or_default();
        for b in &e.extends {
            match self.keys.symbols.resolve(b, &cx) {
                Some(Target::Err) => self.hierarchy(None, out, stack)?,
                Some(Target::Decl(l, TypeDecl::Enum(_))) => self.hierarchy(Some(l), out, stack)?,
                Some(_) => {
                    return invalid(format!("`{}` isn't an enum", self.keys.path(b, &cx)));
                }
                None => return Err(Error::Unknown(self.keys.path(b, &cx))),
            }
        }
        stack.pop();
        out.push(key);
        Ok(())
    }

    /// `T??` other than a box: `undefined` is `Undefined`, and `Value`'s
    /// absence is `null`.
    fn nested(&self, v: &Type, cx: &Context<'_>, flow: Flow) -> Result<Rc<EnumLayout>> {
        let value = Repr {
            js_null: true,
            ..self.lower(v, cx, flow)?
        };
        let wire = self.wire(&value)?;
        let slots: Vec<ValType> = wire.iter().map(ValType::nullable).collect();
        Ok(Rc::new(EnumLayout {
            path: format!("{}?", value.ty),
            origin: Origin::Nested,
            shape: Shape::Payload {
                slots: slots.clone(),
            },
            arms: vec![
                ArmLayout {
                    name: "Undefined".into(),
                    index: 1,
                    fallback: false,
                    js: Vec::new(),
                    payload: Vec::new(),
                    slots: Vec::new(),
                },
                ArmLayout {
                    name: "Value".into(),
                    index: 2,
                    fallback: false,
                    js: Vec::new(),
                    payload: vec![value],
                    slots: (0..slots.len()).collect(),
                },
            ],
        }))
    }

    /// The wire values of a `Repr`, flattened in field order.
    pub fn wire(&self, r: &Repr) -> Result<Vec<ValType>> {
        let mut out = Vec::new();
        self.flatten(r, &mut out)?;
        Ok(out)
    }

    fn flatten(&self, r: &Repr, out: &mut Vec<ValType>) -> Result<()> {
        match &r.kind {
            Kind::Unit | Kind::Never => {}
            Kind::Scalar(s) | Kind::NullScalar(s, ScalarNull::Niche(_)) => out.push(carrier(*s)),
            Kind::NullScalar(s, ScalarNull::Present) => {
                out.push(ValType::I32);
                out.push(carrier(*s));
            }
            Kind::Str { carrier, nullable } => match carrier {
                StrCarrier::Linear(_) => {
                    let p = self.ptr("a linear `str`")?;
                    out.push(p.clone());
                    out.push(p);
                }
                StrCarrier::Gc(enc) => out.push(gc_ref(*nullable, gc_str(*enc))),
                StrCarrier::Host => out.push(ValType::EXTERNREF),
            },
            Kind::Vec {
                elem,
                carrier,
                nullable,
                ..
            } => match carrier {
                Carrier::Linear => {
                    let p = self.ptr("a linear `T[]`")?;
                    out.push(p.clone());
                    out.push(p);
                }
                Carrier::Gc => out.push(gc_ref(*nullable, Def::Array(scalar_storage(*elem)))),
                Carrier::Host => out.push(ValType::EXTERNREF),
            },
            Kind::Array {
                elem,
                carrier,
                nullable,
            } => match carrier {
                Carrier::Linear => {
                    let e = self.elem(elem)?;
                    let p = self.ptr("a linear `arr<T>`")?;
                    if !e.fields.is_empty() {
                        out.push(p.clone());
                    }
                    if !e.refs.is_empty() {
                        out.push(ValType::I32);
                    }
                    out.push(p);
                }
                Carrier::Gc => out.push(gc_ref(*nullable, Def::Array(self.gc_elem(elem)?))),
                Carrier::Host => out.push(ValType::EXTERNREF),
            },
            Kind::Tuple { elems, nullable } => {
                if *nullable {
                    out.push(ValType::I32);
                }
                for e in elems {
                    self.flatten(e, out)?;
                }
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Index => out.push(ValType::I32),
                Shape::Carrier(c) => {
                    if *nullable {
                        out.push(ValType::I32);
                    }
                    out.push(carrier(*c));
                }
                Shape::Payload { slots } => {
                    out.push(ValType::I32);
                    out.extend(slots.iter().cloned());
                }
            },
            Kind::Host { .. } => out.push(ValType::EXTERNREF),
            Kind::Handle { carrier, .. } => out.push(self.handle_wire(*carrier)?),
            Kind::Fn { provided, .. } => match provided {
                Some(c) => out.push(self.handle_wire(*c)?),
                None => out.push(ValType::EXTERNREF),
            },
            Kind::Erased { .. } => out.push(match self.profile.handles {
                Carrier::Gc => ValType::ANYREF,
                Carrier::Linear | Carrier::Host => ValType::EXTERNREF,
            }),
            Kind::View { .. } => {
                let p = self.ptr("`*T[]`")?;
                out.push(p.clone());
                out.push(p);
            }
            Kind::Rec { provided, .. } => match provided {
                Some(c) => out.push(self.handle_wire(*c)?),
                None => out.push(ValType::EXTERNREF),
            },
        }
        Ok(())
    }

    /// The element type of a GC `arr<T>`: a scalar's storage, else the
    /// element's wire value when it has one, else a struct of them (§11). A
    /// reference element is always nullable, so the array is defaultable.
    pub fn gc_elem(&self, elem: &Repr) -> Result<Storage> {
        if let Kind::Scalar(s) = elem.kind {
            return Ok(scalar_storage(s));
        }
        let mut w = self.wire(elem)?;
        match w.len() {
            0 => invalid("a `unit` element"),
            1 => Ok(Storage::Val(w.pop().unwrap().nullable())),
            _ => Ok(Storage::Val(gc_ref(
                true,
                Def::Struct(w.into_iter().map(Storage::Val).collect()),
            ))),
        }
    }

    /// A linear `arr<T>` element's memory and table parts (§11).
    pub fn elem(&self, elem: &Repr) -> Result<Elem> {
        let mut e = Elem {
            size: 0,
            align: 1,
            fields: Vec::new(),
            refs: Vec::new(),
        };
        self.place(elem, &mut e)?;
        e.align = e.fields.iter().map(|f| f.size).max().unwrap_or(1);
        Ok(e)
    }

    fn field(e: &mut Elem, size: u32, kind: FieldKind) {
        e.fields.push(Field {
            offset: e.size,
            size,
            kind,
        });
        e.size += size;
    }

    fn place(&self, r: &Repr, e: &mut Elem) -> Result<()> {
        match &r.kind {
            // A tuple's `unit` element stores nothing.
            Kind::Unit => {}
            Kind::View { .. } => return invalid("`*T[]` is legal only in import parameters"),
            Kind::Scalar(s) => Self::field(e, native_size(*s), FieldKind::Scalar(*s)),
            Kind::NullScalar(s, _) => {
                Self::field(e, 1, FieldKind::Present);
                Self::field(e, native_size(*s), FieldKind::Scalar(*s));
            }
            Kind::Str {
                carrier: StrCarrier::Linear(_),
                ..
            }
            | Kind::Vec {
                carrier: Carrier::Linear,
                ..
            } => {
                let p = self.ptr_size("a linear sequence")?;
                Self::field(e, p, FieldKind::Ptr);
                Self::field(e, p, FieldKind::Len);
            }
            Kind::Array {
                elem,
                carrier: Carrier::Linear,
                ..
            } => {
                let inner = self.elem(elem)?;
                let p = self.ptr_size("a linear `arr<T>`")?;
                if !inner.fields.is_empty() {
                    Self::field(e, p, FieldKind::Ptr);
                }
                if !inner.refs.is_empty() {
                    Self::field(e, 4, FieldKind::Base);
                }
                Self::field(e, p, FieldKind::Len);
            }
            Kind::Tuple { elems, nullable } => {
                if *nullable {
                    Self::field(e, 1, FieldKind::Present);
                }
                for t in elems {
                    self.place(t, e)?;
                }
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Index => Self::field(e, 4, FieldKind::Arm),
                Shape::Carrier(c) => {
                    if *nullable {
                        Self::field(e, 1, FieldKind::Present);
                    }
                    Self::field(e, native_size(*c), FieldKind::Scalar(*c));
                }
                Shape::Payload { slots } => {
                    Self::field(e, 4, FieldKind::Arm);
                    for s in slots {
                        if s.is_ref() {
                            e.refs.push(s.clone());
                        } else {
                            Self::field(e, s.size(), FieldKind::Slot(s.clone()));
                        }
                    }
                }
            },
            Kind::Handle {
                carrier: Carrier::Linear,
                ..
            }
            | Kind::Fn {
                provided: Some(Carrier::Linear),
                ..
            }
            | Kind::Rec {
                provided: Some(Carrier::Linear),
                ..
            } => {
                let p = self.ptr_size("a linear handle")?;
                Self::field(e, p, FieldKind::Handle);
            }
            _ => {
                for v in self.wire(r)? {
                    debug_assert!(v.is_ref());
                    e.refs.push(v);
                }
            }
        }
        Ok(())
    }

    /// A linear allocation's unit, for `tw:realloc`/`tw:free` (§11): code
    /// units for `str`, elements for `T[]`, and the memory part of a
    /// `arr<T>` (`None` when it has no memory part). `None` for anything not
    /// allocated in memory.
    pub fn alloc(&self, r: &Repr) -> Result<Option<Alloc>> {
        Ok(match &r.kind {
            Kind::Str {
                carrier: StrCarrier::Linear(enc),
                ..
            } => {
                let n = match enc {
                    Encoding::Utf8 => 1,
                    Encoding::Wtf16 => 2,
                };
                Some(Alloc { size: n, align: n })
            }
            Kind::Vec {
                elem,
                carrier: Carrier::Linear,
                ..
            } => {
                let n = native_size(*elem);
                Some(Alloc { size: n, align: n })
            }
            Kind::Array {
                elem,
                carrier: Carrier::Linear,
                ..
            } => {
                let e = self.elem(elem)?;
                (!e.fields.is_empty()).then_some(Alloc {
                    size: e.size,
                    align: e.align,
                })
            }
            _ => None,
        })
    }

    fn position(&self, role: Pos, repr: Repr) -> Result<Position> {
        let wire = self.wire(&repr)?;
        Ok(Position { role, repr, wire })
    }

    fn fixed(&self, role: Pos, ty: &str, kind: Kind) -> Result<Position> {
        self.position(
            role,
            Repr {
                ty: ty.into(),
                kind,
                js_null: false,
            },
        )
    }

    /// A member's signature (§17, §19). `targs` are a mono variant's type
    /// arguments in canonical spelling, the class's then the member's (as
    /// in its key); with none, type parameters are erased.
    pub fn member(&self, k: &Keyed<'_>, targs: &[Type]) -> Result<Signature> {
        self.member_as(k, targs, None)
    }

    /// [`Layouts::member`], with the parameters typed `params` instead: a
    /// `union` parameter's arm variant ([`crate::union::variants`]).
    pub fn member_as(
        &self,
        k: &Keyed<'_>,
        targs: &[Type],
        overrides: Option<&[Type]>,
    ) -> Result<Signature> {
        let m = k.member;
        let import = matches!(k.role, Role::Import(_));
        let (pflow, rflow) = if import {
            (Flow::ToJs, Flow::ToLang)
        } else {
            (Flow::ToLang, Flow::ToJs)
        };
        let tparams: Vec<&str> = k
            .owner
            .iter()
            .flat_map(|c| &c.tparams)
            .chain(&m.tparams)
            .map(|p| p.name.as_str())
            .collect();
        let mono = !targs.is_empty();
        if mono && targs.len() != tparams.len() {
            return invalid(format!(
                "{} type arguments for {} type parameters",
                targs.len(),
                tparams.len()
            ));
        }
        let global = Context::default();
        let subst: HashMap<&str, Type> = tparams
            .iter()
            .zip(targs)
            .map(|(p, t)| (*p, self.qualify(t, &global, &HashMap::new())))
            .collect();
        let lower = |t: &Type, flow| {
            if mono {
                self.lower(&self.qualify(t, &k.cx, &subst), &global, flow)
            } else {
                self.lower(&erase_assoc(t, &k.cx), &k.cx, flow)
            }
        };
        let owner = match k.owner {
            Some(c) => {
                let p = Path::new([c.name.as_str()]);
                match self.keys.symbols.resolve(&p, &k.cx) {
                    Some(Target::Decl(loc, _)) => Some((c, loc.clone())),
                    _ => return invalid(format!("can't resolve the owner of {}", k.key)),
                }
            }
            None => None,
        };
        let ctor = matches!(m.kind, MemberKind::New(_));
        let instance = owner.is_some() && !m.is_static && !ctor;
        let owner_host = |(c, loc): &(&Class, Location)| Kind::Host {
            of: Host::Class {
                path: location(loc),
                interface: c.kind == ClassKind::Interface,
            },
            nullable: false,
        };
        let exported_owner = owner.as_ref().filter(|(_, l)| self.exported.contains(l));
        let mut params = Vec::new();
        if instance || (ctor && !import && exported_owner.is_some()) {
            let o = owner.as_ref().unwrap();
            params.push(self.fixed(Pos::This, &location(&o.1), owner_host(o))?);
        }
        if instance && !import {
            if let Some((_, loc)) = exported_owner {
                params.push(self.fixed(
                    Pos::Handle,
                    &location(loc),
                    Kind::Handle {
                        class: location(loc),
                        carrier: self.profile.handles,
                        nullable: false,
                    },
                )?);
            }
        }
        for (i, p) in m.params.iter().enumerate() {
            let ty = overrides.map_or(&p.ty, |ts| &ts[i]);
            params.push(self.position(Pos::Param(i), lower(ty, pflow)?)?);
        }
        let mut results = Vec::new();
        if ctor {
            if let Some(o) = &owner {
                let kind = match (import, exported_owner) {
                    (false, Some((_, loc))) => Kind::Handle {
                        class: location(loc),
                        carrier: self.profile.handles,
                        nullable: false,
                    },
                    _ => owner_host(o),
                };
                results.push(self.fixed(Pos::Result, &location(&o.1), kind)?);
            }
        } else if m.result != Type::Unit {
            results.push(self.position(Pos::Result, lower(&m.result, rflow)?)?);
        }
        let throws = match &m.throws {
            Some(e) => Some(self.enum_path(e, &k.cx, rflow)?),
            None => None,
        };
        Ok(Signature {
            params,
            results,
            throws,
        })
    }

    /// `t` with every path resolved to its canonical location and type
    /// parameters substituted, so it reads the same from any context.
    fn qualify(&self, t: &Type, cx: &Context<'_>, subst: &HashMap<&str, Type>) -> Type {
        let q = |t: &Type| self.qualify(t, cx, subst);
        let qb = |t: &Type| Box::new(q(t));
        let qs = |ts: &[Type]| ts.iter().map(q).collect::<Vec<_>>();
        let path = |p: &Path| match self.keys.symbols.resolve(p, cx) {
            Some(Target::Decl(loc, _)) => {
                Path::new(loc.module.iter().cloned().chain(loc.path.iter().cloned()))
            }
            _ => p.clone(),
        };
        match t {
            Type::Vec(e) => Type::Vec(qb(e)),
            Type::VecBox(e) => Type::VecBox(qb(e)),
            Type::MutVec(e) => Type::MutVec(qb(e)),
            Type::Array(e) => Type::Array(qb(e)),
            Type::ArrBox(e) => Type::ArrBox(qb(e)),
            Type::Tuple(ts) => Type::Tuple(qs(ts)),
            Type::TupleBox(ts) => Type::TupleBox(qs(ts)),
            Type::Future(v, e) => Type::Future(qb(v), path(e)),
            Type::Fn(f) => Type::Fn(Box::new(FnType {
                params: qs(&f.params),
                rest: f.rest,
                result: q(&f.result),
                throws: f.throws.as_ref().map(path),
            })),
            Type::View { mutable, elem } => Type::View {
                mutable: *mutable,
                elem: qb(elem),
            },
            Type::Nullable(t, js_null) => Type::Nullable(qb(t), *js_null),
            Type::Named(r) => {
                if let Some(a) = cx.assoc(&r.path) {
                    let arg = subst.get(a.param.as_str());
                    let row = a.rows.iter().find(|(k, _)| {
                        arg.is_some_and(|arg| self.qualify(k, cx, &HashMap::new()) == *arg)
                    });
                    if let Some((_, t)) = row.or(arg.is_none().then(|| a.rows.first()).flatten()) {
                        return q(t);
                    }
                }
                if let [name] = &r.path.segments[..] {
                    if let Some(s) = subst.get(name.as_str()) {
                        if matches!(
                            self.keys.symbols.resolve(&r.path, cx),
                            Some(Target::TypeParam)
                        ) {
                            return s.clone();
                        }
                    }
                }
                Type::Named(TypeRef {
                    path: path(&r.path),
                    args: qs(&r.args),
                })
            }
            t => t.clone(),
        }
    }

    /// The trampoline the language exports for a function type it
    /// provides: `tw:call:F(env, W(args)…) -> W(R)` (§18).
    pub fn trampoline(&self, f: &FnType, cx: &Context<'_>) -> Result<Signature> {
        let r = self.lower(&Type::Fn(Box::new(f.clone())), cx, Flow::ToJs)?;
        self.fn_op(&r)
    }

    /// `("tw:call", F)(fn, W(args)…) -> W(R)`: the language calls a JS
    /// function (§18).
    pub fn call_import(&self, f: &FnType, cx: &Context<'_>) -> Result<Signature> {
        let r = self.lower(&Type::Fn(Box::new(f.clone())), cx, Flow::ToLang)?;
        self.fn_op(&r)
    }

    /// The operation behind a function value: the trampoline for one the
    /// language provides, the call import for one it receives.
    pub fn fn_op(&self, r: &Repr) -> Result<Signature> {
        let r = &self.rec(r)?;
        let Kind::Fn { sig, provided, .. } = &r.kind else {
            return invalid(format!("`{}` isn't a function type", r.ty));
        };
        let callee = Repr {
            ty: r.ty.clone(),
            kind: Kind::Fn {
                sig: sig.clone(),
                provided: *provided,
                nullable: false,
            },
            js_null: false,
        };
        let role = if provided.is_some() {
            Pos::Env
        } else {
            Pos::Callee
        };
        let mut params = vec![self.position(role, callee)?];
        for (i, p) in sig.params.iter().enumerate() {
            params.push(self.position(Pos::Param(i), p.clone())?);
        }
        Ok(Signature {
            params,
            results: self.result(&sig.result)?,
            throws: sig.throws.clone(),
        })
    }

    fn result(&self, r: &Repr) -> Result<Vec<Position>> {
        Ok(match r.kind {
            Kind::Unit => Vec::new(),
            _ => vec![self.position(Pos::Result, r.clone())?],
        })
    }

    fn extern_pos(&self, role: Pos, ty: &str) -> Result<Position> {
        self.fixed(
            role,
            ty,
            Kind::Host {
                of: Host::Any,
                nullable: false,
            },
        )
    }

    /// `("tw:box", T)`: `(W(T)) -> externref` (§13).
    pub fn box_(&self, t: &Type, cx: &Context<'_>) -> Result<Signature> {
        self.box_of(self.lower(t, cx, Flow::ToJs)?)
    }

    /// [`Layouts::box_`] of a layout.
    pub fn box_of(&self, r: Repr) -> Result<Signature> {
        Ok(Signature {
            params: vec![self.position(Pos::Param(0), r)?],
            results: vec![self.extern_pos(Pos::Result, "box")?],
            throws: None,
        })
    }

    /// `("tw:unbox", T)` and `("tw:unboxUnchecked", T)`: `(externref) ->
    /// W(T)` (§13).
    pub fn unbox(&self, t: &Type, cx: &Context<'_>) -> Result<Signature> {
        // Unboxing a function type recovers the handle the language provided
        // it as, through the pairing (§18).
        let inner = match t {
            Type::Nullable(t, _) => t,
            t => t,
        };
        let flow = match inner {
            Type::Fn(_) => Flow::ToJs,
            _ => Flow::ToLang,
        };
        self.unbox_of(&self.lower(t, cx, flow)?)
    }

    /// [`Layouts::unbox`] of a layout.
    pub fn unbox_of(&self, r: &Repr) -> Result<Signature> {
        Ok(Signature {
            params: vec![self.extern_pos(Pos::Param(0), "box")?],
            results: self.result(r)?,
            throws: None,
        })
    }

    /// Whether the exported class at `loc` has a base, which is always an
    /// imported class (§17). Only a real `new` gives that base its
    /// internals, so its wrapper can't be recreated, and the language
    /// constructs it through JS (`tw:new`).
    pub fn imported_base(&self, loc: &Location) -> bool {
        matches!(
            self.keys.symbols.resolve_decl(&loc.absolute(), &Context::default()),
            Some((_, TypeDecl::Class(c))) if !c.extends.is_empty()
        )
    }

    /// `("tw:new", ctor)`: the constructor of an exported class with an
    /// imported base read as an import, `(W(args)) -> externref` returning
    /// the wrapper (§17).
    pub fn construct(&self, k: &Keyed<'_>) -> Result<Signature> {
        let owner = k.owner.and_then(|c| {
            match self
                .keys
                .symbols
                .resolve(&Path::new([c.name.as_str()]), &k.cx)
            {
                Some(Target::Decl(loc, _)) if self.exported.contains(loc) => Some(loc.clone()),
                _ => None,
            }
        });
        let Some(owner) = owner.filter(|_| matches!(k.member.kind, MemberKind::New(_))) else {
            return invalid(format!(
                "`tw:new` takes an exported class's constructor, not `{}`",
                k.key
            ));
        };
        if !self.imported_base(&owner) {
            return invalid(format!(
                "`{}` has no imported base, so the language constructs it natively, not with `tw:new`",
                crate::key::location(&owner)
            ));
        }
        let mut import = k.clone();
        import.role = Role::Import(crate::protocol::NEW.into());
        self.member(&import, &[])
    }

    /// `("tw:tryUnbox", C)`: `(externref) -> W(C?)`, for a class with
    /// handles or a function type (§13).
    pub fn try_unbox(&self, t: &Type, cx: &Context<'_>) -> Result<Signature> {
        if matches!(t, Type::Nullable(..)) {
            return invalid("`tw:tryUnbox` takes a non-nullable type");
        }
        let sig = self.unbox(&Type::Nullable(Box::new(t.clone()), false), cx)?;
        match sig.results.first().map(|r| &r.repr.kind) {
            Some(
                Kind::Handle { .. }
                | Kind::Fn {
                    provided: Some(_), ..
                },
            ) => Ok(sig),
            _ => invalid(format!(
                "`tw:tryUnbox` takes a class with handles or a function type, not `{}`",
                self.keys.ty(t, cx)
            )),
        }
    }

    /// `("tw:arr", "get<T>")`: `(externref, i32) -> W(T)` (§13).
    pub fn arr_get(&self, t: &Type, cx: &Context<'_>) -> Result<Signature> {
        let r = self.lower(t, cx, Flow::ToLang)?;
        Ok(Signature {
            params: vec![
                self.extern_pos(Pos::Param(0), "box")?,
                self.fixed(Pos::Index, "u32", Kind::Scalar(Scalar::U32))?,
            ],
            results: self.result(&r)?,
            throws: None,
        })
    }

    /// `("tw:future", "take<T>")`: `(externref) -> W(T)` (§16).
    pub fn future_take(&self, t: &Type, cx: &Context<'_>) -> Result<Signature> {
        let r = self.lower(t, cx, Flow::ToLang)?;
        Ok(Signature {
            params: vec![self.extern_pos(Pos::Future, "future")?],
            results: self.result(&r)?,
            throws: None,
        })
    }

    /// `("tw:future", "resolve<T>")`: `(externref, W(T))` (§16).
    pub fn future_resolve(&self, t: &Type, cx: &Context<'_>) -> Result<Signature> {
        let r = self.lower(t, cx, Flow::ToJs)?;
        let mut params = vec![self.extern_pos(Pos::Future, "resolver")?];
        if !matches!(r.kind, Kind::Unit | Kind::Never) {
            params.push(self.position(Pos::Param(0), r)?);
        }
        Ok(Signature {
            params,
            results: Vec::new(),
            throws: None,
        })
    }

    /// `("tw:future", "error<E>")`: `(externref) -> W(E)` (§16).
    pub fn future_error(&self, e: &Path, cx: &Context<'_>) -> Result<Signature> {
        let r = self.enum_repr(e, cx, Flow::ToLang)?;
        Ok(Signature {
            params: vec![self.extern_pos(Pos::Future, "future")?],
            results: vec![self.position(Pos::Result, r)?],
            throws: None,
        })
    }

    /// `("tw:future", "reject<E>")`: `(externref, W(E))` (§16).
    pub fn future_reject(&self, e: &Path, cx: &Context<'_>) -> Result<Signature> {
        let r = self.enum_repr(e, cx, Flow::ToJs)?;
        Ok(Signature {
            params: vec![
                self.extern_pos(Pos::Future, "resolver")?,
                self.position(Pos::Param(0), r)?,
            ],
            results: Vec::new(),
            throws: None,
        })
    }

    fn enum_repr(&self, e: &Path, cx: &Context<'_>, flow: Flow) -> Result<Repr> {
        let layout = self.enum_path(e, cx, flow)?;
        Ok(Repr {
            ty: layout.path.clone(),
            kind: Kind::Enum {
                layout,
                nullable: false,
            },
            js_null: false,
        })
    }

    /// `("tw:tag", E)`: the tag's parameters, `W(E)` (§15).
    pub fn tag(&self, e: &Path, cx: &Context<'_>) -> Result<Vec<ValType>> {
        self.wire(&self.enum_repr(e, cx, Flow::ToLang)?)
    }

    /// The `tw:terminated` global (§15): with a linear memory, an
    /// immutable pointer-sized global holding the address of the state
    /// byte; otherwise a mutable `i32` holding the state.
    pub fn terminated(&self) -> Global {
        match self.ptr("") {
            Ok(ty) => Global { ty, mutable: false },
            Err(_) => Global {
                ty: ValType::I32,
                mutable: true,
            },
        }
    }

    /// `("tw:destroy", F | C)`, `tw:drop:F` and `tw:drop:C`: `(handle)`.
    pub fn handle_op(&self) -> Result<FuncType> {
        Ok(FuncType {
            params: vec![self.handle()?],
            results: Vec::new(),
        })
    }

    /// The language's side of a core function type (§12). Under
    /// `results = single`, slot 0 stays the result and the other slots are
    /// spilled, in slot order.
    pub fn lang_core(&self, ft: &FuncType) -> Result<LangCore> {
        if self.profile.results == Results::Multi || ft.results.len() <= 1 {
            return Ok(LangCore {
                ty: ft.clone(),
                spill: Vec::new(),
            });
        }
        let spill = ft.results[1..].to_vec();
        for v in &spill {
            if spill_name(v).is_none() {
                return Err(Error::Invalid(format!(
                    "`results = single` can't spill `{v}`; only numbers and `externref` spill"
                )));
            }
        }
        Ok(LangCore {
            ty: FuncType {
                params: ft.params.clone(),
                results: ft.results[..1].to_vec(),
            },
            spill,
        })
    }

    /// The `tw` operations with fixed signatures (§19), by default name:
    /// `tw:instanceof`, `tw:arr` `len`, the untyped `tw:future` and
    /// `tw:loop` imports, and the language exports. `None` for a name that
    /// isn't one, or whose profile requirement (a linear memory) is unmet.
    pub fn fixed_op(&self, module: Option<&str>, name: &str) -> Option<FuncType> {
        use crate::protocol as tw;
        use ValType::{I32, I64};
        const EXTERNREF: ValType = ValType::EXTERNREF;
        let ptr = self.ptr("").ok();
        let f = |params: Vec<ValType>, results: Vec<ValType>| Some(FuncType { params, results });
        match (module, name) {
            (Some(tw::INSTANCEOF), _) => f(vec![EXTERNREF], vec![I32]),
            (Some(tw::ARR), "len") => f(vec![EXTERNREF], vec![I32]),
            (Some(tw::IS_NULLISH | tw::IS_UNDEFINED), "") => f(vec![EXTERNREF], vec![I32]),
            (Some(tw::UNIT | tw::TRUE | tw::FALSE), "") => f(vec![], vec![EXTERNREF]),
            (Some(tw::FUTURE), "poll") => f(vec![EXTERNREF, I32], vec![I32]),
            (Some(tw::FUTURE), "drop") => f(vec![EXTERNREF], vec![]),
            (Some(tw::FUTURE), "new") => f(vec![], vec![EXTERNREF, EXTERNREF]),
            (Some(tw::LOOP), "work_available" | "drop") => f(vec![I32], vec![]),
            (None, tw::REALLOC) => {
                let p = ptr?;
                f(vec![p.clone(), p.clone(), p.clone(), p.clone()], vec![p])
            }
            (None, tw::FREE) => {
                let p = ptr?;
                f(vec![p.clone(), p.clone(), p], vec![])
            }
            (None, tw::TABLE_ALLOC) => f(vec![I32], vec![I32]),
            (None, tw::TABLE_FREE) => f(vec![I32, I32], vec![]),
            (None, tw::LOOP_TICK) => f(vec![I32, I32], vec![I32]),
            (None, tw::LOOP_NEXT_SLEEP) => f(vec![I32], vec![I64]),
            (None, tw::FUTURE_WAKE) => f(vec![I32], vec![]),
            _ => None,
        }
    }
}

fn collect_exported(
    items: &[Decl],
    module: Option<&str>,
    ns: &mut Vec<String>,
    exported: bool,
    out: &mut HashSet<Location>,
) {
    for d in items {
        match d {
            Decl::Namespace(n) => {
                ns.push(n.name.clone());
                collect_exported(&n.items, module, ns, exported || n.export, out);
                ns.pop();
            }
            Decl::Module(m) => {
                collect_exported(&m.items, Some(&m.spec), &mut Vec::new(), false, out)
            }
            Decl::Class(c) if c.export || exported => {
                let mut path = ns.clone();
                path.push(c.name.clone());
                out.insert(Location {
                    module: module.map(Into::into),
                    path,
                });
            }
            _ => {}
        }
    }
}

/// A box's unboxed type (§13).
fn unboxed(t: &Type) -> Option<Type> {
    Some(match t {
        Type::ScalarBox(s) => Type::Scalar(*s),
        Type::StrBox => Type::Str,
        Type::BigIntBox => Type::BigInt,
        Type::UnitBox => Type::Unit,
        Type::VecBox(e) => Type::Vec(e.clone()),
        Type::ArrBox(e) => Type::Array(e.clone()),
        Type::TupleBox(ts) => Type::Tuple(ts.clone()),
        _ => return None,
    })
}
