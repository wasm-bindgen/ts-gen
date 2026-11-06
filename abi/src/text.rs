//! The IR text format, specified in IR.md: [`emit`] and [`parse`] are its
//! reference writer and reader, and `parse(&emit(ir)) == Ok(ir)`.

use std::collections::HashSet;
use std::fmt::Write;

use crate::ir::{
    Arm, Class, ClassKind, Function, Head, Ir, JsType, Kind, Literal, Member, MemberKind, Named,
    Param, Provide, Type,
};
use crate::OwnerRef;

/// The format version `emit` writes and `parse` reads.
pub const VERSION: u32 = 1;

/// Declared class and kind binding names, which bare names resolve to.
#[derive(Default)]
struct Names {
    decls: HashSet<(Option<String>, String)>,
}

/// What a bare (unqualified) type name means in a scope.
#[derive(Debug, PartialEq)]
enum Bare {
    Wasm(Type),
    Head(Head),
    Null,
    Undefined,
    Ref(OwnerRef),
    Unknown,
}

impl Names {
    fn of(ir: &Ir) -> Names {
        let decls = ir
            .classes
            .iter()
            .map(|c| (c.module.clone(), c.name.clone()))
            .chain(ir.kinds.iter().map(|k| (k.module.clone(), k.name.clone())))
            .collect();
        Names { decls }
    }

    fn has(&self, module: Option<&str>, path: &str) -> bool {
        self.decls
            .contains(&(module.map(String::from), path.to_string()))
    }

    fn bare(&self, path: &str, scope: &Scope) -> Bare {
        if !path.contains('.') {
            let fixed = match path {
                "void" => Some(Bare::Wasm(Type::Void)),
                "f64" => Some(Bare::Wasm(Type::F64)),
                "i64" => Some(Bare::Wasm(Type::I64)),
                "bool" => Some(Bare::Wasm(Type::Bool)),
                "Any" => Some(Bare::Head(Head::Any)),
                "String" => Some(Bare::Head(Head::String)),
                "Number" => Some(Bare::Head(Head::Number)),
                "Boolean" => Some(Bare::Head(Head::Boolean)),
                "BigInt" => Some(Bare::Head(Head::BigInt)),
                "Null" => Some(Bare::Null),
                "Undefined" => Some(Bare::Undefined),
                _ => None,
            };
            if let Some(b) = fixed {
                return b;
            }
            if scope.params.iter().any(|p| p == path) {
                return Bare::Head(Head::Var(path.into()));
            }
        }
        if let Some(m) = scope.module {
            if self.has(Some(m), path) {
                return Bare::Ref(OwnerRef {
                    module: Some(m.into()),
                    path: path.into(),
                });
            }
        }
        if self.has(None, path) || path.starts_with(|c: char| c.is_uppercase()) {
            return Bare::Ref(OwnerRef::global(path));
        }
        Bare::Unknown
    }
}

/// Where a type is written: the enclosing module and the class and member
/// type parameters.
#[derive(Clone, Copy)]
struct Scope<'a> {
    module: Option<&'a str>,
    params: &'a [String],
}

const GLOBAL: Scope<'static> = Scope {
    module: None,
    params: &[],
};

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An identifier as is, anything else as a string.
fn name(s: &str) -> String {
    if is_ident(s) {
        s.into()
    } else {
        quote(s)
    }
}

/// A member's JS name, quoting `new`, which would read as a constructor.
fn member_name(s: &str) -> String {
    if s == "new" {
        quote(s)
    } else {
        name(s)
    }
}

/// The result a constructor or `new {}` has when the text omits it.
fn class_type(module: Option<&str>, path: &str, params: &[String]) -> Type {
    Type::js(Head::Named(Named {
        name: OwnerRef {
            module: module.map(String::from),
            path: path.into(),
        },
        args: params
            .iter()
            .map(|p| JsType {
                head: Head::Var(p.clone()),
                null: false,
                undefined: false,
            })
            .collect(),
    }))
}

fn doc(out: &mut String, indent: &str, doc: &Option<String>) {
    if let Some(d) = doc {
        for line in d.split('\n') {
            if line.is_empty() {
                let _ = writeln!(out, "{indent}///");
            } else {
                let _ = writeln!(out, "{indent}/// {line}");
            }
        }
    }
}

struct Writer {
    names: Names,
}

#[derive(Clone, Copy, PartialEq)]
enum Item<'a> {
    Class(&'a Class),
    Kind(&'a Kind),
    Function(&'a Function),
}

impl<'a> Item<'a> {
    fn module_of(self) -> Option<&'a str> {
        match self {
            Item::Class(c) => c.module.as_deref(),
            Item::Kind(k) => k.module.as_deref(),
            Item::Function(f) => f.module.as_deref(),
        }
    }

    fn has_doc(self) -> bool {
        match self {
            Item::Class(c) => c.doc.is_some(),
            Item::Kind(k) => k.doc.is_some(),
            Item::Function(f) => f.member.doc.is_some(),
        }
    }
}

/// Write `ir` in the text format.
pub fn emit(ir: &Ir) -> String {
    let w = Writer {
        names: Names::of(ir),
    };
    let mut out = format!("ir {VERSION};\n");
    let items: Vec<Item> = ir
        .classes
        .iter()
        .map(Item::Class)
        .chain(ir.kinds.iter().map(Item::Kind))
        .chain(ir.functions.iter().map(Item::Function))
        .collect();
    for group in items.chunk_by(|a, b| a.module_of() == b.module_of()) {
        let module = group[0].module_of();
        out.push('\n');
        let indent = match module {
            Some(m) => {
                let _ = writeln!(out, "module {} {{", quote(m));
                "  "
            }
            None => "",
        };
        let scope = Scope {
            module,
            params: &[],
        };
        for (i, item) in group.iter().enumerate() {
            if i > 0 {
                let prev = group[i - 1];
                if !matches!((prev, item), (Item::Function(_), Item::Function(_))) || item.has_doc()
                {
                    out.push('\n');
                }
            }
            match *item {
                Item::Class(c) => w.class(&mut out, indent, c),
                Item::Kind(k) => w.kind(&mut out, indent, k, scope),
                Item::Function(f) => w.function(&mut out, indent, f, scope),
            }
        }
        if module.is_some() {
            out.push_str("}\n");
        }
    }
    for (i, p) in ir.provides.iter().enumerate() {
        if i == 0 || p.doc.is_some() {
            out.push('\n');
        }
        w.provide(&mut out, p);
    }
    out
}

impl Writer {
    fn reference(&self, r: &OwnerRef, scope: Scope) -> String {
        if self.names.bare(&r.path, &scope) == Bare::Ref(r.clone()) {
            return r.path.clone();
        }
        match &r.module {
            Some(m) => format!("{}.{}", quote(m), r.path),
            None => format!("globalThis.{}", r.path),
        }
    }

    fn named(&self, n: &Named, scope: Scope) -> String {
        let mut s = self.reference(&n.name, scope);
        if !n.args.is_empty() {
            let args: Vec<String> = n.args.iter().map(|a| self.js(a, scope)).collect();
            let _ = write!(s, "<{}>", args.join(", "));
        }
        s
    }

    fn head(&self, h: &Head, scope: Scope) -> String {
        match h {
            Head::Any => "Any".into(),
            Head::String => "String".into(),
            Head::Number => "Number".into(),
            Head::Boolean => "Boolean".into(),
            Head::BigInt => "BigInt".into(),
            Head::Named(n) => self.named(n, scope),
            Head::Var(v) => v.clone(),
            Head::Callback { params, result } => {
                let params: Vec<String> = params.iter().map(|p| self.ty(p, scope)).collect();
                format!("({}) => {}", params.join(", "), self.ty(result, scope))
            }
            Head::Tuple(items) => {
                let items: Vec<String> = items.iter().map(|i| self.js(i, scope)).collect();
                format!("[{}]", items.join(", "))
            }
            Head::Literal(Literal::String(v)) => quote(v),
            Head::Literal(Literal::Int(n)) => n.to_string(),
            Head::Empty => String::new(),
        }
    }

    fn js(&self, j: &JsType, scope: Scope) -> String {
        let mut parts = Vec::new();
        if j.head != Head::Empty {
            let head = self.head(&j.head, scope);
            if (j.null || j.undefined) && matches!(j.head, Head::Callback { .. }) {
                parts.push(format!("({head})"));
            } else {
                parts.push(head);
            }
        }
        // An `Empty` admitting neither has no text; `Null` is the nearest.
        if j.null || (j.head == Head::Empty && !j.undefined) {
            parts.push("Null".into());
        }
        if j.undefined {
            parts.push("Undefined".into());
        }
        parts.join(" | ")
    }

    fn ty(&self, t: &Type, scope: Scope) -> String {
        match t {
            Type::Js(j) => self.js(j, scope),
            t => wasm_name(t).into(),
        }
    }

    fn params(&self, params: &[Param], variadic: bool, scope: Scope) -> String {
        let list: Vec<String> = params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let rest = if variadic && i + 1 == params.len() {
                    "..."
                } else {
                    ""
                };
                format!("{rest}{}: {}", name(&p.name), self.ty(&p.ty, scope))
            })
            .collect();
        format!("({})", list.join(", "))
    }

    fn class(&self, out: &mut String, indent: &str, c: &Class) {
        let scope = Scope {
            module: c.module.as_deref(),
            params: &c.params,
        };
        doc(out, indent, &c.doc);
        let modifier = match c.kind {
            ClassKind::Class => "",
            ClassKind::Interface => "shape ",
            ClassKind::Namespace => "static ",
        };
        let _ = write!(out, "{indent}{modifier}class {}", c.path);
        if !c.params.is_empty() {
            let _ = write!(out, "<{}>", c.params.join(", "));
        }
        if c.name != c.path {
            let _ = write!(out, " as {}", c.name);
        }
        if !c.extends.is_empty() {
            let bases: Vec<String> = c.extends.iter().map(|e| self.named(e, scope)).collect();
            let _ = write!(out, " extends {}", bases.join(", "));
        }
        if c.members.is_empty() {
            out.push_str(" {}\n");
        } else {
            out.push_str(" {\n");
            let inner = format!("{indent}  ");
            for m in &c.members {
                doc(out, &inner, &m.doc);
                out.push_str(&inner);
                self.member(out, m, Some(c), scope);
            }
            let _ = writeln!(out, "{indent}}}");
        }
        for a in &c.aliases {
            let _ = writeln!(out, "{indent}alias {a} = {};", c.name);
        }
        for m in &c.members {
            for a in &m.aliases {
                let _ = writeln!(
                    out,
                    "{indent}alias {}.{} = {}.{};",
                    c.name,
                    name(a),
                    c.name,
                    name(&m.name)
                );
            }
        }
    }

    fn kind(&self, out: &mut String, indent: &str, k: &Kind, scope: Scope) {
        doc(out, indent, &k.doc);
        let _ = write!(out, "{indent}kind {}", k.path);
        if k.name != k.path {
            let _ = write!(out, " as {}", k.name);
        }
        if k.fallback {
            out.push_str(" fallback");
        }
        if k.arms.is_empty() {
            out.push_str(" {}\n");
            return;
        }
        out.push_str(" {\n");
        let inner = format!("{indent}  ");
        for a in &k.arms {
            doc(out, &inner, &a.doc);
            let _ = write!(out, "{inner}{} = {}", name(&a.name), self.js(&a.ty, scope));
            if !a.aliases.is_empty() {
                let aliases: Vec<String> = a.aliases.iter().map(|a| name(a)).collect();
                let _ = write!(out, " alias {}", aliases.join(", "));
            }
            out.push_str(";\n");
        }
        let _ = writeln!(out, "{indent}}}");
    }

    fn function(&self, out: &mut String, indent: &str, f: &Function, scope: Scope) {
        let m = &f.member;
        doc(out, indent, &m.doc);
        out.push_str(indent);
        self.member(out, m, None, scope);
        for a in &m.aliases {
            let _ = writeln!(out, "{indent}alias {} = {};", name(a), name(&m.name));
        }
    }

    /// One member line, `;` and newline included. `class` is `None` for a
    /// top-level member.
    fn member(&self, out: &mut String, m: &Member, class: Option<&Class>, scope: Scope) {
        let mut js = match class {
            Some(_) => member_name(&m.js),
            None => name(&m.js),
        };
        if m.optional {
            js.push('?');
        }
        let generics = if m.type_params.is_empty() {
            String::new()
        } else {
            format!("<{}>", m.type_params.join(", "))
        };
        js.push_str(&generics);
        let all = [scope.params, &m.type_params].concat();
        let scope = Scope {
            params: &all,
            ..scope
        };
        let params = self.params(&m.params, m.variadic, scope);
        let result = |out: &mut String| {
            if m.result != Type::Void {
                let _ = write!(out, ": {}", self.ty(&m.result, scope));
            }
        };
        let mut default_name = m.js.as_str();
        let statik = if class.is_some() { "static " } else { "" };
        match (m.kind, class) {
            (MemberKind::Constructor | MemberKind::Object, Some(c)) => {
                default_name = "new";
                if m.kind == MemberKind::Object {
                    out.push_str("new {}");
                } else if m.js == c.path {
                    let _ = write!(out, "new{generics}{params}");
                } else {
                    let _ = write!(out, "new {}{generics}{params}", m.js);
                }
                if m.result != class_type(c.module.as_deref(), &c.name, &c.params) {
                    let _ = write!(out, ": {}", self.ty(&m.result, scope));
                }
            }
            (MemberKind::Get, Some(_)) | (MemberKind::StaticGet, _) => {
                let s = if m.kind == MemberKind::StaticGet {
                    statik
                } else {
                    ""
                };
                let _ = write!(out, "{s}get {js}");
                if !m.params.is_empty() {
                    out.push_str(&params);
                }
                let _ = write!(out, ": {}", self.ty(&m.result, scope));
            }
            (MemberKind::Set, Some(_)) | (MemberKind::StaticSet, _) => {
                let s = if m.kind == MemberKind::StaticSet {
                    statik
                } else {
                    ""
                };
                let _ = write!(out, "{s}set {js}{params}");
                result(out);
            }
            (MemberKind::Method, Some(_)) => {
                let _ = write!(out, "{js}{params}");
                result(out);
            }
            (MemberKind::StaticMethod, Some(_)) => {
                let _ = write!(out, "static {js}{params}");
                result(out);
            }
            // A top-level member is static on the namespace; the
            // instance kinds have no top-level form.
            (_, None) => {
                let _ = write!(out, "function {js}{params}");
                result(out);
            }
        }
        if m.throws {
            out.push_str(" throws");
        } else if !m.errors.is_empty() {
            out.push_str(" raises");
        }
        if !m.errors.is_empty() {
            let errors: Vec<String> = m.errors.iter().map(|e| self.js(e, scope)).collect();
            let _ = write!(out, " {}", errors.join(" | "));
        }
        if m.nothrow {
            out.push_str(" nothrow");
        }
        if let Some(c) = &m.cohort {
            let _ = write!(out, " cohort {}", name(c));
        }
        if let Some(module) = &m.module {
            let _ = write!(out, " from {}", quote(module));
        }
        if m.name != default_name {
            let _ = write!(out, " as {}", name(&m.name));
        }
        out.push_str(";\n");
    }

    fn provide(&self, out: &mut String, p: &Provide) {
        doc(out, "", &p.doc);
        let path: Vec<String> = p.js.iter().map(|s| name(s)).collect();
        let _ = write!(
            out,
            "provide {}{}",
            path.join("."),
            self.params(&p.params, false, GLOBAL)
        );
        if p.is_async {
            let _ = write!(out, ": async {}", self.ty(&p.result, GLOBAL));
        } else if p.result != Type::Void {
            let _ = write!(out, ": {}", self.ty(&p.result, GLOBAL));
        }
        if Some(p.name.as_str()) != p.js.last().map(String::as_str) {
            let _ = write!(out, " as {}", name(&p.name));
        }
        out.push_str(";\n");
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(String),
    Doc(String),
    P(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: usize,
    col: usize,
}

const PUNCT: [&str; 18] = [
    "...", "=>", "{", "}", "(", ")", "[", "]", "<", ">", ",", ";", ":", ".", "|", "=", "-", "?",
];

fn lex(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let (mut i, mut line, mut col) = (0, 1, 1);
    let at = |i: usize| chars.get(i).copied();
    while i < chars.len() {
        let c = chars[i];
        let (tl, tc) = (line, col);
        let start = i;
        let tok = if c == '\n' {
            i += 1;
            line += 1;
            col = 1;
            continue;
        } else if c.is_whitespace() {
            i += 1;
            col += 1;
            continue;
        } else if c == '/' && at(i + 1) == Some('/') {
            let end = chars[i..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(chars.len(), |n| i + n);
            let text: String = chars[i..end].iter().collect();
            col += end - i;
            i = end;
            match text.strip_prefix("///") {
                Some(rest) if !rest.starts_with('/') => {
                    Tok::Doc(rest.strip_prefix(' ').unwrap_or(rest).into())
                }
                _ => continue,
            }
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                let Some(c) = at(i) else {
                    return Err(format!("{tl}:{tc}: unterminated string"));
                };
                i += 1;
                match c {
                    '"' => break,
                    '\n' => return Err(format!("{tl}:{tc}: unterminated string")),
                    '\\' => {
                        let e = at(i);
                        i += 1;
                        match e {
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some('n') => s.push('\n'),
                            Some('r') => s.push('\r'),
                            Some('t') => s.push('\t'),
                            Some('u') if at(i) == Some('{') => {
                                let end = chars[i..].iter().position(|&c| c == '}');
                                let code = end
                                    .map(|n| chars[i + 1..i + n].iter().collect::<String>())
                                    .and_then(|h| u32::from_str_radix(&h, 16).ok())
                                    .and_then(char::from_u32);
                                match (code, end) {
                                    (Some(c), Some(n)) => {
                                        s.push(c);
                                        i += n + 1;
                                    }
                                    _ => {
                                        let col = tc + (i - start);
                                        return Err(format!("{tl}:{col}: bad `\\u{{...}}` escape"));
                                    }
                                }
                            }
                            _ => {
                                let col = tc + (i - start) - 2;
                                return Err(format!("{tl}:{col}: unknown escape"));
                            }
                        }
                    }
                    c => s.push(c),
                }
            }
            col += i - start;
            Tok::Str(s)
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            while at(i).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                i += 1;
            }
            col += i - start;
            Tok::Ident(chars[start..i].iter().collect())
        } else if c.is_ascii_digit() {
            while at(i).is_some_and(|c| c.is_ascii_digit()) {
                i += 1;
            }
            col += i - start;
            Tok::Num(chars[start..i].iter().collect())
        } else {
            let p = PUNCT
                .iter()
                .find(|p| p.chars().enumerate().all(|(k, pc)| at(i + k) == Some(pc)))
                .ok_or_else(|| format!("{tl}:{tc}: unexpected character `{c}`"))?;
            i += p.len();
            col += p.len();
            Tok::P(p)
        };
        toks.push(Token {
            tok,
            line: tl,
            col: tc,
        });
    }
    toks.push(Token {
        tok: Tok::Eof,
        line,
        col,
    });
    Ok(toks)
}

/// A pending `alias`, resolved once every declaration is read.
struct AliasDecl {
    module: Option<String>,
    lhs: Vec<String>,
    rhs: Vec<String>,
    at: (usize, usize),
}

enum Part {
    Ty(Type),
    Null,
    Undefined,
}

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    /// `None` on the first pass, which only collects declarations and
    /// resolves every non-fixed name leniently.
    names: Option<&'a Names>,
    ir: Ir,
    aliases: Vec<AliasDecl>,
    functions: HashSet<(Option<String>, String)>,
    decls: HashSet<(Option<String>, String)>,
    /// A namespace is never a type, so it may share a kind's name, as a
    /// TS enum merged with a namespace does.
    namespaces: HashSet<(Option<String>, String)>,
}

type Res<T> = Result<T, String>;

impl<'a> Parser<'a> {
    fn new(toks: &'a [Token], names: Option<&'a Names>) -> Self {
        Parser {
            toks,
            pos: 0,
            names,
            ir: Ir::default(),
            aliases: Vec::new(),
            functions: HashSet::new(),
            decls: HashSet::new(),
            namespaces: HashSet::new(),
        }
    }

    fn peek(&self) -> &'a Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &'a Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn here(&self) -> (usize, usize) {
        let t = &self.toks[self.pos];
        (t.line, t.col)
    }

    fn err_at<T>(&self, at: (usize, usize), msg: impl std::fmt::Display) -> Res<T> {
        Err(format!("{}:{}: {msg}", at.0, at.1))
    }

    fn err<T>(&self, msg: impl std::fmt::Display) -> Res<T> {
        self.err_at(self.here(), msg)
    }

    fn bump(&mut self) -> &'a Tok {
        let t = self.peek();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::P(q) if *q == p)
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
            self.err(format!("expected `{p}`, found {}", describe(self.peek())))
        }
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == kw)
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
            t => self.err(format!("expected {what}, found {}", describe(t))),
        }
    }

    /// An identifier or a string.
    fn name(&mut self, what: &str) -> Res<String> {
        match self.peek() {
            Tok::Ident(s) | Tok::Str(s) => {
                self.bump();
                Ok(s.clone())
            }
            t => self.err(format!("expected {what}, found {}", describe(t))),
        }
    }

    fn is_name_at(&self, n: usize) -> bool {
        matches!(self.peek_at(n), Tok::Ident(_) | Tok::Str(_))
    }

    /// `ident ("." ident)*`, joined.
    fn path(&mut self, what: &str) -> Res<String> {
        let mut path = self.ident(what)?;
        while self.is(".") && matches!(self.peek_at(1), Tok::Ident(_)) {
            self.bump();
            path.push('.');
            path.push_str(&self.ident(what)?);
        }
        Ok(path)
    }

    /// `name ("." name)*`, as segments.
    fn segments(&mut self, what: &str) -> Res<Vec<String>> {
        let mut segs = vec![self.name(what)?];
        while self.eat(".") {
            segs.push(self.name(what)?);
        }
        Ok(segs)
    }

    fn docs(&mut self) -> Option<String> {
        let mut lines: Vec<&str> = Vec::new();
        while let Tok::Doc(d) = self.peek() {
            lines.push(d);
            self.bump();
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    fn file(&mut self) -> Res<()> {
        if !self.eat_kw("ir") {
            return self.err("expected the `ir <version>;` header");
        }
        let at = self.here();
        let Tok::Num(v) = self.bump() else {
            return self.err_at(at, "expected a version number");
        };
        if v.parse::<u32>().ok() != Some(VERSION) {
            return self.err_at(
                at,
                format!("format version {v}, this reader reads {VERSION}"),
            );
        }
        self.expect(";")?;
        while *self.peek() != Tok::Eof {
            self.decl(None)?;
        }
        Ok(())
    }

    fn decl(&mut self, module: Option<&str>) -> Res<()> {
        let doc = self.docs();
        let at = self.here();
        let Tok::Ident(kw) = self.peek() else {
            return self.err(format!(
                "expected a declaration, found {}",
                describe(self.peek())
            ));
        };
        let scope = Scope {
            module,
            params: &[],
        };
        match kw.as_str() {
            "class" => {
                self.bump();
                self.class(module, ClassKind::Class, doc, at)
            }
            "shape" | "static" if self.peek_at(1) == &Tok::Ident("class".into()) => {
                let kind = if kw == "shape" {
                    ClassKind::Interface
                } else {
                    ClassKind::Namespace
                };
                self.bump();
                self.bump();
                self.class(module, kind, doc, at)
            }
            "kind" => {
                self.bump();
                self.kind(module, doc, at)
            }
            "function" | "get" | "set" => {
                let kind = match kw.as_str() {
                    "function" => MemberKind::StaticMethod,
                    "get" => MemberKind::StaticGet,
                    _ => MemberKind::StaticSet,
                };
                self.bump();
                let js = self.name("a name")?;
                let mut member = self.signature(kind, js, doc, scope)?;
                let js = member.js.clone();
                let all = member.type_params.clone();
                self.end_member(
                    &mut member,
                    js,
                    Scope {
                        params: &all,
                        ..scope
                    },
                )?;
                if !self
                    .functions
                    .insert((module.map(String::from), member.name.clone()))
                {
                    return self.err_at(at, format!("duplicate top-level name `{}`", member.name));
                }
                self.ir.functions.push(Function {
                    module: module.map(String::from),
                    member,
                });
                Ok(())
            }
            "alias" if doc.is_none() => {
                self.bump();
                let lhs = self.segments("an alias name")?;
                self.expect("=")?;
                let rhs = self.segments("a class, member or function")?;
                self.expect(";")?;
                self.aliases.push(AliasDecl {
                    module: module.map(String::from),
                    lhs,
                    rhs,
                    at,
                });
                Ok(())
            }
            "module" if doc.is_none() && module.is_none() => {
                self.bump();
                let Tok::Str(spec) = self.bump() else {
                    return self.err_at(at, "expected the module specifier string after `module`");
                };
                self.expect("{")?;
                while !self.eat("}") {
                    if *self.peek() == Tok::Eof {
                        return self.err("expected `}` closing the module");
                    }
                    self.decl(Some(spec))?;
                }
                Ok(())
            }
            "provide" if module.is_none() => {
                self.bump();
                self.provide(doc)
            }
            "alias" | "module" if doc.is_some() => self.err_at(
                at,
                "a doc comment must precede a class, kind, member or provide",
            ),
            "module" | "provide" => self.err_at(at, format!("`{kw}` is only allowed at top level")),
            _ => self.err_at(at, format!("expected a declaration, found `{kw}`")),
        }
    }

    fn declare(&mut self, module: Option<&str>, path: &str, at: (usize, usize)) -> Res<()> {
        if !self.decls.insert((module.map(String::from), path.into())) {
            return self.err_at(at, format!("`{path}` is declared twice"));
        }
        Ok(())
    }

    fn class(
        &mut self,
        module: Option<&str>,
        kind: ClassKind,
        doc: Option<String>,
        at: (usize, usize),
    ) -> Res<()> {
        let path = self.path("a class path")?;
        let params = self.type_params(&[])?;
        let name = if self.eat_kw("as") {
            self.path("a binding name")?
        } else {
            path.clone()
        };
        if kind == ClassKind::Namespace {
            if !self
                .namespaces
                .insert((module.map(String::from), name.clone()))
            {
                return self.err_at(at, format!("`{name}` is declared twice"));
            }
        } else {
            self.declare(module, &name, at)?;
        }
        let scope = Scope {
            module,
            params: &params,
        };
        let mut extends = Vec::new();
        if self.eat_kw("extends") {
            loop {
                let e_at = self.here();
                match self.arm(scope)? {
                    Part::Ty(Type::Js(JsType {
                        head: Head::Named(n),
                        null: false,
                        undefined: false,
                    })) => extends.push(n),
                    _ => return self.err_at(e_at, "a class extends classes"),
                }
                if !self.eat(",") {
                    break;
                }
            }
        }
        self.expect("{")?;
        let mut members: Vec<Member> = Vec::new();
        while !self.eat("}") {
            if *self.peek() == Tok::Eof {
                return self.err("expected `}` closing the class");
            }
            let doc = self.docs();
            let m_at = self.here();
            if self.is("}") {
                return self.err_at(m_at, "a doc comment must precede a member");
            }
            let m = self.member(&path, &name, doc, scope)?;
            if members.iter().any(|o| o.name == m.name) {
                return self.err_at(m_at, format!("duplicate member `{}` in `{name}`", m.name));
            }
            members.push(m);
        }
        self.ir.classes.push(Class {
            name,
            path,
            module: module.map(String::from),
            kind,
            params,
            extends,
            aliases: Vec::new(),
            doc,
            members,
        });
        Ok(())
    }

    /// A member of the class at `path` with binding name `class`.
    fn member(
        &mut self,
        path: &str,
        class: &str,
        doc: Option<String>,
        scope: Scope,
    ) -> Res<Member> {
        if self.is_kw("new") && matches!(self.peek_at(1), Tok::P("(" | "{" | "<") | Tok::Ident(_)) {
            self.bump();
            let js = match self.peek() {
                Tok::Ident(_) => self.path("a constructor path")?,
                _ => path.into(),
            };
            let type_params = self.type_params(scope.params)?;
            let all = [scope.params, &type_params].concat();
            let inner = Scope {
                params: &all,
                ..scope
            };
            let object = self.eat("{");
            let (kind, (params, variadic)) = if object {
                self.expect("}")?;
                (MemberKind::Object, (Vec::new(), false))
            } else {
                (MemberKind::Constructor, self.params(inner)?)
            };
            let result = if self.eat(":") {
                self.ty(inner)?
            } else {
                class_type(scope.module, class, scope.params)
            };
            let mut m = Member {
                name: String::new(),
                js,
                kind,
                type_params,
                params,
                result,
                throws: false,
                errors: Vec::new(),
                nothrow: false,
                variadic,
                cohort: None,
                optional: false,
                module: None,
                aliases: Vec::new(),
                doc,
            };
            self.end_member(&mut m, "new".into(), inner)?;
            return Ok(m);
        }
        let statik = self.is_kw("static") && self.is_name_at(1);
        if statik {
            self.bump();
        }
        let access = if (self.is_kw("get") || self.is_kw("set")) && self.is_name_at(1) {
            let get = self.is_kw("get");
            self.bump();
            Some(get)
        } else {
            None
        };
        let kind = match (statik, access) {
            (false, None) => MemberKind::Method,
            (false, Some(true)) => MemberKind::Get,
            (false, Some(false)) => MemberKind::Set,
            (true, None) => MemberKind::StaticMethod,
            (true, Some(true)) => MemberKind::StaticGet,
            (true, Some(false)) => MemberKind::StaticSet,
        };
        let js = self.name("a member name")?;
        let mut m = self.signature(kind, js, doc, scope)?;
        let js = m.js.clone();
        let all = [scope.params, &m.type_params].concat();
        self.end_member(
            &mut m,
            js,
            Scope {
                params: &all,
                ..scope
            },
        )?;
        Ok(m)
    }

    /// Parameters and result of a non-constructor member, after its name.
    fn signature(
        &mut self,
        kind: MemberKind,
        js: String,
        doc: Option<String>,
        scope: Scope,
    ) -> Res<Member> {
        let get = matches!(kind, MemberKind::Get | MemberKind::StaticGet);
        let optional = self.eat("?");
        let type_params = self.type_params(scope.params)?;
        let all = [scope.params, &type_params].concat();
        let scope = Scope {
            params: &all,
            ..scope
        };
        let (params, variadic) = if get && !self.is("(") {
            (Vec::new(), false)
        } else {
            self.params(scope)?
        };
        let result = if get {
            self.expect(":")?;
            self.ty(scope)?
        } else if self.eat(":") {
            self.ty(scope)?
        } else {
            Type::Void
        };
        Ok(Member {
            name: String::new(),
            js,
            kind,
            type_params,
            params,
            result,
            throws: false,
            errors: Vec::new(),
            nothrow: false,
            variadic,
            cohort: None,
            optional,
            module: None,
            aliases: Vec::new(),
            doc,
        })
    }

    /// `(("throws" errors?) | ("raises" errors))? "nothrow"? ("cohort" name)?
    /// ("from" string)? ("as" name)? ";"`.
    fn end_member(&mut self, m: &mut Member, default: String, scope: Scope) -> Res<()> {
        m.throws = self.eat_kw("throws");
        let raises = !m.throws && self.eat_kw("raises");
        let typed = !(self.is(";")
            || ["as", "nothrow", "cohort", "from"]
                .iter()
                .any(|kw| self.is_kw(kw)));
        if raises || (m.throws && typed) {
            m.errors = self.errors(scope)?;
        }
        m.nothrow = self.eat_kw("nothrow");
        if self.eat_kw("cohort") {
            m.cohort = Some(self.name("a cohort binding name")?);
        }
        if self.eat_kw("from") {
            let Tok::Str(s) = self.peek() else {
                return self.err("expected the module specifier string after `from`");
            };
            m.module = Some(s.clone());
            self.bump();
        }
        m.name = if self.eat_kw("as") {
            self.name("a binding name")?
        } else {
            default
        };
        self.expect(";")
    }

    /// Exception types, `|`-separated, each arm one type: unlike a value
    /// type, `Null` and `Undefined` arms stand alone.
    fn errors(&mut self, scope: Scope) -> Res<Vec<JsType>> {
        let mut errors = Vec::new();
        loop {
            let at = self.here();
            let j = match self.arm(scope)? {
                Part::Null => JsType {
                    head: Head::Empty,
                    null: true,
                    undefined: false,
                },
                Part::Undefined => JsType {
                    head: Head::Empty,
                    null: false,
                    undefined: true,
                },
                Part::Ty(Type::Js(j)) => j,
                Part::Ty(_) => return self.err_at(at, "an exception type is a JS type"),
            };
            if errors.contains(&j) {
                return self.err_at(at, "duplicate exception type");
            }
            errors.push(j);
            if !self.eat("|") {
                break;
            }
        }
        Ok(errors)
    }

    fn params(&mut self, scope: Scope) -> Res<(Vec<Param>, bool)> {
        self.expect("(")?;
        let mut params = Vec::new();
        let mut variadic = false;
        if !self.eat(")") {
            loop {
                let at = self.here();
                if variadic {
                    return self.err_at(at, "a rest parameter must be last");
                }
                variadic = self.eat("...");
                let name = self.name("a parameter name")?;
                self.expect(":")?;
                let ty_at = self.here();
                let ty = self.ty(scope)?;
                let array = |t: &Type| {
                    matches!(t, Type::Js(JsType { head: Head::Named(n), .. })
                        if n.name == OwnerRef::global("Array"))
                };
                if variadic && self.names.is_some() && !array(&ty) {
                    return self.err_at(ty_at, "a rest parameter is an `Array<T>`");
                }
                if ty == Type::Void {
                    return self.err_at(ty_at, "`void` is only a result");
                }
                params.push(Param { name, ty });
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
        }
        Ok((params, variadic))
    }

    /// `("<" ident ("," ident)* ">")?`, none of them among `outer`.
    fn type_params(&mut self, outer: &[String]) -> Res<Vec<String>> {
        let mut params = Vec::new();
        if self.eat("<") {
            loop {
                let p_at = self.here();
                let p = self.ident("a type parameter")?;
                if params.contains(&p) || outer.contains(&p) {
                    return self.err_at(p_at, format!("duplicate type parameter `{p}`"));
                }
                params.push(p);
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(">")?;
        }
        Ok(params)
    }

    fn kind(&mut self, module: Option<&str>, doc: Option<String>, at: (usize, usize)) -> Res<()> {
        let path = self.path("a kind path")?;
        let name = if self.eat_kw("as") {
            self.path("a binding name")?
        } else {
            path.clone()
        };
        self.declare(module, &name, at)?;
        let fallback = self.eat_kw("fallback");
        let scope = Scope {
            module,
            params: &[],
        };
        self.expect("{")?;
        let mut arms: Vec<Arm> = Vec::new();
        while !self.eat("}") {
            if *self.peek() == Tok::Eof {
                return self.err("expected `}` closing the kind");
            }
            let doc = self.docs();
            let a_at = self.here();
            if self.is("}") {
                return self.err_at(a_at, "a doc comment must precede an arm");
            }
            let arm = self.name("an arm name")?;
            self.expect("=")?;
            let ty_at = self.here();
            let Type::Js(ty) = self.ty(scope)? else {
                return self.err_at(ty_at, "a kind's arms are JS types");
            };
            if ty.null || ty.undefined {
                return self.err_at(ty_at, "a kind's arms admit no `Null` or `Undefined`");
            }
            let mut aliases = Vec::new();
            if self.eat_kw("alias") {
                loop {
                    aliases.push(self.name("an arm alias")?);
                    if !self.eat(",") {
                        break;
                    }
                }
            }
            self.expect(";")?;
            let names = || std::iter::once(&arm).chain(&aliases);
            let dup = names().enumerate().find(|&(i, n)| {
                arms.iter().any(|a| &a.name == n || a.aliases.contains(n))
                    || names().take(i).any(|m| m == n)
            });
            if let Some((_, n)) = dup {
                return self.err_at(a_at, format!("duplicate arm `{n}` in `{name}`"));
            }
            arms.push(Arm {
                name: arm,
                ty,
                aliases,
                doc,
            });
        }
        self.ir.kinds.push(Kind {
            name,
            path,
            module: module.map(String::from),
            arms,
            fallback,
            doc,
        });
        Ok(())
    }

    fn provide(&mut self, doc: Option<String>) -> Res<()> {
        let at = self.here();
        let js = self.segments("a JS path")?;
        let (params, _) = self.params(GLOBAL)?;
        let (result, is_async) = if self.eat(":") {
            let is_async = self.is_kw("async")
                && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Str(_) | Tok::P("("));
            if is_async {
                self.bump();
            }
            (self.ty(GLOBAL)?, is_async)
        } else {
            (Type::Void, false)
        };
        let name = if self.eat_kw("as") {
            self.name("an export name")?
        } else {
            js.last().cloned().unwrap_or_default()
        };
        self.expect(";")?;
        if self.ir.provides.iter().any(|p| p.name == name) {
            return self.err_at(at, format!("duplicate provide `{name}`"));
        }
        self.ir.provides.push(Provide {
            name,
            js,
            params,
            result,
            is_async,
            doc,
        });
        Ok(())
    }

    /// A union: one non-empty arm, and `Null` / `Undefined` arms.
    fn ty(&mut self, scope: Scope) -> Res<Type> {
        let mut head: Option<(Type, (usize, usize))> = None;
        let (mut null, mut undefined) = (false, false);
        let start = self.here();
        loop {
            let at = self.here();
            match self.arm(scope)? {
                Part::Null => null = true,
                Part::Undefined => undefined = true,
                Part::Ty(t) => {
                    if head.is_some() {
                        return self.err_at(
                            at,
                            "a union has one arm besides `Null` and `Undefined`; declare a kind for more",
                        );
                    }
                    head = Some((t, at));
                }
            }
            if !self.eat("|") {
                break;
            }
        }
        let (t, at) = head.unwrap_or((Type::js(Head::Empty), start));
        if !null && !undefined {
            return Ok(t);
        }
        match t {
            Type::Js(mut j) => {
                j.null |= null;
                j.undefined |= undefined;
                Ok(Type::Js(j))
            }
            t => self.err_at(
                at,
                format!(
                    "`{}` is a wasm value and cannot be `Null` or `Undefined`; use its JS type (`Number | Undefined`)",
                    wasm_name(&t)
                ),
            ),
        }
    }

    fn arm(&mut self, scope: Scope) -> Res<Part> {
        let at = self.here();
        if self.eat("(") {
            let mut items = Vec::new();
            if !self.is(")") {
                loop {
                    items.push(self.ty(scope)?);
                    if !self.eat(",") {
                        break;
                    }
                }
            }
            self.expect(")")?;
            if self.eat("=>") {
                let result = self.ty(scope)?;
                return Ok(Part::Ty(Type::js(Head::Callback {
                    params: items,
                    result: Box::new(result),
                })));
            }
            return match <[Type; 1]>::try_from(items) {
                Ok([t]) => Ok(Part::Ty(t)),
                Err(_) => self.err("expected `=>` after a callback's parameter types"),
            };
        }
        if self.eat("[") {
            let mut items = Vec::new();
            if !self.is("]") {
                loop {
                    let i_at = self.here();
                    match self.ty(scope)? {
                        Type::Js(j) => items.push(j),
                        _ => return self.err_at(i_at, "tuple elements are JS types"),
                    }
                    if !self.eat(",") {
                        break;
                    }
                }
            }
            self.expect("]")?;
            return Ok(Part::Ty(Type::js(Head::Tuple(items))));
        }
        let negative = self.is("-");
        if negative || matches!(self.peek(), Tok::Num(_)) {
            if negative {
                self.bump();
            }
            let Tok::Num(n) = self.bump() else {
                return self.err_at(at, "expected an integer after `-`");
            };
            let text = if negative { format!("-{n}") } else { n.clone() };
            let Ok(n) = text.parse::<i64>() else {
                return self.err_at(at, format!("integer literal `{text}` is out of range"));
            };
            return Ok(Part::Ty(Type::js(Head::Literal(Literal::Int(n)))));
        }
        if let Tok::Str(v) = self.peek() {
            if self.peek_at(1) != &Tok::P(".") {
                self.bump();
                return Ok(Part::Ty(Type::js(Head::Literal(Literal::String(
                    v.clone(),
                )))));
            }
        }
        let qualified = match self.peek() {
            Tok::Str(m) => {
                self.bump();
                self.expect(".")?;
                Some(Some(m.clone()))
            }
            Tok::Ident(g) if g == "globalThis" && self.peek_at(1) == &Tok::P(".") => {
                self.bump();
                self.bump();
                Some(None)
            }
            Tok::Ident(_) => None,
            t => return self.err(format!("expected a type, found {}", describe(t))),
        };
        let path = self.path("a type name")?;
        let mut args = Vec::new();
        if self.eat("<") {
            loop {
                let a_at = self.here();
                match self.ty(scope)? {
                    Type::Js(j) => args.push(j),
                    _ => return self.err_at(a_at, "type arguments are JS types"),
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(">")?;
        }
        let named =
            |name: OwnerRef, args| Ok(Part::Ty(Type::js(Head::Named(Named { name, args }))));
        if let Some(module) = qualified {
            return named(OwnerRef { module, path }, args);
        }
        let bare = match self.names {
            Some(names) => names.bare(&path, &scope),
            None => match Names::default().bare(&path, &scope) {
                Bare::Unknown => Bare::Ref(OwnerRef::global(path.clone())),
                b => b,
            },
        };
        if !args.is_empty() && !matches!(bare, Bare::Ref(_)) {
            return self.err_at(at, format!("`{path}` takes no type arguments"));
        }
        match bare {
            Bare::Wasm(t) => Ok(Part::Ty(t)),
            Bare::Head(h) => Ok(Part::Ty(Type::js(h))),
            Bare::Null => Ok(Part::Null),
            Bare::Undefined => Ok(Part::Undefined),
            Bare::Ref(r) => named(r, args),
            Bare::Unknown => self.err_at(at, format!("unknown type `{path}`")),
        }
    }

    fn resolve_aliases(&mut self) -> Res<()> {
        for a in std::mem::take(&mut self.aliases) {
            let rhs = a.rhs.join(".");
            let lhs = a.lhs.join(".");
            let module = a.module.as_deref();
            let ir = &mut self.ir;
            if let Some(c) = ir
                .classes
                .iter_mut()
                .find(|c| c.module.as_deref() == module && c.name == rhs)
            {
                c.aliases.push(lhs);
                continue;
            }
            if let [class @ .., member] = a.rhs.as_slice() {
                let class = class.join(".");
                if let Some(c) = ir
                    .classes
                    .iter_mut()
                    .find(|c| c.module.as_deref() == module && c.name == class)
                {
                    if let Some(i) = c.members.iter().position(|m| &m.name == member) {
                        let alias = match a.lhs.as_slice() {
                            [prefix @ .., alias] if prefix.join(".") == class => alias.clone(),
                            _ => {
                                return self.err_at(
                                    a.at,
                                    format!("a member alias stays in its class: `alias {class}.<name> = {rhs};`"),
                                )
                            }
                        };
                        let taken = c
                            .members
                            .iter()
                            .any(|m| m.name == alias || m.aliases.contains(&alias));
                        if taken {
                            return self
                                .err_at(a.at, format!("duplicate member `{alias}` in `{class}`"));
                        }
                        c.members[i].aliases.push(alias);
                        continue;
                    }
                }
            }
            if let Some(i) = ir
                .functions
                .iter()
                .position(|f| f.module.as_deref() == module && f.member.name == rhs)
            {
                if !self.functions.insert((a.module.clone(), lhs.clone())) {
                    return self.err_at(a.at, format!("duplicate top-level name `{lhs}`"));
                }
                self.ir.functions[i].member.aliases.push(lhs);
                continue;
            }
            return self.err_at(
                a.at,
                format!("`{rhs}` names no class, member or function in this module"),
            );
        }
        Ok(())
    }
}

fn wasm_name(t: &Type) -> &'static str {
    match t {
        Type::Void => "void",
        Type::F64 => "f64",
        Type::I64 => "i64",
        Type::Bool => "bool",
        Type::Js(_) => "a JS type",
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("`{s}`"),
        Tok::Str(s) => quote(s),
        Tok::Num(n) => format!("`{n}`"),
        Tok::Doc(_) => "a doc comment".into(),
        Tok::P(p) => format!("`{p}`"),
        Tok::Eof => "the end of the file".into(),
    }
}

/// Read a file in the text format. Errors are `line:col: message`.
pub fn parse(src: &str) -> Result<Ir, String> {
    let toks = lex(src)?;
    let mut first = Parser::new(&toks, None);
    first.file()?;
    let names = Names::of(&first.ir);
    let mut second = Parser::new(&toks, Some(&names));
    second.file()?;
    second.resolve_aliases()?;
    Ok(second.ir)
}
