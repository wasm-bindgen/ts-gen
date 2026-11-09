//! Static validation of an [`Ir`].
//!
//! The parser only enforces the grammar. Everything IR.md calls a
//! validation error is checked here, so the bridge and the language
//! generators can rely on a validated IR:
//!
//! * declarations: built-in names are escaped, type names are unique in
//!   their scope, module export lists name the block's declarations;
//! * types: every path resolves, with the right number of type arguments;
//!   `unit` not in an array; no `T??`; `T[]` element types; `?` never
//!   on `*T[]`; `*T[]` only in import parameters, and `mut T[]` only in
//!   parameters;
//! * enums: through [`enums::flatten`] (indices, names, `@fallback`,
//!   aliases, carriers), plus literals on every implicit unit arm, unique
//!   literals, and `throws` enums that end in a `@fallback` with only
//!   `(Str)` arms;
//! * members: the member forms, `@js` entries where they apply, and
//!   colliding keys;
//! * `@js(args)` and `@js(super)`: each parameter at most once, known
//!   names, a trailing spread of a sequence, `p? {..}` on a nullable `p`,
//!   `*T[]` only in `@js(args)`, and `@js` expressions type-checked
//!   against IR members;
//! * `export class`: at most one imported parent (IR.md §17).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::enums::{self, EnumRef, Shape};
use crate::ir::*;
use crate::key::{Keys, Role};
use crate::resolve::{libs_for, Context, Location, Symbols, Target, TypeDecl};
use crate::text::Error;

#[derive(Clone, Copy, Debug, Default)]
pub struct Options<'a> {
    /// Treat names the IR doesn't declare as opaque host types, for IR
    /// fragments such as the spec's examples. The producer always emits a
    /// complete IR.
    pub allow_unresolved: bool,
    /// The lib IRs available to `with` (§5).
    pub libs: &'a [Ir],
}

pub fn validate(ir: &Ir) -> Result<(), Vec<Error>> {
    validate_with(ir, Options::default())
}

/// Every error, in source order.
pub fn validate_with(ir: &Ir, opts: Options<'_>) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();
    let libs = match libs_for(ir, opts.libs) {
        Ok(libs) => libs,
        Err(missing) => {
            if !opts.allow_unresolved {
                errors.extend(missing.iter().map(|m| {
                    Error::new(
                        Span::default(),
                        format!("`with {m};` needs the lib IR `{m}`, which wasn't supplied"),
                    )
                }));
            }
            Vec::new()
        }
    };
    let v = Validator {
        ir,
        libs: libs.clone(),
        symbols: Symbols::with_libs(ir, &libs),
        opts,
        errors: RefCell::new(Vec::new()),
        throws_ok: RefCell::new(HashMap::new()),
        in_alias: Cell::new(false),
    };
    v.items(&ir.items, &Context::default(), false);
    v.keys();
    errors.extend(v.errors.into_inner());
    errors.sort_by_key(|e| (e.span.line, e.span.col));
    errors.dedup();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Element types of `T[]`, `mut T[]` and `*T[]`: the typed arrays.
pub fn is_vec_element(s: Scalar) -> bool {
    !matches!(s, Scalar::Bool | Scalar::I53 | Scalar::U53)
}

/// The well-known symbols `@js(symbol = ...)` may name.
pub const WELL_KNOWN_SYMBOLS: &[&str] = &[
    "asyncDispose",
    "asyncIterator",
    "dispose",
    "hasInstance",
    "isConcatSpreadable",
    "iterator",
    "match",
    "matchAll",
    "replace",
    "search",
    "species",
    "split",
    "toPrimitive",
    "toStringTag",
    "unscopables",
];

/// Where a type appears, for the position-dependent rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pos {
    /// A parameter of a member the language imports.
    ImportParam,
    /// A parameter of a member the language exports.
    ExportParam,
    Result,
    FutureValue,
    /// Anywhere else: inside another type, a payload, a type argument.
    Inner,
}

impl Pos {}

/// What an `@js` expression evaluates to, as far as it's known.
#[derive(Clone)]
enum Value<'a> {
    Unknown,
    /// An instance of a type.
    Instance(Type, Context<'a>),
    /// A class itself, for statics and constructors.
    Static(&'a Class, Location),
}

/// The scope of an `@js` template or expression.
struct Scope<'m, 'a> {
    member: &'a Member,
    cx: &'m Context<'a>,
    lets: &'m HashMap<&'a str, Value<'a>>,
    /// `$result` is in scope (`cancel`).
    result: bool,
    /// References to each parameter.
    uses: HashMap<&'a str, usize>,
}

struct Validator<'a> {
    ir: &'a Ir,
    libs: Vec<&'a Ir>,
    symbols: Symbols<'a>,
    opts: Options<'a>,
    errors: RefCell<Vec<Error>>,
    /// Whether each enum used in `throws` qualifies, by declaration.
    throws_ok: RefCell<HashMap<*const Enum, bool>>,
    in_alias: Cell<bool>,
}

impl<'a> Validator<'a> {
    fn err(&self, span: Span, message: impl Into<String>) {
        self.errors.borrow_mut().push(Error::new(span, message));
    }

    fn builtin_name(&self, span: Span, name: &str) {
        if BUILTIN_TYPE_NAMES.contains(&name) {
            self.err(
                span,
                format!("`{name}` is a built-in type name; declare it escaped, with `@js(name = \"{name}\")`"),
            );
        }
    }

    fn items(&self, items: &'a [Decl], cx: &Context<'a>, exported: bool) {
        let mut types: HashMap<&str, Span> = HashMap::new();
        for d in items {
            if let Decl::Class(Class { name, meta, .. })
            | Decl::Enum(Enum { name, meta, .. })
            | Decl::Alias(Alias { name, meta, .. }) = d
            {
                if types.insert(name, meta.span).is_some() {
                    self.err(meta.span, format!("`{name}` is declared twice"));
                }
            }
        }
        for d in items {
            self.decl_attrs(d);
            match d {
                Decl::Namespace(n) => {
                    self.builtin_name(n.meta.span, &n.name);
                    if n.export && cx.module.is_some() {
                        self.err(
                            n.meta.span,
                            "a `module` block describes imports; it can't export",
                        );
                    }
                    let mut inner = cx.clone();
                    inner.namespace.push(&n.name);
                    self.items(&n.items, &inner, exported || n.export);
                }
                Decl::Module(m) => self.module(m, cx),
                Decl::Class(c) => self.class(c, cx, exported),
                Decl::Enum(e) => self.enum_(e, cx),
                Decl::Alias(a) => self.alias(a, cx),
                Decl::Member(m) => {
                    if m.export && cx.module.is_some() {
                        self.err(
                            m.meta.span,
                            "a `module` block describes imports; it can't export",
                        );
                    }
                    let role = if exported || m.export {
                        Pos::ExportParam
                    } else {
                        Pos::ImportParam
                    };
                    if m.export
                        && cx.namespace.is_empty()
                        && !matches!(m.kind, MemberKind::Method(_))
                    {
                        self.err(
                            m.meta.span,
                            format!("`{}` can't be a module export: an ES module binding can't be an accessor, so export it from a namespace", member_label(m)),
                        );
                    }
                    self.member(m, None, role, cx, false);
                }
            }
        }
    }

    /// Declaration-level attributes: `@fallback` and `@once` belong to arms
    /// and parameters.
    fn decl_attrs(&self, d: &Decl) {
        let meta = d.meta();
        if meta.attrs.fallback {
            self.err(meta.span, "`@fallback` goes on an enum arm");
        }
        if meta.attrs.once {
            self.err(meta.span, "`@once` goes inline on a callback parameter");
        }
        if matches!(d, Decl::Member(_)) {
            return;
        }
        for j in &meta.attrs.js {
            match j {
                JsEntry::Name(n) => self.rename(meta.span, n),
                _ => self.err(meta.span, "only `@js(name = ...)` applies to a declaration"),
            }
        }
    }

    fn rename(&self, span: Span, name: &str) {
        if name.is_empty() || name.contains('.') {
            self.err(
                span,
                format!("`@js(name = \"{name}\")` must be one non-empty segment"),
            );
        }
    }

    fn module(&self, m: &'a Module, cx: &Context<'a>) {
        if cx.module.is_some() || !cx.namespace.is_empty() {
            self.err(m.meta.span, "a `module` block must be at the top level");
        }
        // A member renamed apart from its overloads is exported under its
        // JS name.
        let mut declared: HashSet<&str> = m
            .items
            .iter()
            .flat_map(|d| {
                let js = d.meta().attrs.js.iter().find_map(|e| match e {
                    JsEntry::Name(n) => Some(n.as_str()),
                    _ => None,
                });
                d.name().into_iter().chain(js)
            })
            .collect();
        // The value a `@js(new = ...)` constructor runs is exported too.
        let mut news = Vec::new();
        new_values(&self.ir.items, None, &mut news);
        let news: Vec<&str> = news
            .into_iter()
            .filter(|(spec, _)| *spec == Some(m.spec.as_str()))
            .map(|(_, head)| head)
            .collect();
        declared.extend(news.iter().copied());
        let mut names = HashSet::new();
        for e in m.exports.iter().flatten() {
            if !declared.contains(e.name.as_str()) {
                self.err(
                    m.meta.span,
                    format!(
                        "`{}` exports `{}`, which it doesn't declare",
                        m.spec, e.name
                    ),
                );
            }
            let as_name = e.alias.as_deref().unwrap_or(&e.name);
            if !names.insert(as_name) {
                self.err(
                    m.meta.span,
                    format!("`{}` exports `{as_name}` twice", m.spec),
                );
            }
        }
        let exported: HashSet<&str> = m
            .exports
            .iter()
            .flatten()
            .map(|e| e.name.as_str())
            .collect();
        for head in news {
            if !exported.contains(head) {
                self.err(
                    m.meta.span,
                    format!(
                        "a `@js(new = ...)` constructor runs `{head}`, which `{}` doesn't export",
                        m.spec
                    ),
                );
            }
        }
        for d in &m.items {
            let js = d.meta().attrs.js.iter().find_map(|e| match e {
                JsEntry::Name(n) => Some(n.as_str()),
                _ => None,
            });
            if !d.name().into_iter().chain(js).any(|n| exported.contains(n)) {
                self.unexported(d, &m.spec, &mut Vec::new());
            }
        }
        let inner = Context {
            module: Some(&m.spec),
            ..Context::default()
        };
        self.items(&m.items, &inner, false);
    }

    /// A module declaration its export list doesn't reach has no JS value
    /// (§5): only its instance members are usable, so a class (whose
    /// `instanceof` needs the value) and anything static are errors.
    fn unexported(&self, d: &'a Decl, spec: &str, at: &mut Vec<&'a str>) {
        let label = |at: &[&str], n: &str| {
            let mut segs = at.to_vec();
            segs.push(n);
            segs.join(".")
        };
        let needs_value = |m: &Member| {
            m.is_static
                || match m.kind {
                    MemberKind::New(_) => {
                        !m.meta.attrs.js.contains(&JsEntry::Object)
                            && m.meta.attrs.js.new_value().is_none()
                    }
                    _ => false,
                }
        };
        match d {
            Decl::Class(c) => {
                let name = label(at, &c.name);
                if c.kind == ClassKind::Class {
                    self.err(
                        c.meta.span,
                        format!("the class `{name}` in module \"{spec}\" has no JS value: export it, or declare it as an interface"),
                    );
                }
                for m in c.members.iter().filter(|m| needs_value(m)) {
                    self.err(
                        m.meta.span,
                        format!("`{name}.{}` in module \"{spec}\" has no JS value, since `{name}` isn't exported", member_label(m)),
                    );
                }
            }
            Decl::Member(m) => self.err(
                m.meta.span,
                format!(
                    "`{}` in module \"{spec}\" has no JS value, since it isn't exported",
                    label(at, &member_label(m))
                ),
            ),
            Decl::Namespace(n) => {
                at.push(&n.name);
                for d in &n.items {
                    self.unexported(d, spec, at);
                }
                at.pop();
            }
            Decl::Enum(_) | Decl::Module(_) | Decl::Alias(_) => {}
        }
    }

    /// An alias names a type valid anywhere (§3), and reaches itself only
    /// through a type constructor.
    fn alias(&self, a: &'a Alias, cx: &Context<'a>) {
        let span = a.meta.span;
        self.builtin_name(span, &a.name);
        let acx = cx.with_tparams(&a.tparams);
        self.tparams(span, &a.tparams, &acx);
        self.in_alias.set(true);
        self.ty(&a.ty, Pos::Inner, &acx, span);
        self.in_alias.set(false);
        let Some(Target::Decl(loc, _)) = self.symbols.resolve(&Path::new([a.name.as_str()]), cx)
        else {
            return;
        };
        let this = TypeRef {
            path: loc.absolute(),
            args: a.tparams.iter().map(|_| Type::Any).collect(),
        };
        let this = Type::Named(this);
        if self.unguarded(&this, cx, &mut Vec::new(), loc) {
            self.err(
                span,
                format!(
                    "`{}` reaches itself without a type constructor between (`fn`, `arr<T>`, `T[]`, a tuple, `future` or a generic argument)",
                    a.name
                ),
            );
        } else if self.symbols.recursive(loc)
            && !matches!(self.symbols.head(&this, cx), Type::Fn(_))
        {
            // A back-edge lays out only as a function value (§3).
            self.err(
                span,
                format!(
                    "`{}` reaches itself but isn't a function type, so it has no layout",
                    a.name
                ),
            );
        }
    }

    /// Whether `t` names one of `cx`'s type parameters.
    fn open(&self, t: &Type, cx: &Context<'_>) -> bool {
        let mut o = false;
        t.walk(&mut |t| {
            if let Type::Named(r) = t {
                o |= r.path.base == Base::Lexical
                    && matches!(self.symbols.resolve(&r.path, cx), Some(Target::TypeParam));
            }
        });
        o
    }

    /// Whether unfolding `t` reaches a back-edge whose arguments name one of
    /// `cx`'s type parameters, which has no layout.
    fn open_back_edge(&self, t: &Type, cx: &Context<'_>, on: &mut Vec<Location>) -> bool {
        let open = |t: &Type| self.open(t, cx);
        let mut found = false;
        t.walk(&mut |t| {
            let Type::Named(r) = t else { return };
            if found {
                return;
            }
            let Some((loc, u)) = self.symbols.unfold(r, cx) else {
                return;
            };
            if on.contains(&loc) {
                found = r.args.iter().any(open);
            } else {
                on.push(loc);
                found = self.open_back_edge(&u, cx, on);
                on.pop();
            }
        });
        found
    }

    /// Whether `t` reaches the alias `to` through aliases and `?` alone.
    fn unguarded(&self, t: &Type, cx: &Context<'_>, on: &mut Vec<Location>, to: &Location) -> bool {
        match t {
            Type::Nullable(t, _) => self.unguarded(t, cx, on, to),
            Type::Named(r) => {
                let Some((loc, u)) = self.symbols.unfold(r, cx) else {
                    return false;
                };
                if on.contains(&loc) {
                    return loc == *to;
                }
                on.push(loc);
                let found = self.unguarded(&u, cx, on, to);
                on.pop();
                found
            }
            _ => false,
        }
    }

    fn tparams(&self, span: Span, ps: &'a [TypeParam], cx: &Context<'a>) {
        let mut seen = HashSet::new();
        for p in ps {
            self.builtin_name(span, &p.name);
            if !seen.insert(&p.name) {
                self.err(
                    span,
                    format!("type parameter `{}` is declared twice", p.name),
                );
            }
            match &p.bound {
                Some(Bound::Subtype(Type::Any)) | None => {}
                Some(Bound::Subtype(t @ Type::Named(r))) => {
                    self.ty(t, Pos::Inner, cx, span);
                    if let Some(Target::Decl(_, TypeDecl::Enum(_))) = self.symbols.resolve(&r.path, cx) {
                        self.err(span, format!("`{}` bounds `{}` by an enum; use `in (..)`", p.name, path(&r.path)));
                    }
                }
                Some(Bound::Subtype(_)) => self.err(
                    span,
                    format!("`{}: ...` bounds by a class, an interface or `Any`; use `in (..)` for concrete types", p.name),
                ),
                Some(Bound::In(ts)) => {
                    for t in ts {
                        self.ty(t, Pos::Inner, cx, span);
                    }
                }
            }
            let Some(d) = &p.default else { continue };
            self.ty(d, Pos::Inner, cx, span);
            if let Some(Bound::In(ts)) = &p.bound {
                if !ts.contains(d) {
                    self.err(
                        span,
                        format!(
                            "`{}`'s default `{}` isn't in its bound",
                            p.name,
                            crate::text::print_type(d)
                        ),
                    );
                }
            }
            let later: Vec<&str> = ps
                .iter()
                .skip_while(|q| q.name != p.name)
                .map(|q| q.name.as_str())
                .collect();
            let mut forward = None;
            d.walk(&mut |t| {
                if let Type::Named(r) = t {
                    if let [n] = r.path.segments.as_slice() {
                        if later.contains(&n.as_str()) {
                            forward.get_or_insert(n.clone());
                        }
                    }
                }
            });
            if let Some(n) = forward {
                self.err(
                    span,
                    format!(
                        "`{}`'s default names `{n}`, which isn't declared before it",
                        p.name
                    ),
                );
            }
        }
    }

    fn class(&self, c: &'a Class, cx: &Context<'a>, exported: bool) {
        let span = c.meta.span;
        self.builtin_name(span, &c.name);
        let ccx = cx.with_class(c);
        self.tparams(span, &c.tparams, &ccx);
        self.assoc(c, &ccx);
        for e in &c.extends {
            let t = Type::Named(e.clone());
            self.ty(&t, Pos::Inner, &ccx, span);
            if let Some(Target::Decl(_, TypeDecl::Enum(_)) | Target::Err) =
                self.symbols.resolve(&e.path, &ccx)
            {
                self.err(
                    span,
                    format!("`{}` extends the enum `{}`", c.name, path(&e.path)),
                );
            }
        }
        if c.export {
            self.export_class(c, cx);
        }
        let role = if c.export || exported {
            Pos::ExportParam
        } else {
            Pos::ImportParam
        };
        for m in &c.members {
            self.member(m, Some(c), role, &ccx, false);
        }
    }

    /// Each associated type matches on a concretely bounded parameter of
    /// `c`, with one well-typed row per element of the bound, in order.
    fn assoc(&self, c: &'a Class, cx: &Context<'a>) {
        let mut names = HashSet::new();
        for a in &c.assoc {
            let span = a.meta.span;
            let what = format!("`{}.{}`", c.name, a.name);
            if !names.insert(a.name.as_str()) || c.tparams.iter().any(|p| p.name == a.name) {
                self.err(span, format!("{what} is declared twice"));
            }
            let Some(p) = c.tparams.iter().find(|p| p.name == a.param) else {
                self.err(
                    span,
                    format!(
                        "{what} matches `{}`, which isn't a type parameter of `{}`",
                        a.param, c.name
                    ),
                );
                continue;
            };
            let Some(Bound::In(bound)) = &p.bound else {
                self.err(
                    span,
                    format!("{what} matches `{}`, which has no `in (..)` bound", a.param),
                );
                continue;
            };
            let keys: Vec<&Type> = a.rows.iter().map(|(k, _)| k).collect();
            if keys != bound.iter().collect::<Vec<_>>() {
                let bound: Vec<String> = bound.iter().map(crate::text::print_type).collect();
                self.err(
                    span,
                    format!(
                        "{what} needs one row per element of `{}`'s bound, in order: {}",
                        a.param,
                        bound.join(", ")
                    ),
                );
            }
            for (_, t) in &a.rows {
                self.ty(t, Pos::Inner, cx, span);
                if self.mentions_assoc(t, cx) {
                    self.err(span, format!("{what}: a row can't name an associated type"));
                }
            }
        }
    }

    fn mentions_assoc(&self, t: &Type, cx: &Context<'a>) -> bool {
        let mut found = false;
        t.walk(&mut |t| {
            if let Type::Named(r) = t {
                found |= cx.assoc(&r.path).is_some();
            }
        });
        found
    }

    fn export_class(&self, c: &'a Class, cx: &Context<'a>) {
        let span = c.meta.span;
        if cx.module.is_some() {
            self.err(span, "a `module` block describes imports; it can't export");
        }
        if c.extends.len() > 1 {
            self.err(
                span,
                format!("`export class {}` may extend one imported class", c.name),
            );
        }
        let Some(parent) = c.extends.first() else {
            return;
        };
        let base = match self.symbols.resolve(&parent.path, cx) {
            Some(Target::Decl(_, TypeDecl::Class(b))) => b,
            _ => return,
        };
        if base.kind != ClassKind::Class || base.export {
            self.err(
                span,
                format!(
                    "`export class {}` may extend only an imported class, not `{}`",
                    c.name,
                    path(&parent.path)
                ),
            );
        }
    }

    fn enum_(&self, e: &'a Enum, cx: &Context<'a>) {
        let span = e.meta.span;
        for m in &e.members {
            if !m.is_static {
                self.err(
                    m.meta.span,
                    format!("an enum's members are statics; `{}` isn't", e.name),
                );
            }
            self.member(m, None, Pos::ImportParam, cx, true);
        }
        self.builtin_name(span, &e.name);
        let mut p = cx
            .namespace
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        p.push(e.name.clone());
        let this = EnumRef {
            loc: Some(Location {
                module: cx.module.map(Into::into),
                path: p,
            }),
            decl: e,
        };
        let flat = match enums::flatten(&self.symbols, this) {
            Ok(f) => Some(f),
            Err(errs) => {
                // Bases that don't resolve are fine in a fragment.
                let errs: Vec<Error> = errs
                    .into_iter()
                    .filter(|x| {
                        !(self.opts.allow_unresolved && x.message.ends_with("isn't declared"))
                    })
                    .collect();
                let none = errs.is_empty();
                self.errors.borrow_mut().extend(errs);
                if none && e.extends.is_empty() {
                    None
                } else {
                    return;
                }
            }
        };
        let carrier = match &flat {
            Some(f) => match f.shape {
                Shape::Carrier(c) => Some(c),
                _ => None,
            },
            None => e.carrier,
        };
        if flat.as_ref().is_some_and(|f| f.arms.is_empty()) {
            self.err(span, format!("`{}` has no arms", e.name));
        }
        for a in &e.arms {
            self.arm(a, carrier, cx);
        }
        if let Some(f) = &flat {
            let mut literals: HashMap<&str, &str> = HashMap::new();
            for fa in &f.arms {
                for j in &fa.arm.meta.attrs.js {
                    if let JsEntry::Literal(l) = j {
                        if let Some(other) = literals.insert(l, &fa.arm.name) {
                            self.err(
                                fa.arm.meta.span,
                                format!(
                                    "arms `{other}` and `{}` both have the literal \"{l}\"",
                                    fa.arm.name
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    fn arm(&self, a: &'a Arm, carrier: Option<Scalar>, cx: &Context<'a>) {
        let span = a.meta.span;
        if matches!(a.value, Some(ArmValue::Alias(_))) {
            return;
        }
        if a.meta.attrs.once {
            self.err(span, "`@once` goes inline on a callback parameter");
        }
        let unit = a.payload.is_empty();
        let fallback = a.meta.attrs.fallback;
        let mut literals = 0;
        for j in &a.meta.attrs.js {
            match j {
                JsEntry::Literal(_) if carrier.is_some() => self.err(
                    span,
                    format!(
                        "arm `{}` of a carrier enum crosses as its number, and has no literal",
                        a.name
                    ),
                ),
                JsEntry::Literal(_) if !unit || fallback => self.err(
                    span,
                    format!(
                        "only a unit arm that isn't `@fallback` has a literal, not `{}`",
                        a.name
                    ),
                ),
                JsEntry::Literal(_) => literals += 1,
                JsEntry::Class(_) if unit => self.err(
                    span,
                    format!(
                        "an error class match needs a `(Str)` payload, which `{}` lacks",
                        a.name
                    ),
                ),
                JsEntry::Class(_) => {}
                _ => self.err(
                    span,
                    format!(
                        "arm `{}` takes only a literal or an error class match",
                        a.name
                    ),
                ),
            }
        }
        if literals > 1 {
            self.err(span, format!("arm `{}` has more than one literal", a.name));
        }
        if let Some(c) = carrier {
            if !unit {
                self.err(
                    span,
                    format!(
                        "arm `{}` of a `{}` carrier enum can't have a payload",
                        a.name,
                        c.name()
                    ),
                );
            }
            if fallback {
                self.err(
                    span,
                    format!(
                        "a `{}` carrier enum is an open set, and has no `@fallback`",
                        c.name()
                    ),
                );
            }
        } else if unit && !fallback && literals == 0 {
            self.err(
                span,
                format!("unit arm `{}` needs its JS literal, `@js(\"...\")`", a.name),
            );
        }
        if fallback && a.payload.len() > 1 {
            self.err(
                span,
                format!(
                    "`@fallback` arm `{}` keeps the value in at most one payload",
                    a.name
                ),
            );
        }
        for t in &a.payload {
            self.ty(t, Pos::Inner, cx, span);
        }
    }

    /// `enum_static`: an enum's static, which has no class owner.
    fn member(
        &self,
        m: &'a Member,
        owner: Option<&'a Class>,
        role: Pos,
        cx: &Context<'a>,
        enum_static: bool,
    ) {
        let span = m.meta.span;
        let mcx = cx.with_tparams(&m.tparams);
        self.tparams(span, &m.tparams, &mcx);
        let label = member_label(m);
        if owner.is_some() && m.export {
            self.err(
                span,
                format!("`export` marks top-level members; `{label}` is a class member"),
            );
        }
        if owner.is_none() && m.is_static && !enum_static {
            self.err(
                span,
                format!("`static` applies to class members, not `{label}`"),
            );
        }
        match &m.kind {
            MemberKind::New(_) if owner.is_none() => {
                self.err(span, "a constructor must be inside a class or interface")
            }
            MemberKind::Get(_) if m.result == Type::Unit => {
                self.err(span, format!("getter `{label}` needs a result"))
            }
            MemberKind::Set(_) if m.result != Type::Unit => {
                self.err(span, format!("setter `{label}` can't have a result"))
            }
            MemberKind::Set(_) if m.params.is_empty() => {
                let args = m.meta.attrs.js.args().map(<[_]>::len);
                if args != Some(1) {
                    self.err(
                        span,
                        format!("setter `{label}` has no parameter, so `@js(args = (...))` must give its one value"),
                    );
                }
            }
            _ => {}
        }
        if m.meta.attrs.fallback {
            self.err(span, "`@fallback` goes on an enum arm");
        }
        if m.meta.attrs.once {
            self.err(span, "`@once` goes inline on a callback parameter");
        }

        let mut names = HashSet::new();
        for p in &m.params {
            if !names.insert(p.name.as_str()) {
                self.err(p.span, format!("parameter `{}` is declared twice", p.name));
            }
            self.ty(&p.ty, role, &mcx, p.span);
            if p.unshared && !matches!(p.ty, Type::View { .. }) {
                self.err(
                    p.span,
                    format!("`@js(unshared)` applies to a view, not `{}`", p.name),
                );
            }
            if matches!(p.ty, Type::Nullable(_, true)) && role == Pos::ExportParam {
                self.err(
                    p.span,
                    format!(
                        "`@js(null)` on `{}` has no effect: an export's parameters come from JS",
                        p.name
                    ),
                );
            }
            if p.union {
                if role == Pos::ExportParam {
                    self.err(
                        p.span,
                        format!(
                            "`union` applies to an import's parameter; `{}` comes from JS",
                            p.name
                        ),
                    );
                } else if let Some(why) = crate::union::invalid(&self.symbols, &p.ty, &mcx) {
                    self.err(p.span, format!("`{}`: {why}", p.name));
                }
            }
            if p.once {
                let head = |t: &Type| self.symbols.head(t, cx);
                let callback = match head(&p.ty) {
                    Type::Fn(_) => true,
                    Type::Nullable(t, _) => matches!(head(&t), Type::Fn(_)),
                    _ => false,
                };
                if !callback {
                    self.err(
                        p.span,
                        format!("`@once` applies to a callback parameter, not `{}`", p.name),
                    );
                } else if role == Pos::ExportParam {
                    self.err(
                        p.span,
                        format!("`@once` applies to a callback the language provides; `{}` comes from JS", p.name),
                    );
                }
            }
        }
        self.ty(&m.result, Pos::Result, &mcx, span);
        if let Some(t) = &m.throws {
            self.throws(t, &mcx, span);
        }
        self.member_js(m, owner, &mcx, role == Pos::ExportParam);
    }

    /// `incoming`: an export, whose templates are patterns (§7).
    fn member_js(&self, m: &'a Member, owner: Option<&'a Class>, cx: &Context<'a>, incoming: bool) {
        let span = m.meta.span;
        let label = member_label(m);
        let mut kinds: HashMap<&str, usize> = HashMap::new();
        let mut lets: HashMap<&'a str, Value<'a>> = HashMap::new();
        let params: HashSet<&str> = m.params.iter().map(|p| p.name.as_str()).collect();
        let mut args: Option<&'a [Telem]> = None;
        for j in &m.meta.attrs.js {
            let kind = match j {
                JsEntry::Literal(_) | JsEntry::Class(_) => {
                    self.err(
                        span,
                        format!("`{label}`: literals and error class matches go on enum arms"),
                    );
                    continue;
                }
                JsEntry::Name(n) => {
                    self.rename(span, n);
                    "name"
                }
                JsEntry::New(p) => {
                    if !matches!(m.kind, MemberKind::New(_))
                        || incoming
                        || owner.is_some_and(|c| c.export)
                        || m.meta.attrs.js.contains(&JsEntry::Object)
                    {
                        self.err(
                            span,
                            format!(
                                "`@js(new = {})` applies to an imported constructor, not `{label}`",
                                path(p)
                            ),
                        );
                    }
                    if let Base::Module(spec) = &p.base {
                        let known = self
                            .ir
                            .items
                            .iter()
                            .any(|d| matches!(d, Decl::Module(b) if b.spec == *spec));
                        if !known {
                            self.err(span, format!("`@js(new = {})` names the module {spec:?}, which has no `module` block", path(p)));
                        }
                    }
                    "new"
                }
                JsEntry::Object => {
                    if !matches!(m.kind, MemberKind::New(_)) || !m.params.is_empty() {
                        self.err(span, format!("`@js(object)` applies to a constructor with no parameters, not `{label}`"));
                    }
                    "object"
                }
                JsEntry::Index => {
                    if !matches!(m.kind, MemberKind::Method(_))
                        || !(1..=2).contains(&m.params.len())
                    {
                        self.err(
                            span,
                            format!("`@js(index)` applies to a method taking an index, and a value to set; not `{label}`"),
                        );
                    }
                    "index"
                }
                JsEntry::Symbol(s) => {
                    if matches!(m.kind, MemberKind::New(_)) {
                        self.err(
                            span,
                            format!("`@js(symbol = {s})` applies to a method, getter or setter, not `{label}`"),
                        );
                    }
                    if m.meta
                        .attrs
                        .js
                        .iter()
                        .any(|e| matches!(e, JsEntry::Name(_)))
                    {
                        self.err(
                            span,
                            format!("`{label}`: `@js(symbol = {s})` names the JS member; it excludes `name`"),
                        );
                    }
                    if !WELL_KNOWN_SYMBOLS.contains(&s.as_str()) {
                        self.err(span, format!("`{s}` isn't a well-known symbol"));
                    }
                    "symbol"
                }
                JsEntry::This(p) => {
                    self.err(
                        span,
                        format!("`@js(this = {p})` on `{label}` isn't supported yet (IR.md §22)"),
                    );
                    "this"
                }
                JsEntry::Args(t) => {
                    args = Some(t);
                    "args"
                }
                JsEntry::Super(t) => {
                    let ok =
                        matches!(m.kind, MemberKind::New(_)) && owner.is_some_and(|c| c.export);
                    if !ok {
                        self.err(span, format!("`@js(super = ...)` applies to an exported class's constructor, not `{label}`"));
                    }
                    let mut scope = Scope::new(m, cx, &lets, false);
                    self.template(t, "super", &mut scope, span, false);
                    "super"
                }
                JsEntry::Let(name, e) => {
                    if incoming {
                        self.err(span, format!("`let {name}` on `{label}` binds nothing: an export's arguments come from JS"));
                    }
                    if params.contains(name.as_str()) || lets.contains_key(name.as_str()) {
                        self.err(span, format!("`let {name}` is already a name in `{label}`"));
                    }
                    let mut scope = Scope::new(m, cx, &lets, false);
                    let v = self.expr(e, &mut scope, span);
                    self.no_pointer_uses(&scope, span);
                    lets.insert(name, v);
                    continue;
                }
                JsEntry::Cancel(e) => {
                    if !matches!(m.result, Type::Future(..)) {
                        self.err(span, format!("`@js(cancel = ...)` needs a `future` result, which `{label}` lacks"));
                    }
                    let mut scope = Scope::new(m, cx, &lets, true);
                    self.expr(e, &mut scope, span);
                    self.no_pointer_uses(&scope, span);
                    "cancel"
                }
            };
            *kinds.entry(kind).or_default() += 1;
        }
        for (k, n) in kinds {
            if n > 1 {
                self.err(span, format!("`{label}` has more than one `@js({k} ...)`"));
            }
        }
        let mut scope = Scope::new(m, cx, &lets, false);
        if let Some(t) = args {
            self.template(t, "args", &mut scope, span, incoming);
        }
    }

    fn no_pointer_uses(&self, scope: &Scope<'_, 'a>, span: Span) {
        for p in &scope.member.params {
            if matches!(p.ty, Type::View { .. }) && scope.uses.contains_key(p.name.as_str()) {
                self.err(
                    span,
                    format!("the view `{}` is used only in `@js(args)`", p.name),
                );
            }
        }
    }

    fn template(
        &self,
        t: &'a [Telem],
        which: &str,
        scope: &mut Scope<'_, 'a>,
        span: Span,
        incoming: bool,
    ) {
        let label = member_label(scope.member);
        let mut fields = HashSet::new();
        for (i, e) in t.iter().enumerate() {
            match e {
                Telem::Param(n) => {
                    self.use_name(n, scope);
                }
                Telem::Lit(_) | Telem::Undefined => {}
                Telem::Spread(n) => {
                    if i + 1 != t.len() {
                        self.err(
                            span,
                            format!("`...{n}` must be the last element of `@js({which})`"),
                        );
                    }
                    match param_type(scope.member, n) {
                        Some(Type::Array(_) | Type::ArrBox(_) | Type::Vec(_)) => {
                            *scope
                                .uses
                                .entry(
                                    scope
                                        .member
                                        .params
                                        .iter()
                                        .find(|p| p.name == *n)
                                        .unwrap()
                                        .name
                                        .as_str(),
                                )
                                .or_default() += 1;
                        }
                        Some(_) => self.err(span, format!("`...{n}` spreads a sequence parameter")),
                        None => self.err(
                            span,
                            format!("`@js({which})`: `{label}` has no parameter `{n}`"),
                        ),
                    }
                }
                Telem::Fill { param, fields: fs } => {
                    match param_type(scope.member, param) {
                        Some(Type::Nullable(..)) => {
                            self.use_name(param, scope);
                        }
                        Some(_) => self.err(
                            span,
                            format!("`{param}? {{..}}` fills a nullable parameter"),
                        ),
                        None => self.err(
                            span,
                            format!("`@js({which})`: `{label}` has no parameter `{param}`"),
                        ),
                    }
                    fields.clear();
                    for (f, x) in fs {
                        if !fields.insert(f.as_str()) {
                            self.err(span, format!("`{param}? {{..}}` sets `{f}` twice"));
                        }
                        let bind = matches!(x, Expr::Ref { path, call: None }
                            if path.len() == 1 && param_type(scope.member, &path[0]).is_some());
                        if incoming && !bind {
                            self.pattern(x, scope, span);
                        }
                        self.expr(x, scope, span);
                    }
                }
                Telem::Expr(x) => {
                    if incoming {
                        self.pattern(x, scope, span);
                    }
                    self.expr(x, scope, span);
                }
            }
        }
        for p in &scope.member.params {
            if scope.uses.get(p.name.as_str()).copied().unwrap_or(0) > 1 {
                self.err(
                    span,
                    format!(
                        "`{}` appears more than once in `@js({which})`, so it can't be inverted",
                        p.name
                    ),
                );
            }
        }
    }

    /// A bare name in a template: a parameter (counted) or a `let`.
    fn use_name(&self, n: &'a str, scope: &mut Scope<'_, 'a>) -> bool {
        if let Some(p) = scope.member.params.iter().find(|p| p.name == n) {
            *scope.uses.entry(p.name.as_str()).or_default() += 1;
            true
        } else {
            if !scope.lets.contains_key(n) {
                self.err(
                    scope.member.meta.span,
                    format!(
                        "`{n}` isn't a parameter of `{}` or a `let`",
                        member_label(scope.member)
                    ),
                );
            }
            false
        }
    }

    /// An expression in an incoming template matches by `===`, so it's a
    /// constant: no parameters, `let`s or calls.
    fn pattern(&self, e: &Expr, scope: &Scope<'_, 'a>, span: Span) {
        let constant = match e {
            Expr::Lit(_) => true,
            Expr::Ref { path, call } => {
                call.is_none()
                    && path[0] != "$result"
                    && param_type(scope.member, &path[0]).is_none()
                    && !scope.lets.contains_key(path[0].as_str())
            }
        };
        if !constant {
            self.err(
                span,
                format!(
                    "`{}` in `@js(args)` of export `{}` can't be matched: an incoming pattern takes parameters, constants and constant paths",
                    crate::text::print_expr(e),
                    member_label(scope.member)
                ),
            );
        }
    }

    fn expr(&self, e: &'a Expr, scope: &mut Scope<'_, 'a>, span: Span) -> Value<'a> {
        let (segs, call) = match e {
            Expr::Lit(_) => return Value::Unknown,
            Expr::Ref { path, call } => (path.as_slice(), call.as_deref()),
        };
        let head = segs[0].as_str();
        let (mut value, rest) = if head == "$result" {
            if !scope.result {
                self.err(span, "`$result` is only in scope in `@js(cancel = ...)`");
            }
            (Value::Unknown, &segs[1..])
        } else if let Some(p) = scope.member.params.iter().find(|p| p.name == head) {
            *scope.uses.entry(p.name.as_str()).or_default() += 1;
            (Value::Instance(p.ty.clone(), scope.cx.clone()), &segs[1..])
        } else if let Some(v) = scope.lets.get(head) {
            (v.clone(), &segs[1..])
        } else {
            match self.static_head(segs, scope.cx) {
                Some((v, n)) => (v, &segs[n..]),
                None => {
                    if !self.opts.allow_unresolved {
                        self.err(span, format!("`{}` names nothing in scope", segs.join(".")));
                    }
                    (Value::Unknown, &segs[segs.len()..])
                }
            }
        };
        for (i, seg) in rest.iter().enumerate() {
            let last = i + 1 == rest.len();
            value = self.access(value, seg, last && call.is_some(), span);
        }
        if rest.is_empty() && call.is_some() {
            if let Value::Static(..) = value {
                self.err(span, format!("`{}` isn't callable", segs.join(".")));
            }
        }
        for a in call.into_iter().flatten() {
            self.expr(a, scope, span);
        }
        value
    }

    /// The longest prefix of `segs` naming a class.
    fn static_head(&self, segs: &[String], cx: &Context<'a>) -> Option<(Value<'a>, usize)> {
        (1..=segs.len()).rev().find_map(|n| {
            match self
                .symbols
                .resolve_decl(&Path::new(segs[..n].iter().cloned()), cx)?
            {
                (loc, TypeDecl::Class(c)) => Some((Value::Static(c, loc), n)),
                _ => None,
            }
        })
    }

    /// `value.seg`, called when `call`.
    fn access(&self, value: Value<'a>, seg: &str, call: bool, span: Span) -> Value<'a> {
        let (class, loc, is_static) = match value {
            Value::Unknown => return Value::Unknown,
            Value::Static(c, loc) => (c, loc, true),
            Value::Instance(t, cx) => match self.class_of(&t, &cx) {
                Some((c, loc)) => (c, loc, false),
                None => return Value::Unknown,
            },
        };
        let mut seen = HashSet::new();
        match self.find_member(class, &loc, seg, is_static, &mut seen) {
            Some((m, mcx)) => match &m.kind {
                MemberKind::New(_) => Value::Instance(
                    Type::Named(TypeRef {
                        path: Path::new(loc.path.iter().cloned()),
                        args: Vec::new(),
                    }),
                    Context {
                        module: mcx.module,
                        ..Context::default()
                    },
                ),
                MemberKind::Get(_) if !call => Value::Instance(m.result.clone(), mcx),
                MemberKind::Method(_) if call => Value::Instance(m.result.clone(), mcx),
                _ => Value::Unknown,
            },
            None => {
                let what = if is_static { "static member" } else { "member" };
                self.err(
                    span,
                    format!("`{}` has no {what} `{seg}`", crate::key::location(&loc)),
                );
                Value::Unknown
            }
        }
    }

    fn class_of(&self, t: &Type, cx: &Context<'a>) -> Option<(&'a Class, Location)> {
        let Type::Named(r) = t else {
            return None;
        };
        match self.symbols.resolve_decl(&r.path, cx)? {
            (loc, TypeDecl::Class(c)) => Some((c, loc)),
            _ => None,
        }
    }

    /// A member named `seg`, here or inherited through `extends`.
    fn find_member(
        &self,
        class: &'a Class,
        loc: &Location,
        seg: &str,
        is_static: bool,
        seen: &mut HashSet<*const Class>,
    ) -> Option<(&'a Member, Context<'a>)> {
        if !seen.insert(class) {
            return None;
        }
        let cx = context_of(loc, self.ir);
        let found = class.members.iter().find(|m| match &m.kind {
            MemberKind::New(None) => is_static && seg == "new",
            MemberKind::New(Some(n)) => is_static && seg == format!("new:{n}"),
            MemberKind::Method(n) | MemberKind::Get(n) => m.is_static == is_static && n == seg,
            MemberKind::Set(_) => false,
        });
        if let Some(m) = found {
            return Some((m, cx.with_class(class)));
        }
        class
            .extends
            .iter()
            .find_map(|e| match self.symbols.resolve_decl(&e.path, &cx)? {
                (bloc, TypeDecl::Class(b)) => self.find_member(b, &bloc, seg, is_static, seen),
                _ => None,
            })
    }

    fn throws(&self, p: &Path, cx: &Context<'a>, span: Span) {
        let e = match enums::resolve(&self.symbols, p, cx) {
            Some(e) => e,
            None => {
                let what = match self.symbols.resolve(p, cx) {
                    Some(_) => format!("`{}` isn't an enum", path(p)),
                    None if self.opts.allow_unresolved => return,
                    None => format!("`{}` isn't declared", path(p)),
                };
                self.err(span, format!("`throws {}`: {what}", path(p)));
                return;
            }
        };
        let key: *const Enum = e.decl;
        let cached = self.throws_ok.borrow().get(&key).copied();
        let ok = cached.unwrap_or_else(|| {
            let ok = match enums::flatten(&self.symbols, e.clone()) {
                // The enum's own errors are reported at its declaration.
                Err(_) => true,
                Ok(f) => {
                    f.fallback().is_some() && f.arms.iter().all(|a| a.arm.payload == [Type::StrBox])
                }
            };
            self.throws_ok.borrow_mut().insert(key, ok);
            ok
        });
        if !ok {
            self.err(
                span,
                format!("`{e}` is used in `throws`, so it must end in a `@fallback` and have only `(Str)` arms"),
            );
        }
    }

    fn ty(&self, t: &'a Type, pos: Pos, cx: &Context<'a>, span: Span) {
        let p = |message: String| self.err(span, message);
        let text = || crate::text::print_type(t);
        match t {
            Type::Never if !matches!(pos, Pos::Result | Pos::FutureValue) => {
                p("`never` is only a result".into())
            }
            Type::Never => {}
            Type::Unit
            | Type::Scalar(_)
            | Type::Str
            | Type::Any
            | Type::ScalarBox(_)
            | Type::StrBox
            | Type::BigInt
            | Type::BigIntBox
            | Type::UnitBox => {}
            Type::Vec(e) | Type::VecBox(e) => self.element(t, e, cx, span),
            Type::MutVec(e) => {
                if !matches!(pos, Pos::ImportParam | Pos::ExportParam) {
                    p(format!("`{}` is only a parameter type", text()));
                }
                self.element(t, e, cx, span);
            }
            Type::View { elem, .. } => {
                if pos != Pos::ImportParam {
                    p(format!("`{}` is only an import parameter type", text()));
                }
                self.element(t, elem, cx, span);
            }
            Type::Nullable(inner, js_null) => match &**inner {
                Type::Unit => p("`unit?` is invalid".into()),
                Type::Never => p("`never?` is invalid".into()),
                Type::Nullable(t, _) if matches!(**t, Type::Nullable(..)) => {
                    p(format!("`{}`: `?` nests once, as `T??`", text()))
                }
                Type::Nullable(_, inner_null) if *js_null || *inner_null => p(format!(
                    "`{}`: `@js(null)` doesn't apply to `T??`, whose inner absence is `null`",
                    text()
                )),
                Type::View { .. } => p(format!("`?` is invalid on a view: `{}`", text())),
                Type::MutVec(_) => self.ty(inner, pos, cx, span),
                _ => self.ty(inner, Pos::Inner, cx, span),
            },
            Type::Array(e) | Type::ArrBox(e) if **e == Type::Unit => {
                p(format!("`{}`: an array element can't be `unit`", text()))
            }
            Type::Array(e) | Type::ArrBox(e) => self.ty(e, Pos::Inner, cx, span),
            Type::Tuple(ts) | Type::TupleBox(ts) => {
                for e in ts {
                    self.ty(e, Pos::Inner, cx, span);
                }
            }
            Type::Future(v, e) => {
                self.ty(v, Pos::FutureValue, cx, span);
                if !e.is_err() {
                    self.throws(e, cx, span);
                }
            }
            Type::Fn(f) => {
                for a in &f.params {
                    self.ty(a, Pos::Inner, cx, span);
                }
                if f.rest && !matches!(f.params.last(), Some(Type::Array(_) | Type::ArrBox(_))) {
                    self.err(
                        span,
                        format!(
                            "`{}`: a rest parameter is an `arr<T>` or `Arr<T>`",
                            crate::text::print_type(t)
                        ),
                    );
                }
                self.ty(&f.result, Pos::Result, cx, span);
                if let Some(e) = &f.throws {
                    self.throws(e, cx, span);
                }
            }
            Type::Named(r) => self.named(r, cx, span),
        }
    }

    /// `seq`'s element `e` is a typed-array type; any other is an `arr<T>`.
    fn element(&self, seq: &Type, e: &'a Type, cx: &Context<'a>, span: Span) {
        if matches!(e, Type::Scalar(s) if is_vec_element(*s)) {
            return;
        }
        self.ty(e, Pos::Inner, cx, span);
        let e = crate::text::print_type(e);
        let arr = match seq {
            Type::VecBox(_) => format!("Arr<{e}>"),
            _ => format!("arr<{e}>"),
        };
        self.err(
            span,
            format!(
                "`{}`: the element must be a typed-array type; use `{arr}`",
                crate::text::print_type(seq)
            ),
        );
    }

    fn named(&self, r: &'a TypeRef, cx: &Context<'a>, span: Span) {
        for a in &r.args {
            self.ty(a, Pos::Inner, cx, span);
        }
        if let [head, _, ..] = r.path.segments.as_slice() {
            if r.path.base == Base::Lexical && is_builtin_name(head) {
                self.err(
                    span,
                    format!(
                        "`{}` starts with the built-in `{head}`, so it's written rooted or module-based (`.{}`)",
                        path(&r.path),
                        path(&r.path)
                    ),
                );
                return;
            }
        }
        let expected = match self.symbols.resolve(&r.path, cx) {
            Some(Target::TypeParam) | Some(Target::Assoc) | Some(Target::Err) => 0,
            Some(Target::Decl(_, TypeDecl::Enum(_))) => 0,
            Some(Target::Decl(_, TypeDecl::Class(c))) => c.tparams.len(),
            Some(Target::Decl(_, TypeDecl::Alias(a))) => {
                let least = a
                    .tparams
                    .iter()
                    .rposition(|p| p.default.is_none())
                    .map_or(0, |i| i + 1);
                if !self.in_alias.get()
                    && r.args.iter().any(|t| self.open(t, cx))
                    && self.open_back_edge(&Type::Named(r.clone()), cx, &mut Vec::new())
                {
                    self.err(
                        span,
                        format!(
                            "`{}` recurses over a type parameter, which has no layout",
                            path(&r.path)
                        ),
                    );
                }
                if r.args.len() < least || r.args.len() > a.tparams.len() {
                    self.err(
                        span,
                        format!(
                            "`{}` takes {least} to {} type arguments, not {}",
                            path(&r.path),
                            a.tparams.len(),
                            r.args.len()
                        ),
                    );
                }
                return;
            }
            None if self.opts.allow_unresolved => return,
            None => {
                self.err(span, format!("`{}` isn't declared", path(&r.path)));
                return;
            }
        };
        if r.args.len() != expected {
            self.err(
                span,
                format!(
                    "`{}` takes {expected} type arguments, not {}",
                    path(&r.path),
                    r.args.len()
                ),
            );
        }
    }

    /// Members whose keys collide in the same declaration (IR.md §7: the
    /// producer renames on conflict).
    fn keys(&self) {
        let keys = Keys::with_libs(self.ir, &self.libs);
        let mut seen: HashMap<(String, String), Span> = HashMap::new();
        for k in keys.members() {
            let role = match &k.role {
                Role::Import(m) => m.clone(),
                Role::Export => String::new(),
            };
            if seen
                .insert((role, k.key.clone()), k.member.meta.span)
                .is_some()
            {
                self.err(
                    k.member.meta.span,
                    format!("the key `{}` is declared twice", k.key),
                );
            }
        }
        // A derived arm variant that's also declared is the same call (§7).
        let members = keys.members();
        let imports: HashMap<(&str, &str), &Member> = members
            .iter()
            .filter_map(|k| match &k.role {
                Role::Import(m) => Some(((m.as_str(), k.key.as_str()), k.member)),
                Role::Export => None,
            })
            .collect();
        for k in &members {
            let Role::Import(module) = &k.role else {
                continue;
            };
            for (key, _) in crate::union::variants(&keys, k) {
                if let Some(real) = imports.get(&(module.as_str(), key.as_str())) {
                    if !crate::union::same_call(k.member, real) {
                        self.err(
                            real.meta.span,
                            format!(
                                "`{key}` is an arm variant of `{}`'s `union`, but a different JS call",
                                k.key
                            ),
                        );
                    }
                }
            }
        }
    }
}

impl<'m, 'a> Scope<'m, 'a> {
    fn new(
        member: &'a Member,
        cx: &'m Context<'a>,
        lets: &'m HashMap<&'a str, Value<'a>>,
        result: bool,
    ) -> Scope<'m, 'a> {
        Scope {
            member,
            cx,
            lets,
            result,
            uses: HashMap::new(),
        }
    }
}

/// The context a declaration at `loc` is written in.
fn context_of<'a>(loc: &Location, ir: &'a Ir) -> Context<'a> {
    let module = loc.module.as_deref().and_then(|spec| {
        ir.items.iter().find_map(|d| match d {
            Decl::Module(m) if m.spec == spec => Some(m.spec.as_str()),
            _ => None,
        })
    });
    let mut namespace = Vec::new();
    let mut items = match module {
        Some(spec) => ir
            .items
            .iter()
            .find_map(|d| match d {
                Decl::Module(m) if m.spec == spec => Some(&m.items),
                _ => None,
            })
            .unwrap(),
        None => &ir.items,
    };
    for seg in &loc.path[..loc.path.len() - 1] {
        let ns = items.iter().find_map(|d| match d {
            Decl::Namespace(n) if &n.name == seg => Some(n),
            _ => None,
        });
        match ns {
            Some(n) => {
                namespace.push(n.name.as_str());
                items = &n.items;
            }
            None => break,
        }
    }
    Context {
        module,
        namespace,
        ..Context::default()
    }
}

/// The module (`None` for the globals) and first segment of each
/// `@js(new = ...)` value among `items`, declared in `module`.
fn new_values<'a>(
    items: &'a [Decl],
    module: Option<&'a str>,
    out: &mut Vec<(Option<&'a str>, &'a str)>,
) {
    for d in items {
        match d {
            Decl::Class(c) => {
                for p in c.members.iter().filter_map(|m| m.meta.attrs.js.new_value()) {
                    let spec = match &p.base {
                        Base::Lexical => module,
                        Base::Root => None,
                        Base::Module(s) => Some(s.as_str()),
                    };
                    out.push((spec, &p.segments[0]));
                }
            }
            Decl::Namespace(n) => new_values(&n.items, module, out),
            Decl::Module(m) => new_values(&m.items, Some(&m.spec), out),
            _ => {}
        }
    }
}

fn path(p: &Path) -> String {
    crate::text::print_path(p)
}

fn member_label(m: &Member) -> String {
    match &m.kind {
        MemberKind::Method(n) => n.clone(),
        MemberKind::Get(n) => format!("get {n}"),
        MemberKind::Set(n) => format!("set {n}"),
        MemberKind::New(None) => "new".into(),
        MemberKind::New(Some(n)) => format!("new {n}"),
    }
}

fn param_type<'a>(m: &'a Member, name: &str) -> Option<&'a Type> {
    m.params.iter().find(|p| p.name == name).map(|p| &p.ty)
}
