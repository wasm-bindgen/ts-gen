use std::fmt::Write;

use super::{name, quote};
use crate::ir::*;

const INDENT: &str = "  ";

pub(super) fn print(ir: &Ir) -> String {
    let mut out = String::from("ir 1;\n");
    if let Some(lib) = &ir.lib {
        out.push_str(&format!("lib {lib};\n"));
    }
    for w in &ir.with {
        out.push_str(&format!("with {w};\n"));
    }
    if !ir.items.is_empty() {
        out.push('\n');
    }
    items(&mut out, "", &ir.items);
    out
}

fn items(out: &mut String, indent: &str, items: &[Decl]) {
    for (i, d) in items.iter().enumerate() {
        if i > 0 && !(matches!(d, Decl::Member(_)) && matches!(items[i - 1], Decl::Member(_))) {
            out.push('\n');
        }
        decl(out, indent, d);
    }
}

fn meta(out: &mut String, indent: &str, m: &Meta) {
    if let Some(doc) = &m.doc {
        for line in doc.split('\n') {
            if line.is_empty() {
                let _ = writeln!(out, "{indent}///");
            } else {
                let _ = writeln!(out, "{indent}/// {line}");
            }
        }
    }
    if !m.attrs.js.is_empty() {
        let _ = writeln!(out, "{indent}{}", js_attr(&m.attrs.js));
    }
    if m.attrs.fallback {
        let _ = writeln!(out, "{indent}@fallback");
    }
    if m.attrs.once {
        let _ = writeln!(out, "{indent}@once");
    }
}

fn js_attr(entries: &[JsEntry]) -> String {
    let entries: Vec<String> = entries.iter().map(js_entry).collect();
    format!("@js({})", entries.join(", "))
}

fn decl(out: &mut String, indent: &str, d: &Decl) {
    meta(out, indent, d.meta());
    let inner = format!("{indent}{INDENT}");
    match d {
        Decl::Namespace(ns) => {
            let export = if ns.export { "export " } else { "" };
            let _ = write!(out, "{indent}{export}namespace {} {{", name(&ns.name));
            block(
                out,
                indent,
                |out| items(out, &inner, &ns.items),
                ns.items.is_empty(),
            );
        }
        Decl::Class(c) => {
            let kw = match c.kind {
                ClassKind::Class => "class",
                ClassKind::Interface => "interface",
            };
            let export = if c.export { "export " } else { "" };
            let _ = write!(
                out,
                "{indent}{export}{kw} {}{}",
                name(&c.name),
                tparams(&c.tparams)
            );
            if !c.extends.is_empty() {
                let parents: Vec<String> = c.extends.iter().map(typeref).collect();
                let _ = write!(out, " extends {}", parents.join(", "));
            }
            out.push_str(" {");
            block(
                out,
                indent,
                |out| {
                    for a in &c.assoc {
                        meta(out, &inner, &a.meta);
                        let rows: Vec<String> = a
                            .rows
                            .iter()
                            .map(|(k, t)| format!("{} => {}", ty(k), ty(t)))
                            .collect();
                        let _ = writeln!(
                            out,
                            "{inner}type {} = match {} {{ {} }};",
                            name(&a.name),
                            a.param,
                            rows.join(", ")
                        );
                    }
                    for m in &c.members {
                        meta(out, &inner, &m.meta);
                        let _ = writeln!(out, "{inner}{}", member(m));
                    }
                },
                c.assoc.is_empty() && c.members.is_empty(),
            );
        }
        Decl::Enum(e) => {
            let _ = write!(out, "{indent}enum {}", name(&e.name));
            if let Some(c) = e.carrier {
                let _ = write!(out, ": {}", c.name());
            }
            if !e.extends.is_empty() {
                let bases: Vec<String> = e.extends.iter().map(path).collect();
                let _ = write!(out, " extends {}", bases.join(", "));
            }
            out.push_str(" {");
            block(
                out,
                indent,
                |out| {
                    for a in &e.arms {
                        arm(out, &inner, a);
                    }
                    for m in &e.members {
                        meta(out, &inner, &m.meta);
                        let _ = writeln!(out, "{inner}{}", member(m));
                    }
                },
                e.arms.is_empty() && e.members.is_empty(),
            );
        }
        Decl::Module(m) => {
            let _ = write!(out, "{indent}module {} {{", quote(&m.spec));
            let empty = m.items.is_empty() && m.exports.is_none();
            block(
                out,
                indent,
                |out| {
                    items(out, &inner, &m.items);
                    if let Some(exports) = &m.exports {
                        if !m.items.is_empty() {
                            out.push('\n');
                        }
                        let names: Vec<String> = exports
                            .iter()
                            .map(|e| match &e.alias {
                                Some(a) => format!("{} as {}", name(&e.name), name(a)),
                                None => name(&e.name),
                            })
                            .collect();
                        let _ = writeln!(out, "{inner}export {{ {} }};", names.join(", "));
                    }
                },
                empty,
            );
        }
        Decl::Member(m) => {
            let _ = writeln!(out, "{indent}{}", member(m));
        }
        Decl::Alias(a) => {
            let _ = writeln!(
                out,
                "{indent}type {}{} = {};",
                name(&a.name),
                tparams(&a.tparams),
                ty(&a.ty)
            );
        }
    }
}

/// Closes a `{` already written: `{}` when empty.
fn block(out: &mut String, indent: &str, body: impl FnOnce(&mut String), empty: bool) {
    if empty {
        out.push_str("}\n");
        return;
    }
    out.push('\n');
    body(out);
    let _ = writeln!(out, "{indent}}}");
}

fn arm(out: &mut String, indent: &str, a: &Arm) {
    if let Some(doc) = &a.meta.doc {
        for line in doc.split('\n') {
            if line.is_empty() {
                let _ = writeln!(out, "{indent}///");
            } else {
                let _ = writeln!(out, "{indent}/// {line}");
            }
        }
    }
    out.push_str(indent);
    if !a.meta.attrs.js.is_empty() {
        out.push_str(&js_attr(&a.meta.attrs.js));
        out.push(' ');
    }
    if a.meta.attrs.fallback {
        out.push_str("@fallback ");
    }
    if a.meta.attrs.once {
        out.push_str("@once ");
    }
    out.push_str(&name(&a.name));
    if !a.payload.is_empty() {
        let tys: Vec<String> = a.payload.iter().map(ty).collect();
        let _ = write!(out, "({})", tys.join(", "));
    }
    match &a.value {
        Some(ArmValue::Int(n)) => {
            let _ = write!(out, " = {n}");
        }
        Some(ArmValue::Alias(n)) => {
            let _ = write!(out, " = .{}", name(n));
        }
        None => {}
    }
    out.push_str(";\n");
}

fn member(m: &Member) -> String {
    let mut s = String::new();
    if m.export {
        s.push_str("export ");
    }
    if m.is_static {
        s.push_str("static ");
    }
    match &m.kind {
        MemberKind::Method(n) => {
            s.push_str(&name(n));
            s.push_str(&tparams(&m.tparams));
        }
        MemberKind::Get(n) => {
            let _ = write!(s, "get {}", name(n));
        }
        MemberKind::Set(n) => {
            let _ = write!(s, "set {}", name(n));
        }
        MemberKind::New(None) => s.push_str("new"),
        MemberKind::New(Some(n)) => {
            let _ = write!(s, "new {}", name(n));
        }
    }
    let params: Vec<String> = m.params.iter().map(param).collect();
    let _ = write!(s, "({})", params.join(", "));
    if matches!(m.kind, MemberKind::Get(_)) || m.result != Type::Unit {
        let _ = write!(s, ": {}", ty(&m.result));
    }
    s.push_str(&throws(&m.throws));
    s.push(';');
    s
}

fn throws(t: &Option<Path>) -> String {
    match t {
        None => String::new(),
        Some(p) if p.is_err() => " throws".into(),
        Some(p) => format!(" throws {}", path(p)),
    }
}

fn param(p: &Param) -> String {
    let mut s = String::new();
    if p.unshared {
        s.push_str("@js(unshared) ");
    }
    if p.once {
        s.push_str("@once ");
    }
    let t = ty(&p.ty);
    let t = match (p.union, t.strip_prefix("@js(null) ")) {
        (true, Some(rest)) => format!("@js(null) union {rest}"),
        (true, None) => format!("union {t}"),
        (false, _) => t,
    };
    let _ = write!(s, "{}: {t}", name(&p.name));
    s
}

fn tparams(ps: &[TypeParam]) -> String {
    if ps.is_empty() {
        return String::new();
    }
    let ps: Vec<String> = ps
        .iter()
        .map(|p| {
            let head = match &p.bound {
                None => p.name.clone(),
                Some(Bound::Subtype(t)) => format!("{}: {}", p.name, ty(t)),
                Some(Bound::In(set)) => {
                    let set: Vec<String> = set.iter().map(ty).collect();
                    format!("{} in ({})", p.name, set.join(", "))
                }
            };
            match &p.default {
                Some(d) => format!("{head} = {}", ty(d)),
                None => head,
            }
        })
        .collect();
    format!("<{}>", ps.join(", "))
}

pub(super) fn path(p: &Path) -> String {
    let segs: Vec<String> = p.segments.iter().map(|s| name(s)).collect();
    anchor(p, segs.join("."))
}

fn anchor(p: &Path, s: String) -> String {
    match &p.base {
        Base::Lexical => s,
        Base::Root => format!(".{s}"),
        Base::Module(spec) => format!("{}.{s}", quote(spec)),
    }
}

/// A type path, quoting a segment that would read back as a built-in.
fn type_path(p: &Path) -> String {
    let segs: Vec<String> = p
        .segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            // `"str".X` would read back as a module path, and a lexical
            // `str.X` is a validation error (§4).
            if i == 0
                && p.base == Base::Lexical
                && p.segments.len() == 1
                && (BUILTIN_TYPE_NAMES.contains(&s.as_str()) || s == "mut")
            {
                quote(s)
            } else {
                name(s)
            }
        })
        .collect();
    anchor(p, segs.join("."))
}

fn typeref(t: &TypeRef) -> String {
    let mut s = type_path(&t.path);
    if !t.args.is_empty() {
        let _ = write!(s, "<{}>", list(&t.args));
    }
    s
}

fn list(ts: &[Type]) -> String {
    let ts: Vec<String> = ts.iter().map(ty).collect();
    ts.join(", ")
}

/// Whether a type is safe where the grammar wants a `primary`.
fn is_primary(t: &Type) -> bool {
    !matches!(
        t,
        Type::Nullable(..) | Type::Vec(_) | Type::Fn(_) | Type::View { .. } | Type::MutVec(_)
    )
}

/// The `T[]` that `*` or `mut` applies to.
fn whole_vec(t: &Type) -> String {
    if is_primary(t) {
        format!("{}[]", ty(t))
    } else {
        format!("({})[]", ty(t))
    }
}

/// The operand of a postfix operator: a function type whose tail would
/// absorb the operator needs parentheses.
fn operand(t: &Type) -> String {
    match t {
        Type::Fn(f) if f.result != Type::Unit || f.throws.is_some() => format!("({})", ty(t)),
        Type::Nullable(_, true) => format!("({})", ty(t)),
        _ => ty(t),
    }
}

pub(super) fn ty(t: &Type) -> String {
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
        Type::Vec(t) => format!("{}[]", operand(t)),
        Type::VecBox(t) => format!("Vec<{}>", ty(t)),
        Type::MutVec(t) => format!("mut {}", whole_vec(t)),
        Type::Array(t) => format!("arr<{}>", ty(t)),
        Type::ArrBox(t) => format!("Arr<{}>", ty(t)),
        Type::Tuple(ts) if ts.len() == 1 => format!("({},)", ty(&ts[0])),
        Type::Tuple(ts) => format!("({})", list(ts)),
        Type::TupleBox(ts) => format!("Tuple<{}>", list(ts)),
        Type::Future(t, e) if e.is_err() => format!("future<{}>", ty(t)),
        Type::Future(t, e) => format!("future<{}, {}>", ty(t), path(e)),
        Type::Fn(f) => {
            let mut ps: Vec<String> = f.params.iter().map(ty).collect();
            if let (true, Some(last)) = (f.rest, ps.last_mut()) {
                last.insert_str(0, "...");
            }
            let mut s = format!("fn({})", ps.join(", "));
            if f.result != Type::Unit || f.throws.is_some() {
                // A result that is itself a function without `throws` would
                // take this function's `throws`.
                let absorbs = matches!(&f.result, Type::Fn(g) if g.throws.is_none());
                if absorbs && f.throws.is_some() {
                    let _ = write!(s, " -> ({})", ty(&f.result));
                } else {
                    let _ = write!(s, " -> {}", ty(&f.result));
                }
            }
            s.push_str(&throws(&f.throws));
            s
        }
        Type::View { mutable, elem } => {
            let m = if *mutable { "mut " } else { "" };
            format!("*{m}{}", whole_vec(elem))
        }
        Type::Nullable(t, false) => format!("{}?", operand(t)),
        Type::Nullable(t, true) => format!("@js(null) {}?", operand(t)),
        Type::Named(r) => typeref(r),
    }
}

fn literal(l: &Literal) -> String {
    match l {
        Literal::Str(s) => quote(s),
        Literal::Int(n) => n.to_string(),
        Literal::Bool(b) => b.to_string(),
        Literal::Null => "null".into(),
    }
}

pub(crate) fn expr(e: &Expr) -> String {
    match e {
        Expr::Lit(l) => literal(l),
        Expr::Ref { path, call } => {
            let segs: Vec<String> = path.iter().map(|s| name(s)).collect();
            let mut s = segs.join(".");
            if let Some(args) = call {
                let args: Vec<String> = args.iter().map(expr).collect();
                let _ = write!(s, "({})", args.join(", "));
            }
            s
        }
    }
}

fn telem(t: &Telem) -> String {
    match t {
        Telem::Param(p) => p.clone(),
        Telem::Lit(l) => literal(l),
        Telem::Undefined => "undefined".into(),
        Telem::Spread(p) => format!("...{p}"),
        Telem::Fill { param, fields } => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(k, e)| format!("{} = {}", name(k), expr(e)))
                .collect();
            format!("{param}? {{ {} }}", fields.join(", "))
        }
        Telem::Expr(e) => expr(e),
    }
}

fn template(ts: &[Telem]) -> String {
    let ts: Vec<String> = ts.iter().map(telem).collect();
    format!("({})", ts.join(", "))
}

fn js_entry(e: &JsEntry) -> String {
    match e {
        JsEntry::Literal(s) => quote(s),
        JsEntry::Class(preds) => {
            let preds: Vec<String> = preds
                .iter()
                .map(|p| {
                    let mut s = path(&p.class);
                    if !p.props.is_empty() {
                        let props: Vec<String> = p
                            .props
                            .iter()
                            .map(|(k, v)| format!("{k} = {}", literal(v)))
                            .collect();
                        let _ = write!(s, "[{}]", props.join(", "));
                    }
                    s
                })
                .collect();
            preds.join(" | ")
        }
        JsEntry::Name(n) => format!("name = {}", quote(n)),
        JsEntry::Object => "object".into(),
        JsEntry::Index => "index".into(),
        JsEntry::Symbol(s) => format!("symbol = {s}"),
        JsEntry::This(p) => format!("this = {p}"),
        JsEntry::Args(t) => format!("args = {}", template(t)),
        JsEntry::Super(t) => format!("super = {}", template(t)),
        JsEntry::Let(n, e) => format!("let {n} = {}", expr(e)),
        JsEntry::Cancel(e) => format!("cancel = {}", expr(e)),
        JsEntry::New(p) => format!("new = {}", path(p)),
    }
}
