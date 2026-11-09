//! What a type path names (IR.md §3, §4, §5).
//!
//! Paths are written relative to where they appear (IR.md §4). Inside a
//! namespace, `X` looks through the enclosing namespaces outwards; inside a
//! `module` block, the block's own declarations come before the globals. A
//! path based at a module specifier (`"node:buffer".Buffer`) names a
//! declaration of that block from anywhere, and a rooted one (`.X`) a
//! global.

use std::collections::{HashMap, HashSet};

use crate::ir::*;

/// A class, interface, enum or alias declaration.
#[derive(Clone, Copy, Debug)]
pub enum TypeDecl<'a> {
    Class(&'a Class),
    Enum(&'a Enum),
    Alias(&'a Alias),
}

/// A declaration's location: its module block, if any, and its full path
/// inside it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub module: Option<String>,
    pub path: Vec<String>,
}

impl Location {
    /// Where the declaration's own types are written.
    pub fn context(&self) -> Context<'_> {
        Context {
            module: self.module.as_deref(),
            namespace: self.path[..self.path.len() - 1]
                .iter()
                .map(String::as_str)
                .collect(),
            ..Context::default()
        }
    }

    /// The path naming the declaration from anywhere.
    pub fn absolute(&self) -> Path {
        Path {
            base: match &self.module {
                Some(m) => Base::Module(m.clone()),
                None => Base::Root,
            },
            segments: self.path.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Target<'a> {
    /// A type parameter in scope; the path is its name.
    TypeParam,
    /// An associated type of the enclosing class (§13); see
    /// [`Context::assoc`].
    Assoc,
    Decl(&'a Location, TypeDecl<'a>),
    /// The built-in `err` enum, when no user declaration shadows it.
    Err,
}

/// Where a path appears.
#[derive(Clone, Debug, Default)]
pub struct Context<'a> {
    pub module: Option<&'a str>,
    /// The enclosing namespaces, inside `module`.
    pub namespace: Vec<&'a str>,
    /// Type parameters in scope, innermost last.
    pub tparams: Vec<&'a str>,
    /// The enclosing class's associated types.
    pub assoc: Vec<&'a Assoc>,
}

impl<'a> Context<'a> {
    pub fn with_tparams(&self, ps: &'a [TypeParam]) -> Context<'a> {
        let mut cx = self.clone();
        cx.tparams.extend(ps.iter().map(|p| p.name.as_str()));
        cx
    }

    /// Inside `c`: its type parameters and associated types in scope.
    pub fn with_class(&self, c: &'a Class) -> Context<'a> {
        let mut cx = self.with_tparams(&c.tparams);
        cx.assoc = c.assoc.iter().collect();
        cx
    }

    /// The associated type `path` names, if it names one.
    pub fn assoc(&self, path: &Path) -> Option<&'a Assoc> {
        match path.segments.as_slice() {
            [name] if path.base == Base::Lexical && !self.tparams.contains(&name.as_str()) => {
                self.assoc.iter().copied().find(|a| a.name == *name)
            }
            _ => None,
        }
    }
}

pub struct Symbols<'a> {
    types: HashMap<Location, (Location, TypeDecl<'a>)>,
    modules: HashSet<&'a str>,
}

/// The libs `ir`'s `with` names, in order, from those `supplied`; the
/// names no supplied lib declares, as `Err`.
pub fn libs_for<'a>(ir: &Ir, supplied: &'a [Ir]) -> Result<Vec<&'a Ir>, Vec<String>> {
    let mut libs = Vec::new();
    let mut missing = Vec::new();
    for name in &ir.with {
        match supplied.iter().find(|l| l.lib.as_ref() == Some(name)) {
            Some(l) => libs.push(l),
            None => missing.push(name.clone()),
        }
    }
    if missing.is_empty() {
        Ok(libs)
    } else {
        Err(missing)
    }
}

impl<'a> Symbols<'a> {
    pub fn new(ir: &'a Ir) -> Symbols<'a> {
        Symbols::with_libs(ir, &[])
    }

    /// `ir`'s declarations, then those of the libs it resolves against
    /// (`with`, §5), which its own shadow.
    pub fn with_libs(ir: &'a Ir, libs: &[&'a Ir]) -> Symbols<'a> {
        let mut s = Symbols {
            types: HashMap::new(),
            modules: HashSet::new(),
        };
        s.collect(None, &mut Vec::new(), &ir.items);
        for lib in libs {
            s.collect(None, &mut Vec::new(), &lib.items);
        }
        s
    }

    fn collect(&mut self, module: Option<&'a str>, ns: &mut Vec<String>, items: &'a [Decl]) {
        for d in items {
            let decl = match d {
                Decl::Namespace(n) => {
                    ns.push(n.name.clone());
                    self.collect(module, ns, &n.items);
                    ns.pop();
                    continue;
                }
                Decl::Module(m) => {
                    self.modules.insert(&m.spec);
                    self.collect(Some(&m.spec), &mut Vec::new(), &m.items);
                    continue;
                }
                Decl::Member(_) => continue,
                Decl::Class(c) => (&c.name, TypeDecl::Class(c)),
                Decl::Enum(e) => (&e.name, TypeDecl::Enum(e)),
                Decl::Alias(a) => (&a.name, TypeDecl::Alias(a)),
            };
            let mut path = ns.clone();
            path.push(decl.0.clone());
            let loc = Location {
                module: module.map(Into::into),
                path,
            };
            self.types.entry(loc.clone()).or_insert((loc, decl.1));
        }
    }

    pub fn is_module(&self, spec: &str) -> bool {
        self.modules.contains(spec)
    }

    fn get(&self, module: Option<&str>, path: &[String]) -> Option<Target<'_>> {
        let loc = Location {
            module: module.map(Into::into),
            path: path.to_vec(),
        };
        self.types.get(&loc).map(|(l, d)| Target::Decl(l, *d))
    }

    /// [`Symbols::resolve`] to a declaration, which outlives the symbols.
    pub fn resolve_decl(&self, path: &Path, cx: &Context<'_>) -> Option<(Location, TypeDecl<'a>)> {
        match self.resolve(path, cx)? {
            Target::Decl(loc, _) => self.types.get(loc).map(|(l, d)| (l.clone(), *d)),
            _ => None,
        }
    }

    /// `None` when the path names nothing declared.
    pub fn resolve(&self, path: &Path, cx: &Context<'_>) -> Option<Target<'_>> {
        let segs = &path.segments;
        match &path.base {
            Base::Root => return self.get(None, segs),
            Base::Module(spec) => return self.get(Some(spec), segs),
            Base::Lexical => {}
        }
        if segs.len() == 1 && cx.tparams.contains(&segs[0].as_str()) {
            return Some(Target::TypeParam);
        }
        if cx.assoc(path).is_some() {
            return Some(Target::Assoc);
        }
        let scopes: Vec<Option<&str>> = match cx.module {
            Some(m) => vec![Some(m), None],
            None => vec![None],
        };
        for scope in scopes {
            let depth = if scope == cx.module {
                cx.namespace.len()
            } else {
                0
            };
            for k in (0..=depth).rev() {
                let mut full: Vec<String> =
                    cx.namespace[..k].iter().map(|s| s.to_string()).collect();
                full.extend(segs.iter().cloned());
                if let Some(t) = self.get(scope, &full) {
                    return Some(t);
                }
            }
        }
        if path.is_err() {
            return Some(Target::Err);
        }
        None
    }

    /// The alias `r` names, unfolded once: its type with every path made
    /// absolute and `r`'s arguments, else the defaults, for its
    /// parameters, so it reads in `cx` as `r` does. References inside it,
    /// to the alias itself too, stay as written.
    pub fn unfold(&self, r: &TypeRef, cx: &Context<'_>) -> Option<(Location, Type)> {
        let Some(Target::Decl(loc, TypeDecl::Alias(a))) = self.resolve(&r.path, cx) else {
            return None;
        };
        let acx = loc.context().with_tparams(&a.tparams);
        let mut subst = HashMap::new();
        for (i, p) in a.tparams.iter().enumerate() {
            let t = match (r.args.get(i), &p.default) {
                (Some(t), _) => t.clone(),
                (None, Some(d)) => self.absolute(d, &acx, &subst),
                (None, None) => Type::Any,
            };
            subst.insert(p.name.as_str(), t);
        }
        Some((loc.clone(), self.absolute(&a.ty, &acx, &subst)))
    }

    /// `t` with its head unfolded until it isn't an alias.
    pub fn head(&self, t: &Type, cx: &Context<'_>) -> Type {
        let mut t = t.clone();
        // A valid IR's aliases reach a type constructor (§3).
        for _ in 0..64 {
            match &t {
                Type::Named(r) => match self.unfold(r, cx) {
                    Some((_, u)) => t = u,
                    None => break,
                },
                _ => break,
            }
        }
        t
    }

    /// `t`, written in `cx`, with every declaration's path absolute and
    /// the type parameters `subst` maps replaced.
    pub fn absolute(&self, t: &Type, cx: &Context<'_>, subst: &HashMap<&str, Type>) -> Type {
        let q = |t: &Type| self.absolute(t, cx, subst);
        let qb = |t: &Type| Box::new(q(t));
        let qs = |ts: &[Type]| ts.iter().map(q).collect::<Vec<_>>();
        let path = |p: &Path| match self.resolve(p, cx) {
            Some(Target::Decl(loc, _)) => loc.absolute(),
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
            Type::Named(r) => match (self.resolve(&r.path, cx), &r.path.segments[..]) {
                (Some(Target::TypeParam), [name]) if subst.contains_key(name.as_str()) => {
                    subst[name.as_str()].clone()
                }
                _ => Type::Named(TypeRef {
                    path: path(&r.path),
                    args: qs(&r.args),
                }),
            },
            t => t.clone(),
        }
    }

    /// Whether the alias at `loc` reaches itself, through other aliases.
    pub fn recursive(&self, loc: &Location) -> bool {
        let Some((_, TypeDecl::Alias(a))) = self.types.get(loc) else {
            return false;
        };
        let mut seen = HashSet::new();
        self.reaches(
            &a.ty,
            &loc.context().with_tparams(&a.tparams),
            loc,
            &mut seen,
        )
    }

    fn reaches(
        &self,
        t: &Type,
        cx: &Context<'_>,
        to: &Location,
        seen: &mut HashSet<Location>,
    ) -> bool {
        let mut found = false;
        t.walk(&mut |t| {
            let Type::Named(r) = t else { return };
            if let Some(Target::Decl(l, TypeDecl::Alias(b))) = self.resolve(&r.path, cx) {
                if l == to {
                    found = true;
                } else if !found && seen.insert(l.clone()) {
                    found = self.reaches(&b.ty, &l.context().with_tparams(&b.tparams), to, seen);
                }
            }
        });
        found
    }
}
