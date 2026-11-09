//! Entry points (IR.md §14, §17, §18): JS calling into the language through
//! exports, exported classes and callback trampolines, with the pairing that
//! wraps language handles in JS values.
//!
//! Every entry checks the termination flag, lowers its arguments on a
//! cleanup list released after the call (arguments are borrowed), lifts and
//! frees the result, and maps a language tag escaping into JS (§15).

use std::collections::HashMap;
use std::fmt::Write;
use std::rc::Rc;

use ts_gen_abi::ir::*;
use ts_gen_abi::key::{location, Keyed, Role};
use ts_gen_abi::layout::{native_size, EnumLayout, Host, Kind, Pos, Repr, Signature};
use ts_gen_abi::profile::{Carrier, Encoding, StrCarrier};
use ts_gen_abi::protocol;
use ts_gen_abi::resolve::{Context, Location, Target, TypeDecl};

use crate::form::Space;
use crate::js::{is_ident, js_str, lit, lowers_alloc, prop, Body, Glue};
use crate::plan;
use crate::{unsupported, Error, Result};

/// An entry argument: a wire value as is, or a JS value to lower.
pub enum Arg {
    Raw(String),
    Js(String),
}

struct Base {
    value: String,
}

/// An exported class: its JS class variable and declaration.
pub struct ExportClass<'a> {
    pub var: String,
    pub loc: Location,
    pub class: &'a Class,
}

impl<'a, 'b> Glue<'a, 'b> {
    /// The language export `key` reached through `$E[i]`.
    pub(crate) fn entry(&mut self, key: &str, sig: &Signature) -> usize {
        if let Some(i) = self.entry_memo.get(key) {
            return *i;
        }
        let i = self.entries.len();
        self.entries.push((key.to_string(), sig.clone()));
        self.entry_memo.insert(key.to_string(), i);
        i
    }

    /// Calls the language export `key`. `on_raw` takes the result's wire
    /// (`{r}`) instead of lifting it.
    pub(crate) fn entry_call(
        &mut self,
        b: &mut Body,
        key: &str,
        sig: &Signature,
        args: &[Arg],
        on_raw: Option<&str>,
    ) -> Result<bool> {
        let idx = self.entry(key, sig);
        // A raw argument passes through, so only lowered arguments and
        // results use scratch.
        let gc = sig
            .params
            .iter()
            .zip(args)
            .filter(|(_, a)| matches!(a, Arg::Js(_)))
            .map(|(p, _)| p)
            .chain(&sig.results)
            .any(|p| self.f.has_gc(&p.repr));
        let alloc = sig.params.iter().any(|p| lowers_alloc(&p.repr));
        if self.l.profile.termination_flag {
            b.line("$checkLive();");
        }
        if alloc {
            b.line("const cx = $cx;");
            b.line("const cxm = cx.n;");
        }
        if gc {
            b.line("const m = $S.mark();");
        }
        b.open("try {");
        let mut wire = Vec::new();
        for (p, a) in sig.params.iter().zip(args) {
            match a {
                Arg::Raw(e) => wire.push(e.clone()),
                Arg::Js(v) => {
                    let v = b.bind(v.clone());
                    let m = self.mode();
                    wire.extend(self.lower(b, &p.repr, &v, m)?);
                }
            }
        }
        let call = format!("$E[{idx}]({})", wire.join(", "));
        match sig.results.first() {
            None => b.line(format!("{call};")),
            Some(r) => {
                b.line(format!("const r = {call};"));
                let n = self.f.jsw(&r.repr)?.len();
                let ws: Vec<String> = if n == 1 {
                    vec!["r".into()]
                } else {
                    (0..n).map(|i| b.bind(format!("r[{i}]"))).collect()
                };
                if let Some(stmt) = on_raw {
                    b.line(stmt.replace("{r}", &ws[0]));
                } else if lowers_alloc(&r.repr) {
                    // The result is ours: free it once read (§11).
                    b.open("try {");
                    let v = self.lift(&r.repr, &ws)?;
                    b.line(format!("return {v};"));
                    b.close("} finally {");
                    b.depth += 1;
                    self.release(b, &r.repr, &ws)?;
                    b.close("}");
                } else {
                    let v = self.lift(&r.repr, &ws)?;
                    b.line(format!("return {v};"));
                }
            }
        }
        b.close("} catch (e) {");
        b.depth += 1;
        b.line("throw $outgoing(e);");
        if alloc || gc {
            b.close("} finally {");
            b.depth += 1;
            if alloc {
                b.line("$release(cx, cxm);");
            }
            if gc {
                b.line("$S.reset(m);");
            }
        }
        b.close("}");
        Ok(on_raw.is_none() && !sig.results.is_empty())
    }

    /// Frees what a result `r` owns in the language's space, from its glue
    /// wire values `w`.
    pub(crate) fn release(&mut self, b: &mut Body, r: &Repr, w: &[String]) -> Result<()> {
        match &r.kind {
            Kind::Str {
                carrier: StrCarrier::Linear(e),
                nullable,
            } => {
                let unit = match e {
                    Encoding::Utf8 => 1,
                    Encoding::Wtf16 => 2,
                };
                self.free_seq(b, w, unit, unit, *nullable);
            }
            Kind::Vec {
                elem,
                carrier: Carrier::Linear,
                nullable,
                ..
            } => {
                let size = native_size(*elem);
                self.free_seq(b, w, size, size, *nullable);
            }
            Kind::Array { elem, carrier, .. } if lowers_alloc(r) => {
                let h = self.array_release(elem, Space::of(*carrier))?;
                b.line(format!("{h}({});", w.join(", ")));
            }
            Kind::Tuple { elems, nullable } if lowers_alloc(r) => {
                let mut at = 0;
                if *nullable {
                    b.open(format!("if ({}) {{", w[0]));
                    at = 1;
                }
                for e in elems {
                    let n = self.f.jsw(e)?.len();
                    self.release(b, e, &w[at..at + n])?;
                    at += n;
                }
                if *nullable {
                    b.close("}");
                }
            }
            Kind::Enum { layout, .. } if lowers_alloc(r) => {
                let h = self.enum_release(layout)?;
                b.line(format!("{h}({});", w.join(", ")));
            }
            _ => {}
        }
        Ok(())
    }

    fn free_seq(&self, b: &mut Body, w: &[String], size: u32, align: u32, nullable: bool) {
        let p = self.num(Space::Lang, &w[0]);
        let n = self.num(Space::Lang, &w[1]);
        let null = if nullable {
            format!("{} !== {} && ", w[1], self.null_len(Space::Lang))
        } else {
            String::new()
        };
        b.line(format!(
            "if ({null}{n} !== 0) $L.free({p}, {n} * {size}, {align});"
        ));
    }

    fn array_release(&mut self, elem: &Repr, s: Space) -> Result<String> {
        let key = format!("arelease {s:?} {elem:?}");
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
            b.line(format!(
                "if (len === {} || len === {}) return;",
                g.null_len(s),
                g.zero_len(s)
            ));
            b.line(format!("const n = {};", g.num(s, "len")));
            if e.size > 0 {
                b.line(format!("const base = {};", g.num(s, "p")));
            }
            if lowers_alloc(&elem) {
                b.open("for (let i = 0; i < n; i++) {");
                if e.size > 0 {
                    b.line(format!("const dv = {}.dv();", s.js()));
                    b.line(format!("const o = base + i * {};", e.size));
                }
                if e.refs > 0 {
                    b.line(format!("const x = x0 + i * {};", e.refs));
                }
                let w = g.load_elem(&mut b, s, &e, false);
                g.release(&mut b, &elem, &w)?;
                b.close("}");
            }
            if s == Space::Lang {
                if e.size > 0 {
                    b.line(format!("$L.free(base, n * {}, {});", e.size, e.align));
                }
                if e.refs > 0 {
                    b.line(format!("$L.tfree(x0, n * {});", e.refs));
                }
            }
            Ok(b.finish(&format!("function {name}({})", ps.join(", "))))
        })
    }

    fn enum_release(&mut self, e: &Rc<EnumLayout>) -> Result<String> {
        let key = format!("erelease {}", e.path);
        let e = e.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            let n = g.f.jsw(&Repr {
                ty: e.path.clone(),
                kind: Kind::Enum {
                    layout: e.clone(),
                    nullable: false,
                },
                js_null: false,
            })?;
            let ps: Vec<String> = std::iter::once("arm".to_string())
                .chain((1..n.len()).map(|i| format!("s{}", i - 1)))
                .collect();
            let gc = g.f.enum_has_gc(&e);
            b.open("switch (arm) {");
            let mut region = 0;
            for a in &e.arms {
                let slots = g.arm_wire(&e, a, gc, &mut region)?;
                if !a.payload.iter().any(lowers_alloc) {
                    continue;
                }
                b.open(format!("case {}: {{", a.index));
                let ws: Vec<String> = slots.iter().map(|i| ps[i + 1].clone()).collect();
                let mut at = 0;
                for p in &a.payload {
                    let k = g.f.jsw(p)?.len();
                    g.release(&mut b, p, &ws[at..at + k])?;
                    at += k;
                }
                b.line("break;");
                b.close("}");
            }
            b.close("}");
            Ok(b.finish(&format!("function {name}({})", ps.join(", "))))
        })
    }

    fn zero_len(&self, s: Space) -> &'static str {
        match self.f.ptr(s) {
            crate::form::Jv::I64 => "0n",
            _ => "0",
        }
    }

    // Handles (§14).

    /// The pairing registry of exported class instances. One registry
    /// serves every class, so a subclass instance's wrapper is found
    /// through its base's static type.
    pub(crate) fn class_handles(&mut self) -> Result<String> {
        let key = "handles".to_string();
        if let Some(v) = self.memo.get(&key) {
            return Ok(v.clone());
        }
        let v = "$HC".to_string();
        let linear = self.l.profile.handles == Carrier::Linear;
        self.handles.push(format!(
            "const {v} = new $Handles(\"an exported class instance\", {linear}, null);"
        ));
        self.memo.insert(key, v.clone());
        Ok(v)
    }

    /// The drop export a class's wrappers call when collected, if any.
    fn class_drop(&mut self, class: &str) -> String {
        if self.l.profile.handles == Carrier::Linear && self.l.profile.finalizers {
            let name = self
                .l
                .profile
                .name(&protocol::drop_export(class))
                .to_string();
            if !self.drops.contains(&name) {
                self.drops.push(name.clone());
            }
            js_str(&name)
        } else {
            "null".into()
        }
    }

    /// The pairing registry of the wrappers of a function type the
    /// language provides, with `tw:drop:F` in linear.
    pub(crate) fn fn_handles(&mut self, f: &str) -> String {
        let key = format!("fnhandles {f}");
        if let Some(v) = self.memo.get(&key) {
            return v.clone();
        }
        let linear = self.l.profile.handles == Carrier::Linear;
        let drop = if linear {
            let name = self.l.profile.name(&protocol::drop_export(f)).to_string();
            if !self.drops.contains(&name) {
                self.drops.push(name.clone());
            }
            js_str(&name)
        } else {
            "null".into()
        };
        let v = format!("$H{}", self.handles.len());
        self.handles.push(format!(
            "const {v} = new $Handles({}, {linear}, {drop});",
            js_str(&format!("a `{f}` function"))
        ));
        self.memo.insert(key, v.clone());
        v
    }

    fn export_class(&self, class: &str) -> Option<&ExportClass<'a>> {
        self.export_classes
            .iter()
            .find(|c| location(&c.loc) == class)
    }

    /// The JS class variable of an exported class.
    pub(crate) fn class_var(&mut self, class: &str) -> Result<String> {
        self.export_class(class)
            .map(|c| c.var.clone())
            .ok_or_else(|| Error::Import(format!("`{class}` isn't an exported class")))
    }

    /// A handle as its JS wrapper (§17). A base-less class's wrapper holds
    /// no state of its own, so it's made on first crossing, and again if
    /// collected. One with an imported base was made by `tw:new`, so an
    /// unpaired handle of it is a `TypeError`.
    pub(crate) fn wrap_class(&mut self, class: &str, h: &str) -> Result<String> {
        let reg = self.class_handles()?;
        let c = self
            .export_class(class)
            .ok_or_else(|| Error::Import(format!("`{class}` isn't an exported class")))?;
        if self.l.imported_base(&c.loc) {
            return Ok(format!(
                "{reg}.paired({h}, {})",
                js_str(&format!("`{class}`"))
            ));
        }
        let var = c.var.clone();
        let drop = self.class_drop(class);
        Ok(format!(
            "{reg}.wrap({h}, () => Object.create({var}.prototype), {drop})"
        ))
    }

    /// An exported class's base: an exported class's JS class, or an
    /// imported class's JS value.
    fn class_base(&mut self, class: &str) -> Result<Option<Base>> {
        let Some(c) = self.export_class(class) else {
            return Ok(None);
        };
        let (Some(parent), loc) = (c.class.extends.first(), c.loc.clone()) else {
            return Ok(None);
        };
        let cx = Context {
            module: None,
            namespace: loc.path[..loc.path.len() - 1]
                .iter()
                .map(String::as_str)
                .collect(),
            ..Context::default()
        };
        let path = parent.path.clone();
        let Some((ploc, TypeDecl::Class(_))) = self.l.keys.symbols.resolve_decl(&path, &cx) else {
            return Err(Error::Import(format!(
                "the base of `{class}` doesn't name a class"
            )));
        };
        if let Some(e) = self.export_class(&location(&ploc)) {
            return Ok(Some(Base {
                value: e.var.clone(),
            }));
        }
        let value = self.decl_value(&ploc)?;
        Ok(Some(Base { value }))
    }

    /// The name of a function type, as its protocol operations spell it.
    pub(crate) fn fn_name(r: &Repr) -> String {
        match &r.kind {
            Kind::Fn { nullable: true, .. } => {
                let t = r.ty.strip_suffix('?').unwrap_or(&r.ty);
                t.strip_prefix('(')
                    .and_then(|t| t.strip_suffix(')'))
                    .unwrap_or(t)
                    .to_string()
            }
            _ => r.ty.clone(),
        }
    }

    /// The helper making (or reusing) the JS function that wraps a handle
    /// of a function type the language provides; calls go through its
    /// trampoline `tw:call:F` (§18).
    pub(crate) fn fn_wrap(&mut self, r: &Repr) -> Result<String> {
        let f = Self::fn_name(r);
        let key = format!("fnwrap {f}");
        let r = r.clone();
        self.helper(key, move |g, name| {
            let reg = g.fn_handles(&f);
            let sig = g.l.fn_op(&r)?;
            let rest = matches!(&r.kind, Kind::Fn { sig, .. } if sig.rest);
            let tramp = g.trampoline(&f, &reg, &sig, rest)?;
            let mut b = Body::default();
            b.open(format!("return {reg}.wrap(env, () => {{"));
            let ps = if rest {
                "...a".to_string()
            } else {
                let n = sig
                    .params
                    .iter()
                    .filter(|p| matches!(p.role, Pos::Param(_)))
                    .count();
                (0..n)
                    .map(|i| format!("a{i}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let args = if rest { "a".to_string() } else { ps.clone() };
            let sep = if args.is_empty() { "" } else { ", " };
            b.line(format!(
                "const f = function ({ps}) {{ return {tramp}(f, env{sep}{args}); }};"
            ));
            b.line("return f;");
            b.close("});");
            Ok(b.finish(&format!("function {name}(env)")))
        })
    }

    fn trampoline(&mut self, f: &str, reg: &str, sig: &Signature, rest: bool) -> Result<String> {
        let key = format!("tramp {f}");
        let f = f.to_string();
        let reg = reg.to_string();
        let sig = sig.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            let export = g.l.profile.name(&protocol::call_export(&f)).to_string();
            let last = sig.params.len().saturating_sub(2);
            let args: Vec<Arg> = sig
                .params
                .iter()
                .map(|p| match p.role {
                    Pos::Param(i) if rest && i == last => Arg::Js(format!("a.slice({i})")),
                    Pos::Param(i) if rest => Arg::Js(format!("a[{i}]")),
                    Pos::Param(i) => Arg::Js(format!("a{i}")),
                    _ => Arg::Raw("env".into()),
                })
                .collect();
            // A callback returning a future rejects it on any synchronous
            // failure, as exports do (§16).
            let fold = sig.results.first().is_some_and(|r| {
                matches!(
                    r.repr.kind,
                    Kind::Host {
                        of: Host::Future { .. },
                        ..
                    }
                )
            });
            if fold {
                b.open("try {");
            }
            b.line(format!(
                "const env = f[$flagged] === undefined ? h : {reg}.call(f);"
            ));
            g.entry_call(&mut b, &export, &sig, &args, None)?;
            if fold {
                b.close("} catch (e) {");
                b.depth += 1;
                b.line("return Promise.reject(e);");
                b.close("}");
            }
            let ps = if rest {
                "a".to_string()
            } else {
                let n = sig
                    .params
                    .iter()
                    .filter(|p| matches!(p.role, Pos::Param(_)))
                    .count();
                (0..n)
                    .map(|i| format!("a{i}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let sep = if ps.is_empty() { "" } else { ", " };
            Ok(b.finish(&format!("function {name}(f, h{sep}{ps})")))
        })
    }

    // Errors leaving the language (§15).

    /// `$outgoing(e)`: a language tag as its JS error; anything else as is.
    pub fn outgoing(&mut self) -> Result<String> {
        let mut b = Body::default();
        b.open("if (e instanceof WebAssembly.Exception) {");
        let tags = self.tags.clone();
        for (_, var, layout) in &tags {
            let h = self.error_out(layout)?;
            b.line(format!(
                "if (e.is({var})) return {h}(e.getArg({var}, 0), e.getArg({var}, 1));"
            ));
        }
        b.close("}");
        b.line("return e;");
        Ok(b.finish("function $outgoing(e)"))
    }

    /// Constructs an arm's first class with the message, satisfying its
    /// predicates.
    pub(crate) fn error_out(&mut self, e: &Rc<EnumLayout>) -> Result<String> {
        let key = format!("eout {}", e.path);
        let e = e.clone();
        self.helper(key, move |g, name| {
            let mut b = Body::default();
            b.line("const msg = m == null ? \"\" : String(m);");
            b.open("switch (arm) {");
            for a in &e.arms {
                let pred = a.js.classes().and_then(<[_]>::first);
                let Some(p) = pred else { continue };
                let c = g.error_class(&p.class)?;
                let props: Vec<String> = p
                    .props
                    .iter()
                    .map(|(k, l)| format!("{}: {}", js_str(k), lit(l)))
                    .collect();
                b.line(format!(
                    "case {}: return $construct({c}, msg, {{ {} }});",
                    a.index,
                    props.join(", ")
                ));
            }
            b.close("}");
            b.line("return new Error(msg);");
            Ok(b.finish(&format!("function {name}(arm, m)")))
        })
    }

    // Exports (§17).

    /// The exported classes, with their JS class variables.
    pub fn collect_export_classes(ir: &'a Ir) -> Vec<ExportClass<'a>> {
        fn walk<'a>(
            items: &'a [Decl],
            path: &mut Vec<String>,
            exported: bool,
            out: &mut Vec<ExportClass<'a>>,
        ) {
            for d in items {
                match d {
                    Decl::Namespace(n) => {
                        path.push(n.name.clone());
                        walk(&n.items, path, exported || n.export, out);
                        path.pop();
                    }
                    Decl::Class(c) if c.export || exported => {
                        let mut p = path.clone();
                        p.push(c.name.clone());
                        out.push(ExportClass {
                            var: format!("$C{}", out.len()),
                            loc: Location {
                                module: None,
                                path: p,
                            },
                            class: c,
                        });
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&ir.items, &mut Vec::new(), false, &mut out);
        out
    }

    /// The JS of every export: the classes, the namespace objects and the
    /// ESM export list.
    pub fn exports(&mut self) -> Result<String> {
        let mut keys: Vec<Keyed<'a>> = Vec::new();
        for k in self.l.keys.members() {
            if k.role == Role::Export {
                let variants = self.mono_variants(&k)?;
                keys.extend(variants);
            }
        }
        let mut out = String::new();
        // Namespace path → (JS name → object member source).
        let mut tree: Tree = Tree::default();
        let ncls = self.export_classes.len();
        for ci in 0..ncls {
            let (var, loc) = {
                let c = &self.export_classes[ci];
                (c.var.clone(), c.loc.clone())
            };
            let members: Vec<Keyed<'a>> = keys
                .iter()
                .filter(|k| k.owner.is_some() && self.owner_loc(k).as_ref() == Some(&loc))
                .cloned()
                .collect();
            let src = self.class_src(ci, &members)?;
            let _ = writeln!(out, "{src}\n");
            let (ns, name) = loc.path.split_at(loc.path.len() - 1);
            tree.at(ns)
                .entries
                .push((name[0].clone(), Entry::Value(var)));
        }
        // Free functions, grouped by namespace and JS name.
        let mut groups: Vec<(Vec<String>, String, MemberKind, Vec<Keyed<'a>>)> = Vec::new();
        for k in keys.iter().filter(|k| k.owner.is_none()) {
            let ns: Vec<String> = k.cx.namespace.iter().map(|s| s.to_string()).collect();
            let js = js_name(k.member);
            let cat = category(&k.member.kind);
            match groups
                .iter_mut()
                .find(|(n, j, c, _)| *n == ns && *j == js && category(c) == cat)
            {
                Some(g) => g.3.push(k.clone()),
                None => groups.push((ns, js, k.member.kind.clone(), vec![k.clone()])),
            }
        }
        for (ns, js, kind, arms) in groups {
            let src = self.export_fn(&js, &kind, &arms, ns.is_empty())?;
            tree.at(&ns).entries.push((js, Entry::Member(src)));
        }
        // Namespace objects, innermost first, then the ESM export list.
        let mut n = 0;
        let mut esm = Vec::new();
        for (name, e) in tree.entries {
            let v = match e {
                Entry::Value(v) => v,
                Entry::Member(src) => {
                    let v = format!("$X{n}");
                    n += 1;
                    let _ = writeln!(out, "const {v} = {src};\n");
                    v
                }
            };
            esm.push((name, v));
        }
        for (name, child) in tree.children {
            let v = emit_ns(child, &mut out, &mut n);
            esm.push((name, v));
        }
        let list: Vec<String> = esm
            .iter()
            .map(|(name, v)| {
                if name == v {
                    v.clone()
                } else if is_export_ident(name) {
                    format!("{v} as {name}")
                } else {
                    format!("{v} as {}", js_str(name))
                }
            })
            .collect();
        if !list.is_empty() {
            let _ = writeln!(out, "export {{ {} }};", list.join(", "));
        }
        Ok(out)
    }

    /// A generic export as the language provides it (§13): its erased key
    /// when exported, otherwise each mono variant it exports
    /// (`util.id<u32>(T)`), in the order of the concrete bound when there
    /// is one. Variants dispatch on their arguments like overloads.
    fn mono_variants(&mut self, k: &Keyed<'a>) -> Result<Vec<Keyed<'a>>> {
        if k.member.tparams.is_empty() || self.lang_exports.contains(&k.key) {
            return Ok(vec![k.clone()]);
        }
        let mut out: Vec<(Keyed<'a>, Vec<Type>)> = Vec::new();
        for name in &self.lang_exports {
            let Some((base, list)) = plan::mono(name) else {
                continue;
            };
            if base != k.key {
                continue;
            }
            let what = || format!("the export `{name}`");
            let targs = plan::mono_args(&list, &what)?;
            plan::bounds(self.l, k, &targs, &what)?;
            let mut v = k.clone();
            v.key = name.clone();
            out.push((v, targs));
        }
        if out.is_empty() {
            // The erased key, reported as missing by the export check.
            return Ok(vec![k.clone()]);
        }
        if k.owner.is_some_and(|c| !c.tparams.is_empty()) {
            return unsupported(format!(
                "`{}`: mono variants of a generic class's members (an instance's type arguments aren't known to JS)",
                k.key
            ));
        }
        let rank = |targs: &[Type]| -> Vec<usize> {
            k.member
                .tparams
                .iter()
                .zip(targs)
                .map(|(p, t)| match &p.bound {
                    Some(Bound::In(set)) => set.iter().position(|b| b == t).unwrap_or(usize::MAX),
                    _ => 0,
                })
                .collect()
        };
        out.sort_by_key(|(_, t)| rank(t));
        Ok(out
            .into_iter()
            .map(|(v, targs)| {
                self.mono_targs.insert(v.key.clone(), targs);
                v
            })
            .collect())
    }

    fn owner_loc(&self, k: &Keyed<'_>) -> Option<Location> {
        let c = k.owner?;
        match self
            .l
            .keys
            .symbols
            .resolve(&Path::new([c.name.as_str()]), &k.cx)
        {
            Some(Target::Decl(loc, _)) => Some(loc.clone()),
            _ => None,
        }
    }

    /// An exported class as a JS class (§17).
    fn class_src(&mut self, ci: usize, members: &[Keyed<'a>]) -> Result<String> {
        let (var, canonical, cname, through_js) = {
            let c = &self.export_classes[ci];
            let through_js = self.l.imported_base(&c.loc);
            (
                c.var.clone(),
                location(&c.loc),
                c.class.name.clone(),
                through_js,
            )
        };
        let reg = self.class_handles()?;
        let drop = self.class_drop(&canonical);
        let base = self.class_base(&canonical)?;
        let mut s = match &base {
            Some(b) => format!("class {var} extends {} {{\n", b.value),
            None => format!("class {var} {{\n"),
        };
        // Constructors.
        let ctors: Vec<Keyed<'a>> = members
            .iter()
            .filter(|k| matches!(k.member.kind, MemberKind::New(_)))
            .cloned()
            .collect();
        let mut b = Body::default();
        b.depth = 1;
        if ctors.is_empty() {
            b.line(format!(
                "throw new TypeError({});",
                js_str(&format!("`{cname}` has no constructor"))
            ));
        } else {
            let label = format!("new {cname}");
            let has_base = base.is_some();
            let reg2 = reg.clone();
            // A `tw:new` arm, taken before `super` can construct another.
            let pick = through_js && ctors.len() > 1;
            if pick {
                b.line("const arm = $newArm;");
            }
            if through_js {
                b.line("$newArm = undefined;");
            }
            self.dispatch(&mut b, &label, &ctors, pick, &mut |g, b, k, sig, binds| {
                if has_base {
                    let sup = k.member.meta.attrs.js.super_args().map(<[_]>::to_vec);
                    let args = match sup {
                        Some(t) => {
                            let map: HashMap<&str, String> =
                                binds.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
                            g.template(b, &t, &map, &k.cx)?.join(", ")
                        }
                        None => "...a".into(),
                    };
                    b.line(format!("super({args});"));
                }
                let args = entry_args(sig, k.member, binds, None);
                let on_raw = format!("{reg2}.pair(this, {{r}}, {drop});");
                g.entry_call(b, &k.key, sig, &args, Some(&on_raw))
            })?;
        }
        let _ = writeln!(s, "{}", class_member(b.finish("  constructor(...a)")));
        // Methods and accessors, grouped by JS entry.
        let mut groups: Vec<(bool, String, MemberKind, Vec<Keyed<'a>>)> = Vec::new();
        for k in members {
            if matches!(k.member.kind, MemberKind::New(_)) {
                continue;
            }
            let js = js_name(k.member);
            let st = k.member.is_static;
            let cat = category(&k.member.kind);
            match groups
                .iter_mut()
                .find(|(s, j, c, _)| *s == st && *j == js && category(c) == cat)
            {
                Some(g) => g.3.push(k.clone()),
                None => groups.push((st, js, k.member.kind.clone(), vec![k.clone()])),
            }
        }
        for (is_static, js, kind, arms) in groups {
            let symbol = arms[0]
                .member
                .meta
                .attrs
                .js
                .symbol()
                .map(|s| format!("[Symbol.{s}]"));
            let name = symbol.unwrap_or_else(|| method_key(&js));
            let prefix = if is_static { "static " } else { "" };
            let mut b = Body::default();
            b.depth = 1;
            let head = match kind {
                MemberKind::Get(_) => format!("  {prefix}get {name}()"),
                MemberKind::Set(_) => {
                    b.line("const a = [v];");
                    format!("  {prefix}set {name}(v)")
                }
                _ => format!("  {prefix}{name}(...a)"),
            };
            if matches!(kind, MemberKind::Get(_)) {
                b.line("const a = [];");
            }
            let label = format!("{cname}.{js}");
            let reg2 = reg.clone();
            let var2 = var.clone();
            let cname2 = cname.clone();
            self.dispatch(&mut b, &label, &arms, false, &mut |g, b, k, sig, binds| {
                let handle = (!k.member.is_static)
                    .then(|| format!("{reg2}.unwrap(this, {var2}, {})", js_str(&cname2)));
                let args = entry_args(sig, k.member, binds, handle);
                g.entry_call(b, &k.key, sig, &args, None)
            })?;
            let _ = writeln!(s, "{}", class_member(b.finish(&head)));
        }
        s.push('}');
        Ok(s)
    }

    /// A namespace member or top-level function export, as an object
    /// member (or a function expression at the top level).
    fn export_fn(
        &mut self,
        js: &str,
        kind: &MemberKind,
        arms: &[Keyed<'a>],
        top: bool,
    ) -> Result<String> {
        let mut b = Body::default();
        let head = match kind {
            MemberKind::Get(_) => {
                b.line("const a = [];");
                format!("get {}()", method_key(js))
            }
            MemberKind::Set(_) => {
                b.line("const a = [v];");
                format!("set {}(v)", method_key(js))
            }
            _ if top => format!("function {}(...a)", fn_ident(js)),
            _ => format!("{}(...a)", method_key(js)),
        };
        self.dispatch(&mut b, js, arms, false, &mut |g, b, k, sig, binds| {
            let args = entry_args(sig, k.member, binds, None);
            g.entry_call(b, &k.key, sig, &args, None)
        })?;
        Ok(b.finish(&head))
    }

    /// Incoming dispatch over the overloads sharing one JS entry (§7):
    /// arity, then constant patterns, then type tests in declaration order.
    /// A single arm still checks its pattern, even with validation off.
    #[allow(clippy::type_complexity)]
    fn dispatch(
        &mut self,
        b: &mut Body,
        label: &str,
        arms: &[Keyed<'a>],
        pick: bool,
        body: &mut dyn FnMut(
            &mut Self,
            &mut Body,
            &Keyed<'a>,
            &Signature,
            &[(String, String)],
        ) -> Result<bool>,
    ) -> Result<()> {
        let single = arms.len() == 1;
        // An export returning a future folds its argument failures into
        // the promise's rejection (§16).
        let fold = arms
            .iter()
            .any(|k| matches!(k.member.result, Type::Future(..)));
        if fold {
            b.open("try {");
        }
        let mut seen: Vec<(String, &str)> = Vec::new();
        for k in arms {
            let targs = self.mono_targs.get(&k.key).cloned().unwrap_or_default();
            let sig = self.l.member(k, &targs)?;
            let m = k.member;
            let template: Vec<Telem> = match m.meta.attrs.js.args().map(<[_]>::to_vec) {
                Some(t) => t,
                None => m
                    .params
                    .iter()
                    .map(|p| Telem::Param(p.name.clone()))
                    .collect(),
            };
            let mut conds = Vec::new();
            let mut binds: Vec<(String, String)> = Vec::new();
            let spread = matches!(template.last(), Some(Telem::Spread(_)));
            let n = template.len();
            if spread {
                conds.push(format!("a.length >= {}", n - 1));
            } else {
                let required = template
                    .iter()
                    .rposition(|t| !matches!(t, Telem::Undefined | Telem::Fill { .. }))
                    .map_or(0, |i| i + 1);
                // Past the parameters, only `undefined`s: WebIDL drops
                // trailing undefined arguments, and workerd calls a Durable
                // Object's `fetch(request)` with `(request, undefined,
                // undefined)`.
                if required > 0 {
                    conds.push(format!("a.length >= {required}"));
                }
                conds.push(format!("$upTo(a, {n})"));
            }
            for (i, t) in template.iter().enumerate() {
                match t {
                    Telem::Param(p) => binds.push((p.clone(), format!("a[{i}]"))),
                    Telem::Spread(p) => binds.push((p.clone(), format!("a.slice({i})"))),
                    Telem::Lit(l) => conds.push(format!("a[{i}] === {}", lit(l))),
                    Telem::Undefined => conds.push(format!("a[{i}] === undefined")),
                    // A fill binds its parameter to the object, or `null`
                    // when absent; its fields are patterns on that object.
                    Telem::Fill { param, fields } => {
                        binds.push((param.clone(), format!("a[{i}]")));
                        let mut tests = Vec::new();
                        for (f, x) in fields {
                            let v = prop(&format!("a[{i}]"), f);
                            match x {
                                Expr::Ref { path, call: None }
                                    if path.len() == 1
                                        && m.params.iter().any(|p| p.name == path[0]) =>
                                {
                                    let v = if is_ident(f) {
                                        format!("a[{i}]?.{f}")
                                    } else {
                                        format!("a[{i}]?.[{}]", js_str(f))
                                    };
                                    binds.push((path[0].clone(), v))
                                }
                                _ => tests.push(format!(
                                    "{v} === {}",
                                    self.expr(x, &HashMap::new(), &k.cx)?
                                )),
                            }
                        }
                        if !tests.is_empty() {
                            conds.push(format!("(a[{i}] == null || {})", tests.join(" && ")));
                        }
                    }
                    Telem::Expr(x) => conds.push(format!(
                        "a[{i}] === {}",
                        self.expr(x, &HashMap::new(), &k.cx)?
                    )),
                }
            }
            if !single {
                for (pi, p) in m.params.iter().enumerate() {
                    let Some((_, v)) = binds.iter().find(|(n, _)| *n == p.name) else {
                        continue;
                    };
                    let pos = sig.params.iter().find(|x| x.role == Pos::Param(pi));
                    if let Some(pos) = pos {
                        let repr = pos.repr.clone();
                        let t = self.test(&repr, v)?;
                        if t != "true" {
                            conds.push(t);
                        }
                    }
                }
            }
            let cond = conds.join(" && ");
            if let Some((_, other)) = seen.iter().find(|(c, _)| *c == cond) {
                return unsupported(format!(
                    "`{other}` and `{}` can't be told apart by their arguments",
                    k.key
                ));
            }
            seen.push((cond.clone(), &k.key));
            // `tw:new` names its constructor, which may not be the first
            // arm its arguments match.
            let cond = if pick {
                let key = js_str(&k.key);
                format!("(arm ?? {key}) === {key} && {cond}")
            } else {
                cond
            };
            b.open(format!("if ({cond}) {{"));
            if !body(self, b, k, &sig, &binds)? {
                b.line("return;");
            }
            b.close("}");
        }
        let msg = if single {
            format!("`{label}` doesn't match its arguments")
        } else {
            format!("no overload of `{label}` matches its arguments")
        };
        b.line(format!("throw new TypeError({});", js_str(&msg)));
        if fold {
            b.close("} catch (e) {");
            b.depth += 1;
            b.line("return Promise.reject(e);");
            b.close("}");
        }
        Ok(())
    }
}

/// An entry's arguments by position: the JS receiver, the handle, and the
/// parameters the template bound by name.
fn entry_args(
    sig: &Signature,
    m: &Member,
    binds: &[(String, String)],
    handle: Option<String>,
) -> Vec<Arg> {
    sig.params
        .iter()
        .map(|p| match p.role {
            Pos::This => Arg::Raw("this".into()),
            Pos::Handle => Arg::Raw(handle.clone().unwrap_or_default()),
            Pos::Param(i) => Arg::Js(
                binds
                    .iter()
                    .find(|(n, _)| *n == m.params[i].name)
                    .map_or_else(|| "undefined".into(), |(_, v)| v.clone()),
            ),
            _ => Arg::Js(String::new()),
        })
        .collect()
}

/// A class body member, its closing brace indented.
fn class_member(src: String) -> String {
    match src.strip_suffix("\n}") {
        Some(s) => format!("{s}\n  }}"),
        None => src,
    }
}

fn js_name(m: &Member) -> String {
    m.meta
        .attrs
        .js
        .rename()
        .map(String::from)
        .or_else(|| m.kind.name().map(String::from))
        .unwrap_or_default()
}

fn category(k: &MemberKind) -> u8 {
    match k {
        MemberKind::Method(_) => 0,
        MemberKind::Get(_) => 1,
        MemberKind::Set(_) => 2,
        MemberKind::New(_) => 3,
    }
}

fn is_export_ident(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn method_key(s: &str) -> String {
    if is_export_ident(s) {
        s.to_string()
    } else {
        format!("[{}]", js_str(s))
    }
}

fn fn_ident(s: &str) -> String {
    if is_export_ident(s) {
        s.to_string()
    } else {
        "exported".into()
    }
}

enum Entry {
    Value(String),
    Member(String),
}

#[derive(Default)]
struct Tree {
    entries: Vec<(String, Entry)>,
    children: Vec<(String, Tree)>,
}

impl Tree {
    fn at(&mut self, path: &[String]) -> &mut Tree {
        let mut t = self;
        for seg in path {
            let i = match t.children.iter().position(|(n, _)| n == seg) {
                Some(i) => i,
                None => {
                    t.children.push((seg.clone(), Tree::default()));
                    t.children.len() - 1
                }
            };
            t = &mut t.children[i].1;
        }
        t
    }
}

/// Emits a namespace object (children first) and returns its variable.
fn emit_ns(t: Tree, out: &mut String, n: &mut usize) -> String {
    let mut fields = Vec::new();
    for (name, child) in t.children {
        let v = emit_ns(child, out, n);
        fields.push(format!("  {}: {v},", method_key(&name)));
    }
    for (name, e) in t.entries {
        match e {
            Entry::Value(v) => fields.push(format!("  {}: {v},", method_key(&name))),
            Entry::Member(src) => {
                let src = src.replace('\n', "\n  ");
                fields.push(format!("  {src},"));
            }
        }
    }
    let v = format!("$N{n}");
    *n += 1;
    let _ = writeln!(out, "const {v} = {{\n{}\n}};\n", fields.join("\n"));
    v
}
