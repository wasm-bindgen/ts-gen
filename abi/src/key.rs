//! Canonical type spelling, member keys and import modules (IR.md §3, §5,
//! §6, §17).
//!
//! A key is the Wasm import (or export) name of a member: its declaration's
//! full path, `#` or `.`, the member name, then the canonical parameter
//! types. Type paths are printed resolved, so a key never depends on where
//! the member is written.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::Write;

use crate::ir::*;
use crate::protocol;
use crate::resolve::{Context, Location, Symbols, Target};
use crate::text::{name, quote};

pub struct Keys<'a> {
    ir: &'a Ir,
    pub symbols: Symbols<'a>,
    /// The aliases [`Keys::ty`] is spelling the expansion of.
    expanding: RefCell<Vec<Location>>,
}

/// Whether the language imports a member or exports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// Imported from this module field: a module specifier, or
    /// [`protocol::GLOBAL`].
    Import(String),
    Export,
}

/// A declared member with its key, as [`Keys::members`] lists them.
#[derive(Clone, Debug)]
pub struct Keyed<'a> {
    pub role: Role,
    pub key: String,
    pub member: &'a Member,
    /// The enclosing class or interface; `None` for a top-level function.
    pub owner: Option<&'a Class>,
    /// Where the member's types resolve, with its type parameters in scope.
    pub cx: Context<'a>,
}

/// The import module field of a declaration (§5).
pub fn import_module(module: Option<&str>) -> &str {
    module.unwrap_or(protocol::GLOBAL)
}

/// A declaration's location in canonical spelling: `"spec".Path` inside a
/// module block, the dotted path otherwise.
pub fn location(loc: &Location) -> String {
    let path: Vec<String> = loc.path.iter().map(|s| name(s)).collect();
    match &loc.module {
        Some(m) => format!("{}.{}", quote(m), path.join(".")),
        None => path.join("."),
    }
}

impl<'a> Keys<'a> {
    pub fn new(ir: &'a Ir) -> Keys<'a> {
        Keys::with_libs(ir, &[])
    }

    /// Keys of `ir`'s own members, with paths resolved against its libs.
    pub fn with_libs(ir: &'a Ir, libs: &[&'a Ir]) -> Keys<'a> {
        Keys {
            ir,
            symbols: Symbols::with_libs(ir, libs),
            expanding: RefCell::new(Vec::new()),
        }
    }

    /// A path in canonical spelling: resolved to its declaration, or as
    /// written when it names a type parameter or nothing declared.
    pub fn path(&self, p: &Path, cx: &Context<'_>) -> String {
        match self.symbols.resolve(p, cx) {
            Some(Target::Decl(loc, _)) => location(loc),
            _ => crate::text::print_path(p),
        }
    }

    fn list(&self, ts: &[Type], cx: &Context<'_>) -> String {
        let ts: Vec<String> = ts.iter().map(|t| self.ty(t, cx)).collect();
        ts.join(",")
    }

    fn operand(&self, t: &Type, cx: &Context<'_>) -> String {
        match t {
            Type::Fn(f) if f.result != Type::Unit || f.throws.is_some() => {
                format!("({})", self.ty(t, cx))
            }
            _ => self.ty(t, cx),
        }
    }

    fn whole_vec(&self, t: &Type, cx: &Context<'_>) -> String {
        match t {
            Type::Nullable(..)
            | Type::Vec(_)
            | Type::Fn(_)
            | Type::View { .. }
            | Type::MutVec(_) => {
                format!("({})[]", self.ty(t, cx))
            }
            _ => format!("{}[]", self.ty(t, cx)),
        }
    }

    fn throws(&self, t: &Option<Path>, cx: &Context<'_>) -> String {
        match t {
            Some(p) => format!(" throws {}", self.path(p, cx)),
            None => String::new(),
        }
    }

    /// A type in canonical spelling (§3): `Map<Str,f64>`, `arr<(u32,str)>`,
    /// `fn(Event)->unit throws KvError`. An alias spells as its expansion,
    /// but for a reference back into an alias being expanded, which spells
    /// as the alias (`fn(Next<f64>)` for `Next<f64>`).
    pub fn ty(&self, t: &Type, cx: &Context<'_>) -> String {
        match t {
            Type::Unit => "unit".into(),
            Type::Never => "never".into(),
            Type::Scalar(s) => s.name().into(),
            Type::Str => "str".into(),
            Type::Any => "Any".into(),
            Type::ScalarBox(s) => s.box_name().into(),
            Type::StrBox => "Str".into(),
            Type::BigInt => "bigint".into(),
            Type::BigIntBox => "BigInt".into(),
            Type::UnitBox => "Unit".into(),
            Type::Vec(t) => format!("{}[]", self.operand(t, cx)),
            Type::VecBox(t) => format!("Vec<{}>", self.ty(t, cx)),
            Type::MutVec(t) => format!("mut {}", self.whole_vec(t, cx)),
            Type::Array(t) => format!("arr<{}>", self.ty(t, cx)),
            Type::ArrBox(t) => format!("Arr<{}>", self.ty(t, cx)),
            Type::Tuple(ts) if ts.len() == 1 => format!("({},)", self.ty(&ts[0], cx)),
            Type::Tuple(ts) => format!("({})", self.list(ts, cx)),
            Type::TupleBox(ts) => format!("Tuple<{}>", self.list(ts, cx)),
            Type::Future(t, e) if e.is_err() => format!("future<{}>", self.ty(t, cx)),
            Type::Future(t, e) => format!("future<{},{}>", self.ty(t, cx), self.path(e, cx)),
            Type::Fn(f) => {
                let mut ps: Vec<String> = f.params.iter().map(|t| self.ty(t, cx)).collect();
                if let (true, Some(last)) = (f.rest, ps.last_mut()) {
                    last.insert_str(0, "...");
                }
                let mut s = format!("fn({})", ps.join(","));
                if f.result != Type::Unit || f.throws.is_some() {
                    let absorbs = matches!(&f.result, Type::Fn(g) if g.throws.is_none());
                    if absorbs && f.throws.is_some() {
                        let _ = write!(s, "->({})", self.ty(&f.result, cx));
                    } else {
                        let _ = write!(s, "->{}", self.ty(&f.result, cx));
                    }
                }
                s.push_str(&self.throws(&f.throws, cx));
                s
            }
            Type::View { mutable, elem } => {
                let m = if *mutable { "mut " } else { "" };
                format!("*{m}{}", self.whole_vec(elem, cx))
            }
            Type::Nullable(t, _) => format!("{}?", self.operand(t, cx)),
            Type::Named(r) => {
                if let Some((loc, t)) = self.symbols.unfold(r, cx) {
                    if !self.expanding.borrow().contains(&loc) {
                        self.expanding.borrow_mut().push(loc);
                        let s = self.ty(&t, cx);
                        self.expanding.borrow_mut().pop();
                        return s;
                    }
                }
                let mut s = self.path(&r.path, cx);
                if !r.args.is_empty() {
                    let _ = write!(s, "<{}>", self.list(&r.args, cx));
                }
                s
            }
        }
    }

    /// A member's key (§6). `owner` is the full path of the enclosing class,
    /// or of the enclosing namespaces for a top-level member. `targs` are a
    /// mono-concretized variant's type arguments, the class's then the
    /// member's; parameters keep their declared spelling.
    pub fn member(
        &self,
        owner: &[&str],
        in_class: bool,
        m: &Member,
        targs: &[Type],
        cx: &Context<'_>,
    ) -> String {
        let mname = match &m.kind {
            MemberKind::Method(n) => name(n),
            MemberKind::Get(n) => format!("get:{}", name(n)),
            MemberKind::Set(n) => format!("set:{}", name(n)),
            MemberKind::New(None) => "new".into(),
            MemberKind::New(Some(n)) => format!("new:{}", name(n)),
        };
        let instance = in_class && !m.is_static && !matches!(m.kind, MemberKind::New(_));
        let mut s = String::new();
        if !owner.is_empty() {
            let segs: Vec<String> = owner.iter().map(|s| name(s)).collect();
            s.push_str(&segs.join("."));
            s.push(if instance { '#' } else { '.' });
        }
        s.push_str(&mname);
        if !targs.is_empty() {
            let _ = write!(s, "<{}>", self.list(targs, cx));
        }
        let params: Vec<String> = m.params.iter().map(|p| self.ty(&p.ty, cx)).collect();
        let _ = write!(s, "({})", params.join(","));
        s
    }

    /// Every declared member with its key and role, in declaration order.
    /// Statics and constructors of a module declaration that the block
    /// doesn't export have no import (§5), and are left out.
    pub fn members(&self) -> Vec<Keyed<'a>> {
        let mut out = Vec::new();
        self.walk(&self.ir.items, &Context::default(), false, None, &mut out);
        out
    }

    fn walk(
        &self,
        items: &'a [Decl],
        cx: &Context<'a>,
        exported: bool,
        module_exports: Option<&HashSet<&'a str>>,
        out: &mut Vec<Keyed<'a>>,
    ) {
        let reachable = |top: &str| match (cx.module, module_exports) {
            (None, _) => true,
            (Some(_), Some(names)) => names.contains(top),
            (Some(_), None) => false,
        };
        let module_field = import_module(cx.module).to_string();
        for d in items {
            match d {
                Decl::Namespace(n) => {
                    let mut inner = cx.clone();
                    inner.namespace.push(&n.name);
                    self.walk(&n.items, &inner, exported || n.export, module_exports, out);
                }
                Decl::Module(m) => {
                    let names: HashSet<&str> = m
                        .exports
                        .iter()
                        .flatten()
                        .map(|e| e.name.as_str())
                        .collect();
                    let inner = Context {
                        module: Some(&m.spec),
                        ..Context::default()
                    };
                    self.walk(&m.items, &inner, false, Some(&names), out);
                }
                Decl::Class(c) => {
                    let mut owner = cx.namespace.clone();
                    owner.push(&c.name);
                    let class_cx = cx.with_class(c);
                    let statics = reachable(owner[0]);
                    for m in &c.members {
                        let instance = !m.is_static && !matches!(m.kind, MemberKind::New(_));
                        // A constructor through another value reaches that.
                        let through = m.meta.attrs.js.new_value().is_some();
                        if !instance && !statics && !through {
                            continue;
                        }
                        let mcx = class_cx.with_tparams(&m.tparams);
                        out.push(Keyed {
                            role: if c.export || exported {
                                Role::Export
                            } else {
                                Role::Import(module_field.clone())
                            },
                            key: self.member(&owner, true, m, &[], &mcx),
                            member: m,
                            owner: Some(c),
                            cx: mcx,
                        });
                    }
                }
                Decl::Member(m) => {
                    let top = cx
                        .namespace
                        .first()
                        .copied()
                        .or(m.kind.name())
                        .unwrap_or("");
                    if !reachable(top) {
                        continue;
                    }
                    let mcx = cx.with_tparams(&m.tparams);
                    out.push(Keyed {
                        role: if exported || m.export {
                            Role::Export
                        } else {
                            Role::Import(module_field.clone())
                        },
                        key: self.member(&cx.namespace, false, m, &[], &mcx),
                        member: m,
                        owner: None,
                        cx: mcx,
                    });
                }
                Decl::Alias(_) => {}
                Decl::Enum(e) => {
                    let mut owner = cx.namespace.clone();
                    owner.push(&e.name);
                    if !reachable(owner[0]) {
                        continue;
                    }
                    for m in &e.members {
                        let mcx = cx.with_tparams(&m.tparams);
                        out.push(Keyed {
                            role: if exported {
                                Role::Export
                            } else {
                                Role::Import(module_field.clone())
                            },
                            key: self.member(&owner, true, m, &[], &mcx),
                            member: m,
                            owner: None,
                            cx: mcx,
                        });
                    }
                }
            }
        }
    }
}
