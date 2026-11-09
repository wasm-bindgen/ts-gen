use super::lex::{Tok, Token};
use super::{describe, Error};
use crate::ir::*;

type Res<T> = Result<T, Error>;

pub(super) struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    /// A member's `@js(null)`, which its result takes.
    js_null: Option<Span>,
}

impl<'a> Parser<'a> {
    pub(super) fn new(toks: &'a [Token]) -> Self {
        Parser {
            toks,
            pos: 0,
            js_null: None,
        }
    }

    fn peek(&self) -> &'a Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &'a Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn here(&self) -> Span {
        self.toks[self.pos].span
    }

    fn err_at<T>(&self, at: Span, msg: impl Into<String>) -> Res<T> {
        Err(Error::new(at, msg))
    }

    fn err<T>(&self, msg: impl Into<String>) -> Res<T> {
        self.err_at(self.here(), msg)
    }

    fn unexpected<T>(&self, expected: &str) -> Res<T> {
        self.err(format!(
            "expected {expected}, found {}",
            describe(self.peek())
        ))
    }

    fn bump(&mut self) -> &'a Tok {
        let t = self.peek();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is(&self, p: &str) -> bool {
        is_p(self.peek(), p)
    }

    fn eat(&mut self, p: &str) -> bool {
        let yes = self.is(p);
        if yes {
            self.bump();
        }
        yes
    }

    fn expect(&mut self, p: &str) -> Res<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.unexpected(&format!("`{p}`"))
        }
    }

    fn is_kw(&self, kw: &str) -> bool {
        is_kw(self.peek(), kw)
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        let yes = self.is_kw(kw);
        if yes {
            self.bump();
        }
        yes
    }

    fn ident(&mut self, what: &str) -> Res<String> {
        match self.peek() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s.clone())
            }
            _ => self.unexpected(what),
        }
    }

    /// An identifier or a string.
    fn name(&mut self, what: &str) -> Res<String> {
        match self.peek() {
            Tok::Ident(s) | Tok::Str(s) => {
                self.bump();
                Ok(s.clone())
            }
            _ => self.unexpected(what),
        }
    }

    fn string(&mut self, what: &str) -> Res<String> {
        match self.peek() {
            Tok::Str(s) => {
                self.bump();
                Ok(s.clone())
            }
            _ => self.unexpected(what),
        }
    }

    fn is_name_at(&self, n: usize) -> bool {
        matches!(self.peek_at(n), Tok::Ident(_) | Tok::Str(_))
    }

    fn path(&mut self, what: &str) -> Res<Path> {
        let mut base = Base::Lexical;
        if self.is(".") && self.is_name_at(1) {
            self.bump();
            base = Base::Root;
        } else if let (Tok::Str(spec), true) = (self.peek(), is_p(self.peek_at(1), ".")) {
            base = Base::Module(spec.clone());
            self.bump();
            self.bump();
        }
        let mut segments = vec![self.name(what)?];
        while self.is(".") && self.is_name_at(1) {
            self.bump();
            segments.push(self.name(what)?);
        }
        Ok(Path { base, segments })
    }

    /// `a, b, c` up to (not including) `close`, allowing a trailing comma
    /// when `trailing` is set.
    fn list<T>(
        &mut self,
        close: &str,
        trailing: bool,
        mut item: impl FnMut(&mut Self) -> Res<T>,
    ) -> Res<Vec<T>> {
        let mut out = Vec::new();
        if self.is(close) {
            return Ok(out);
        }
        loop {
            out.push(item(self)?);
            if !self.eat(",") {
                break;
            }
            if trailing && self.is(close) {
                break;
            }
        }
        Ok(out)
    }

    pub(super) fn file(&mut self) -> Res<Ir> {
        if !self.eat_kw("ir") {
            return self.err("expected the `ir 1;` header");
        }
        let at = self.here();
        match self.bump() {
            Tok::Int(1) => {}
            Tok::Int(v) => return self.err_at(at, format!("IR version {v}; this reader reads 1")),
            _ => return self.err_at(at, "expected a version number"),
        }
        self.expect(";")?;
        let lib = if self.is_header("lib") {
            self.bump();
            Some(self.header_name()?)
        } else {
            None
        };
        let mut with = Vec::new();
        while self.is_header("with") {
            self.bump();
            with.push(self.header_name()?);
        }
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            items.push(self.decl()?);
        }
        self.stray_null()?;
        Ok(Ir { lib, with, items })
    }

    /// `kw name;`, rather than a member named `kw`.
    fn is_header(&self, kw: &str) -> bool {
        self.is_kw(kw)
            && matches!(self.peek_at(1), Tok::Ident(_))
            && *self.peek_at(2) == Tok::P(";")
    }

    fn header_name(&mut self) -> Res<String> {
        let name = self.ident("a lib name")?;
        self.expect(";")?;
        Ok(name)
    }

    fn docs(&mut self) -> Option<String> {
        let mut lines: Vec<&str> = Vec::new();
        while let Tok::Doc(d) = self.peek() {
            lines.push(d);
            self.bump();
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    fn meta(&mut self) -> Res<Meta> {
        self.stray_null()?;
        let doc = self.docs();
        let span = self.here();
        let mut attrs = Attrs::default();
        while self.is("@") {
            let at = self.here();
            self.bump();
            match self.ident("an attribute name")?.as_str() {
                "js" => {
                    self.expect("(")?;
                    let entries = self.list(")", true, Self::js_entry)?;
                    if entries.is_empty() {
                        return self.err_at(at, "empty `@js()`");
                    }
                    for e in entries {
                        match e {
                            Some(e) => attrs.js.push(e),
                            None => self.js_null = Some(at),
                        }
                    }
                    self.expect(")")?;
                }
                "fallback" => attrs.fallback = true,
                "once" => attrs.once = true,
                other => return self.err_at(at, format!("unknown attribute `@{other}`")),
            }
        }
        if let Tok::Doc(_) = self.peek() {
            return self.err("a doc comment must come before the attributes");
        }
        Ok(Meta { doc, attrs, span })
    }

    fn stray_null(&mut self) -> Res<()> {
        match self.js_null.take() {
            Some(at) => self.err_at(at, "`@js(null)` goes on a member or a `T?`"),
            None => Ok(()),
        }
    }

    /// `@js(null)` on `t`, which must be a `T?`.
    fn with_null(&self, at: Span, t: Type) -> Res<Type> {
        match t {
            Type::Nullable(t, _) => Ok(Type::Nullable(t, true)),
            t => self.err_at(
                at,
                format!(
                    "`@js(null)` applies to a `T?`, not `{}`",
                    super::print::ty(&t)
                ),
            ),
        }
    }

    fn decl(&mut self) -> Res<Decl> {
        let meta = self.meta()?;
        let export = self.is_kw("export")
            && ["namespace", "class", "interface"]
                .iter()
                .any(|k| is_kw(self.peek_at(1), k))
            && self.is_name_at(2);
        if export {
            self.bump();
        }
        let named = self.is_name_at(1);
        if named && self.eat_kw("namespace") {
            let name = self.name("a namespace name")?;
            self.expect("{")?;
            let mut items = Vec::new();
            while !self.eat("}") {
                items.push(self.decl()?);
            }
            return Ok(Decl::Namespace(Namespace {
                meta,
                export,
                name,
                items,
            }));
        }
        if named && (self.is_kw("class") || self.is_kw("interface")) {
            let kind = if self.eat_kw("class") {
                ClassKind::Class
            } else {
                self.bump();
                if export {
                    return self.err_at(meta.span, "an interface can't be exported");
                }
                ClassKind::Interface
            };
            return self.class(meta, kind, export).map(Decl::Class);
        }
        if named && self.eat_kw("enum") {
            return self.enumeration(meta).map(Decl::Enum);
        }
        if named && self.is_kw("type") && ["=", "<"].iter().any(|p| is_p(self.peek_at(2), p)) {
            self.bump();
            let name = self.name("an alias name")?;
            let tparams = self.tparams()?;
            self.expect("=")?;
            let ty = self.ty()?;
            self.expect(";")?;
            return Ok(Decl::Alias(Alias {
                meta,
                name,
                tparams,
                ty,
            }));
        }
        if self.is_kw("module") && matches!(self.peek_at(1), Tok::Str(_)) {
            self.bump();
            return self.module(meta).map(Decl::Module);
        }
        self.member(meta).map(Decl::Member)
    }

    fn module(&mut self, meta: Meta) -> Res<Module> {
        let spec = self.string("a module specifier")?;
        self.expect("{")?;
        let mut items = Vec::new();
        let mut exports = None;
        while !self.eat("}") {
            if self.is_kw("export") && is_p(self.peek_at(1), "{") {
                let at = self.here();
                self.bump();
                self.bump();
                let names = self.list("}", false, |p| {
                    let name = p.name("an exported declaration")?;
                    let alias = if p.eat_kw("as") {
                        Some(p.name("an export name")?)
                    } else {
                        None
                    };
                    Ok(ExportName { name, alias })
                })?;
                self.expect("}")?;
                self.expect(";")?;
                if names.is_empty() {
                    return self.err_at(at, "an empty export list");
                }
                exports = Some(names);
                self.expect("}")?;
                break;
            }
            items.push(self.decl()?);
        }
        Ok(Module {
            meta,
            spec,
            items,
            exports,
        })
    }

    fn class(&mut self, meta: Meta, kind: ClassKind, export: bool) -> Res<Class> {
        let name = self.name("a class name")?;
        let tparams = self.tparams()?;
        let mut extends = Vec::new();
        if self.eat_kw("extends") {
            loop {
                extends.push(self.typeref()?);
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect("{")?;
        let mut assoc = Vec::new();
        let mut members = Vec::new();
        while !self.eat("}") {
            let meta = self.meta()?;
            if self.is_kw("type") && self.is_name_at(1) && is_p(self.peek_at(2), "=") {
                self.bump();
                assoc.push(self.assoc(meta)?);
                continue;
            }
            members.push(self.member(meta)?);
        }
        Ok(Class {
            meta,
            kind,
            export,
            name,
            tparams,
            extends,
            assoc,
            members,
        })
    }

    /// `Name = match P { A => T, B => U };`, after `type`.
    fn assoc(&mut self, meta: Meta) -> Res<Assoc> {
        let name = self.name("an associated type name")?;
        self.expect("=")?;
        if !self.eat_kw("match") {
            return self.unexpected("`match`");
        }
        let param = self.ident("a type parameter")?;
        let at = self.here();
        self.expect("{")?;
        let rows = self.list("}", true, |p| {
            let key = p.ty()?;
            p.expect("=>")?;
            Ok((key, p.ty()?))
        })?;
        if rows.is_empty() {
            return self.err_at(at, "an empty `match`");
        }
        self.expect("}")?;
        self.expect(";")?;
        Ok(Assoc {
            meta,
            name,
            param,
            rows,
        })
    }

    fn tparams(&mut self) -> Res<Vec<TypeParam>> {
        if !self.eat("<") {
            return Ok(Vec::new());
        }
        let params = self.list(">", false, |p| {
            let name = p.ident("a type parameter")?;
            let bound = if p.eat(":") {
                Some(Bound::Subtype(p.ty()?))
            } else if p.eat_kw("in") {
                p.expect("(")?;
                let set = p.list(")", false, Self::ty)?;
                p.expect(")")?;
                if set.is_empty() {
                    return p.err("an empty concrete bound");
                }
                Some(Bound::In(set))
            } else {
                None
            };
            let default = if p.eat("=") { Some(p.ty()?) } else { None };
            Ok(TypeParam {
                name,
                bound,
                default,
            })
        })?;
        self.expect(">")?;
        if params.is_empty() {
            return self.err("an empty type parameter list");
        }
        Ok(params)
    }

    fn enumeration(&mut self, meta: Meta) -> Res<Enum> {
        let name = self.name("an enum name")?;
        let mut carrier = None;
        let mut extends = Vec::new();
        if self.eat(":") {
            let at = self.here();
            let t = self.ident("an integer carrier")?;
            match Scalar::from_name(&t).filter(|s| s.is_int_carrier()) {
                Some(s) => carrier = Some(s),
                None => return self.err_at(at, format!("`{t}` is not an integer carrier")),
            }
        } else if self.eat_kw("extends") {
            loop {
                extends.push(self.path("a base enum")?);
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect("{")?;
        let mut arms = Vec::new();
        let mut members = Vec::new();
        while !self.eat("}") {
            let meta = self.meta()?;
            if self.is_kw("static") {
                members.push(self.member(meta)?);
                continue;
            }
            let name = self.name("an arm name")?;
            let mut payload = Vec::new();
            let at = self.here();
            if self.eat("(") {
                payload = self.list(")", false, Self::ty)?;
                if payload.is_empty() {
                    return self.err_at(at, "an empty payload; write a unit arm without `()`");
                }
                self.expect(")")?;
            }
            let value = if self.eat("=") {
                if self.eat(".") {
                    Some(ArmValue::Alias(self.name("an arm name")?))
                } else if let Tok::Int(n) = self.peek() {
                    self.bump();
                    Some(ArmValue::Int(*n))
                } else {
                    return self.unexpected("an integer or `.Arm`");
                }
            } else {
                None
            };
            self.expect(";")?;
            arms.push(Arm {
                meta,
                name,
                payload,
                value,
            });
        }
        Ok(Enum {
            meta,
            name,
            carrier,
            extends,
            arms,
            members,
        })
    }

    /// A modifier keyword is a modifier unless it is itself the member name.
    fn modifier(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) && !is_p(self.peek_at(1), "(") && !is_p(self.peek_at(1), "<") {
            self.bump();
            true
        } else {
            false
        }
    }

    fn member(&mut self, meta: Meta) -> Res<Member> {
        let export = self.modifier("export");
        let is_static = self.modifier("static");
        let accessor = self.is_name_at(1) && is_p(self.peek_at(2), "(");
        let (kind, tparams) = if accessor && self.eat_kw("get") {
            (MemberKind::Get(self.name("a property name")?), Vec::new())
        } else if accessor && self.eat_kw("set") {
            (MemberKind::Set(self.name("a property name")?), Vec::new())
        } else if self.is_kw("new") && is_p(self.peek_at(1), "(") {
            self.bump();
            (MemberKind::New(None), Vec::new())
        } else if accessor && self.eat_kw("new") {
            (
                MemberKind::New(Some(self.name("a constructor name")?)),
                Vec::new(),
            )
        } else {
            let name = self.name("a member")?;
            (MemberKind::Method(name), self.tparams()?)
        };
        self.expect("(")?;
        let params = self.list(")", false, Self::param)?;
        self.expect(")")?;
        let result = match kind {
            MemberKind::Get(_) => {
                if !params.is_empty() {
                    return self.err_at(params[0].span, "a getter takes no parameters");
                }
                self.expect(":")?;
                self.ty()?
            }
            MemberKind::Set(_) if params.len() > 1 => {
                return self.err_at(params[1].span, "a setter takes at most one parameter");
            }
            MemberKind::Method(_) if self.eat(":") => self.ty()?,
            _ => Type::Unit,
        };
        // A member's `@js(null)` is its result's, or its `future<T?>`'s.
        let result = match (self.js_null.take(), result) {
            (Some(at), Type::Future(v, e)) => Type::Future(Box::new(self.with_null(at, *v)?), e),
            (Some(at), t) => self.with_null(at, t)?,
            (None, t) => t,
        };
        let throws = self.throws()?;
        self.expect(";")?;
        Ok(Member {
            meta,
            export,
            is_static,
            kind,
            tparams,
            params,
            result,
            throws,
        })
    }

    fn throws(&mut self) -> Res<Option<Path>> {
        if !self.eat_kw("throws") {
            return Ok(None);
        }
        if self.is_name_at(0) {
            self.path("an error enum").map(Some)
        } else {
            Ok(Some(Path::err()))
        }
    }

    fn param(&mut self) -> Res<Param> {
        let span = self.here();
        let (mut js_null, mut once, mut unshared) = (None, false, false);
        while self.is("@") {
            let at = self.here();
            self.bump();
            match self.ident("an attribute name")?.as_str() {
                "js" => {
                    self.expect("(")?;
                    if self.eat_kw("null") {
                        js_null = Some(at);
                    } else if self.eat_kw("unshared") {
                        unshared = true;
                    } else {
                        return self.err_at(
                            at,
                            "only `@js(null)` and `@js(unshared)` are allowed on a parameter",
                        );
                    }
                    self.expect(")")?;
                }
                "once" => once = true,
                other => {
                    return self.err_at(at, format!("`@{other}` is not allowed on a parameter"))
                }
            }
        }
        let name = self.name("a parameter name")?;
        self.expect(":")?;
        // `@js(null) union E?`: the type-position attribute ahead of `union`.
        let keyword = |p: &Self, at: usize| {
            is_kw(p.peek_at(at), "union") && !matches!(p.peek_at(at + 1), Tok::P("," | ")" | "="))
        };
        if js_null.is_none()
            && self.is("@")
            && is_kw(self.peek_at(1), "js")
            && is_p(self.peek_at(2), "(")
            && is_kw(self.peek_at(3), "null")
            && is_p(self.peek_at(4), ")")
            && keyword(self, 5)
        {
            js_null = Some(self.here());
            for _ in 0..5 {
                self.bump();
            }
        }
        let union = keyword(self, 0);
        if union {
            self.bump();
        }
        let mut ty = self.ty()?;
        if let Some(at) = js_null {
            ty = self.with_null(at, ty)?;
        }
        Ok(Param {
            name,
            ty,
            once,
            unshared,
            union,
            span,
        })
    }

    pub(super) fn end(&mut self) -> Res<()> {
        self.stray_null()?;
        match self.peek() {
            Tok::Eof => Ok(()),
            _ => self.unexpected("the end of the input"),
        }
    }

    pub(super) fn ty(&mut self) -> Res<Type> {
        if self.is("@") && is_kw(self.peek_at(1), "js") && is_kw(self.peek_at(3), "null") {
            let at = self.here();
            for _ in 0..4 {
                self.bump();
            }
            self.expect(")")?;
            let t = self.ty()?;
            return self.with_null(at, t);
        }
        let mut t = self.prefix()?;
        loop {
            if self.eat("?") {
                t = Type::Nullable(Box::new(t), false);
            } else if self.eat_brackets() {
                t = Type::Vec(Box::new(t));
            } else {
                return Ok(t);
            }
        }
    }

    fn eat_brackets(&mut self) -> bool {
        let at = self.is("[") && is_p(self.peek_at(1), "]");
        if at {
            self.bump();
            self.bump();
        }
        at
    }

    /// `*` and `mut` take a whole `T[]`.
    fn whole_vec(&mut self, what: &str) -> Res<Box<Type>> {
        let elem = self.primary()?;
        if !self.eat_brackets() {
            return self.err(format!("`{what}` applies to a whole `T[]`"));
        }
        Ok(Box::new(elem))
    }

    fn prefix(&mut self) -> Res<Type> {
        if self.eat("*") {
            let mutable = self.eat_kw("mut");
            let elem = self.whole_vec(if mutable { "*mut" } else { "*" })?;
            return Ok(Type::View { mutable, elem });
        }
        if self.is_kw("mut") && (self.is_name_at(1) || is_p(self.peek_at(1), "(")) {
            self.bump();
            return Ok(Type::MutVec(self.whole_vec("mut")?));
        }
        if self.is_kw("fn") && is_p(self.peek_at(1), "(") {
            self.bump();
            self.bump();
            let mut rest = false;
            let params = self.list(")", false, |p| {
                if rest {
                    return p.err("a rest parameter `...arr<T>` must be the last parameter");
                }
                rest = p.eat("...");
                p.ty()
            })?;
            self.expect(")")?;
            let result = if self.eat("->") {
                self.ty()?
            } else {
                Type::Unit
            };
            let throws = self.throws()?;
            return Ok(Type::Fn(Box::new(FnType {
                params,
                rest,
                result,
                throws,
            })));
        }
        self.primary()
    }

    /// `<T>`.
    fn angle1(&mut self) -> Res<Type> {
        self.expect("<")?;
        let t = self.ty()?;
        self.expect(">")?;
        Ok(t)
    }

    /// `<A, B, ...>`, at least one.
    fn angles(&mut self) -> Res<Vec<Type>> {
        self.expect("<")?;
        let ts = self.list(">", false, Self::ty)?;
        if ts.is_empty() {
            return self.err("an empty type argument list");
        }
        self.expect(">")?;
        Ok(ts)
    }

    fn primary(&mut self) -> Res<Type> {
        if self.is("(") {
            let at = self.here();
            self.bump();
            if self.is(")") {
                return self.err_at(at, "`()` is not a type; tuples have at least one element");
            }
            let first = self.ty()?;
            if self.eat(")") {
                return Ok(first);
            }
            self.expect(",")?;
            let mut elems = vec![first];
            if !self.eat(")") {
                elems.extend(self.list(")", false, Self::ty)?);
                self.expect(")")?;
            }
            return Ok(Type::Tuple(elems));
        }
        let Tok::Ident(id) = self.peek() else {
            if matches!(self.peek(), Tok::Str(_)) || self.is(".") && self.is_name_at(1) {
                return self.typeref().map(Type::Named);
            }
            return self.unexpected("a type");
        };
        let angled = is_p(self.peek_at(1), "<");
        if let Some(s) = Scalar::from_name(id) {
            self.bump();
            return Ok(Type::Scalar(s));
        }
        if let Some(s) = Scalar::from_box_name(id) {
            self.bump();
            return Ok(Type::ScalarBox(s));
        }
        let simple = match id.as_str() {
            "unit" => Some(Type::Unit),
            "never" => Some(Type::Never),
            "str" => Some(Type::Str),
            "Str" => Some(Type::StrBox),
            "bigint" => Some(Type::BigInt),
            "BigInt" => Some(Type::BigIntBox),
            "Unit" => Some(Type::UnitBox),
            "Any" => Some(Type::Any),
            _ => None,
        };
        if let Some(t) = simple {
            self.bump();
            return Ok(t);
        }
        if angled {
            let wrap: Option<fn(Box<Type>) -> Type> = match id.as_str() {
                "arr" => Some(Type::Array),
                "Vec" => Some(Type::VecBox),
                "Arr" => Some(Type::ArrBox),
                _ => None,
            };
            if let Some(wrap) = wrap {
                self.bump();
                return Ok(wrap(Box::new(self.angle1()?)));
            }
            match id.as_str() {
                "Tuple" => {
                    self.bump();
                    return Ok(Type::TupleBox(self.angles()?));
                }
                "future" => {
                    self.bump();
                    self.expect("<")?;
                    let value = self.ty()?;
                    let error = if self.eat(",") {
                        self.path("an error enum")?
                    } else {
                        Path::err()
                    };
                    self.expect(">")?;
                    return Ok(Type::Future(Box::new(value), error));
                }
                _ => {}
            }
        }
        if matches!(
            id.as_str(),
            "arr" | "Vec" | "Arr" | "Tuple" | "future" | "fn"
        ) {
            return self.err(format!("`{id}` needs type arguments"));
        }
        self.typeref().map(Type::Named)
    }

    fn typeref(&mut self) -> Res<TypeRef> {
        let path = self.path("a type")?;
        let args = if self.is("<") {
            self.angles()?
        } else {
            Vec::new()
        };
        Ok(TypeRef { path, args })
    }

    /// `None` for `null`, which isn't an entry: a member's result takes it.
    fn js_entry(&mut self) -> Res<Option<JsEntry>> {
        if let Tok::Str(s) = self.peek() {
            self.bump();
            return Ok(Some(JsEntry::Literal(s.clone())));
        }
        if self.is_kw("null") && matches!(self.peek_at(1), Tok::P(",") | Tok::P(")")) {
            self.bump();
            return Ok(None);
        }
        self.js_entry_some().map(Some)
    }

    fn js_entry_some(&mut self) -> Res<JsEntry> {
        let keyed = is_p(self.peek_at(1), "=");
        let bare = matches!(self.peek_at(1), Tok::P(",") | Tok::P(")"));
        let Tok::Ident(id) = self.peek() else {
            return self.unexpected("a `@js` entry");
        };
        match id.as_str() {
            "name" if keyed => {
                self.bump();
                self.bump();
                Ok(JsEntry::Name(self.string("a JS name")?))
            }
            "symbol" if keyed => {
                self.bump();
                self.bump();
                Ok(JsEntry::Symbol(self.ident("a well-known symbol")?))
            }
            "this" if keyed => {
                self.bump();
                self.bump();
                Ok(JsEntry::This(self.ident("a parameter")?))
            }
            "args" | "super" if keyed => {
                let is_args = id == "args";
                self.bump();
                self.bump();
                self.expect("(")?;
                let t = self.list(")", true, Self::telem)?;
                self.expect(")")?;
                Ok(if is_args {
                    JsEntry::Args(t)
                } else {
                    JsEntry::Super(t)
                })
            }
            "cancel" if keyed => {
                self.bump();
                self.bump();
                Ok(JsEntry::Cancel(self.expr()?))
            }
            "new" if keyed => {
                self.bump();
                self.bump();
                Ok(JsEntry::New(self.path("a JS value")?))
            }
            "let" if matches!(self.peek_at(1), Tok::Ident(_)) && is_p(self.peek_at(2), "=") => {
                self.bump();
                let name = self.ident("a binding")?;
                self.bump();
                Ok(JsEntry::Let(name, self.expr()?))
            }
            "object" if bare => {
                self.bump();
                Ok(JsEntry::Object)
            }
            "index" if bare => {
                self.bump();
                Ok(JsEntry::Index)
            }
            _ => {
                let mut preds = Vec::new();
                loop {
                    let class = self.path("an error class")?;
                    let mut props = Vec::new();
                    if self.eat("[") {
                        props = self.list("]", false, |p| {
                            let k = p.ident("a property")?;
                            p.expect("=")?;
                            Ok((k, p.literal()?))
                        })?;
                        self.expect("]")?;
                    }
                    preds.push(ClassPred { class, props });
                    if !self.eat("|") {
                        break;
                    }
                }
                Ok(JsEntry::Class(preds))
            }
        }
    }

    fn try_literal(&mut self) -> Option<Literal> {
        let lit = match self.peek() {
            Tok::Str(s) => Literal::Str(s.clone()),
            Tok::Int(n) => Literal::Int(*n),
            Tok::Ident(s) if s == "true" => Literal::Bool(true),
            Tok::Ident(s) if s == "false" => Literal::Bool(false),
            Tok::Ident(s) if s == "null" => Literal::Null,
            _ => return None,
        };
        self.bump();
        Some(lit)
    }

    fn literal(&mut self) -> Res<Literal> {
        match self.try_literal() {
            Some(l) => Ok(l),
            None => self.unexpected("a literal"),
        }
    }

    fn telem(&mut self) -> Res<Telem> {
        if self.eat("...") {
            return Ok(Telem::Spread(self.ident("a sequence parameter")?));
        }
        if let Some(l) = self.try_literal() {
            return Ok(Telem::Lit(l));
        }
        let Tok::Ident(id) = self.peek() else {
            return self.unexpected("a template element");
        };
        let next = self.peek_at(1);
        if is_p(next, ".") || is_p(next, "(") {
            return Ok(Telem::Expr(self.expr()?));
        }
        if is_p(next, "?") {
            let param = id.clone();
            self.bump();
            self.bump();
            self.expect("{")?;
            let fields = self.list("}", false, |p| {
                let k = p.name("a field")?;
                p.expect("=")?;
                Ok((k, p.expr()?))
            })?;
            self.expect("}")?;
            return Ok(Telem::Fill { param, fields });
        }
        self.bump();
        Ok(if id == "undefined" {
            Telem::Undefined
        } else {
            Telem::Param(id.clone())
        })
    }

    fn expr(&mut self) -> Res<Expr> {
        if let Some(l) = self.try_literal() {
            return Ok(Expr::Lit(l));
        }
        let mut path = vec![self.ident("an expression")?];
        while self.is(".") && self.is_name_at(1) {
            self.bump();
            path.push(self.name("a member")?);
        }
        let call = if self.eat("(") {
            let args = self.list(")", false, Self::expr)?;
            self.expect(")")?;
            Some(args)
        } else {
            None
        };
        Ok(Expr::Ref { path, call })
    }
}

fn is_p(t: &Tok, p: &str) -> bool {
    matches!(t, Tok::P(q) if *q == p)
}

fn is_kw(t: &Tok, kw: &str) -> bool {
    matches!(t, Tok::Ident(s) if s == kw)
}
