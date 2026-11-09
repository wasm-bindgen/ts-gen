//! The JS glue: every conversion between the JS ABI and the glue wire
//! (IR.md §7–§13, §15).
//!
//! The glue only ever sees linear forms ([`crate::form`]), so one set of
//! conversions serves every profile: a GC value reaches it already
//! translated into the bridge's scratch space. Validation, `@js` call
//! shapes, enum discrimination and error mapping all happen here, in one
//! crossing per call.

use std::collections::HashMap;
use std::fmt::Write;
use std::rc::Rc;

use ts_gen_abi::enums;
use ts_gen_abi::ir::*;
use ts_gen_abi::key::{location, Keyed, Role};
use ts_gen_abi::layout::{
    ArmLayout, EnumLayout, Host, Kind, Layouts, Origin, Pos, Position, Repr, ScalarNull, Shape,
    Signature,
};
use ts_gen_abi::profile::{BoxNone, Carrier, Encoding, StrCarrier, Throws};
use ts_gen_abi::resolve::{Context, Location, Target, TypeDecl};
use ts_gen_abi::text;

use crate::entry::ExportClass;
use crate::form::{scalar_jv, Forms, Jv, Num, Space, Store};
use crate::plan::{Func, Op};
use crate::{unsupported, Error, Result};

/// A JS string literal.
pub fn js_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn is_ident(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// `base.name`, or `base["name"]` when `name` isn't an identifier.
pub fn prop(base: &str, name: &str) -> String {
    if is_ident(name) {
        format!("{base}.{name}")
    } else {
        format!("{base}[{}]", js_str(name))
    }
}

/// `base?.name`, or `base?.["name"]` when `name` isn't an identifier.
fn opt_prop(base: &str, name: &str) -> String {
    if is_ident(name) {
        format!("{base}?.{name}")
    } else {
        format!("{base}?.[{}]", js_str(name))
    }
}

/// Statements of one generated function, with fresh temporaries.
#[derive(Default)]
pub struct Body {
    lines: Vec<String>,
    pub(crate) depth: usize,
    n: usize,
}

impl Body {
    pub(crate) fn line(&mut self, s: impl AsRef<str>) {
        let pad = "  ".repeat(self.depth + 1);
        self.lines.push(format!("{pad}{}", s.as_ref()));
    }

    pub(crate) fn open(&mut self, s: impl AsRef<str>) {
        self.line(s);
        self.depth += 1;
    }

    pub(crate) fn close(&mut self, s: impl AsRef<str>) {
        self.depth -= 1;
        self.line(s);
    }

    pub(crate) fn fresh(&mut self) -> String {
        self.n += 1;
        format!("t{}", self.n)
    }

    /// Binds `expr` to a fresh `const` unless it's already a plain name.
    pub(crate) fn bind(&mut self, expr: String) -> String {
        if is_ident(&expr) || expr.parse::<f64>().is_ok() {
            return expr;
        }
        let t = self.fresh();
        self.line(format!("const {t} = {expr};"));
        t
    }

    pub(crate) fn finish(self, head: &str) -> String {
        let mut s = format!("{head} {{\n");
        for l in self.lines {
            s.push_str(&l);
            s.push('\n');
        }
        s.push('}');
        s
    }
}

pub struct Glue<'a, 'b> {
    pub l: &'b Layouts<'a>,
    pub f: &'b Forms,
    pub ir: &'a Ir,
    /// The lib IRs `ir` resolves against.
    pub libs: Vec<&'a Ir>,
    pub validate: bool,
    pub helpers: Vec<String>,
    pub(crate) memo: HashMap<String, String>,
    pub modules: Vec<String>,
    /// The class values checks test against, as `$cls{i}`, resolved once
    /// at instantiate time.
    pub classes: Vec<String>,
    /// Tag variables, by enum path.
    pub tags: Vec<(String, String, Rc<EnumLayout>)>,
    n: usize,
    /// The language exports entries call, as `$E[i]`.
    pub entries: Vec<(String, Signature)>,
    pub(crate) entry_memo: HashMap<String, usize>,
    /// Pairing registries, as declarations.
    pub handles: Vec<String>,
    /// The drop exports finalizers call.
    pub drops: Vec<String>,
    pub export_classes: Vec<ExportClass<'a>>,
    /// Whether the language drives loops through the bridge (§16).
    pub loops: bool,
    /// The return area of the import `func` is generating, if it has one.
    pub ret: Option<crate::wat::RetArea>,
    /// Whether the glue wrote `ret`; a glue returning a helper's array
    /// leaves it to a wrapper.
    pub ret_used: bool,
    /// Whether an `arr<str>` lowers through the bridge's `allocn`.
    pub allocn: bool,
    /// The GC values the bridge module converts in and out of the
    /// language's table (`$B.c{i}`, `$B.d{i}`).
    pub convs: Vec<Repr>,
    /// The language module's export names, where mono export variants are
    /// found (§13).
    pub lang_exports: Vec<String>,
    /// A mono export variant's type arguments, by its key.
    pub(crate) mono_targs: HashMap<String, Vec<Type>>,
}

/// An arm `enum_lower` tests: the arm of each enum on the way down to a
/// nested enum's arm, and that arm's place in its tier.
struct Leaf {
    route: Vec<usize>,
    rank: u8,
}

#[derive(Clone, Copy)]
pub struct Mode {
    pub checked: bool,
    /// `tw:unbox` and `tw:arr get`: normalize `undefined` to none even
    /// unchecked, since a box position passes it through (§13).
    pub unbox: bool,
}

pub(crate) fn lit(l: &Literal) -> String {
    match l {
        Literal::Str(s) => js_str(s),
        Literal::Int(i) => i.to_string(),
        Literal::Bool(b) => b.to_string(),
        Literal::Null => "null".into(),
    }
}

fn typed_array(s: Scalar) -> &'static str {
    match s {
        Scalar::I8 => "Int8Array",
        Scalar::U8 => "Uint8Array",
        Scalar::I16 => "Int16Array",
        Scalar::U16 => "Uint16Array",
        Scalar::I32 => "Int32Array",
        Scalar::U32 => "Uint32Array",
        Scalar::I64 => "BigInt64Array",
        Scalar::U64 => "BigUint64Array",
        Scalar::F16 => "Float16Array",
        Scalar::F32 => "Float32Array",
        Scalar::F64 => "Float64Array",
        _ => unreachable!("validated: not a `vec` element"),
    }
}

fn lift_scalar(s: Scalar, w: &str) -> String {
    match s {
        Scalar::Bool => format!("({w} !== 0)"),
        Scalar::I8 => format!("({w} << 24 >> 24)"),
        Scalar::U8 => format!("({w} & 255)"),
        Scalar::I16 => format!("({w} << 16 >> 16)"),
        Scalar::U16 => format!("({w} & 65535)"),
        Scalar::U32 => format!("({w} >>> 0)"),
        Scalar::I53 => format!("$liftI53({w})"),
        Scalar::U53 => format!("$liftU53({w})"),
        Scalar::U64 => format!("BigInt.asUintN(64, {w})"),
        Scalar::I32 | Scalar::I64 | Scalar::F16 | Scalar::F32 | Scalar::F64 => w.to_string(),
    }
}

fn lower_scalar(s: Scalar, v: &str, m: Mode) -> String {
    // Unchecked coercions keep the carrier's range, so the language can
    // still trust it.
    if !m.checked {
        return match s {
            Scalar::Bool => format!("({v} ? 1 : 0)"),
            Scalar::I8 => format!("($uint({v}) << 24 >> 24)"),
            Scalar::U8 => format!("($uint({v}) & 255)"),
            Scalar::I16 => format!("($uint({v}) << 16 >> 16)"),
            Scalar::U16 => format!("($uint({v}) & 65535)"),
            Scalar::I32 | Scalar::U32 => format!("$uint({v})"),
            Scalar::I53 | Scalar::U53 | Scalar::I64 | Scalar::U64 => format!("$ubig({v})"),
            Scalar::F16 | Scalar::F32 | Scalar::F64 => format!("Number({v})"),
        };
    }
    let int = |lo: i64, hi: i64| format!("$int({v}, {lo}, {hi}, {})", js_str(s.name()));
    match s {
        Scalar::Bool => format!("$bool({v})"),
        Scalar::I8 => int(-128, 127),
        Scalar::U8 => int(0, 255),
        Scalar::I16 => int(-32768, 32767),
        Scalar::U16 => int(0, 65535),
        Scalar::I32 => int(i32::MIN as i64, i32::MAX as i64),
        Scalar::U32 => int(0, u32::MAX as i64),
        Scalar::I53 => format!("$i53({v})"),
        Scalar::U53 => format!("$u53({v})"),
        Scalar::I64 => format!("$i64({v})"),
        Scalar::U64 => format!("$u64({v})"),
        Scalar::F16 | Scalar::F32 | Scalar::F64 => format!("$num({v}, {})", js_str(s.name())),
    }
}

/// The JS literal of a niche on its wire type.
fn niche(s: Scalar, n: i64) -> String {
    match scalar_jv(s) {
        Jv::I64 => format!("{n}n"),
        _ => n.to_string(),
    }
}

impl<'a, 'b> Glue<'a, 'b> {
    pub fn new(l: &'b Layouts<'a>, f: &'b Forms, ir: &'a Ir, libs: Vec<&'a Ir>) -> Glue<'a, 'b> {
        Glue {
            l,
            f,
            ir,
            libs,
            validate: l.profile.validation,
            helpers: Vec::new(),
            memo: HashMap::new(),
            modules: Vec::new(),
            classes: Vec::new(),
            tags: Vec::new(),
            n: 0,
            entries: Vec::new(),
            entry_memo: HashMap::new(),
            handles: Vec::new(),
            drops: Vec::new(),
            export_classes: Glue::collect_export_classes(ir),
            loops: false,
            ret: None,
            ret_used: false,
            allocn: false,
            convs: Vec::new(),
            lang_exports: Vec::new(),
            mono_targs: HashMap::new(),
        }
    }

    pub(crate) fn mode(&self) -> Mode {
        Mode {
            checked: self.validate,
            unbox: false,
        }
    }

    pub(crate) fn helper(
        &mut self,
        key: String,
        build: impl FnOnce(&mut Self, &str) -> Result<String>,
    ) -> Result<String> {
        if let Some(n) = self.memo.get(&key) {
            return Ok(n.clone());
        }
        self.n += 1;
        let name = format!("$h{}", self.n);
        self.memo.insert(key, name.clone());
        let src = build(self, &name)?;
        self.helpers.push(src);
        Ok(name)
    }

    /// A pointer-sized wire value read as a JS number.
    pub(crate) fn num(&self, s: Space, w: &str) -> String {
        match self.f.ptr(s) {
            Jv::I64 => format!("Number({w})"),
            _ => w.to_string(),
        }
    }

    /// A JS number as a pointer-sized wire value.
    fn wire_ptr(&self, s: Space, v: &str) -> String {
        match self.f.ptr(s) {
            Jv::I64 => format!("BigInt({v})"),
            _ => v.to_string(),
        }
    }

    pub(crate) fn null_len(&self, s: Space) -> &'static str {
        match self.f.ptr(s) {
            Jv::I64 => "-1n",
            _ => "-1",
        }
    }

    // Declarations and their JS values.

    fn module_var(&mut self, spec: &str) -> String {
        let i = match self.modules.iter().position(|m| m == spec) {
            Some(i) => i,
            None => {
                self.modules.push(spec.to_string());
                self.modules.len() - 1
            }
        };
        format!("$m{i}")
    }

    /// The JS value of a declaration (§5): a member get on `globalThis`, or
    /// on the module's namespace through an export naming the declaration.
    pub(crate) fn decl_value(&mut self, loc: &Location) -> Result<String> {
        self.path_value(loc.module.as_deref(), &loc.path)
    }

    fn module_exports(&self, spec: &str) -> Vec<ExportName> {
        std::iter::once(self.ir)
            .chain(self.libs.iter().copied())
            .flat_map(|ir| &ir.items)
            .find_map(|d| match d {
                Decl::Module(m) if m.spec == spec => Some(m.exports.clone().unwrap_or_default()),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub(crate) fn path_value(&mut self, module: Option<&str>, path: &[String]) -> Result<String> {
        self.path_expr(module, path, prop)
    }

    /// The path's value, reaching each segment past the first with `member`.
    fn path_expr(
        &mut self,
        module: Option<&str>,
        path: &[String],
        member: fn(&str, &str) -> String,
    ) -> Result<String> {
        let first = match module {
            None => prop("globalThis", &path[0]),
            Some(spec) => {
                let exports = self.module_exports(spec);
                let Some(e) = exports.iter().find(|e| e.name == path[0]) else {
                    return Err(Error::Import(format!(
                        "`{}` isn't exported by module {}",
                        path[0],
                        js_str(spec)
                    )));
                };
                let m = self.module_var(spec);
                prop(&m, e.alias.as_deref().unwrap_or(&e.name))
            }
        };
        Ok(path[1..].iter().fold(first, |b, s| member(&b, s)))
    }

    /// The variable holding the value a class check tests against, named in
    /// canonical spelling: the class, or `null` when its path doesn't hold
    /// a function (§8).
    pub(crate) fn class_value(&mut self, canonical: &str) -> Result<String> {
        let path = match text::parse_type(canonical) {
            Ok(Type::Named(r)) => r.path,
            _ => return Err(Error::Import(format!("`{canonical}` isn't a class path"))),
        };
        self.class_check(&path)
    }

    pub(crate) fn class_check(&mut self, path: &Path) -> Result<String> {
        let loc = match self.l.keys.symbols.resolve_decl(path, &Context::default()) {
            Some((loc, TypeDecl::Class(_))) => loc,
            _ => {
                return Err(Error::Import(format!(
                    "`{}` doesn't name a class",
                    path.segments.join(".")
                )))
            }
        };
        let v = self.path_expr(loc.module.as_deref(), &loc.path, opt_prop)?;
        let i = match self.classes.iter().position(|c| *c == v) {
            Some(i) => i,
            None => {
                self.classes.push(v);
                self.classes.len() - 1
            }
        };
        Ok(format!("$cls{i}"))
    }

    /// An error class match names an IR class, or else a JS global such as
    /// the built-in `err`'s `TypeError` (§15).
    pub(crate) fn error_class(&mut self, path: &Path) -> Result<String> {
        match self.l.keys.symbols.resolve_decl(path, &Context::default()) {
            Some((loc, TypeDecl::Class(_))) => self.decl_value(&loc),
            _ => self.path_value(None, &path.segments),
        }
    }

    // Wire → JS.

    /// `r` as a JS value, from its glue wire values `w` (plain names).
    pub fn lift(&mut self, r: &Repr, w: &[String]) -> Result<String> {
        if let Kind::Rec { .. } = r.kind {
            return self.lift(&self.l.rec(r)?, w);
        }
        let absent = if r.js_null { "null" } else { "undefined" };
        Ok(match &r.kind {
            Kind::Rec { .. } => unreachable!(),
            Kind::Unit | Kind::Never => "undefined".into(),
            Kind::Scalar(s) => lift_scalar(*s, &w[0]),
            Kind::NullScalar(s, ScalarNull::Niche(n)) => format!(
                "({} === {} ? {absent} : {})",
                w[0],
                niche(*s, *n),
                lift_scalar(*s, &w[0])
            ),
            Kind::NullScalar(s, ScalarNull::Present) => {
                format!("({} !== 0 ? {} : {absent})", w[0], lift_scalar(*s, &w[1]))
            }
            Kind::Str { carrier, nullable } => {
                let (s, f) = match carrier {
                    StrCarrier::Host | StrCarrier::Gc(Encoding::Wtf16) => {
                        return Ok(if *nullable {
                            format!("({} ?? {absent})", w[0])
                        } else {
                            w[0].clone()
                        })
                    }
                    StrCarrier::Linear(e) => (Space::Lang, *e),
                    StrCarrier::Gc(e) => (Space::Scratch, *e),
                };
                let lift = match f {
                    Encoding::Utf8 => "$liftUtf8",
                    Encoding::Wtf16 => "$liftWtf16",
                };
                let e = format!(
                    "{lift}({}, {}, {})",
                    s.js(),
                    self.num(s, &w[0]),
                    self.num(s, &w[1])
                );
                self.nullable_seq(*nullable, s, &w[1], e, absent)
            }
            // `T[]` is a fresh copy, except `mut T[]`, whose writes land in
            // the language's own array, and a host bigint, a primitive.
            Kind::Vec {
                elem,
                carrier: Carrier::Host,
                nullable,
                writeback: false,
                bigint: false,
            } => {
                let ta = typed_array(*elem);
                if *nullable {
                    format!("({w} == null ? {absent} : {ta}.from({w}))", w = w[0])
                } else {
                    format!("{ta}.from({})", w[0])
                }
            }
            // A host value is the JS value itself, the receiver's to keep.
            Kind::Vec {
                carrier: Carrier::Host,
                nullable,
                ..
            }
            | Kind::Array {
                carrier: Carrier::Host,
                nullable,
                ..
            } => {
                if *nullable {
                    format!("({} ?? {absent})", w[0])
                } else {
                    w[0].clone()
                }
            }
            Kind::Vec {
                carrier,
                nullable,
                bigint: true,
                ..
            } => {
                let s = Space::of(*carrier);
                let e = format!(
                    "$bigOf($liftVec({}, {}, {}, BigUint64Array))",
                    s.js(),
                    self.num(s, &w[0]),
                    self.num(s, &w[1]),
                );
                self.nullable_seq(*nullable, s, &w[1], e, absent)
            }
            Kind::Vec {
                elem,
                carrier,
                nullable,
                ..
            } => {
                let s = Space::of(*carrier);
                let e = format!(
                    "$liftVec({}, {}, {}, {})",
                    s.js(),
                    self.num(s, &w[0]),
                    self.num(s, &w[1]),
                    typed_array(*elem)
                );
                self.nullable_seq(*nullable, s, &w[1], e, absent)
            }
            Kind::Array {
                elem,
                carrier,
                nullable,
            } => {
                let s = Space::of(*carrier);
                let h = self.array_lift(elem, s)?;
                let len = w.last().unwrap();
                let e = format!("{h}({})", w.join(", "));
                self.nullable_seq(*nullable, s, len, e, absent)
            }
            Kind::Tuple { elems, nullable } => {
                let mut at = usize::from(*nullable);
                let mut parts = Vec::new();
                for e in elems {
                    let n = self.f.jsw(e)?.len();
                    parts.push(self.lift(e, &w[at..at + n])?);
                    at += n;
                }
                let t = format!("[{}]", parts.join(", "));
                if *nullable {
                    format!("({} !== 0 ? {t} : {absent})", w[0])
                } else {
                    t
                }
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Carrier(c) if *nullable => {
                    format!("({} !== 0 ? {} : {absent})", w[0], lift_scalar(*c, &w[1]))
                }
                Shape::Carrier(c) => lift_scalar(*c, &w[0]),
                _ => {
                    let h = self.enum_lift(layout)?;
                    let e = format!("{h}({})", w.join(", "));
                    if *nullable {
                        format!("({} === 0 ? {absent} : {e})", w[0])
                    } else {
                        e
                    }
                }
            },
            // JS sees the promise of a future the language holds.
            Kind::Host {
                of: Host::Future { .. },
                nullable,
            } => {
                if *nullable {
                    format!("($promise({}) ?? {absent})", w[0])
                } else {
                    format!("$promise({})", w[0])
                }
            }
            // An erased `T` is its JS value: a box, a host reference or a
            // wrapper (§13), with `ref.null` as `undefined` under
            // `box-none = null`, else as `null` like a box's. Validation
            // catches a raw language object.
            Kind::Erased { .. } => {
                let v = if self.l.profile.handles != Carrier::Linear && self.l.profile.validation {
                    format!("$erased({})", w[0])
                } else {
                    w[0].clone()
                };
                if self.null_none() {
                    format!("({v} ?? undefined)")
                } else {
                    v
                }
            }
            // Under `box-none = null`, a box's none is `ref.null`, written
            // as the node's default, and the `Unit` box is `ref.null` too.
            Kind::Host {
                of: Host::Box(inner),
                ..
            } if self.null_none() && inner.kind == Kind::Unit => "undefined".into(),
            Kind::Host {
                of: Host::Box(_),
                nullable: true,
            } if self.null_none() => format!("({} ?? {absent})", w[0]),
            // Under `box-none = unit`, a box is faithful, except that a
            // `@js(null) T?` writes the unit box, the language's none, as
            // `null` (§13).
            Kind::Host {
                of: Host::Box(_), ..
            } if r.js_null => format!("({w} === undefined ? null : {w})", w = w[0]),
            Kind::Host {
                of: Host::Box(_), ..
            } => w[0].clone(),
            Kind::Host { nullable, .. } => {
                if *nullable {
                    format!("({} ?? {absent})", w[0])
                } else {
                    w[0].clone()
                }
            }
            Kind::View { elem, .. } => self.view(*elem, w, false),
            Kind::Handle {
                class, nullable, ..
            } => {
                let e = self.wrap_class(class, &w[0])?;
                self.nullable_handle(*nullable, &w[0], e, absent)
            }
            Kind::Fn {
                provided: Some(_),
                nullable,
                ..
            } => {
                let h = self.fn_wrap(r)?;
                self.nullable_handle(*nullable, &w[0], format!("{h}({})", w[0]), absent)
            }
            Kind::Fn { nullable, .. } => {
                if *nullable {
                    format!("({} ?? {absent})", w[0])
                } else {
                    w[0].clone()
                }
            }
        })
    }

    /// A typed array over the language's memory (§11). An `unshared` one
    /// is a copy when the memory is shared.
    fn view(&self, elem: Scalar, w: &[String], unshared: bool) -> String {
        format!(
            "$view({}, {}, {}, {unshared})",
            typed_array(elem),
            self.num(Space::Lang, &w[0]),
            self.num(Space::Lang, &w[1])
        )
    }

    /// A nullable handle is `0` in linear and `null` otherwise (§14).
    fn nullable_handle(&self, nullable: bool, w: &str, e: String, absent: &str) -> String {
        if !nullable {
            return e;
        }
        let null = match self.l.profile.handles {
            Carrier::Linear => self.wire_ptr_lit(Space::Lang, "0".into()),
            _ => "null".into(),
        };
        format!("({w} === {null} ? {absent} : {e})")
    }

    fn nullable_seq(&self, nullable: bool, s: Space, len: &str, e: String, absent: &str) -> String {
        if nullable {
            format!("({len} === {} ? {absent} : {e})", self.null_len(s))
        } else {
            e
        }
    }

    /// The bridge module's conversion of a GC value held externalized in
    /// the language's table, by index: `$B.c{i}` reads it into scratch,
    /// `$B.d{i}` writes it back.
    pub(crate) fn conv(&mut self, r: &Repr) -> usize {
        match self.convs.iter().position(|c| c == r) {
            Some(i) => i,
            None => {
                self.convs.push(r.clone());
                self.convs.len() - 1
            }
        }
    }

    /// Reads an element's glue wire values from memory, converting GC
    /// values out of the table when `convert`, else leaving them unread.
    pub(crate) fn load_elem(
        &mut self,
        b: &mut Body,
        s: Space,
        e: &crate::form::Elem,
        convert: bool,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for st in &e.stores {
            match *st {
                Store::Conv { idx, n, conv } if convert => {
                    let i = self.conv(&e.convs[conv as usize]);
                    let v = b.bind(format!("$B.c{i}({}.table.get(x + {idx}))", s.js()));
                    if n == 1 {
                        out.push(v);
                    } else {
                        out.extend((0..n).map(|j| format!("{v}[{j}]")));
                    }
                }
                Store::Conv { n, .. } => {
                    out.extend((0..n).map(|_| "undefined".to_string()));
                }
                Store::Arm { off, ref arms, .. } => {
                    let arm = Store::Num { off, num: Num::I32 };
                    let read = self.load(b, s, &arm);
                    let a = b.bind(read);
                    out.push(a.clone());
                    // Only the arm's own payload is read; the others are zero.
                    for arm in arms {
                        let on = format!("{a} === {}", arm.index);
                        for (st, zeros) in &arm.values {
                            let zero: Vec<&str> = zeros.iter().map(|j| j.zero()).collect();
                            match *st {
                                Store::Conv { idx, n, conv } if convert => {
                                    let i = self.conv(&e.convs[conv as usize]);
                                    let read = format!("$B.c{i}({}.table.get(x + {idx}))", s.js());
                                    if n == 1 {
                                        out.push(b.bind(format!("{on} ? {read} : {}", zero[0])));
                                    } else {
                                        let v = b
                                            .bind(format!("{on} ? {read} : [{}]", zero.join(", ")));
                                        out.extend((0..n).map(|j| format!("{v}[{j}]")));
                                    }
                                }
                                Store::Conv { n, .. } => {
                                    out.extend((0..n).map(|_| "undefined".to_string()));
                                }
                                _ => {
                                    let read = self.load(b, s, st);
                                    out.push(b.bind(format!("{on} ? {read} : {}", zero[0])));
                                }
                            }
                        }
                    }
                }
                _ => out.push(self.load(b, s, st)),
            }
        }
        out
    }

    /// Writes an element's glue wire values to memory, converting GC values
    /// into the table.
    fn store_elem(&mut self, b: &mut Body, s: Space, e: &crate::form::Elem, w: &[String]) {
        let mut at = 0;
        for st in &e.stores {
            match *st {
                Store::Conv { idx, n, conv } => {
                    let i = self.conv(&e.convs[conv as usize]);
                    let args = w[at..at + n as usize].join(", ");
                    b.line(format!("{}.table.set(x + {idx}, $B.d{i}({args}));", s.js()));
                    at += n as usize;
                }
                Store::Arm {
                    off,
                    ref slots,
                    ref arms,
                } => {
                    let a = w[at].clone();
                    at += 1;
                    self.store(b, s, &Store::Num { off, num: Num::I32 }, &a);
                    // Unused slots are zero or null (§8).
                    for st in slots {
                        let zero = match st {
                            Store::Num { num: Num::I64, .. } => "0n",
                            Store::Num { .. } => "0",
                            _ => "null",
                        };
                        self.store(b, s, st, zero);
                    }
                    for arm in arms {
                        let n: usize = arm.values.iter().map(|(_, z)| z.len()).sum();
                        b.open(format!("if ({a} === {}) {{", arm.index));
                        let mut k = at;
                        for (st, zeros) in &arm.values {
                            match *st {
                                Store::Conv { idx, conv, .. } => {
                                    let i = self.conv(&e.convs[conv as usize]);
                                    let args = w[k..k + zeros.len()].join(", ");
                                    b.line(format!(
                                        "{}.table.set(x + {idx}, $B.d{i}({args}));",
                                        s.js()
                                    ));
                                }
                                _ => self.store(b, s, st, &w[k]),
                            }
                            k += zeros.len();
                        }
                        b.close("}");
                        at += n;
                    }
                }
                _ => {
                    self.store(b, s, st, &w[at]);
                    at += 1;
                }
            }
        }
    }

    /// Reads one glue wire value from memory.
    fn load(&self, b: &mut Body, s: Space, st: &Store) -> String {
        let e = match *st {
            Store::Num { off, num } => format!("dv.get{}(o + {off}, true)", num.dv()),
            Store::Niche { off, num, niche } => {
                let n = if num == Num::I64 {
                    format!("{niche}n")
                } else {
                    niche.to_string()
                };
                format!(
                    "(dv.getUint8(o + {off}) ? dv.get{}(o + {}, true) : {n})",
                    num.dv(),
                    off + 1
                )
            }
            Store::Flag { off } => format!("dv.getUint8(o + {off})"),
            Store::Ref { idx } => format!("{}.table.get(x + {idx})", s.js()),
            Store::Conv { .. } | Store::Arm { .. } => unreachable!("read by load_elem"),
        };
        b.bind(e)
    }

    fn store(&self, b: &mut Body, s: Space, st: &Store, w: &str) {
        match *st {
            Store::Num { off, num } => b.line(format!("dv.set{}(o + {off}, {w}, true);", num.dv())),
            Store::Niche { off, num, niche } => {
                let n = if num == Num::I64 {
                    format!("{niche}n")
                } else {
                    niche.to_string()
                };
                let zero = if num == Num::I64 { "0n" } else { "0" };
                b.open(format!("if ({w} === {n}) {{"));
                b.line(format!("dv.setUint8(o + {off}, 0);"));
                b.line(format!(
                    "dv.set{}(o + {}, {zero}, true);",
                    num.dv(),
                    off + 1
                ));
                b.close("} else {");
                b.depth += 1;
                b.line(format!("dv.setUint8(o + {off}, 1);"));
                b.line(format!("dv.set{}(o + {}, {w}, true);", num.dv(), off + 1));
                b.close("}");
            }
            Store::Flag { off } => b.line(format!("dv.setUint8(o + {off}, {w});")),
            Store::Ref { idx } => b.line(format!("{}.table.set(x + {idx}, {w});", s.js())),
            Store::Conv { .. } | Store::Arm { .. } => unreachable!("written by store_elem"),
        }
    }

    fn array_lift(&mut self, elem: &Repr, s: Space) -> Result<String> {
        let key = format!("alift {s:?} {:?}", elem);
        let e = self.f.elem(elem, s)?;
        let elem = elem.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            let mut ps = Vec::new();
            if e.size > 0 {
                ps.push("p");
            }
            if e.refs > 0 {
                ps.push("x0");
            }
            ps.push("len");
            if e.size > 0 {
                b.line(format!("const base = {};", g.num(s, "p")));
            }
            b.line(format!("const n = {};", g.num(s, "len")));
            b.line("const out = new Array(n);");
            b.open("for (let i = 0; i < n; i++) {");
            if e.size > 0 {
                b.line(format!("const dv = {}.dv();", s.js()));
                b.line(format!("const o = base + i * {};", e.size));
            }
            if e.refs > 0 {
                b.line(format!("const x = x0 + i * {};", e.refs));
            }
            // Converted elements pass through scratch, one at a time.
            let conv = !e.convs.is_empty();
            if conv {
                b.line("const m = $S.mark();");
                b.open("try {");
            }
            let w = g.load_elem(&mut b, s, &e, true);
            let v = g.lift(&elem, &w)?;
            b.line(format!("out[i] = {v};"));
            if conv {
                b.close("} finally {");
                b.depth += 1;
                b.line("$S.reset(m);");
                b.close("}");
            }
            b.close("}");
            b.line("return out;");
            Ok(b.finish(&format!("function {name}({})", ps.join(", "))))
        })
    }

    fn enum_lift(&mut self, e: &Rc<EnumLayout>) -> Result<String> {
        let key = format!("elift {}", e.path);
        let e = e.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            let gc = g.f.enum_has_gc(&e);
            let slots: Vec<String> = match &e.shape {
                Shape::Payload { slots } if !gc => {
                    (0..slots.len()).map(|i| format!("s{i}")).collect()
                }
                Shape::Payload { .. } => {
                    let mut n = 0;
                    for a in &e.arms {
                        for p in &a.payload {
                            n += g.f.jsw(p)?.len();
                        }
                    }
                    (0..n).map(|i| format!("s{i}")).collect()
                }
                _ => Vec::new(),
            };
            let mut ps = vec!["arm".to_string()];
            ps.extend(slots.iter().cloned());
            b.open("switch (arm) {");
            let mut region = 0;
            for a in &e.arms {
                let ws = g.arm_wire(&e, a, gc, &mut region)?;
                let ws: Vec<String> = ws.iter().map(|i| slots[*i].clone()).collect();
                b.open(format!("case {}: {{", a.index));
                let v = g.arm_value(&e, a, &ws)?;
                b.line(format!("return {v};"));
                b.close("}");
            }
            b.close("}");
            b.line(format!(
                "throw new TypeError({});",
                js_str(&format!("an invalid `{}` arm", e.path))
            ));
            Ok(b.finish(&format!("function {name}({})", ps.join(", "))))
        })
    }

    /// The glue wire indices of an arm's payload: its slots, or (for an enum
    /// with a GC payload) its own region.
    pub(crate) fn arm_wire(
        &self,
        _e: &EnumLayout,
        a: &ArmLayout,
        gc: bool,
        region: &mut usize,
    ) -> Result<Vec<usize>> {
        if gc {
            let mut n = 0;
            for p in &a.payload {
                n += self.f.jsw(p)?.len();
            }
            let out = (*region..*region + n).collect();
            *region += n;
            Ok(out)
        } else {
            Ok(a.slots.clone())
        }
    }

    /// An arm's JS value, from its payload's glue wire values.
    fn arm_value(&mut self, e: &EnumLayout, a: &ArmLayout, w: &[String]) -> Result<String> {
        if a.payload.is_empty() {
            if e.origin == Origin::Nested {
                return Ok("undefined".into());
            }
            return Ok(match a.js.literal() {
                Some(l) => js_str(l),
                None => format!(
                    "(() => {{ throw new TypeError({}); }})()",
                    js_str(&format!("`{}.{}` has no JS value", e.path, a.name))
                ),
            });
        }
        let mut at = 0;
        let mut vals = Vec::new();
        for p in &a.payload {
            let n = self.f.jsw(p)?.len();
            vals.push(self.lift(p, &w[at..at + n])?);
            at += n;
        }
        Ok(if vals.len() == 1 {
            vals.pop().unwrap()
        } else {
            format!("[{}]", vals.join(", "))
        })
    }

    // JS → wire.

    /// `v` (a plain name) as `r`'s glue wire values, validated, allocating
    /// in the language's space on the cleanup list `cx`.
    pub fn lower(&mut self, b: &mut Body, r: &Repr, v: &str, m: Mode) -> Result<Vec<String>> {
        if let Kind::Rec { .. } = r.kind {
            return self.lower(b, &self.l.rec(r)?, v, m);
        }
        let what = js_str(&r.ty);
        if r.js_null && m.checked && self.l.profile.strict_null {
            b.line(format!("if ({v} === undefined) $fail({what}, {v});"));
        }
        // A non-allocating lowering runs without a cleanup list.
        let cx = if lowers_alloc(r) { "cx" } else { "null" };
        Ok(match &r.kind {
            Kind::Unit | Kind::Never => Vec::new(),
            Kind::Scalar(s) => vec![b.bind(lower_scalar(*s, v, m))],
            Kind::NullScalar(s, ScalarNull::Niche(n)) => vec![b.bind(format!(
                "({v} == null ? {} : {})",
                niche(*s, *n),
                lower_scalar(*s, v, m)
            ))],
            Kind::NullScalar(s, ScalarNull::Present) => {
                let zero = scalar_jv(*s).zero();
                vec![
                    b.bind(format!("({v} == null ? 0 : 1)")),
                    b.bind(format!(
                        "({v} == null ? {zero} : {})",
                        lower_scalar(*s, v, m)
                    )),
                ]
            }
            Kind::Str { carrier, nullable } => {
                let check = if m.checked {
                    format!("$str({v})")
                } else {
                    format!("$ustr({v})")
                };
                let (s, enc) = match carrier {
                    StrCarrier::Host | StrCarrier::Gc(Encoding::Wtf16) => {
                        return Ok(vec![b.bind(if *nullable {
                            format!("({v} == null ? null : {check})")
                        } else {
                            check
                        })])
                    }
                    StrCarrier::Linear(e) => (Space::Lang, *e),
                    StrCarrier::Gc(e) => (Space::Scratch, *e),
                };
                let f = match enc {
                    Encoding::Utf8 => "$lowerUtf8",
                    Encoding::Wtf16 => "$lowerWtf16",
                };
                let e = format!("{f}({}, {check}, {cx})", s.js());
                self.seq_lower(b, *nullable, s, v, e)
            }
            Kind::Vec {
                carrier,
                nullable,
                bigint: true,
                ..
            } => {
                let check = if m.checked {
                    format!("$bigint({v}, {what})")
                } else {
                    format!("(typeof {v} === \"bigint\" ? {v} : 0n)")
                };
                if *carrier == Carrier::Host {
                    return Ok(vec![b.bind(if *nullable {
                        format!("({v} == null ? null : {check})")
                    } else {
                        check
                    })]);
                }
                let s = Space::of(*carrier);
                let e = format!(
                    "$lowerVec({}, $bigLimbs({check}), BigUint64Array, {cx})",
                    s.js()
                );
                self.seq_lower(b, *nullable, s, v, e)
            }
            Kind::Vec {
                elem,
                carrier: Carrier::Host,
                nullable,
                writeback,
                ..
            } => {
                let ta = typed_array(*elem);
                let check = if m.checked {
                    format!("$typed({v}, {ta}, {what})")
                } else {
                    format!("({v} instanceof {ta} ? {v} : new {ta}(0))")
                };
                // A fresh copy, except `mut T[]`, whose writes land in the
                // caller's own array.
                let e = if *writeback {
                    check
                } else {
                    format!("{ta}.from({check})")
                };
                vec![b.bind(if *nullable {
                    format!("({v} == null ? null : {e})")
                } else {
                    e
                })]
            }
            Kind::Array {
                elem,
                carrier: Carrier::Host,
                nullable,
            } => {
                let h = self.host_array(elem, m)?;
                vec![b.bind(if *nullable {
                    format!("({v} == null ? null : {h}({v}))")
                } else {
                    format!("{h}({v})")
                })]
            }
            Kind::Vec {
                elem,
                carrier,
                nullable,
                ..
            } => {
                let s = Space::of(*carrier);
                let ta = typed_array(*elem);
                let check = if m.checked {
                    format!("$typed({v}, {ta}, {what})")
                } else {
                    format!("({v} instanceof {ta} ? {v} : new {ta}(0))")
                };
                let e = format!("$lowerVec({}, {check}, {ta}, {cx})", s.js());
                self.seq_lower(b, *nullable, s, v, e)
            }
            Kind::Array {
                elem,
                carrier,
                nullable,
            } => {
                let s = Space::of(*carrier);
                let h = self.array_lower(elem, s, m)?;
                let n = self.f.jsw(r)?.len();
                let t = if *nullable {
                    let mut zeros: Vec<String> = vec!["0".into(); n - 1];
                    zeros = zeros.into_iter().map(|z| self.wire_ptr_lit(s, z)).collect();
                    zeros.push(self.null_len(s).into());
                    b.bind(format!(
                        "({v} == null ? [{}] : {h}({v}, {cx}))",
                        zeros.join(", ")
                    ))
                } else {
                    b.bind(format!("{h}({v}, {cx})"))
                };
                (0..n).map(|i| format!("{t}[{i}]")).collect()
            }
            Kind::Tuple { elems, nullable } => {
                let n = elems.len();
                let jsw = self.f.jsw(r)?;
                if *nullable {
                    let outs: Vec<String> = jsw.iter().map(|_| b.fresh()).collect();
                    b.line(format!(
                        "let {};",
                        outs.iter()
                            .zip(&jsw)
                            .map(|(o, j)| format!("{o} = {}", j.zero()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    b.open(format!("if ({v} != null) {{"));
                    let inner = self.tuple_lower(b, elems, v, n, &what, m)?;
                    b.line(format!("{} = 1;", outs[0]));
                    for (o, w) in outs[1..].iter().zip(inner) {
                        b.line(format!("{o} = {w};"));
                    }
                    b.close("}");
                    outs
                } else {
                    self.tuple_lower(b, elems, v, n, &what, m)?
                }
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Carrier(c) if *nullable => {
                    let zero = scalar_jv(*c).zero();
                    vec![
                        b.bind(format!("({v} == null ? 0 : 1)")),
                        b.bind(format!(
                            "({v} == null ? {zero} : {})",
                            lower_scalar(*c, v, m)
                        )),
                    ]
                }
                Shape::Carrier(c) => vec![b.bind(lower_scalar(*c, v, m))],
                Shape::Index => {
                    let h = self.enum_lower(layout, m)?;
                    vec![b.bind(if *nullable {
                        format!("({v} == null ? 0 : {h}({v}, {cx}))")
                    } else {
                        format!("{h}({v}, {cx})")
                    })]
                }
                Shape::Payload { .. } => {
                    let h = self.enum_lower(layout, m)?;
                    let jsw = self.f.jsw(r)?;
                    let t = if *nullable {
                        let zeros: Vec<&str> = jsw.iter().map(|j| j.zero()).collect();
                        b.bind(format!(
                            "({v} == null ? [{}] : {h}({v}, {cx}))",
                            zeros.join(", ")
                        ))
                    } else {
                        b.bind(format!("{h}({v}, {cx})"))
                    };
                    (0..jsw.len()).map(|i| format!("{t}[{i}]")).collect()
                }
            },
            Kind::Host {
                of: Host::Future { value, .. },
                nullable,
            } => {
                let check = self.resolution_check(value, m)?;
                let e = format!("$future({v}, {check})");
                vec![b.bind(if *nullable {
                    format!("({v} == null ? null : {e})")
                } else {
                    e
                })]
            }
            Kind::Host {
                of: Host::Box(inner),
                ..
            } if inner.kind == Kind::Unit => vec![b.bind(self.none().into())],
            // A box is the host value as it is: only `tw:unbox` tests it.
            // Under `box-none = null`, both empties are `ref.null`.
            Kind::Host {
                of: of @ Host::Box(_),
                nullable,
            } => vec![b.bind(
                if *nullable && (m.unbox || self.null_none()) && !of.nested() {
                    format!("({v} ?? null)")
                } else {
                    v.to_string()
                },
            )],
            Kind::Host { of, nullable } => {
                let check = if !m.checked {
                    v.to_string()
                } else {
                    match of {
                        of if of.symbol() => format!("$symbol({v}, {what})"),
                        Host::Class {
                            path,
                            interface: false,
                        } => {
                            let c = self.class_value(path)?;
                            format!("$instance({v}, {c}, {what})")
                        }
                        _ => format!("$some({v}, {what})"),
                    }
                };
                vec![b.bind(if *nullable && (m.checked || m.unbox) {
                    format!("({v} == null ? null : {check})")
                } else {
                    check
                })]
            }
            Kind::Handle {
                class, nullable, ..
            } => {
                let reg = self.class_handles()?;
                let c = self.class_var(class)?;
                let e = format!("{reg}.unwrap({v}, {c}, {what})");
                vec![b.bind(self.nullable_unwrap(*nullable, v, e))]
            }
            Kind::Fn {
                provided: Some(_),
                nullable,
                ..
            } => {
                let reg = self.fn_handles(&Self::fn_name(r));
                let e = format!("{reg}.unwrap({v}, Function, {what})");
                vec![b.bind(self.nullable_unwrap(*nullable, v, e))]
            }
            Kind::Fn { nullable, .. } => {
                let check = if m.checked {
                    format!("$fn({v}, {what})")
                } else {
                    v.to_string()
                };
                vec![b.bind(if *nullable && (m.checked || m.unbox) {
                    format!("({v} == null ? null : {check})")
                } else {
                    check
                })]
            }
            Kind::Erased { nullable } => {
                vec![b.bind(if *nullable && m.unbox || self.null_none() {
                    format!("({v} ?? null)")
                } else {
                    v.to_string()
                })]
            }
            _ => return unsupported(format!("`{}`", r.ty)),
        })
    }

    fn null_none(&self) -> bool {
        self.l.profile.box_none == BoxNone::Null
    }

    /// A box's none, as JS passes it to Wasm.
    fn none(&self) -> &'static str {
        if self.null_none() {
            "null"
        } else {
            "undefined"
        }
    }

    fn nullable_unwrap(&self, nullable: bool, v: &str, e: String) -> String {
        if !nullable {
            return e;
        }
        let null = match self.l.profile.handles {
            Carrier::Linear => self.wire_ptr_lit(Space::Lang, "0".into()),
            _ => "null".into(),
        };
        format!("({v} == null ? {null} : {e})")
    }

    /// Validates a future's resolution against its `take<T>` type when it
    /// settles, so a mismatch rejects it (§16); `null` when unchecked.
    fn resolution_check(&mut self, value: &Repr, m: Mode) -> Result<String> {
        if value.kind == Kind::Never {
            return Ok("$neverResolves".into());
        }
        if !m.checked || value.kind == Kind::Unit {
            return Ok("null".into());
        }
        let key = format!("resolves {:?}", value);
        let value = value.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            let t = g.test(&value, "v")?;
            if t != "true" {
                b.line(format!("if (!({t})) $fail({}, v);", js_str(&value.ty)));
            }
            Ok(b.finish(&format!("function {name}(v)")))
        })
    }

    /// A JS array as a host `T[]`: a fresh array of its elements, each
    /// tested against `T` when checked.
    fn host_array(&mut self, elem: &Repr, m: Mode) -> Result<String> {
        let key = format!("ahost {} {} {:?}", m.checked, m.unbox, elem);
        let elem = elem.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            if !m.checked {
                b.line("return Array.isArray(v) ? v.slice() : [];");
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            let what = js_str(&format!("{}[]", elem.ty));
            b.line(format!("$array(v, {what});"));
            b.line("const out = new Array(v.length);");
            b.open("for (let i = 0; i < v.length; i++) {");
            b.line("const e = v[i];");
            // Scalars and strings normalize as they validate.
            let norm = |s: Scalar| lift_scalar(s, &lower_scalar(s, "e", m));
            match &elem.kind {
                Kind::Scalar(s) => b.line(format!("out[i] = {};", norm(*s))),
                Kind::NullScalar(s, _) => b.line(format!("out[i] = e == null ? e : {};", norm(*s))),
                Kind::Str { nullable, .. } => b.line(if *nullable {
                    "out[i] = e == null ? e : $str(e);"
                } else {
                    "out[i] = $str(e);"
                }),
                _ => {
                    let t = g.test(&elem, "e")?;
                    if t != "true" {
                        b.line(format!("if (!({t})) $fail({}, e);", js_str(&elem.ty)));
                    }
                    b.line("out[i] = e;");
                }
            }
            b.close("}");
            b.line("return out;");
            Ok(b.finish(&format!("function {name}(v)")))
        })
    }

    fn wire_ptr_lit(&self, s: Space, z: String) -> String {
        match self.f.ptr(s) {
            Jv::I64 => format!("{z}n"),
            _ => z,
        }
    }

    fn seq_lower(
        &mut self,
        b: &mut Body,
        nullable: bool,
        s: Space,
        v: &str,
        e: String,
    ) -> Vec<String> {
        let t = if nullable {
            b.bind(format!("({v} == null ? [0, -1] : {e})"))
        } else {
            b.bind(e)
        };
        vec![
            b.bind(self.wire_ptr(s, &format!("{t}[0]"))),
            b.bind(self.wire_ptr(s, &format!("{t}[1]"))),
        ]
    }

    fn tuple_lower(
        &mut self,
        b: &mut Body,
        elems: &[Repr],
        v: &str,
        n: usize,
        what: &str,
        m: Mode,
    ) -> Result<Vec<String>> {
        if m.checked {
            b.line(format!("$tuple({v}, {n}, {what});"));
        }
        let mut out = Vec::new();
        for (i, e) in elems.iter().enumerate() {
            let x = b.bind(format!("{v}[{i}]"));
            out.extend(self.lower(b, e, &x, m)?);
        }
        Ok(out)
    }

    fn array_lower(&mut self, elem: &Repr, s: Space, m: Mode) -> Result<String> {
        let key = format!("alower {} {} {s:?} {:?}", m.checked, m.unbox, elem);
        let e = self.f.elem(elem, s)?;
        let elem = elem.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            if m.checked {
                b.line(format!("$array(v, {});", js_str(&format!("{}[]", elem.ty))));
            } else {
                b.line("if (!Array.isArray(v)) v = [];");
            }
            b.line("const n = v.length;");
            if s == Space::Lang
                && !g.f.wide
                && matches!(
                    elem.kind,
                    Kind::Str {
                        carrier: StrCarrier::Linear(Encoding::Utf8),
                        nullable: false
                    }
                )
            {
                g.allocn = true;
                let check = if m.checked { "$str" } else { "$ustr" };
                b.line("const p = n ? $L.alloc(n * 8, 4, cx) : 4;");
                b.line(format!("if (n) $lowerStrs(v, n, p, cx, {check});"));
                b.line("return [p, n];");
                return Ok(b.finish(&format!("function {name}(v, cx)")));
            }
            let mut out = Vec::new();
            if e.size > 0 {
                b.line(format!(
                    "const p = n ? {}.alloc(n * {}, {}, cx) : {};",
                    s.js(),
                    e.size,
                    e.align,
                    e.align
                ));
                out.push(g.wire_ptr(s, "p"));
            }
            if e.refs > 0 {
                b.line(format!(
                    "const x0 = n ? {}.talloc(n * {}, cx) : 0;",
                    s.js(),
                    e.refs
                ));
                out.push("x0".into());
            }
            out.push(g.wire_ptr(s, "n"));
            b.open("for (let i = 0; i < n; i++) {");
            b.line("const e = v[i];");
            let conv = !e.convs.is_empty();
            if conv {
                b.line("const m = $S.mark();");
                b.open("try {");
            }
            let w = g.lower(&mut b, &elem, "e", m)?;
            if e.size > 0 {
                b.line(format!("const dv = {}.dv();", s.js()));
                b.line(format!("const o = p + i * {};", e.size));
            }
            if e.refs > 0 {
                b.line(format!("const x = x0 + i * {};", e.refs));
            }
            g.store_elem(&mut b, s, &e, &w);
            if conv {
                b.close("} finally {");
                b.depth += 1;
                b.line("$S.reset(m);");
                b.close("}");
            }
            b.close("}");
            b.line(format!("return [{}];", out.join(", ")));
            Ok(b.finish(&format!("function {name}(v, cx)")))
        })
    }

    /// The discrimination order of `e`'s arms (§8).
    fn order(&self, e: &EnumLayout) -> Result<Vec<usize>> {
        Ok(match self.flat_of(e)? {
            Some(f) => f.discrimination,
            None => (0..e.arms.len()).collect(),
        })
    }

    /// The flattened enum behind a layout; `None` for `T??`.
    fn flat_of(&self, e: &EnumLayout) -> Result<Option<enums::Flat<'b>>> {
        let r = match &e.origin {
            Origin::Nested => return Ok(None),
            Origin::Err => enums::EnumRef {
                loc: None,
                decl: enums::builtin_err(),
            },
            Origin::Decl(loc) => {
                let mut segs: Vec<String> = loc.module.iter().cloned().collect();
                segs.extend(loc.path.iter().cloned());
                enums::resolve(&self.l.keys.symbols, &Path::new(segs), &Context::default())
                    .ok_or_else(|| Error::Import(format!("can't resolve the enum `{}`", e.path)))?
            }
        };
        let flat = enums::flatten(&self.l.keys.symbols, r).map_err(|es| {
            Error::Import(
                es.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        let names: Vec<&str> = flat.arms.iter().map(|a| a.arm.name.as_str()).collect();
        let mine: Vec<&str> = e.arms.iter().map(|a| a.name.as_str()).collect();
        if names != mine {
            return Err(Error::Import(format!(
                "the arm order of `{}` differs between the layout and the enum helpers",
                e.path
            )));
        }
        Ok(Some(flat))
    }

    /// The nested enum an arm holds, when its leaves join the containing
    /// enum's discrimination (§8).
    fn nested(a: &ArmLayout) -> Option<&Rc<EnumLayout>> {
        if !a.js.is_empty() || a.fallback {
            return None;
        }
        match a.payload.as_slice() {
            [Repr {
                kind:
                    Kind::Enum {
                        layout,
                        nullable: false,
                    },
                ..
            }] if layout.origin != Origin::Nested && !matches!(layout.shape, Shape::Carrier(_)) => {
                Some(layout)
            }
            _ => None,
        }
    }

    /// The arms `e` tests, in order, with each nested enum's arms in place
    /// of the arm holding it: within each tier (own arms, then base arms),
    /// by kind and then in order, as if flattened; the `@fallback` last
    /// (§8). An enum already being expanded stays one arm.
    fn leaves(&self, e: &EnumLayout, stack: &mut Vec<String>) -> Result<Vec<Leaf>> {
        let Some(flat) = self.flat_of(e)? else {
            return Ok((0..e.arms.len())
                .map(|i| Leaf {
                    route: vec![i],
                    rank: 0,
                })
                .collect());
        };
        let d = &flat.discrimination;
        let fallback = d.last().copied().filter(|&i| e.arms[i].fallback);
        let end = d.len() - usize::from(fallback.is_some());
        stack.push(e.path.clone());
        let mut out = Vec::new();
        for tier in [&d[..flat.own_tier], &d[flat.own_tier..end]] {
            let mut ls = Vec::new();
            for &i in tier {
                match Self::nested(&e.arms[i]) {
                    Some(n) if !stack.contains(&n.path) => {
                        for mut l in self.leaves(n, stack)? {
                            l.route.insert(0, i);
                            ls.push(l);
                        }
                    }
                    _ => ls.push(Leaf {
                        route: vec![i],
                        rank: flat.ranks[i],
                    }),
                }
            }
            ls.sort_by_key(|l| l.rank);
            out.extend(ls);
        }
        stack.pop();
        out.extend(fallback.map(|i| Leaf {
            route: vec![i],
            rank: u8::MAX,
        }));
        Ok(out)
    }

    /// An enum's zero wire values, and each arm's slots, in its helpers'
    /// `[arm, slots...]` form.
    fn enum_parts(&self, e: &EnumLayout) -> Result<(Vec<&'static str>, Vec<Vec<usize>>)> {
        let gc = self.f.enum_has_gc(e);
        let zeros: Vec<&'static str> = match &e.shape {
            Shape::Payload { slots } if !gc => slots
                .iter()
                .map(|s| crate::form::Jv::of(s).zero())
                .collect(),
            Shape::Payload { .. } => {
                let mut z = Vec::new();
                for a in &e.arms {
                    for p in &a.payload {
                        z.extend(self.f.jsw(p)?.iter().map(|j| j.zero()));
                    }
                }
                z
            }
            _ => Vec::new(),
        };
        let mut regions = Vec::new();
        let mut at = 0;
        for a in &e.arms {
            regions.push(self.arm_wire(e, a, gc, &mut at)?);
        }
        Ok((zeros, regions))
    }

    /// `e`'s wire for its arm `i`, from the arm's payload values.
    fn arm_wire_values(&self, e: &EnumLayout, i: usize, vals: &[String]) -> Result<Vec<String>> {
        let a = &e.arms[i];
        if !matches!(e.shape, Shape::Payload { .. }) {
            return Ok(vec![a.index.to_string()]);
        }
        let (zeros, regions) = self.enum_parts(e)?;
        let mut out: Vec<String> = zeros.iter().map(|z| z.to_string()).collect();
        for (s, v) in regions[i].iter().zip(vals) {
            out[*s] = v.clone();
        }
        out.insert(0, a.index.to_string());
        Ok(out)
    }

    /// The payload values of the arm `route[0]` of `e`, lowering `v` into
    /// the nested arms the rest of the route names.
    fn route_lower(
        &mut self,
        b: &mut Body,
        e: &EnumLayout,
        route: &[usize],
        v: &str,
        m: Mode,
    ) -> Result<Vec<String>> {
        let a = &e.arms[route[0]];
        if route.len() == 1 {
            return self.arm_lower(b, a, v, m);
        }
        let n = Self::nested(a).expect("a nested arm").clone();
        let inner = self.route_lower(b, &n, &route[1..], v, m)?;
        self.arm_wire_values(&n, route[1], &inner)
    }

    /// A dispatch test for an arm payload (§8, "Arm tests").
    pub(crate) fn test(&mut self, r: &Repr, v: &str) -> Result<String> {
        let base = match &r.kind {
            Kind::Scalar(Scalar::Bool) | Kind::NullScalar(Scalar::Bool, _) => {
                format!("typeof {v} === \"boolean\"")
            }
            Kind::Scalar(s) | Kind::NullScalar(s, _) if matches!(s, Scalar::I64 | Scalar::U64) => {
                format!("(typeof {v} === \"bigint\" || typeof {v} === \"number\")")
            }
            Kind::Scalar(_) | Kind::NullScalar(..) => format!("typeof {v} === \"number\""),
            Kind::Str { .. } => format!("typeof {v} === \"string\""),
            // A `unit` value is ignored, not checked (§3).
            Kind::Unit => "true".into(),
            Kind::Vec { bigint: true, .. } => format!("typeof {v} === \"bigint\""),
            Kind::Vec { elem, .. } => format!("{v} instanceof {}", typed_array(*elem)),
            Kind::Array { .. } => format!("Array.isArray({v})"),
            Kind::Tuple { elems, .. } => {
                format!("(Array.isArray({v}) && {v}.length === {})", elems.len())
            }
            Kind::Host { of, .. } => match of {
                of if of.symbol() => format!("typeof {v} === \"symbol\""),
                Host::Class {
                    path,
                    interface: false,
                } => format!("$isInstance({v}, {})", self.class_value(path)?),
                Host::Class { .. } => format!("$isObject({v})"),
                Host::Any => format!("{v} != null"),
                Host::Box(inner) => return self.test(inner, v),
                Host::Future { .. } => format!("$isThenable({v})"),
            },
            Kind::Enum { layout, .. } => {
                let t = self.enum_test(layout)?;
                format!("{t}({v})")
            }
            Kind::Handle { class, .. } => {
                let reg = self.class_handles()?;
                let c = self.class_var(class)?;
                format!("{reg}.owns({v}, {c})")
            }
            Kind::Fn {
                provided: Some(_), ..
            } => {
                let reg = self.fn_handles(&Self::fn_name(r));
                format!("{reg}.owns({v}, Function)")
            }
            Kind::Fn { .. } => format!("typeof {v} === \"function\""),
            Kind::Erased { .. } => format!("{v} != null"),
            _ => return unsupported(format!("an arm test for `{}`", r.ty)),
        };
        let nullable = match &r.kind {
            Kind::NullScalar(..) => true,
            Kind::Str { nullable, .. }
            | Kind::Vec { nullable, .. }
            | Kind::Array { nullable, .. }
            | Kind::Tuple { nullable, .. }
            | Kind::Enum { nullable, .. }
            | Kind::Host { nullable, .. }
            | Kind::Handle { nullable, .. }
            | Kind::Fn { nullable, .. }
            | Kind::Erased { nullable } => *nullable,
            _ => false,
        };
        Ok(if !nullable {
            base
        } else if base == format!("{v} != null") {
            "true".into()
        } else {
            format!("({v} == null || {base})")
        })
    }

    /// Whether `v` is some arm of the enum, for a nested enum's arm test.
    fn enum_test(&mut self, e: &Rc<EnumLayout>) -> Result<String> {
        let key = format!("etest {}", e.path);
        let e = e.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            for i in g.order(&e)? {
                let a = &e.arms[i];
                if a.fallback {
                    b.line("return true;");
                    return Ok(b.finish(&format!("function {name}(v)")));
                }
                let t = g.arm_test(&e, a, "v")?;
                b.line(format!("if ({t}) return true;"));
            }
            b.line("return false;");
            Ok(b.finish(&format!("function {name}(v)")))
        })
    }

    fn arm_test(&mut self, e: &EnumLayout, a: &ArmLayout, v: &str) -> Result<String> {
        if let Some(l) = a.js.literal() {
            return Ok(format!("{v} === {}", js_str(l)));
        }
        if let Some(preds) = a.js.classes() {
            return self.class_test(preds, v);
        }
        if e.origin == Origin::Nested {
            return Ok(if a.payload.is_empty() {
                format!("{v} === undefined")
            } else {
                "true".into()
            });
        }
        match a.payload.len() {
            0 => Ok("false".into()),
            1 => self.test(&a.payload[0], v),
            n => Ok(format!("(Array.isArray({v}) && {v}.length === {n})")),
        }
    }

    /// `@js(C[p = lit, ...] | ...)`: an `instanceof` one of the classes,
    /// then that class's property predicates.
    fn class_test(&mut self, preds: &[ClassPred], v: &str) -> Result<String> {
        let mut alts = Vec::new();
        for p in preds {
            let c = self.error_class(&p.class)?;
            let mut t = format!("$isInstance({v}, {c})");
            for (k, l) in &p.props {
                let _ = write!(t, " && {} === {}", prop(v, k), lit(l));
            }
            alts.push(format!("({t})"));
        }
        Ok(format!("({})", alts.join(" || ")))
    }

    /// Discriminates `v` into its arm and fills that arm's slots (§8): the
    /// helper returns the index, or `[arm, slots...]` for a payload enum.
    /// A nested enum's arms are tested among the containing enum's.
    fn enum_lower(&mut self, e: &Rc<EnumLayout>, m: Mode) -> Result<String> {
        let key = format!("elower {} {} {}", m.checked, m.unbox, e.path);
        let e = e.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            let leaves = g.leaves(&e, &mut Vec::new())?;
            let leaf = |l: &Leaf| {
                let mut x: &EnumLayout = &e;
                for &i in &l.route[..l.route.len() - 1] {
                    x = Self::nested(&x.arms[i]).expect("a nested arm");
                }
                (x, &x.arms[l.route[l.route.len() - 1]])
            };
            // Literals are tested before any other arm.
            let (lits, rest): (Vec<&Leaf>, Vec<&Leaf>) = leaves.iter().partition(|l| {
                leaf(l)
                    .1
                    .js
                    .iter()
                    .any(|j| matches!(j, JsEntry::Literal(_)))
            });
            let payload = matches!(e.shape, Shape::Payload { .. });
            for l in lits.into_iter().chain(rest) {
                let (x, a) = leaf(l);
                let test = if a.fallback {
                    None
                } else {
                    match g.arm_test(x, a, "v")? {
                        t if t == "false" => continue,
                        t => Some(t),
                    }
                };
                b.open(match &test {
                    Some(t) => format!("if ({t}) {{"),
                    None => "{".into(),
                });
                let vals = g.route_lower(&mut b, &e, &l.route, "v", m)?;
                let w = g.arm_wire_values(&e, l.route[0], &vals)?;
                if payload {
                    b.line(format!("return [{}];", w.join(", ")));
                } else {
                    b.line(format!("return {};", w[0]));
                }
                b.close("}");
                if test.is_none() {
                    return Ok(b.finish(&format!("function {name}(v, cx)")));
                }
            }
            b.line(format!("$fail({}, v);", js_str(&e.path)));
            Ok(b.finish(&format!("function {name}(v, cx)")))
        })
    }

    fn arm_lower(&mut self, b: &mut Body, a: &ArmLayout, v: &str, m: Mode) -> Result<Vec<String>> {
        match a.payload.len() {
            0 => Ok(Vec::new()),
            1 => self.lower(b, &a.payload[0], v, m),
            n => {
                if m.checked {
                    b.line(format!("$tuple({v}, {n}, {});", js_str(&a.name)));
                }
                let mut out = Vec::new();
                for (i, p) in a.payload.iter().enumerate() {
                    let x = b.bind(format!("{v}[{i}]"));
                    out.extend(self.lower(b, p, &x, m)?);
                }
                Ok(out)
            }
        }
    }

    // Errors (§15).

    pub fn tag_var(&mut self, e: &Rc<EnumLayout>) -> String {
        if let Some((_, v, _)) = self.tags.iter().find(|(p, _, _)| *p == e.path) {
            return v.clone();
        }
        let v = format!("$tag{}", self.tags.len());
        self.tags.push((e.path.clone(), v.clone(), e.clone()));
        v
    }

    /// Maps a caught JS value to `[arm, message]` of a `throws` enum: arms in
    /// discrimination order by class match, the `@fallback` last.
    fn error_map(&mut self, e: &Rc<EnumLayout>) -> Result<String> {
        let key = format!("emap {}", e.path);
        let e = e.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            for i in g.order(&e)? {
                let a = &e.arms[i];
                if a.fallback {
                    b.line(format!("return [{}, $message(e)];", a.index));
                    return Ok(b.finish(&format!("function {name}(e)")));
                }
                if let Some(preds) = a.js.classes() {
                    let t = g.class_test(preds, "e")?;
                    b.line(format!("if ({t}) return [{}, $message(e)];", a.index));
                }
            }
            Err(Error::Import(format!("`{}` has no `@fallback`", e.path)))
        })
    }

    // Imports.

    /// The glue function for an import, over its glue wire.
    pub fn func(&mut self, name: &str, func: &Func<'_>) -> Result<String> {
        let mut b = Body::default();
        let mut params = Vec::new();
        let mut wires: Vec<Vec<String>> = Vec::new();
        for (i, p) in func.sig.params.iter().enumerate() {
            let n = self.f.jsw(&p.repr)?.len();
            let ws: Vec<String> = (0..n).map(|j| format!("w{i}_{j}")).collect();
            params.extend(ws.iter().cloned());
            wires.push(ws);
        }
        match &func.op {
            Op::InstanceOf(path) => {
                let c = self.class_check(path)?;
                b.line(format!("return $isInstance(v, {c}) ? 1 : 0;"));
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            Op::ArrLen => {
                b.line("return $array(v, \"Arr\").length;");
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            Op::IsNullish => {
                b.line("return v == null ? 1 : 0;");
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            Op::IsUndefined => {
                b.line("return v === undefined ? 1 : 0;");
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            Op::Constant(_) => unreachable!("served by the bridge module"),
            Op::ArrGet(r) => {
                b.line("$array(v, \"Arr\");");
                b.line("const i = w1_0 >>> 0;");
                b.line("if (i >= v.length) throw new TypeError(\"an index out of range\");");
                b.line("const e = v[i];");
                let body = |g: &mut Self, b: &mut Body| {
                    g.lower(
                        b,
                        r,
                        "e",
                        Mode {
                            checked: true,
                            unbox: true,
                        },
                    )
                };
                self.returning(&mut b, body)?;
                return Ok(b.finish(&format!("function {name}(v, w1_0)")));
            }
            // `tw:box T?` writes none as the box's none, whatever the
            // subject's `@js`.
            Op::Box(r) => {
                let r = &Repr {
                    js_null: self.null_none(),
                    ..r.clone()
                };
                let v = if self.null_none() && r.kind == Kind::Unit {
                    "null".into()
                } else {
                    self.lift(r, &wires[0])?
                };
                b.line(format!("return {v};"));
            }
            Op::Unbox { repr, checked } => {
                let m = Mode {
                    checked: *checked && self.validate,
                    unbox: true,
                };
                let v = match &repr.kind {
                    Kind::Vec {
                        elem: Scalar::U8, ..
                    } if m.checked => {
                        b.line("const e = v == null ? v : $bytes(v);");
                        "e"
                    }
                    _ => "v",
                };
                let body = |g: &mut Self, b: &mut Body| g.lower(b, repr, v, m);
                self.returning(&mut b, body)?;
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            Op::TryUnbox(r) => {
                let (reg, c) = match &r.kind {
                    Kind::Handle { class, .. } => (self.class_handles()?, self.class_var(class)?),
                    _ => (self.fn_handles(&Self::fn_name(r)), "Function".to_string()),
                };
                let null = self.nullable_unwrap(true, "h", "h".into());
                b.line(format!("const h = {reg}.tryUnwrap(v, {c});"));
                b.line(format!("return {null};"));
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            Op::Member(k) => self.member(&mut b, k, func, &wires)?,
            Op::New(k) => self.construct(&mut b, k, func, &wires)?,
            Op::Destroy(r) => {
                let reg = match &r.kind {
                    Kind::Handle { .. } => self.class_handles()?,
                    _ => self.fn_handles(&Self::fn_name(r)),
                };
                b.line(format!("{reg}.destroy({});", wires[0][0]));
            }
            Op::Poll => {
                b.line("return $poll(f, w);");
                return Ok(b.finish(&format!("function {name}(f, w)")));
            }
            Op::FutureDrop => {
                b.line("$dropFuture(f);");
                return Ok(b.finish(&format!("function {name}(f)")));
            }
            Op::FutureNew => {
                b.line("return $newFuture();");
                return Ok(b.finish(&format!("function {name}()")));
            }
            Op::RetPop(_) | Op::RetPush(_) => {
                unreachable!("the bridge module serves the spill")
            }
            Op::ErrRead(e) => {
                let map = self.error_map(e)?;
                b.line(format!("return {map}(v);"));
                return Ok(b.finish(&format!("function {name}(v)")));
            }
            Op::ErrSet(e) => {
                let out = self.error_out(e)?;
                b.line(format!("$pending = {out}(arm, m);"));
                b.line("$B.pending.value = 1;");
                return Ok(b.finish(&format!("function {name}(arm, m)")));
            }
            Op::WorkAvailable => {
                self.loops = true;
                b.line("$loops.available(l);");
                return Ok(b.finish(&format!("function {name}(l)")));
            }
            Op::LoopDrop => {
                self.loops = true;
                b.line("$loops.drop(l);");
                return Ok(b.finish(&format!("function {name}(l)")));
            }
            Op::Take(r) => {
                b.line(format!("const v = $settled({}, 1);", wires[0][0]));
                if let Some(r) = r {
                    let m = self.mode();
                    let body = |g: &mut Self, b: &mut Body| g.lower(b, r, "v", m);
                    self.returning(&mut b, body)?;
                }
            }
            Op::FutureError(e) => {
                let map = self.error_map(e)?;
                b.line(format!("return {map}($settled({}, 2));", wires[0][0]));
            }
            Op::Resolve(r) => {
                let v = match r {
                    Some(r) => self.lift(r, &wires[1])?,
                    None => "undefined".into(),
                };
                b.line(format!("$resolver({}).resolve({v});", wires[0][0]));
            }
            Op::Reject(e) => {
                let out = self.error_out(e)?;
                b.line(format!(
                    "$resolver({}).reject({out}({}));",
                    wires[0][0],
                    wires[1].join(", ")
                ));
            }
            Op::CallFn => {
                let mut callee = String::new();
                let mut rest = false;
                let mut argv = Vec::new();
                for (p, w) in func.sig.params.iter().zip(&wires) {
                    match (&p.role, &p.repr.kind) {
                        (Pos::Callee, Kind::Fn { sig, .. }) => {
                            callee = w[0].clone();
                            rest = sig.rest;
                        }
                        (Pos::Callee, _) => callee = w[0].clone(),
                        _ => {
                            let v = self.lift(&p.repr, w)?;
                            argv.push(b.bind(v));
                        }
                    }
                }
                if let (true, Some(last)) = (rest, argv.last_mut()) {
                    last.insert_str(0, "...");
                }
                let call = format!("{callee}({})", argv.join(", "));
                let throws = func.sig.throws.clone();
                self.complete(&mut b, &call, func.sig.results.first(), throws, false, &[])?;
            }
        }
        Ok(b.finish(&format!("function {name}({})", params.join(", "))))
    }

    /// Lowers a result on a cleanup list released if any part fails.
    fn returning(
        &mut self,
        b: &mut Body,
        body: impl FnOnce(&mut Self, &mut Body) -> Result<Vec<String>>,
    ) -> Result<()> {
        b.line("const cx = $cx;");
        b.line("const cxm = cx.n;");
        b.open("try {");
        let w = body(self, b)?;
        self.ret_line(b, &w);
        b.close("} catch (e) {");
        b.depth += 1;
        b.line("$release(cx, cxm);");
        b.line("throw e;");
        b.close("} finally {");
        b.depth += 1;
        b.line("cx.n = cxm;");
        b.close("}");
        Ok(())
    }

    fn member(
        &mut self,
        b: &mut Body,
        k: &Keyed<'_>,
        func: &Func<'_>,
        wires: &[Vec<String>],
    ) -> Result<()> {
        let m = k.member;
        if !matches!(k.role, Role::Import(_)) {
            return Err(Error::Import(format!("{} is an export", k.key)));
        }
        let js = &m.meta.attrs.js;
        // Lift the receiver and parameters.
        let mut recv = None;
        let mut args: HashMap<&str, String> = HashMap::new();
        let mut writeback = Vec::new();
        for (p, w) in func.sig.params.iter().zip(wires) {
            match p.role {
                Pos::This => recv = Some(w[0].clone()),
                Pos::Param(i) => {
                    let param = &m.params[i];
                    let v = match &p.repr.kind {
                        Kind::View { elem, .. } => self.view(*elem, w, param.unshared),
                        _ => self.lift(&p.repr, w)?,
                    };
                    let v = b.bind(v);
                    if param.once {
                        let reg = self.fn_handles(&Self::fn_name(&p.repr));
                        b.line(format!("{reg}.once({v});"));
                    }
                    // A host `mut T[]` is the language's own array already.
                    if let Kind::Vec {
                        writeback: true,
                        carrier: carrier @ (Carrier::Linear | Carrier::Gc),
                        nullable,
                        ..
                    } = &p.repr.kind
                    {
                        let cond = nullable.then(|| format!("{v} !== undefined && {v} !== null"));
                        writeback.push((Space::of(*carrier), w.clone(), v.clone(), cond));
                    }
                    // An unshared copy of a shared memory writes back.
                    if let (Kind::View { mutable: true, .. }, true) = (&p.repr.kind, param.unshared)
                    {
                        writeback.push((
                            Space::Lang,
                            w.clone(),
                            v.clone(),
                            Some("$L.shared".into()),
                        ));
                    }
                    args.insert(&param.name, v);
                }
                _ => return unsupported(format!("{}: a {:?} position", k.key, p.role)),
            }
        }
        let cx = k.cx.clone();
        let result = func.sig.results.first();
        let future = result.and_then(|r| match &r.repr.kind {
            Kind::Host {
                of: Host::Future { value, .. },
                ..
            } => Some(value.clone()),
            _ => None,
        });
        // Without `throws`, a synchronous throw rejects the future (§16).
        let fold = future.is_some() && func.sig.throws.is_none();
        if fold {
            b.open("try {");
        }
        for j in js {
            if let JsEntry::Let(x, e) = j {
                let v = self.expr(e, &args, &cx)?;
                let t = b.fresh();
                b.line(format!("const {t} = {v};"));
                args.insert(x, t);
            }
        }
        let template = js.args();
        let argv = match template {
            Some(t) => self.template(b, t, &args, &cx)?,
            None => m
                .params
                .iter()
                .map(|p| args[p.name.as_str()].clone())
                .collect(),
        };
        let mut call = self.call(b, k, recv.as_deref(), &argv)?;
        let cancel = js.cancel();
        if let Some(e) = cancel {
            let Some(value) = &future else {
                return Err(Error::Import(format!(
                    "{}: `cancel` needs a `future` result",
                    k.key
                )));
            };
            // `let` values and `$result` live in the cancel closure, with
            // the future.
            let t = b.fresh();
            b.line(format!("const {t} = {call};"));
            args.insert("$result", t.clone());
            let c = self.expr(e, &args, &cx)?;
            let check = self.resolution_check(value, self.mode())?;
            let f = b.fresh();
            b.line(format!(
                "const {f} = $future({t}, {check}, () => {{ {c}(); }});"
            ));
            call = f;
        }
        let throws = func.sig.throws.clone();
        let ctor = matches!(m.kind, MemberKind::New(_));
        let mut wb = Vec::new();
        for (s, w, v, cond) in &writeback {
            let line = format!(
                "$writeback({}, {}, {}, {v});",
                s.js(),
                self.num(*s, &w[0]),
                self.num(*s, &w[1])
            );
            wb.push(match cond {
                Some(c) => format!("if ({c}) {line}"),
                None => line,
            });
        }
        self.complete(b, &call, result, throws, ctor, &wb)?;
        if fold {
            b.close("} catch (e) {");
            b.depth += 1;
            b.line("return $rejected(e);");
            b.close("}");
        }
        Ok(())
    }

    /// `("tw:new", ctor)`: `new C(...)` with the constructor's arguments
    /// as JS passes them, naming the arm so dispatch takes that one (§17).
    fn construct(
        &mut self,
        b: &mut Body,
        k: &Keyed<'_>,
        func: &Func<'_>,
        wires: &[Vec<String>],
    ) -> Result<()> {
        let m = k.member;
        let mut args: HashMap<&str, String> = HashMap::new();
        for (p, w) in func.sig.params.iter().zip(wires) {
            let Pos::Param(i) = p.role else {
                return unsupported(format!("{}: a {:?} position", k.key, p.role));
            };
            let v = self.lift(&p.repr, w)?;
            args.insert(&m.params[i].name, b.bind(v));
        }
        let argv = match m.meta.attrs.js.args() {
            Some(t) => self.template(b, t, &args, &k.cx)?,
            None => m
                .params
                .iter()
                .map(|p| args[p.name.as_str()].clone())
                .collect(),
        };
        let Some(Target::Decl(loc, _)) = self
            .l
            .keys
            .symbols
            .resolve(&Path::new([k.owner.unwrap().name.as_str()]), &k.cx)
        else {
            return Err(Error::Import(format!(
                "can't resolve the owner of {}",
                k.key
            )));
        };
        let var = self.class_var(&location(loc))?;
        let call = format!(
            "($newArm = {}, new {var}({}))",
            js_str(&k.key),
            argv.join(", ")
        );
        let throws = func.sig.throws.clone();
        self.complete(b, &call, func.sig.results.first(), throws, true, &[])
    }

    /// Makes an outgoing call, lowering its result and mapping what it
    /// throws to the `throws` enum's tag.
    fn complete(
        &mut self,
        b: &mut Body,
        call: &str,
        result: Option<&Position>,
        throws: Option<Rc<EnumLayout>>,
        ctor: bool,
        wb: &[String],
    ) -> Result<()> {
        let lowering = result.is_some_and(|r| !ctor && lowers_alloc(&r.repr));
        if lowering {
            b.line("const cx = $cx;");
            b.line("const cxm = cx.n;");
        }
        if throws.is_some() || lowering {
            b.open("try {");
        }
        match result {
            None => {
                b.line(format!("{call};"));
                for l in wb {
                    b.line(l);
                }
            }
            // A `never` result that returns breaks its contract (§3).
            Some(r) if r.repr.kind == Kind::Never => {
                b.line(format!("{call};"));
                for l in wb {
                    b.line(l);
                }
                b.line("throw new TypeError(\"a `never` call returned\");");
            }
            Some(_) if ctor => {
                b.line(format!("const r = {call};"));
                for l in wb {
                    b.line(l);
                }
                b.line("return r;");
            }
            Some(r) => {
                b.line(format!("const r = {call};"));
                for l in wb {
                    b.line(l);
                }
                let w = self.lower(b, &r.repr, "r", self.mode())?;
                self.ret_line(b, &w);
            }
        }
        if throws.is_some() || lowering {
            b.close("} catch (e) {");
            b.depth += 1;
            if lowering {
                b.line("$release(cx, cxm);");
            }
            // Under `throws = js` the language catches the JS exception
            // itself and reads it with `tw:err` (§15).
            match &throws {
                Some(e) if self.l.profile.throws == Throws::Tag => {
                    let map = self.error_map(e)?;
                    let tag = self.tag_var(e);
                    b.line("if (e instanceof WebAssembly.Exception) throw e;");
                    b.line(format!("throw new WebAssembly.Exception({tag}, {map}(e));"));
                }
                _ => b.line("throw e;"),
            }
            if lowering {
                b.close("} finally {");
                b.depth += 1;
                b.line("cx.n = cxm;");
            }
            b.close("}");
        }
        Ok(())
    }

    /// The JS expression of the member call itself.
    fn call(
        &mut self,
        b: &mut Body,
        k: &Keyed<'_>,
        recv: Option<&str>,
        argv: &[String],
    ) -> Result<String> {
        let m = k.member;
        let js = &m.meta.attrs.js;
        let jsname = js
            .rename()
            .map(String::from)
            .or_else(|| m.kind.name().map(String::from));
        let symbol = js.symbol().map(String::from);
        let index = js.iter().any(|j| matches!(j, JsEntry::Index));
        // The constructor of a type a separate value constructs, which may
        // have no JS value of its own (§6).
        if let (MemberKind::New(_), Some(value)) = (&m.kind, js.new_value()) {
            let module = match &value.base {
                Base::Lexical => k.cx.module,
                Base::Root => None,
                Base::Module(spec) => Some(spec.as_str()),
            };
            let value = self.path_value(module, &value.segments)?;
            return Ok(format!("new {value}({})", argv.join(", ")));
        }
        let owner_loc = |g: &Self| -> Result<Option<Location>> {
            match k.owner {
                Some(c) => {
                    let p = Path::new([c.name.as_str()]);
                    match g.l.keys.symbols.resolve(&p, &k.cx) {
                        Some(Target::Decl(loc, _)) => Ok(Some(loc.clone())),
                        _ => Err(Error::Import(format!(
                            "can't resolve the owner of {}",
                            k.key
                        ))),
                    }
                }
                None => Ok(None),
            }
        };
        // The receiver: the instance, the class, or the enclosing scope.
        let base = match (recv, owner_loc(self)?) {
            (Some(r), _) => r.to_string(),
            (None, Some(loc)) => self.decl_value(&loc)?,
            (None, None) => {
                let ns: Vec<String> = k.cx.namespace.iter().map(|s| s.to_string()).collect();
                if ns.is_empty() {
                    match k.cx.module {
                        None => "globalThis".to_string(),
                        Some(_) => String::new(),
                    }
                } else {
                    self.path_value(k.cx.module, &ns)?
                }
            }
        };
        let access = |base: &str, g: &mut Self| -> Result<String> {
            if let Some(s) = &symbol {
                return Ok(format!("{base}[Symbol.{s}]"));
            }
            let n = jsname.clone().unwrap_or_default();
            if base.is_empty() {
                // A top-level member of a module block.
                return g.path_value(k.cx.module, &[n]);
            }
            Ok(prop(base, &n))
        };
        let list = argv.join(", ");
        Ok(match &m.kind {
            MemberKind::New(_) => {
                if js.iter().any(|j| matches!(j, JsEntry::Object)) {
                    "{}".to_string()
                } else {
                    format!("new {base}({list})")
                }
            }
            MemberKind::Method(_) if index => match argv {
                [i] => format!("{base}[{i}]"),
                [i, v] => format!("({base}[{i}] = {v}, undefined)"),
                _ => {
                    return Err(Error::Import(format!(
                        "{}: `@js(index)` takes one or two arguments",
                        k.key
                    )))
                }
            },
            MemberKind::Method(_) => format!("{}({list})", access(&base, self)?),
            MemberKind::Get(_) => access(&base, self)?,
            MemberKind::Set(_) => {
                let v = argv.first().cloned().unwrap_or_else(|| "undefined".into());
                let target = access(&base, self)?;
                b.line(format!("{target} = {v};"));
                "undefined".to_string()
            }
        })
    }

    /// The JS argument list of `@js(args = (...))` (§7), outgoing.
    pub(crate) fn template(
        &mut self,
        b: &mut Body,
        t: &[Telem],
        args: &HashMap<&str, String>,
        cx: &Context<'_>,
    ) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for e in t {
            out.push(match e {
                Telem::Param(p) => args
                    .get(p.as_str())
                    .cloned()
                    .ok_or_else(|| Error::Import(format!("`{p}` isn't a parameter")))?,
                Telem::Lit(l) => lit(l),
                Telem::Undefined => "undefined".into(),
                Telem::Spread(p) => format!(
                    "...{}",
                    args.get(p.as_str())
                        .ok_or_else(|| Error::Import(format!("`{p}` isn't a parameter")))?
                ),
                Telem::Fill { param, fields } => {
                    let v = args
                        .get(param.as_str())
                        .ok_or_else(|| Error::Import(format!("`{param}` isn't a parameter")))?;
                    let o = b.fresh();
                    b.line(format!("const {o} = {v} ?? {{}};"));
                    // An undefined field is left off, as an absent optional
                    // member is: dictionaries enumerate own keys.
                    for (f, x) in fields {
                        let lit = matches!(x, Expr::Lit(_));
                        let x = self.expr(x, args, cx)?;
                        if lit {
                            b.line(format!("{} = {x};", prop(&o, f)));
                        } else {
                            let x = b.bind(x);
                            b.line(format!("if ({x} !== undefined) {} = {x};", prop(&o, f)));
                        }
                    }
                    o
                }
                Telem::Expr(x) => self.expr(x, args, cx)?,
            });
        }
        Ok(out)
    }

    /// A `@js` expression: a path through bindings, parameters and IR
    /// declarations, with `C.new(...)` constructing.
    pub(crate) fn expr(
        &mut self,
        e: &Expr,
        args: &HashMap<&str, String>,
        cx: &Context<'_>,
    ) -> Result<String> {
        let Expr::Ref { path, call } = e else {
            let Expr::Lit(l) = e else { unreachable!() };
            return Ok(lit(l));
        };
        let call_args = match call {
            Some(xs) => Some(
                xs.iter()
                    .map(|x| self.expr(x, args, cx))
                    .collect::<Result<Vec<_>>>()?,
            ),
            None => None,
        };
        if let Some(v) = args.get(path[0].as_str()) {
            let mut s = v.clone();
            for p in &path[1..] {
                s = prop(&s, p);
            }
            if let Some(a) = call_args {
                let _ = write!(s, "({})", a.join(", "));
            }
            return Ok(s);
        }
        // The longest prefix naming a declaration.
        for n in (1..=path.len()).rev() {
            let p = Path::new(path[..n].iter().cloned());
            if let Some((loc, TypeDecl::Class(_))) = self.l.keys.symbols.resolve_decl(&p, cx) {
                let c = self.decl_value(&loc)?;
                let rest = &path[n..];
                return Ok(match (rest, call_args) {
                    ([m], Some(a)) if m == "new" => format!("new {c}({})", a.join(", ")),
                    (rest, a) => {
                        let mut s = c;
                        for r in rest {
                            s = prop(&s, r);
                        }
                        if let Some(a) = a {
                            let _ = write!(s, "({})", a.join(", "));
                        }
                        s
                    }
                });
            }
        }
        Err(Error::Import(format!("`{}` names nothing", path.join("."))))
    }
}

impl Glue<'_, '_> {
    /// Returns the wire values `w`, the ones past the direct result through
    /// the return area, written last so nothing can move it before the
    /// bridge reads it.
    fn ret_line(&mut self, b: &mut Body, w: &[String]) {
        let Some(a) = self.ret.as_ref().filter(|_| w.len() > 1) else {
            b.line(ret(w));
            return;
        };
        self.ret_used = true;
        let w: Vec<String> = w.iter().map(|v| b.bind(v.clone())).collect();
        b.line("const ra = $ret();");
        for (j, vt, off) in &a.slots {
            let at = if vt.ends_with("32") { off / 4 } else { off / 8 };
            b.line(format!("ra.{vt}[{at}] = {};", w[*j]));
        }
        b.line(format!("return {};", w[a.direct]));
    }
}

fn ret(w: &[String]) -> String {
    match w.len() {
        0 => "return;".into(),
        1 => format!("return {};", w[0]),
        _ => format!("return [{}];", w.join(", ")),
    }
}

/// Whether lowering `r` allocates in the language's space.
pub(crate) fn lowers_alloc(r: &Repr) -> bool {
    match &r.kind {
        Kind::Str { carrier, .. } => matches!(carrier, StrCarrier::Linear(_)),
        Kind::Vec { carrier, .. } => *carrier == Carrier::Linear,
        Kind::Array { carrier, elem, .. } => match carrier {
            Carrier::Linear => true,
            Carrier::Gc => lowers_alloc(elem),
            Carrier::Host => false,
        },
        Kind::Tuple { elems, .. } => elems.iter().any(lowers_alloc),
        Kind::Enum { layout, .. } => layout
            .arms
            .iter()
            .any(|a| a.payload.iter().any(lowers_alloc)),
        _ => false,
    }
}
