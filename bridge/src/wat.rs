//! The bridge module (IR.md §11, §15).
//!
//! It provides every import whose wire holds a GC carrier: a forwarder with
//! the language's exact wire type translates each GC value into its linear
//! form in the bridge's scratch memory and table, calls the glue, and
//! translates the results back. Scratch is a stack: each forwarder restores
//! it on the way out, including on unwind, through an exception handler.

use std::collections::HashMap;
use std::fmt::Write;

use ts_gen_abi::layout::spill_name;
use ts_gen_abi::layout::{
    scalar_storage, Def, EnumLayout, FuncType, Heap, Kind, Layouts, Repr, Shape, Signature,
    Storage, ValType,
};
use ts_gen_abi::profile::{Carrier, Eh, Encoding, Results, StrCarrier, Throws};
use ts_gen_abi::protocol;

use crate::form::{Forms, Jv, Num, Store};
use crate::plan::Func;
use crate::{unsupported, Result};

pub struct Wat<'a, 'b> {
    l: &'b Layouts<'a>,
    f: &'b Forms,
    defs: Vec<Def>,
    funcs: Vec<String>,
    imports: Vec<String>,
    memo: HashMap<String, String>,
    n: usize,
    pub js_string: bool,
    /// Whether the glue lowers `arr<str>` through `allocn`.
    pub allocn: bool,
    /// The function types of the language exports reached through the
    /// `entries` table, by slot.
    entries: Vec<String>,
}

/// A function under construction.
struct Fb {
    params: Vec<(String, String)>,
    results: Vec<String>,
    locals: Vec<(String, String)>,
    code: Vec<String>,
    n: usize,
}

impl Fb {
    fn new() -> Fb {
        Fb {
            params: Vec::new(),
            results: Vec::new(),
            locals: Vec::new(),
            code: Vec::new(),
            n: 0,
        }
    }

    fn param(&mut self, ty: String) -> String {
        let n = format!("$p{}", self.params.len());
        self.params.push((n.clone(), ty));
        n
    }

    fn local(&mut self, ty: impl Into<String>) -> String {
        self.n += 1;
        let n = format!("$l{}", self.n);
        self.locals.push((n.clone(), ty.into()));
        n
    }

    fn op(&mut self, s: impl Into<String>) {
        self.code.push(s.into());
    }

    fn finish(self, head: &str) -> String {
        let mut s = format!("  (func {head}");
        for (n, t) in &self.params {
            let _ = write!(s, " (param {n} {t})");
        }
        if !self.results.is_empty() {
            let _ = write!(s, " (result {})", self.results.join(" "));
        }
        s.push('\n');
        for (n, t) in &self.locals {
            let _ = writeln!(s, "    (local {n} {t})");
        }
        for c in &self.code {
            let _ = writeln!(s, "    {c}");
        }
        s.push_str("  )");
        s
    }
}

fn load(num: Num) -> &'static str {
    match num {
        Num::I8 => "i32.load8_s",
        Num::U8 => "i32.load8_u",
        Num::I16 => "i32.load16_s",
        Num::U16 => "i32.load16_u",
        Num::I32 => "i32.load",
        Num::I64 => "i64.load",
        Num::F32 => "f32.load",
        Num::F64 => "f64.load",
        Num::F16 => unreachable!("scratch holds `f16` as `f32`"),
    }
}

fn store(num: Num) -> &'static str {
    match num {
        Num::I8 | Num::U8 => "i32.store8",
        Num::I16 | Num::U16 => "i32.store16",
        Num::I32 => "i32.store",
        Num::I64 => "i64.store",
        Num::F32 => "f32.store",
        Num::F64 => "f64.store",
        Num::F16 => unreachable!("scratch holds `f16` as `f32`"),
    }
}

/// The byte width of a GC array storage, for the scratch copy.
fn storage_size(s: &Storage) -> u32 {
    match s {
        Storage::I8 => 1,
        Storage::I16 => 2,
        Storage::Val(v) => v.size(),
    }
}

impl<'a, 'b> Wat<'a, 'b> {
    pub fn new(l: &'b Layouts<'a>, f: &'b Forms) -> Wat<'a, 'b> {
        Wat {
            l,
            f,
            defs: Vec::new(),
            funcs: Vec::new(),
            imports: Vec::new(),
            memo: HashMap::new(),
            n: 0,
            js_string: false,
            entries: Vec::new(),
            allocn: false,
        }
    }

    fn def(&mut self, d: &Def) -> String {
        let mut inner = Vec::new();
        ValType::Ref(ts_gen_abi::layout::RefType {
            nullable: true,
            heap: Heap::Def(Box::new(d.clone())),
        })
        .defs(&mut inner);
        for d in inner {
            if !self.defs.contains(&d) {
                self.defs.push(d);
            }
        }
        format!("$d{}", self.defs.iter().position(|x| x == d).unwrap())
    }

    pub fn ty_name(&mut self, v: &ValType) -> String {
        self.ty(v)
    }

    fn ty(&mut self, v: &ValType) -> String {
        match v {
            ValType::I32 => "i32".into(),
            ValType::I64 => "i64".into(),
            ValType::F32 => "f32".into(),
            ValType::F64 => "f64".into(),
            ValType::Ref(r) => {
                let heap = match &r.heap {
                    Heap::Extern => "extern".to_string(),
                    Heap::Any => "any".to_string(),
                    Heap::Def(d) => self.def(d),
                };
                match (&r.heap, r.nullable) {
                    (Heap::Extern, true) => "externref".into(),
                    (Heap::Any, true) => "anyref".into(),
                    (_, true) => format!("(ref null {heap})"),
                    (_, false) => format!("(ref {heap})"),
                }
            }
        }
    }

    fn storage(&mut self, s: &Storage) -> String {
        match s {
            Storage::I8 => "i8".into(),
            Storage::I16 => "i16".into(),
            Storage::Val(v) => self.ty(v),
        }
    }

    fn helper(
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
        self.funcs.push(src);
        Ok(name)
    }

    fn wire(&self, r: &Repr) -> Result<Vec<ValType>> {
        Ok(self.l.wire(r)?)
    }

    /// The nullable form of a value type, as helpers take and return it.
    fn loose(&mut self, v: &ValType) -> String {
        self.ty(&v.nullable())
    }

    /// Calls `r`'s translation into its glue wire on `ins`, returning
    /// locals holding the glue wire values.
    fn ser(&mut self, fb: &mut Fb, r: &Repr, ins: &[String]) -> Result<Vec<String>> {
        if !self.f.has_gc(r) {
            return Ok(ins.to_vec());
        }
        let h = self.ser_fn(r)?;
        for i in ins {
            fb.op(format!("local.get {i}"));
        }
        fb.op(format!("call {h}"));
        let outs: Vec<String> = self.f.jsw(r)?.iter().map(|j| fb.local(j.wat())).collect();
        for o in outs.iter().rev() {
            fb.op(format!("local.set {o}"));
        }
        Ok(outs)
    }

    /// Calls the translation of `r`'s glue wire values `ins` back into its
    /// wire, returning locals of the exact wire types.
    fn de(&mut self, fb: &mut Fb, r: &Repr, ins: &[String]) -> Result<Vec<String>> {
        if !self.f.has_gc(r) {
            return Ok(ins.to_vec());
        }
        let h = self.de_fn(r)?;
        for i in ins {
            fb.op(format!("local.get {i}"));
        }
        fb.op(format!("call {h}"));
        let wire = self.wire(r)?;
        let outs: Vec<String> = wire
            .iter()
            .map(|v| {
                let t = self.loose(v);
                fb.local(t)
            })
            .collect();
        for o in outs.iter().rev() {
            fb.op(format!("local.set {o}"));
        }
        // Narrow non-null results back to their exact types.
        let mut exact = Vec::new();
        for (o, v) in outs.iter().zip(&wire) {
            if matches!(v, ValType::Ref(r) if !r.nullable) {
                let t = self.ty(v);
                let x = fb.local(t);
                fb.op(format!("(local.set {x} (ref.as_non_null (local.get {o})))"));
                exact.push(x);
            } else {
                exact.push(o.clone());
            }
        }
        Ok(exact)
    }

    fn sig(&mut self, fb: &mut Fb, ins: &[String], outs: &[String]) -> Vec<String> {
        let ps: Vec<String> = ins.iter().map(|t| fb.param(t.clone())).collect();
        fb.results = outs.to_vec();
        ps
    }

    fn ser_fn(&mut self, r: &Repr) -> Result<String> {
        let key = format!("ser {:?}", r.kind);
        let r = r.clone();
        self.helper(key, move |w, name| {
            let mut fb = Fb::new();
            let wire = w.wire(&r)?;
            let ins: Vec<String> = wire.iter().map(|v| w.loose(v)).collect();
            let outs: Vec<String> = w.f.jsw(&r)?.iter().map(|j| j.wat().to_string()).collect();
            let ps = w.sig(&mut fb, &ins, &outs);
            match &r.kind {
                Kind::Str { carrier: StrCarrier::Gc(Encoding::Utf8), .. } | Kind::Vec { carrier: Carrier::Gc, .. } => {
                    let unit = match &r.kind {
                        Kind::Vec { .. } => match &wire[0] {
                            ValType::Ref(rt) => match &rt.heap {
                                Heap::Def(d) => match &**d {
                                    Def::Array(s) => s.clone(),
                                    _ => unreachable!(),
                                },
                                _ => unreachable!(),
                            },
                            _ => unreachable!(),
                        },
                        _ => Storage::I8,
                    };
                    w.ser_bytes(&mut fb, &ps[0], &wire[0], &unit)?;
                }
                Kind::Str { carrier: StrCarrier::Gc(Encoding::Wtf16), .. } => {
                    w.js_string = true;
                    fb.op(format!(
                        "(if (result externref) (ref.is_null (local.get {p})) (then (ref.null extern)) (else (call $js_from (local.get {p}) (i32.const 0) (array.len (local.get {p})))))",
                        p = ps[0]
                    ));
                }
                Kind::Array { elem, carrier: Carrier::Gc, .. } => w.ser_array(&mut fb, &ps[0], &wire[0], elem)?,
                Kind::Tuple { elems, nullable } => {
                    let mut at = 0;
                    let mut all = Vec::new();
                    if *nullable {
                        all.push(ps[0].clone());
                        at = 1;
                    }
                    for e in elems {
                        let n = w.wire(e)?.len();
                        all.extend(w.ser(&mut fb, e, &ps[at..at + n])?);
                        at += n;
                    }
                    for o in all {
                        fb.op(format!("local.get {o}"));
                    }
                }
                Kind::Enum { layout, .. } => w.ser_enum(&mut fb, &ps, layout)?,
                Kind::Handle { .. } | Kind::Fn { .. } | Kind::Erased { .. } => {
                    fb.op(format!("(extern.convert_any (local.get {}))", ps[0]));
                }
                _ => return unsupported(format!("translating `{}`", r.ty)),
            }
            Ok(fb.finish(name))
        })
    }

    fn de_fn(&mut self, r: &Repr) -> Result<String> {
        let key = format!("de {:?}", r.kind);
        let r = r.clone();
        self.helper(key, move |w, name| {
            let mut fb = Fb::new();
            let wire = w.wire(&r)?;
            let ins: Vec<String> = w.f.jsw(&r)?.iter().map(|j| j.wat().to_string()).collect();
            let outs: Vec<String> = wire.iter().map(|v| w.loose(v)).collect();
            let ps = w.sig(&mut fb, &ins, &outs);
            match &r.kind {
                Kind::Str {
                    carrier: StrCarrier::Gc(Encoding::Utf8),
                    ..
                }
                | Kind::Vec {
                    carrier: Carrier::Gc,
                    ..
                } => {
                    let unit = match &wire[0] {
                        ValType::Ref(rt) => match &rt.heap {
                            Heap::Def(d) => match &**d {
                                Def::Array(s) => s.clone(),
                                _ => unreachable!(),
                            },
                            _ => unreachable!(),
                        },
                        _ => unreachable!(),
                    };
                    w.de_bytes(&mut fb, &ps, &wire[0], &unit)?;
                }
                Kind::Str {
                    carrier: StrCarrier::Gc(Encoding::Wtf16),
                    ..
                } => {
                    w.js_string = true;
                    let t = w.loose(&wire[0]);
                    let d = t
                        .trim_start_matches("(ref null ")
                        .trim_end_matches(')')
                        .to_string();
                    let a = fb.local(t.clone());
                    fb.op(format!(
                        "(if (ref.is_null (local.get {p})) (then (return (ref.null {d}))))",
                        p = ps[0]
                    ));
                    fb.op(format!(
                        "(local.set {a} (array.new_default {d} (call $js_length (local.get {p}))))",
                        p = ps[0]
                    ));
                    fb.op(format!(
                        "(drop (call $js_into (local.get {p}) (local.get {a}) (i32.const 0)))",
                        p = ps[0]
                    ));
                    fb.op(format!("local.get {a}"));
                }
                Kind::Array {
                    elem,
                    carrier: Carrier::Gc,
                    ..
                } => w.de_array(&mut fb, &ps, &wire[0], elem)?,
                Kind::Tuple { elems, nullable } => {
                    let mut at = 0;
                    let mut all = Vec::new();
                    if *nullable {
                        all.push(ps[0].clone());
                        at = 1;
                    }
                    for e in elems {
                        let n = w.f.jsw(e)?.len();
                        all.extend(w.de(&mut fb, e, &ps[at..at + n])?);
                        at += n;
                    }
                    for o in all {
                        fb.op(format!("local.get {o}"));
                    }
                }
                Kind::Enum { layout, .. } => w.de_enum(&mut fb, &ps, layout)?,
                Kind::Handle { .. } | Kind::Fn { .. } | Kind::Erased { .. } => {
                    fb.op(format!("(any.convert_extern (local.get {}))", ps[0]));
                }
                _ => return unsupported(format!("translating `{}`", r.ty)),
            }
            Ok(fb.finish(name))
        })
    }

    /// A string or `vec` array into scratch bytes: `(ptr, len)`, with
    /// `len = -1` for null.
    fn ser_bytes(&mut self, fb: &mut Fb, a: &str, ty: &ValType, unit: &Storage) -> Result<()> {
        let d = self.def_of(ty);
        let size = storage_size(unit);
        let (get, st) = match unit {
            Storage::I8 => ("array.get_u", "i32.store8"),
            Storage::I16 => ("array.get_u", "i32.store16"),
            Storage::Val(ValType::I32) => ("array.get", "i32.store"),
            Storage::Val(ValType::I64) => ("array.get", "i64.store"),
            Storage::Val(ValType::F32) => ("array.get", "f32.store"),
            Storage::Val(ValType::F64) => ("array.get", "f64.store"),
            _ => return unsupported("a reference `vec`"),
        };
        let n = fb.local("i32");
        let p = fb.local("i32");
        let i = fb.local("i32");
        fb.op(format!(
            "(if (ref.is_null (local.get {a})) (then (return (i32.const 0) (i32.const -1))))"
        ));
        fb.op(format!("(local.set {n} (array.len (local.get {a})))"));
        fb.op(format!("(local.set {p} (call $salloc (i32.mul (local.get {n}) (i32.const {size})) (i32.const {size})))"));
        fb.op(format!(
            "(block $b (loop $l (br_if $b (i32.ge_u (local.get {i}) (local.get {n}))) ({st} (i32.add (local.get {p}) (i32.mul (local.get {i}) (i32.const {size}))) ({get} {d} (local.get {a}) (local.get {i}))) (local.set {i} (i32.add (local.get {i}) (i32.const 1))) (br $l)))"
        ));
        fb.op(format!("local.get {p}"));
        fb.op(format!("local.get {n}"));
        Ok(())
    }

    fn de_bytes(&mut self, fb: &mut Fb, ps: &[String], ty: &ValType, unit: &Storage) -> Result<()> {
        let d = self.def_of(ty);
        let size = storage_size(unit);
        let ld = match unit {
            Storage::I8 => "i32.load8_u",
            Storage::I16 => "i32.load16_u",
            Storage::Val(ValType::I32) => "i32.load",
            Storage::Val(ValType::I64) => "i64.load",
            Storage::Val(ValType::F32) => "f32.load",
            Storage::Val(ValType::F64) => "f64.load",
            _ => return unsupported("a reference `vec`"),
        };
        let (p, n) = (&ps[0], &ps[1]);
        let t = self.loose(ty);
        let a = fb.local(t);
        let i = fb.local("i32");
        fb.op(format!(
            "(if (i32.eq (local.get {n}) (i32.const -1)) (then (return (ref.null {d}))))"
        ));
        fb.op(format!(
            "(local.set {a} (array.new_default {d} (local.get {n})))"
        ));
        fb.op(format!(
            "(block $b (loop $l (br_if $b (i32.ge_u (local.get {i}) (local.get {n}))) (array.set {d} (local.get {a}) (local.get {i}) ({ld} (i32.add (local.get {p}) (i32.mul (local.get {i}) (i32.const {size}))))) (local.set {i} (i32.add (local.get {i}) (i32.const 1))) (br $l)))"
        ));
        fb.op(format!("local.get {a}"));
        Ok(())
    }

    fn def_of(&mut self, ty: &ValType) -> String {
        match ty {
            ValType::Ref(r) => match &r.heap {
                Heap::Def(d) => self.def(d),
                _ => unreachable!("not a GC type"),
            },
            _ => unreachable!("not a GC type"),
        }
    }

    /// The element's wire values from an array element on the stack.
    fn elem_parts(&mut self, fb: &mut Fb, elem: &Repr) -> Result<Vec<String>> {
        let wire = self.wire(elem)?;
        let e = self.l.gc_elem(elem)?;
        let t = self.storage(&e);
        let v = fb.local(t);
        fb.op(format!("local.set {v}"));
        if wire.len() == 1 {
            return Ok(vec![v]);
        }
        let Storage::Val(sty) = &e else {
            unreachable!()
        };
        let sd = self.def_of(sty);
        let mut out = Vec::new();
        for (k, f) in wire.iter().enumerate() {
            let ft = self.ty(f);
            let x = fb.local(ft);
            fb.op(format!(
                "(local.set {x} (struct.get {sd} {k} (local.get {v})))"
            ));
            out.push(x);
        }
        Ok(out)
    }

    fn ser_array(&mut self, fb: &mut Fb, a: &str, ty: &ValType, elem: &Repr) -> Result<()> {
        if let Kind::Scalar(s) = elem.kind {
            return self.ser_bytes(fb, a, ty, &scalar_storage(s));
        }
        let d = self.def_of(ty);
        let e = self.f.elem(elem, crate::form::Space::Scratch)?;
        let n = fb.local("i32");
        let p = fb.local("i32");
        let x0 = fb.local("i32");
        let i = fb.local("i32");
        let o = fb.local("i32");
        let x = fb.local("i32");
        let mut nulls = Vec::new();
        if e.size > 0 {
            nulls.push("(i32.const 0)");
        }
        if e.refs > 0 {
            nulls.push("(i32.const 0)");
        }
        nulls.push("(i32.const -1)");
        fb.op(format!(
            "(if (ref.is_null (local.get {a})) (then (return {})))",
            nulls.join(" ")
        ));
        fb.op(format!("(local.set {n} (array.len (local.get {a})))"));
        if e.size > 0 {
            fb.op(format!(
                "(local.set {p} (call $salloc (i32.mul (local.get {n}) (i32.const {})) (i32.const {})))",
                e.size, e.align
            ));
        }
        if e.refs > 0 {
            fb.op(format!(
                "(local.set {x0} (call $stalloc (i32.mul (local.get {n}) (i32.const {}))))",
                e.refs
            ));
        }
        fb.op("(block $b (loop $l");
        fb.op(format!(
            "(br_if $b (i32.ge_u (local.get {i}) (local.get {n})))"
        ));
        fb.op(format!(
            "(local.set {o} (i32.add (local.get {p}) (i32.mul (local.get {i}) (i32.const {}))))",
            e.size
        ));
        fb.op(format!(
            "(local.set {x} (i32.add (local.get {x0}) (i32.mul (local.get {i}) (i32.const {}))))",
            e.refs
        ));
        fb.op(format!("(array.get {d} (local.get {a}) (local.get {i}))"));
        let parts = self.elem_parts(fb, elem)?;
        let jsw = self.ser(fb, elem, &parts)?;
        for (st, v) in e.stores.iter().zip(&jsw) {
            self.store_elem(fb, st, v, &o, &x);
        }
        fb.op(format!(
            "(local.set {i} (i32.add (local.get {i}) (i32.const 1)))"
        ));
        fb.op("(br $l)))");
        if e.size > 0 {
            fb.op(format!("local.get {p}"));
        }
        if e.refs > 0 {
            fb.op(format!("local.get {x0}"));
        }
        fb.op(format!("local.get {n}"));
        Ok(())
    }

    fn de_array(&mut self, fb: &mut Fb, ps: &[String], ty: &ValType, elem: &Repr) -> Result<()> {
        if let Kind::Scalar(s) = elem.kind {
            return self.de_bytes(fb, ps, ty, &scalar_storage(s));
        }
        let d = self.def_of(ty);
        let e = self.f.elem(elem, crate::form::Space::Scratch)?;
        let mut at = 0;
        let p = if e.size > 0 {
            at += 1;
            Some(ps[0].clone())
        } else {
            None
        };
        let x0 = if e.refs > 0 {
            at += 1;
            Some(ps[at - 1].clone())
        } else {
            None
        };
        let n = ps[at].clone();
        let t = self.loose(ty);
        let a = fb.local(t);
        let i = fb.local("i32");
        let o = fb.local("i32");
        let x = fb.local("i32");
        fb.op(format!(
            "(if (i32.eq (local.get {n}) (i32.const -1)) (then (return (ref.null {d}))))"
        ));
        fb.op(format!(
            "(if (i32.eqz (local.get {n})) (then (return (array.new_fixed {d} 0))))"
        ));
        fb.op("(block $b (loop $l");
        fb.op(format!(
            "(br_if $b (i32.ge_u (local.get {i}) (local.get {n})))"
        ));
        let pv = p
            .as_deref()
            .map(|p| format!("(local.get {p})"))
            .unwrap_or("(i32.const 0)".into());
        let xv = x0
            .as_deref()
            .map(|x| format!("(local.get {x})"))
            .unwrap_or("(i32.const 0)".into());
        fb.op(format!(
            "(local.set {o} (i32.add {pv} (i32.mul (local.get {i}) (i32.const {}))))",
            e.size
        ));
        fb.op(format!(
            "(local.set {x} (i32.add {xv} (i32.mul (local.get {i}) (i32.const {}))))",
            e.refs
        ));
        let jsw: Vec<String> = e
            .stores
            .iter()
            .zip(self.f.jsw(elem)?)
            .map(|(st, j)| self.load_elem(fb, st, j, &o, &x))
            .collect();
        let parts = self.de(fb, elem, &jsw)?;
        let wire = self.wire(elem)?;
        if wire.len() == 1 {
            fb.op(format!("local.get {}", parts[0]));
        } else {
            let Storage::Val(sty) = self.l.gc_elem(elem)? else {
                unreachable!()
            };
            let sd = self.def_of(&sty);
            for v in &parts {
                fb.op(format!("local.get {v}"));
            }
            fb.op(format!("struct.new {sd}"));
        }
        let et = self.storage(&self.l.gc_elem(elem)?);
        let ev = fb.local(et);
        fb.op(format!("local.set {ev}"));
        fb.op(format!(
            "(if (i32.eqz (local.get {i})) (then (local.set {a} (array.new {d} (local.get {ev}) (local.get {n})))) (else (array.set {d} (local.get {a}) (local.get {i}) (local.get {ev}))))"
        ));
        fb.op(format!(
            "(local.set {i} (i32.add (local.get {i}) (i32.const 1)))"
        ));
        fb.op("(br $l)))");
        fb.op(format!("local.get {a}"));
        Ok(())
    }

    fn store_elem(&mut self, fb: &mut Fb, st: &Store, v: &str, o: &str, x: &str) {
        match *st {
            Store::Num { off, num } => fb.op(format!(
                "({} offset={off} (local.get {o}) (local.get {v}))",
                store(num)
            )),
            Store::Niche { off, num, niche } => {
                let (ty, eq) = if num == Num::I64 {
                    ("i64", "i64.eq")
                } else {
                    ("i32", "i32.eq")
                };
                fb.op(format!(
                    "(if ({eq} (local.get {v}) ({ty}.const {niche})) (then (i32.store8 offset={off} (local.get {o}) (i32.const 0)) ({} offset={} (local.get {o}) ({ty}.const 0))) (else (i32.store8 offset={off} (local.get {o}) (i32.const 1)) ({} offset={} (local.get {o}) (local.get {v}))))",
                    store(num),
                    off + 1,
                    store(num),
                    off + 1
                ));
            }
            Store::Flag { off } => fb.op(format!(
                "(i32.store8 offset={off} (local.get {o}) (local.get {v}))"
            )),
            Store::Ref { idx } => fb.op(format!(
                "(table.set $st (i32.add (local.get {x}) (i32.const {idx})) (local.get {v}))"
            )),
            Store::Conv { .. } | Store::Arm { .. } => {
                unreachable!("scratch holds no converted elements")
            }
        }
    }

    fn load_elem(&mut self, fb: &mut Fb, st: &Store, j: Jv, o: &str, x: &str) -> String {
        let v = fb.local(j.wat());
        match *st {
            Store::Num { off, num } => fb.op(format!(
                "(local.set {v} ({} offset={off} (local.get {o})))",
                load(num)
            )),
            Store::Niche { off, num, niche } => {
                let ty = if num == Num::I64 { "i64" } else { "i32" };
                fb.op(format!(
                    "(local.set {v} (if (result {ty}) (i32.load8_u offset={off} (local.get {o})) (then ({} offset={} (local.get {o}))) (else ({ty}.const {niche}))))",
                    load(num),
                    off + 1
                ));
            }
            Store::Flag { off } => fb.op(format!(
                "(local.set {v} (i32.load8_u offset={off} (local.get {o})))"
            )),
            Store::Ref { idx } => fb.op(format!(
                "(local.set {v} (table.get $st (i32.add (local.get {x}) (i32.const {idx}))))"
            )),
            Store::Conv { .. } | Store::Arm { .. } => {
                unreachable!("scratch holds no converted elements")
            }
        }
        v
    }

    /// Per-arm regions on the glue wire, shared slots on the wire (§8).
    fn regions(&self, e: &EnumLayout) -> Result<Vec<(usize, usize)>> {
        let mut out = Vec::new();
        let mut at = 1;
        for a in &e.arms {
            let mut n = 0;
            for p in &a.payload {
                n += self.f.jsw(p)?.len();
            }
            out.push((at, n));
            at += n;
        }
        Ok(out)
    }

    fn ser_enum(&mut self, fb: &mut Fb, ps: &[String], e: &EnumLayout) -> Result<()> {
        let Shape::Payload { slots } = &e.shape else {
            unreachable!("only payload enums hold GC values")
        };
        let jsw = self.f.jsw(&Repr {
            ty: e.path.clone(),
            kind: Kind::Enum {
                layout: std::rc::Rc::new(e.clone()),
                nullable: false,
            },
            js_null: false,
        })?;
        let outs: Vec<String> = jsw.iter().map(|j| fb.local(j.wat())).collect();
        let _ = slots;
        fb.op(format!("(local.set {} (local.get {}))", outs[0], ps[0]));
        for (a, (at, n)) in e.arms.iter().zip(self.regions(e)?) {
            if a.payload.is_empty() {
                continue;
            }
            fb.op(format!(
                "(if (i32.eq (local.get {}) (i32.const {})) (then",
                ps[0], a.index
            ));
            let mut k = 0;
            let mut vals = Vec::new();
            for p in &a.payload {
                let m = self.wire(p)?.len();
                let ins: Vec<String> = a.slots[k..k + m]
                    .iter()
                    .map(|s| ps[1 + s].clone())
                    .collect();
                vals.extend(self.ser(fb, p, &ins)?);
                k += m;
            }
            for (o, v) in outs[at..at + n].iter().zip(vals) {
                fb.op(format!("(local.set {o} (local.get {v}))"));
            }
            fb.op("))");
        }
        for o in outs {
            fb.op(format!("local.get {o}"));
        }
        Ok(())
    }

    fn de_enum(&mut self, fb: &mut Fb, ps: &[String], e: &EnumLayout) -> Result<()> {
        let Shape::Payload { slots } = &e.shape else {
            unreachable!("only payload enums hold GC values")
        };
        let outs: Vec<String> = slots
            .iter()
            .map(|s| {
                let t = self.loose(s);
                fb.local(t)
            })
            .collect();
        for (a, (at, n)) in e.arms.iter().zip(self.regions(e)?) {
            if a.payload.is_empty() {
                continue;
            }
            fb.op(format!(
                "(if (i32.eq (local.get {}) (i32.const {})) (then",
                ps[0], a.index
            ));
            let mut k = 0;
            let mut vals = Vec::new();
            for p in &a.payload {
                let m = self.f.jsw(p)?.len();
                vals.extend(self.de(fb, p, &ps[at + k..at + k + m])?);
                k += m;
            }
            let _ = n;
            for (s, v) in a.slots.iter().zip(vals) {
                fb.op(format!("(local.set {} (local.get {v}))", outs[*s]));
            }
            fb.op("))");
        }
        fb.op(format!("local.get {}", ps[0]));
        for o in outs {
            fb.op(format!("local.get {o}"));
        }
        Ok(())
    }

    /// The forwarder for a GC import: the language's exact wire, over the
    /// glue function `glue`.
    /// Returns the glue's return area, when its results use one.
    pub fn forwarder(
        &mut self,
        export: &str,
        glue: &str,
        func: &Func<'_>,
        area: Option<&RetArea>,
    ) -> Result<()> {
        let mut gin = Vec::new();
        let mut gout = Vec::new();
        for p in &func.sig.params {
            gin.extend(self.f.jsw(&p.repr)?.iter().map(|j| j.wat()));
        }
        for r in &func.sig.results {
            gout.extend(self.f.jsw(&r.repr)?.iter().map(|j| j.wat()));
        }
        let mut imp = format!("  (import \"{GLUE}\" \"{glue}\" (func ${glue}");
        if !gin.is_empty() {
            let _ = write!(imp, " (param {})", gin.join(" "));
        }
        if let Some(a) = area {
            let _ = write!(imp, " (result {})", gout[a.direct]);
        } else if !gout.is_empty() {
            let _ = write!(imp, " (result {})", gout.join(" "));
        }
        imp.push_str("))");
        self.imports.push(imp);

        let mut fb = Fb::new();
        let core = func.sig.core();
        let spills = self.l.lang_core(&core)?.spill.len();
        let ps: Vec<String> = core
            .params
            .iter()
            .map(|v| {
                let t = self.ty(v);
                fb.param(t)
            })
            .collect();
        fb.results = core.results.iter().map(|v| self.ty(v)).collect();
        let sp0 = fb.local("i32");
        let tsp0 = fb.local("i32");
        fb.op(format!("(local.set {sp0} (call $sp))"));
        fb.op(format!("(local.set {tsp0} (call $tsp))"));
        let mut body = Fb::new();
        std::mem::swap(&mut body.locals, &mut fb.locals);
        body.n = 100;
        let mut at = 0;
        let mut args = Vec::new();
        let mut writeback = Vec::new();
        for p in &func.sig.params {
            let n = p.wire.len();
            let ins = &ps[at..at + n];
            let out = self.ser(&mut body, &p.repr, ins)?;
            if let Kind::Vec {
                writeback: true,
                carrier: Carrier::Gc,
                ..
            } = &p.repr.kind
            {
                writeback.push((ins[0].clone(), out[0].clone(), p.wire[0].clone()));
            }
            args.extend(out);
            at += n;
        }
        for a in &args {
            body.op(format!("local.get {a}"));
        }
        body.op(format!("call ${glue}"));
        let mut gres = Vec::new();
        for r in &func.sig.results {
            gres.extend(self.f.jsw(&r.repr)?.iter().map(|j| body.local(j.wat())));
        }
        if let Some(a) = area {
            body.op(format!("local.set {}", gres[a.direct]));
            for (j, vt, off) in &a.slots {
                body.op(format!(
                    "(local.set {} ({vt}.load offset={} (i32.const 0)))",
                    gres[*j],
                    a.base + off
                ));
            }
        } else {
            for g in gres.iter().rev() {
                body.op(format!("local.set {g}"));
            }
        }
        for (arr, p, ty) in writeback {
            let h = self.writeback_fn(&ty)?;
            body.op(format!("(call {h} (local.get {arr}) (local.get {p}))"));
        }
        let mut at = 0;
        let mut results = Vec::new();
        for r in &func.sig.results {
            let n = self.f.jsw(&r.repr)?.len();
            results.extend(self.de(&mut body, &r.repr, &gres[at..at + n])?);
            at += n;
        }
        // Locals set inside the handler block aren't initialized after it,
        // so the results are held nullable and narrowed on return.
        let rlocals: Vec<(String, bool)> = core
            .results
            .iter()
            .map(|v| {
                let t = self.loose(v);
                let narrow = matches!(v, ValType::Ref(r) if !r.nullable);
                (body.local(t), narrow)
            })
            .collect();
        for ((l, _), r) in rlocals.iter().zip(&results) {
            body.op(format!("(local.set {l} (local.get {r}))"));
        }
        fb.locals = body.locals;
        let restore = format!("(call $restore (local.get {sp0}) (local.get {tsp0}))");
        match self.l.profile.eh {
            Eh::Modern => {
                let exn = fb.local("exnref");
                fb.op("(block $done");
                fb.op(format!("(local.set {exn} (block $catch (result exnref) (try_table (catch_all_ref $catch)"));
                fb.code
                    .extend(body.code.into_iter().map(|c| format!("  {c}")));
                fb.op(") (br $done)))");
                fb.op(&restore);
                fb.op(format!("(throw_ref (local.get {exn})))"));
            }
            Eh::Legacy => {
                fb.op("try");
                fb.code
                    .extend(body.code.into_iter().map(|c| format!("  {c}")));
                fb.op("catch_all");
                fb.op(format!("  {restore}"));
                fb.op("  rethrow 0");
                fb.op("end");
            }
        }
        fb.op(&restore);
        // `results = single` (§12): slots 1.. go to the spill, in order.
        let at = rlocals.len() - spills;
        if spills > 0 {
            fb.results.truncate(at);
            fb.op("(call $spill_check)");
            for (i, (l, _)) in rlocals[at..].iter().enumerate() {
                let vt = spill_name(&core.results[at + i]).expect("a spill type");
                fb.op(format!("(call $push_{vt} (local.get {l}))"));
            }
        }
        for (l, narrow) in &rlocals[..at] {
            fb.op(if *narrow {
                format!("(ref.as_non_null (local.get {l}))")
            } else {
                format!("local.get {l}")
            });
        }
        self.funcs
            .push(fb.finish(&format!("${export} (export \"{export}\")")));
        Ok(())
    }

    /// The return area of a GC import's glue, over its JS wire results.
    pub fn glue_area(&mut self, func: &Func<'_>) -> Result<Option<RetArea>> {
        let mut gout = Vec::new();
        for r in &func.sig.results {
            gout.extend(self.f.jsw(&r.repr)?.iter().map(|j| j.wat()));
        }
        Ok(self.ret_area(&gout))
    }

    /// A multi-value result from JS costs an iterated array, so a JS
    /// import's extra numeric results return through the scratch memory's
    /// first `RET_AREA` bytes instead, which `glue` writes just before it
    /// returns its one direct result. At most one result may be a
    /// reference, the direct one.
    pub fn ret_area<S: AsRef<str>>(&mut self, results: &[S]) -> Option<RetArea> {
        if results.len() < 2 {
            return None;
        }
        let base = self.ret_base();
        let num = |t: &str| matches!(t, "i32" | "i64" | "f32" | "f64");
        let refs: Vec<usize> = (0..results.len())
            .filter(|&j| !num(results[j].as_ref()))
            .collect();
        if refs.len() > 1 || (results.len() - 1) * 8 > RET_AREA as usize {
            return None;
        }
        let direct = refs.first().copied().unwrap_or(0);
        let slots = (0..results.len())
            .filter(|&j| j != direct)
            .enumerate()
            .map(|(k, j)| (j, results[j].as_ref().to_string(), k as u32 * 8))
            .collect();
        Some(RetArea {
            direct,
            base,
            slots,
        })
    }

    /// Scratch starts with its header, the stack pointers JS reads through
    /// a typed view (a `WebAssembly.Global` costs ~10 ns an access), past
    /// the spill's numbers.
    fn hdr(&self) -> u32 {
        if self.l.profile.results == Results::Single {
            SPILL_NUMBERS * 8
        } else {
            0
        }
    }

    fn ret_base(&self) -> u32 {
        self.hdr() + 8
    }

    /// A pass-through for a linear import with multi-value results, over
    /// the glue function `glue` and its return area.
    pub fn ret_shim(&mut self, export: &str, glue: &str, ty: &FuncType, a: &RetArea) {
        let ps: Vec<String> = ty.params.iter().map(|v| self.ty(v)).collect();
        let rs: Vec<String> = ty.results.iter().map(|v| self.ty(v)).collect();
        let mut imp = format!("  (import \"{GLUE}\" \"{glue}\" (func ${glue}");
        if !ps.is_empty() {
            let _ = write!(imp, " (param {})", ps.join(" "));
        }
        let _ = write!(imp, " (result {})))", rs[a.direct]);
        self.imports.push(imp);
        let mut fb = Fb::new();
        let args: Vec<String> = ps.iter().map(|t| fb.param(t.clone())).collect();
        fb.results = rs.clone();
        let d = fb.local(rs[a.direct].clone());
        let call: String = args.iter().map(|a| format!(" (local.get {a})")).collect();
        fb.op(format!("(local.set {d} (call ${glue}{call}))"));
        for (j, _) in rs.iter().enumerate() {
            if j == a.direct {
                fb.op(format!("local.get {d}"));
            } else {
                let (_, vt, off) = a.slots.iter().find(|s| s.0 == j).unwrap();
                fb.op(format!("({vt}.load offset={} (i32.const 0))", a.base + off));
            }
        }
        self.funcs
            .push(fb.finish(&format!("${export} (export \"{export}\")")));
    }

    /// An entry into a language export with GC positions: JS calls `eN`
    /// over the glue wire, which translates the arguments out of scratch,
    /// calls the export through the `entries` table, and translates the
    /// results into scratch. JS resets scratch after reading them. Returns
    /// the table slot.
    pub fn entry(&mut self, sig: &Signature) -> Result<usize> {
        let slot = self.entries.len();
        let full = sig.core();
        let lang = self.l.lang_core(&full)?;
        let core = &lang.ty;
        let mut ft = String::from("(func");
        if !core.params.is_empty() {
            let ps: Vec<String> = core.params.iter().map(|v| self.ty(v)).collect();
            let _ = write!(ft, " (param {})", ps.join(" "));
        }
        if !core.results.is_empty() {
            let rs: Vec<String> = core.results.iter().map(|v| self.ty(v)).collect();
            let _ = write!(ft, " (result {})", rs.join(" "));
        }
        ft.push(')');
        self.entries.push(ft);
        let mut fb = Fb::new();
        let mut args = Vec::new();
        for p in &sig.params {
            let ins: Vec<String> = self
                .f
                .jsw(&p.repr)?
                .iter()
                .map(|j| fb.param(j.wat().to_string()))
                .collect();
            args.extend(self.de(&mut fb, &p.repr, &ins)?);
        }
        for r in &sig.results {
            fb.results
                .extend(self.f.jsw(&r.repr)?.iter().map(|j| j.wat().to_string()));
        }
        for a in &args {
            fb.op(format!("local.get {a}"));
        }
        fb.op(format!(
            "(call_indirect $entries (type $ft{slot}) (i32.const {slot}))"
        ));
        if !lang.spill.is_empty() {
            // A pending `throws = js` error comes back with no spill, and
            // JS throws it in place of the results (§15).
            if self.l.profile.throws == Throws::Js && sig.throws.is_some() {
                let zeros: Vec<String> = fb
                    .results
                    .iter()
                    .map(|t| match t.as_str() {
                        "externref" => "(ref.null extern)".to_string(),
                        t => format!("({t}.const 0)"),
                    })
                    .collect();
                let s0 = self.ty(&core.results[0]);
                let s0 = fb.local(s0);
                fb.op(format!("local.set {s0}"));
                fb.op(format!(
                    "(if (global.get $pending) (then {} (return)))",
                    zeros.join(" ")
                ));
                fb.op(format!("local.get {s0}"));
            }
            for v in &lang.spill {
                let vt = spill_name(v).expect("a spill type");
                fb.op(format!("(call $pop_{vt})"));
                if matches!(v, ValType::Ref(r) if !r.nullable) {
                    fb.op("ref.as_non_null");
                }
            }
            fb.op("(call $spill_check)");
        }
        let mut res = Vec::new();
        for r in &sig.results {
            for v in self.wire(&r.repr)? {
                let t = self.ty(&v);
                res.push((fb.local(t), r));
            }
        }
        for (l, _) in res.iter().rev() {
            fb.op(format!("local.set {l}"));
        }
        let mut at = 0;
        let mut outs = Vec::new();
        for r in &sig.results {
            let n = self.wire(&r.repr)?.len();
            let ins: Vec<String> = res[at..at + n].iter().map(|(l, _)| l.clone()).collect();
            outs.extend(self.ser(&mut fb, &r.repr, &ins)?);
            at += n;
        }
        for o in outs {
            fb.op(format!("local.get {o}"));
        }
        self.funcs
            .push(fb.finish(&format!("$e{slot} (export \"e{slot}\")")));
        Ok(slot)
    }

    /// The conversions of a GC value held externalized in the language's
    /// table (§10): `c{i}` reads it into its glue wire in scratch, `d{i}`
    /// translates its glue wire back and externalizes it.
    pub fn conv(&mut self, i: usize, r: &Repr) -> Result<()> {
        let wire = self.wire(r)?;
        let loose = self.loose(&wire[0]);
        let jsw: Vec<String> = self.f.jsw(r)?.iter().map(|j| j.wat().to_string()).collect();

        let mut fb = Fb::new();
        let p = fb.param("externref".into());
        fb.results = jsw.clone();
        let x = fb.local(loose.clone());
        fb.op(format!(
            "(local.set {x} (ref.cast {loose} (any.convert_extern (local.get {p}))))"
        ));
        for o in self.ser(&mut fb, r, &[x])? {
            fb.op(format!("local.get {o}"));
        }
        self.funcs
            .push(fb.finish(&format!("$c{i} (export \"c{i}\")")));

        let mut fb = Fb::new();
        let ins: Vec<String> = jsw.into_iter().map(|t| fb.param(t)).collect();
        fb.results = vec!["externref".into()];
        let out = self.de(&mut fb, r, &ins)?;
        fb.op(format!("(extern.convert_any (local.get {}))", out[0]));
        self.funcs
            .push(fb.finish(&format!("$d{i} (export \"d{i}\")")));
        Ok(())
    }

    /// Copies a `mut T[]`'s scratch bytes back into its GC array (§11).
    fn writeback_fn(&mut self, ty: &ValType) -> Result<String> {
        let key = format!("wb {ty}");
        let ty = ty.clone();
        self.helper(key, move |w, name| {
            let d = w.def_of(&ty);
            let ValType::Ref(rt) = &ty else { unreachable!() };
            let Heap::Def(def) = &rt.heap else { unreachable!() };
            let Def::Array(unit) = &**def else { unreachable!() };
            let size = storage_size(unit);
            let ld = match unit {
                Storage::I8 => "i32.load8_u",
                Storage::I16 => "i32.load16_u",
                Storage::Val(ValType::I32) => "i32.load",
                Storage::Val(ValType::I64) => "i64.load",
                Storage::Val(ValType::F32) => "f32.load",
                Storage::Val(ValType::F64) => "f64.load",
                _ => return unsupported("a reference `vec`"),
            };
            let mut fb = Fb::new();
            let lt = w.loose(&ty);
            let a = fb.param(lt);
            let p = fb.param("i32".into());
            let i = fb.local("i32");
            let n = fb.local("i32");
            fb.op(format!("(if (ref.is_null (local.get {a})) (then (return)))"));
            fb.op(format!("(local.set {n} (array.len (local.get {a})))"));
            fb.op(format!(
                "(block $b (loop $l (br_if $b (i32.ge_u (local.get {i}) (local.get {n}))) (array.set {d} (local.get {a}) (local.get {i}) ({ld} (i32.add (local.get {p}) (i32.mul (local.get {i}) (i32.const {size}))))) (local.set {i} (i32.add (local.get {i}) (i32.const 1))) (br $l)))"
            ));
            Ok(fb.finish(name))
        })
    }

    /// A `tw` constant (§13) as a bridge function over the JS value, which
    /// the glue passes as an immutable global: a Wasm to Wasm call.
    pub fn constant(&mut self, group: &str) -> String {
        let name = group.trim_start_matches(protocol::PREFIX);
        if self.memo.insert(group.into(), name.into()).is_some() {
            return name.to_string();
        }
        self.imports.push(format!(
            "  (import \"{GLUE}\" \"{name}\" (global ${name} externref))"
        ));
        self.funcs.push(format!(
            "  (func (export \"{name}\") (result externref) (global.get ${name}))"
        ));
        name.to_string()
    }

    pub fn finish(mut self) -> String {
        let mut s = String::from("(module\n");
        let a16 = if self.js_string {
            Some(self.def(&Def::Array(Storage::I16)))
        } else {
            None
        };
        let defs = std::mem::take(&mut self.defs);
        let mut names: Vec<String> = Vec::new();
        for (i, d) in defs.iter().enumerate() {
            let body = match d {
                Def::Array(st) => format!("(array (mut {}))", self.storage_named(st, &defs)),
                Def::Struct(fs) => {
                    let fs: Vec<String> = fs
                        .iter()
                        .map(|f| format!("(field {})", self.storage_named(f, &defs)))
                        .collect();
                    format!("(struct {})", fs.join(" "))
                }
            };
            names.push(format!("  (type $d{i} {body})"));
        }
        for n in names {
            let _ = writeln!(s, "{n}");
        }
        for i in &self.imports {
            let _ = writeln!(s, "{i}");
        }
        if self.l.profile.results == Results::Single {
            let _ = writeln!(
                s,
                "  (import \"{GLUE}\" \"spill_fault\" (func $fault (param i32)))"
            );
        }
        if let Some(a) = &a16 {
            let _ = writeln!(s, "  (import \"wasm:js-string\" \"length\" (func $js_length (param externref) (result i32)))");
            let _ = writeln!(s, "  (import \"wasm:js-string\" \"fromCharCodeArray\" (func $js_from (param (ref null {a})) (param i32 i32) (result (ref extern))))");
            let _ = writeln!(s, "  (import \"wasm:js-string\" \"intoCharCodeArray\" (func $js_into (param externref) (param (ref null {a})) (param i32) (result i32)))");
        }
        for (i, ft) in self.entries.iter().enumerate() {
            let _ = writeln!(s, "  (type $ft{i} {ft})");
        }
        let single = self.l.profile.results == Results::Single;
        let hdr = self.hdr();
        let sp = (hdr + 8 + RET_AREA).to_le_bytes();
        let sp: String = sp.iter().map(|b| format!("\\{b:02x}")).collect();
        s.push_str(
            &RUNTIME
                .replace("{HDR}", &hdr.to_string())
                .replace("{SP}", &sp),
        );
        if single {
            let p = self.l.profile;
            let global = |name: &str, v: u32| {
                format!(
                    "  (global (export \"{}\") i32 (i32.const {v}))\n",
                    p.name(name)
                )
            };
            let _ = writeln!(
                s,
                "  (table $tw (export \"{}\") {} externref)",
                p.name(protocol::TABLE),
                protocol::DEFAULT_HEAP_BASE
            );
            s.push_str(&global(
                protocol::TABLE_STACK_BASE,
                protocol::DEFAULT_STACK_BASE,
            ));
            s.push_str(&global(
                protocol::TABLE_CONST_BASE,
                protocol::DEFAULT_CONST_BASE,
            ));
            s.push_str(&global(
                protocol::TABLE_HEAP_BASE,
                protocol::DEFAULT_HEAP_BASE,
            ));
            s.push_str(
                &SPILL
                    .replace("{STACK}", &protocol::DEFAULT_STACK_BASE.to_string())
                    .replace("{CONST}", &protocol::DEFAULT_CONST_BASE.to_string())
                    .replace("{NUMBERS}", &SPILL_NUMBERS.to_string()),
            );
            for (vt, store, load) in [
                ("i32", "i32.store", "i32.load"),
                ("i64", "i64.store", "i64.load"),
                ("f32", "f32.store", "f32.load"),
                ("f64", "f64.store", "f64.load"),
            ] {
                let _ = writeln!(s, "  (func $push_{vt} (export \"push_{vt}\") (param $v {vt}) ({store} (call $nslot) (local.get $v)))");
                let _ = writeln!(s, "  (func $pop_{vt} (export \"pop_{vt}\") (result {vt}) (local $v {vt}) (local.set $v ({load} (call $nnext))) (call $settle) (local.get $v))");
            }
        }
        if self.l.profile.throws == Throws::Js {
            s.push_str("  (global $pending (export \"pending\") (mut i32) (i32.const 0))\n");
        }
        if self.allocn {
            s.push_str(ALLOCN);
        }
        if !self.entries.is_empty() {
            let _ = writeln!(
                s,
                "  (table $entries (export \"entries\") {} funcref)",
                self.entries.len()
            );
        }
        if let Some(a) = &a16 {
            let _ = writeln!(s, "  (func (export \"a16get\") (param $a (ref null {a})) (param $i i32) (result i32) (array.get_u {a} (local.get $a) (local.get $i)))");
            let _ = writeln!(s, "  (func (export \"a16set\") (param $a (ref null {a})) (param $i i32) (param $v i32) (array.set {a} (local.get $a) (local.get $i) (local.get $v)))");
        }
        for f in &self.funcs {
            let _ = writeln!(s, "{f}");
        }
        s.push_str(")\n");
        s
    }

    fn storage_named(&self, st: &Storage, defs: &[Def]) -> String {
        match st {
            Storage::I8 => "i8".into(),
            Storage::I16 => "i16".into(),
            Storage::Val(v) => match v {
                ValType::Ref(r) => match &r.heap {
                    Heap::Def(d) => {
                        let i = defs.iter().position(|x| x == &**d).unwrap();
                        if r.nullable {
                            format!("(ref null $d{i})")
                        } else {
                            format!("(ref $d{i})")
                        }
                    }
                    Heap::Extern if r.nullable => "externref".into(),
                    Heap::Any if r.nullable => "anyref".into(),
                    Heap::Extern => "(ref extern)".into(),
                    Heap::Any => "(ref any)".into(),
                },
                v => v.to_string(),
            },
        }
    }
}

/// The numeric spill slots, 8 bytes each, at the start of scratch memory.
/// The module the bridge module imports its glue functions from.
pub(crate) const GLUE: &str = "tw:glue";

pub(crate) const SPILL_NUMBERS: u32 = 64;
const RET_AREA: u32 = 64;

/// A return area's layout: the direct result's index, the area's address,
/// and each other result's index, type and offset in it.
#[derive(Clone)]
pub struct RetArea {
    pub direct: usize,
    pub base: u32,
    pub slots: Vec<(usize, String, u32)>,
}

/// The scratch memory and table, bump allocated and restored as a stack.
const RUNTIME: &str = r#"  (memory $m (export "memory") 1)
  (table $st (export "table") 0 externref)
  (global (export "hdr") i32 (i32.const {HDR}))
  (data (i32.const {HDR}) "{SP}")
  (func $sp (result i32) (i32.load (i32.const {HDR})))
  (func $tsp (result i32) (i32.load offset=4 (i32.const {HDR})))
  (func $set_sp (param $v i32) (i32.store (i32.const {HDR}) (local.get $v)))
  (func $set_tsp (param $v i32) (i32.store offset=4 (i32.const {HDR}) (local.get $v)))
  (func $salloc (param $size i32) (param $align i32) (result i32)
    (local $p i32) (local $end i32) (local $have i32)
    (local.set $p (i32.and (i32.add (call $sp) (i32.sub (local.get $align) (i32.const 1))) (i32.sub (i32.const 0) (local.get $align))))
    (local.set $end (i32.add (local.get $p) (local.get $size)))
    (local.set $have (i32.shl (memory.size) (i32.const 16)))
    (if (i32.gt_u (local.get $end) (local.get $have))
      (then (if (i32.eq (memory.grow (i32.shr_u (i32.add (i32.sub (local.get $end) (local.get $have)) (i32.const 65535)) (i32.const 16))) (i32.const -1)) (then unreachable))))
    (call $set_sp (local.get $end))
    (local.get $p))
  (func $stalloc (param $n i32) (result i32)
    (local $b i32) (local $end i32)
    (local.set $b (call $tsp))
    (local.set $end (i32.add (local.get $b) (local.get $n)))
    (if (i32.gt_u (local.get $end) (table.size $st))
      (then (if (i32.eq (table.grow $st (ref.null extern) (i32.sub (local.get $end) (table.size $st))) (i32.const -1)) (then unreachable))))
    (call $set_tsp (local.get $end))
    (local.get $b))
  (func $restore (param $sp i32) (param $tsp i32)
    (table.fill $st (local.get $tsp) (ref.null extern) (i32.sub (call $tsp) (local.get $tsp)))
    (call $set_tsp (local.get $tsp))
    (call $set_sp (local.get $sp)))
"#;

/// `allocn(q, n)`: `n` language allocations in one call, from the sizes
/// at `q` to the addresses after them, through the language's
/// `tw:realloc` in `rtab`. An empty one is the dangling 1.
const ALLOCN: &str = r#"  (type $realloc (func (param i32 i32 i32 i32) (result i32)))
  (table $rtab (export "rtab") 1 funcref)
  (func (export "allocn") (param $q i32) (param $n i32)
    (local $i i32) (local $a i32) (local $size i32)
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $a (i32.add (local.get $q) (i32.shl (local.get $i) (i32.const 2))))
      (local.set $size (i32.load (local.get $a)))
      (i32.store (i32.add (local.get $a) (i32.shl (local.get $n) (i32.const 2)))
        (if (result i32) (local.get $size)
          (then (call_indirect $rtab (type $realloc) (i32.const 0) (i32.const 0) (i32.const 1) (local.get $size) (i32.const 0)))
          (else (i32.const 1))))
      (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))))
"#;

/// The spill of `results = single` (§12, §14): two FIFO streams, extern
/// references in `tw:table` growing down from `const_base`, and numbers in
/// 8-byte scratch slots. Each stream resets once drained, so it's empty at
/// every crossing.
const SPILL: &str = r#"  (global $rh (mut i32) (i32.const {CONST}))
  (global $rt (mut i32) (i32.const {CONST}))
  (global $nh (mut i32) (i32.const 0))
  (global $nt (mut i32) (i32.const 0))
  (func $settle
    (if (i32.eq (global.get $rh) (global.get $rt))
      (then (global.set $rh (i32.const {CONST})) (global.set $rt (i32.const {CONST}))))
    (if (i32.eq (global.get $nh) (global.get $nt))
      (then (global.set $nh (i32.const 0)) (global.set $nt (i32.const 0)))))
  (func $nslot (result i32)
    (if (i32.ge_u (global.get $nt) (i32.const {NUMBERS})) (then (call $fault (i32.const 1)) unreachable))
    (global.set $nt (i32.add (global.get $nt) (i32.const 1)))
    (i32.shl (i32.sub (global.get $nt) (i32.const 1)) (i32.const 3)))
  (func $nnext (result i32)
    (if (i32.ge_u (global.get $nh) (global.get $nt)) (then (call $fault (i32.const 0)) unreachable))
    (global.set $nh (i32.add (global.get $nh) (i32.const 1)))
    (i32.shl (i32.sub (global.get $nh) (i32.const 1)) (i32.const 3)))
  (func $push_externref (export "push_externref") (param $v externref)
    (if (i32.le_u (global.get $rt) (i32.const {STACK})) (then (call $fault (i32.const 1)) unreachable))
    (global.set $rt (i32.sub (global.get $rt) (i32.const 1)))
    (table.set $tw (global.get $rt) (local.get $v)))
  (func $pop_externref (export "pop_externref") (result externref)
    (local $v externref)
    (if (i32.le_u (global.get $rh) (global.get $rt)) (then (call $fault (i32.const 0)) unreachable))
    (global.set $rh (i32.sub (global.get $rh) (i32.const 1)))
    (local.set $v (table.get $tw (global.get $rh)))
    (table.set $tw (global.get $rh) (ref.null extern))
    (call $settle)
    (local.get $v))
  (func $spill_reset (export "spill_reset")
    (table.fill $tw (global.get $rt) (ref.null extern) (i32.sub (i32.const {CONST}) (global.get $rt)))
    (global.set $rh (i32.const {CONST}))
    (global.set $rt (i32.const {CONST}))
    (global.set $nh (i32.const 0))
    (global.set $nt (i32.const 0)))
  (func $spill_empty (export "spill_empty") (result i32)
    (i32.and (i32.eq (global.get $rh) (global.get $rt)) (i32.eq (global.get $nh) (global.get $nt))))
  (func $spill_check
    (if (i32.eqz (call $spill_empty)) (then (call $fault (i32.const 2)) unreachable)))
"#;
