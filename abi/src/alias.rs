//! An IR with its aliases expanded (IR.md §3), for a reader that predates
//! them: every reference reads as the alias's type, and a back-edge as
//! `Any`.

use std::cell::RefCell;

use crate::ir::*;
use crate::resolve::{Context, Location, Symbols, Target};

/// `ir` without its alias declarations, every reference to one expanded.
pub fn expand(ir: &Ir, libs: &[&Ir]) -> Ir {
    let x = Expander {
        symbols: Symbols::with_libs(ir, libs),
        on: RefCell::new(Vec::new()),
    };
    Ir {
        items: x.items(&ir.items, &Context::default()),
        ..ir.clone()
    }
}

struct Expander<'a> {
    symbols: Symbols<'a>,
    on: RefCell<Vec<Location>>,
}

impl<'a> Expander<'a> {
    fn items(&self, items: &'a [Decl], cx: &Context<'a>) -> Vec<Decl> {
        items
            .iter()
            .filter_map(|d| {
                Some(match d {
                    Decl::Alias(_) => return None,
                    Decl::Namespace(n) => {
                        let mut inner = cx.clone();
                        inner.namespace.push(&n.name);
                        Decl::Namespace(Namespace {
                            items: self.items(&n.items, &inner),
                            ..n.clone()
                        })
                    }
                    Decl::Module(m) => {
                        let inner = Context {
                            module: Some(&m.spec),
                            ..Context::default()
                        };
                        Decl::Module(Module {
                            items: self.items(&m.items, &inner),
                            ..m.clone()
                        })
                    }
                    Decl::Class(c) => {
                        let ccx = cx.with_class(c);
                        Decl::Class(Class {
                            tparams: self.tparams(&c.tparams, &ccx),
                            extends: c.extends.iter().map(|r| self.typeref(r, &ccx)).collect(),
                            assoc: c
                                .assoc
                                .iter()
                                .map(|a| Assoc {
                                    rows: a
                                        .rows
                                        .iter()
                                        .map(|(k, t)| (self.ty(k, &ccx), self.ty(t, &ccx)))
                                        .collect(),
                                    ..a.clone()
                                })
                                .collect(),
                            members: c.members.iter().map(|m| self.member(m, &ccx)).collect(),
                            ..c.clone()
                        })
                    }
                    Decl::Enum(e) => Decl::Enum(Enum {
                        arms: e
                            .arms
                            .iter()
                            .map(|a| Arm {
                                payload: a.payload.iter().map(|t| self.ty(t, cx)).collect(),
                                ..a.clone()
                            })
                            .collect(),
                        members: e.members.iter().map(|m| self.member(m, cx)).collect(),
                        ..e.clone()
                    }),
                    Decl::Member(m) => Decl::Member(self.member(m, cx)),
                })
            })
            .collect()
    }

    fn member(&self, m: &'a Member, cx: &Context<'a>) -> Member {
        let mcx = cx.with_tparams(&m.tparams);
        Member {
            tparams: self.tparams(&m.tparams, &mcx),
            params: m
                .params
                .iter()
                .map(|p| Param {
                    ty: self.ty(&p.ty, &mcx),
                    ..p.clone()
                })
                .collect(),
            result: self.ty(&m.result, &mcx),
            ..m.clone()
        }
    }

    fn tparams(&self, ps: &[TypeParam], cx: &Context<'_>) -> Vec<TypeParam> {
        ps.iter()
            .map(|p| TypeParam {
                name: p.name.clone(),
                bound: p.bound.as_ref().map(|b| match b {
                    Bound::Subtype(t) => Bound::Subtype(self.ty(t, cx)),
                    Bound::In(ts) => Bound::In(ts.iter().map(|t| self.ty(t, cx)).collect()),
                }),
                default: p.default.as_ref().map(|t| self.ty(t, cx)),
            })
            .collect()
    }

    fn typeref(&self, r: &TypeRef, cx: &Context<'_>) -> TypeRef {
        TypeRef {
            path: self.relative(&r.path, cx),
            args: r.args.iter().map(|t| self.ty(t, cx)).collect(),
        }
    }

    /// The shortest lexical path naming what `p` does in `cx`: an
    /// expansion's paths are absolute.
    fn relative(&self, p: &Path, cx: &Context<'_>) -> Path {
        if p.base == Base::Lexical {
            return p.clone();
        }
        let Some(Target::Decl(loc, _)) = self.symbols.resolve(p, cx) else {
            return p.clone();
        };
        for start in (0..loc.path.len()).rev() {
            let short = Path::new(loc.path[start..].iter().cloned());
            if let Some(Target::Decl(l, _)) = self.symbols.resolve(&short, cx) {
                if l == loc {
                    return short;
                }
            }
        }
        p.clone()
    }

    fn ty(&self, t: &Type, cx: &Context<'_>) -> Type {
        let q = |t: &Type| self.ty(t, cx);
        let qb = |t: &Type| Box::new(q(t));
        match t {
            Type::Named(r) => match self.symbols.unfold(r, cx) {
                Some((loc, _)) if self.on.borrow().contains(&loc) => Type::Any,
                Some((loc, u)) => {
                    self.on.borrow_mut().push(loc);
                    let t = q(&u);
                    self.on.borrow_mut().pop();
                    t
                }
                None => Type::Named(self.typeref(r, cx)),
            },
            Type::Vec(e) => Type::Vec(qb(e)),
            Type::VecBox(e) => Type::VecBox(qb(e)),
            Type::MutVec(e) => Type::MutVec(qb(e)),
            Type::Array(e) => Type::Array(qb(e)),
            Type::ArrBox(e) => Type::ArrBox(qb(e)),
            Type::Tuple(ts) => Type::Tuple(ts.iter().map(q).collect()),
            Type::TupleBox(ts) => Type::TupleBox(ts.iter().map(q).collect()),
            Type::Future(v, e) => Type::Future(qb(v), self.relative(e, cx)),
            Type::Fn(f) => Type::Fn(Box::new(FnType {
                params: f.params.iter().map(q).collect(),
                rest: f.rest,
                result: q(&f.result),
                throws: f.throws.as_ref().map(|p| self.relative(p, cx)),
            })),
            Type::View { mutable, elem } => Type::View {
                mutable: *mutable,
                elem: qb(elem),
            },
            Type::Nullable(t, js_null) => Type::Nullable(qb(t), *js_null),
            t => t.clone(),
        }
    }
}
