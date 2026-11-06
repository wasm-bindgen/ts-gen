//! The binding names of the IR for the language that reads it: members'
//! case, the words the language reserves and how a name that is one is
//! escaped, what a constructor is called, and which characters an
//! identifier holds, for classes, kinds, arms and parameters too. Collisions
//! are resolved per scope. The `rust` preset leaves the IR as the Rust
//! bindings name it.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::str::FromStr;

use ts_gen_abi::ir::{Head, Ir, JsType, Kind, Member, MemberKind, Param, Type};

use crate::codegen::signatures::RUST_KEYWORDS as RUST;

/// How a member's words are joined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Case {
    /// `set_status`, as the Rust bindings name it.
    #[default]
    Snake,
    /// `setStatus`.
    Camel,
}

/// A language's naming options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    Rust,
    Zena,
    Kotlin,
}

/// The characters a language's identifiers hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idents {
    /// Unicode letters and digits and `_`, not only `_`: Kotlin's.
    Unicode,
    /// `[A-Za-z_$][A-Za-z0-9_$]*`: Zena's.
    Ascii,
}

impl Idents {
    fn allows(self, c: char, first: bool) -> bool {
        match self {
            Idents::Unicode => c == '_' || c.is_alphabetic() || (!first && c.is_numeric()),
            Idents::Ascii => {
                c == '_' || c == '$' || c.is_ascii_alphabetic() || (!first && c.is_ascii_digit())
            }
        }
    }

    /// `name` as an identifier: a leading digit after `_`, and any other
    /// character it may not hold as `_u<hex>_`, as are the underscores of
    /// a name of only underscores where those are reserved.
    fn apply(self, name: &str) -> String {
        let mut out = String::new();
        for c in name.chars() {
            if self.allows(c, out.is_empty()) {
                out.push(c);
            } else if out.is_empty() && self.allows(c, false) {
                out.push('_');
                out.push(c);
            } else {
                let _ = write!(out, "_u{:x}_", c as u32);
            }
        }
        if out.is_empty() {
            out.push('_');
        }
        if self == Idents::Unicode && out.chars().all(|c| c == '_') {
            out = out.replace('_', "_u5f_");
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Naming {
    pub case: Case,
    /// Names no identifier may take: one that would is escaped.
    pub reserved: Vec<String>,
    /// Appended to a reserved name.
    pub escape: String,
    /// What a constructor's `new` becomes (`create`, `create_with_init`),
    /// for a language that reserves `new`.
    pub constructor: Option<String>,
    /// The characters of every name: members', and classes', kinds' (each
    /// segment of a dotted one), arms' and parameters', which `None`
    /// leaves as they are, as the Rust bindings respell those themselves.
    pub idents: Option<Idents>,
}

impl Default for Naming {
    fn default() -> Self {
        Naming::preset(Preset::Rust)
    }
}

impl FromStr for Case {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "snake" => Ok(Case::Snake),
            "camel" => Ok(Case::Camel),
            _ => Err(format!("unknown case `{s}`: expected `snake` or `camel`")),
        }
    }
}

impl FromStr for Preset {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "rust" => Ok(Preset::Rust),
            "zena" => Ok(Preset::Zena),
            "kotlin" => Ok(Preset::Kotlin),
            _ => Err(format!(
                "unknown naming preset `{s}`: expected `rust`, `zena` or `kotlin`"
            )),
        }
    }
}

impl Naming {
    pub fn preset(preset: Preset) -> Naming {
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect();
        match preset {
            Preset::Rust => Naming {
                case: Case::Snake,
                reserved: words(RUST),
                escape: "_".into(),
                constructor: None,
                idents: None,
            },
            Preset::Zena => Naming {
                case: Case::Snake,
                reserved: words(ZENA),
                escape: "_".into(),
                constructor: Some("create".into()),
                idents: Some(Idents::Ascii),
            },
            Preset::Kotlin => Naming {
                case: Case::Camel,
                reserved: words(KOTLIN),
                escape: "_".into(),
                constructor: None,
                idents: Some(Idents::Unicode),
            },
        }
    }

    /// Renames the bindings of `ir`.
    pub(crate) fn apply(&self, ir: &mut Ir) {
        if self.idents.is_some() {
            self.types(ir);
            for k in &mut ir.kinds {
                self.arms(k);
            }
            let members = ir.classes.iter_mut().flat_map(|c| &mut c.members);
            for m in members.chain(ir.functions.iter_mut().map(|f| &mut f.member)) {
                self.params(&mut m.params);
            }
            for p in &mut ir.provides {
                self.params(&mut p.params);
            }
        }
        for class in &mut ir.classes {
            self.scope(&mut class.members);
        }
        let mut modules: Vec<Option<String>> = Vec::new();
        for f in &ir.functions {
            if !modules.contains(&f.module) {
                modules.push(f.module.clone());
            }
        }
        for module in modules {
            let mut members: Vec<Member> = Vec::new();
            let mut at = Vec::new();
            for (i, f) in ir.functions.iter_mut().enumerate() {
                if f.module == module {
                    members.push(std::mem::replace(&mut f.member, placeholder()));
                    at.push(i);
                }
            }
            self.scope(&mut members);
            for (i, m) in at.into_iter().zip(members) {
                ir.functions[i].member = m;
            }
        }
    }

    /// An identifier for `name`, escaped if reserved.
    fn ident(&self, name: &str) -> String {
        let mut name = match self.idents {
            Some(idents) => idents.apply(name),
            None => name.to_string(),
        };
        if self.reserved.contains(&name) {
            name.push_str(&self.escape);
        }
        name
    }

    /// Renames classes and kinds, segment by segment, in each module's one
    /// namespace, and every reference to them. A name that stays is taken
    /// first; a kind keeps sharing its name with a namespace.
    fn types(&self, ir: &mut Ir) {
        let mut names: Vec<(Option<String>, String)> = Vec::new();
        for c in &ir.classes {
            for n in std::iter::once(&c.name).chain(&c.aliases) {
                names.push((c.module.clone(), n.clone()));
            }
        }
        names.extend(ir.kinds.iter().map(|k| (k.module.clone(), k.name.clone())));
        let respelled = |n: &str| {
            n.split('.')
                .map(|s| self.ident(s))
                .collect::<Vec<_>>()
                .join(".")
        };
        let mut used: HashSet<(Option<String>, String)> = names
            .iter()
            .filter(|(_, n)| respelled(n) == *n)
            .cloned()
            .collect();
        let mut map: HashMap<(Option<String>, String), String> = HashMap::new();
        for key in names {
            let name = respelled(&key.1);
            if name == key.1 || map.contains_key(&key) {
                continue;
            }
            let mut candidate = name.clone();
            for n in 2u32.. {
                if used.insert((key.0.clone(), candidate.clone())) {
                    break;
                }
                candidate = format!("{name}{n}");
            }
            map.insert(key, candidate);
        }
        if map.is_empty() {
            return;
        }
        let rename = |module: &Option<String>, name: &mut String| {
            if let Some(new) = map.get(&(module.clone(), name.clone())) {
                *name = new.clone();
            }
        };
        for c in &mut ir.classes {
            rename(&c.module, &mut c.name);
            for a in &mut c.aliases {
                rename(&c.module, a);
            }
            for e in &mut c.extends {
                retype_named(e, &map);
            }
            for m in &mut c.members {
                retype_member(m, &map);
            }
        }
        for k in &mut ir.kinds {
            rename(&k.module, &mut k.name);
            for a in &mut k.arms {
                retype_js(&mut a.ty, &map);
            }
        }
        for f in &mut ir.functions {
            retype_member(&mut f.member, &map);
        }
        for p in &mut ir.provides {
            for param in &mut p.params {
                retype(&mut param.ty, &map);
            }
            retype(&mut p.result, &map);
        }
    }

    /// Renames a kind's arms and their aliases.
    fn arms(&self, k: &mut Kind) {
        let names = k
            .arms
            .iter_mut()
            .flat_map(|a| std::iter::once(&mut a.name).chain(&mut a.aliases));
        self.distinct(names, |name, n| format!("{name}{n}"));
    }

    fn params(&self, params: &mut [Param]) {
        let case = self.case;
        self.distinct(
            params.iter_mut().map(|p| &mut p.name),
            |name, n| match case {
                Case::Snake => format!("{name}_{n}"),
                Case::Camel => format!("{name}{n}"),
            },
        );
    }

    /// Respells one scope's `names`, those that stay taken first, numbering
    /// a collision with `number`.
    fn distinct<'n>(
        &self,
        names: impl Iterator<Item = &'n mut String>,
        number: impl Fn(&str, u32) -> String,
    ) {
        let mut names: Vec<(&mut String, String)> = names
            .map(|n| {
                let new = self.ident(n);
                (n, new)
            })
            .collect();
        let mut used: HashSet<String> = names
            .iter()
            .filter(|(old, new)| **old == *new)
            .map(|(_, new)| new.clone())
            .collect();
        for (old, new) in &mut names {
            if **old == *new {
                continue;
            }
            let mut candidate = new.clone();
            for n in 2u32.. {
                if used.insert(candidate.clone()) {
                    break;
                }
                candidate = number(new, n);
            }
            **old = candidate;
        }
    }

    /// Names one scope's members, declared members before constructors so
    /// a constructor's `create` yields to a declared one.
    fn scope(&self, members: &mut [Member]) {
        let ctor = |m: &Member| matches!(m.kind, MemberKind::Constructor | MemberKind::Object);
        let mut used = HashSet::new();
        let mut cohorts: HashMap<String, String> = HashMap::new();
        for constructors in [false, true] {
            for m in members.iter_mut().filter(|m| ctor(m) == constructors) {
                m.name = self.declare(&mut used, &m.name, constructors);
                for a in &mut m.aliases {
                    *a = self.declare(&mut used, a, constructors);
                }
                if let Some(c) = &mut m.cohort {
                    let stem = c.trim_start_matches('_');
                    let prefix = &c[..c.len() - stem.len()];
                    let named = cohorts
                        .entry(c.clone())
                        .or_insert_with(|| format!("{prefix}{}", self.name(stem, false)));
                    *c = named.clone();
                }
            }
        }
    }

    fn declare(&self, used: &mut HashSet<String>, name: &str, constructor: bool) -> String {
        let name = self.name(name, constructor);
        if used.insert(name.clone()) {
            return name;
        }
        for n in 2u32.. {
            let candidate = match self.case {
                Case::Snake => format!("{name}_{n}"),
                Case::Camel => format!("{name}{n}"),
            };
            if used.insert(candidate.clone()) {
                return candidate;
            }
        }
        unreachable!("a scope has a free numbered name")
    }

    /// The name a Rust binding name `rust` takes.
    fn name(&self, rust: &str, constructor: bool) -> String {
        // The Rust escape of a keyword (`type_`) stands for the word.
        let mut name = match rust.strip_suffix('_') {
            Some(stem) if RUST.contains(&stem) => stem.to_string(),
            _ => rust.to_string(),
        };
        if let (true, Some(create)) = (constructor, &self.constructor) {
            if name == "new" || name.starts_with("new_") {
                name = format!("{create}{}", &name[3..]);
            }
        }
        if self.case == Case::Camel {
            name = camel(&name);
        }
        self.ident(&name)
    }
}

fn retype_member(m: &mut Member, map: &HashMap<(Option<String>, String), String>) {
    for p in &mut m.params {
        retype(&mut p.ty, map);
    }
    retype(&mut m.result, map);
    for e in &mut m.errors {
        retype_js(e, map);
    }
}

fn retype(t: &mut Type, map: &HashMap<(Option<String>, String), String>) {
    if let Type::Js(j) = t {
        retype_js(j, map);
    }
}

fn retype_js(j: &mut JsType, map: &HashMap<(Option<String>, String), String>) {
    match &mut j.head {
        Head::Named(n) => retype_named(n, map),
        Head::Callback { params, result } => {
            for p in params {
                retype(p, map);
            }
            retype(result, map);
        }
        Head::Tuple(elements) => {
            for e in elements {
                retype_js(e, map);
            }
        }
        _ => {}
    }
}

fn retype_named(n: &mut ts_gen_abi::ir::Named, map: &HashMap<(Option<String>, String), String>) {
    if let Some(new) = map.get(&(n.name.module.clone(), n.name.path.clone())) {
        n.name.path = new.clone();
    }
    for a in &mut n.args {
        retype_js(a, map);
    }
}

/// `set_status` as `setStatus`, keeping leading and trailing underscores.
fn camel(snake: &str) -> String {
    let core = snake.trim_matches('_');
    if core.is_empty() {
        return snake.to_string();
    }
    let lead = &snake[..snake.len() - snake.trim_start_matches('_').len()];
    let trail = &snake[snake.trim_end_matches('_').len()..];
    let mut out = lead.to_string();
    for (i, word) in core.split('_').filter(|w| !w.is_empty()).enumerate() {
        let mut chars = word.chars();
        match chars.next() {
            Some(c) if i > 0 => {
                out.extend(c.to_uppercase());
                out.push_str(chars.as_str());
            }
            _ => out.push_str(word),
        }
    }
    out.push_str(trail);
    out
}

fn placeholder() -> Member {
    Member {
        name: String::new(),
        js: String::new(),
        kind: MemberKind::Method,
        type_params: Vec::new(),
        params: Vec::new(),
        result: ts_gen_abi::ir::Type::Void,
        throws: false,
        errors: Vec::new(),
        nothrow: false,
        variadic: false,
        cohort: None,
        optional: false,
        module: None,
        aliases: Vec::new(),
        doc: None,
    }
}

/// Zena's keywords, and `ref`, which its bindings reserve.
const ZENA: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "declare",
    "decorator",
    "distinct",
    "else",
    "enum",
    "export",
    "extends",
    "extension",
    "false",
    "final",
    "finally",
    "for",
    "from",
    "function",
    "gen",
    "if",
    "implements",
    "import",
    "in",
    "inline",
    "interface",
    "is",
    "let",
    "match",
    "mixin",
    "new",
    "null",
    "on",
    "opaque",
    "operator",
    "ref",
    "return",
    "sealed",
    "static",
    "super",
    "symbol",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "using",
    "var",
    "while",
    "with",
    "yield",
];

/// Kotlin's hard keywords.
const KOTLIN: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_joins_words() {
        assert_eq!(camel("set_status"), "setStatus");
        assert_eq!(camel("json_with_response_init"), "jsonWithResponseInit");
        assert_eq!(camel("_0"), "_0");
        assert_eq!(camel("foo_2"), "foo2");
        assert_eq!(camel("url"), "url");
    }

    #[test]
    fn rust_names_stay() {
        let rust = Naming::default();
        for n in ["type_", "self_", "new_with_init", "set_status", "foo_2"] {
            assert_eq!(rust.name(n, n.starts_with("new")), n);
        }
    }

    #[test]
    fn presets_escape_their_own_words() {
        let zena = Naming::preset(Preset::Zena);
        assert_eq!(zena.name("type_", false), "type_");
        assert_eq!(zena.name("self_", false), "self");
        assert_eq!(zena.name("ref_", false), "ref_");
        assert_eq!(zena.name("new", true), "create");
        assert_eq!(zena.name("new_with_init", true), "create_with_init");
        assert_eq!(zena.name("new_instance", false), "new_instance");
        let kotlin = Naming::preset(Preset::Kotlin);
        assert_eq!(kotlin.name("type_", false), "type");
        assert_eq!(kotlin.name("object", false), "object_");
        assert_eq!(kotlin.name("set_status", false), "setStatus");
        assert_eq!(kotlin.name("new_with_init", true), "newWithInit");
    }

    #[test]
    fn identifiers_hold_the_language_characters() {
        assert_eq!(Idents::Ascii.apply("déjà"), "d_ue9_j_ue0_");
        assert_eq!(Idents::Ascii.apply("$el"), "$el");
        assert_eq!(Idents::Ascii.apply("0x"), "_0x");
        assert_eq!(Idents::Ascii.apply("_"), "_");
        assert_eq!(Idents::Unicode.apply("déjà"), "déjà");
        assert_eq!(Idents::Unicode.apply("$el"), "_u24_el");
        assert_eq!(Idents::Unicode.apply("__"), "_u5f__u5f_");
        let zena = Naming::preset(Preset::Zena);
        assert_eq!(zena.name("déjà", false), "d_ue9_j_ue0_");
    }

    #[test]
    fn presets_name_types_arms_and_params() {
        let src = r#"ir 1;

class déjà {
  f(type: String, "type_": f64, "é": déjà): type;
}

class type {}

class type_ {}

class Ns.match {
  g(x: Ns.match);
}

kind type as té {
  "déjà" = String alias "dàjé";
  "d_ue9_j_ue0_" = Number;
}

function h(k: té, c: [type, (type_) => Ns.match]);
"#;
        let ir = ts_gen_abi::text::parse(src).unwrap();
        let mut rust = ir.clone();
        Naming::preset(Preset::Rust).apply(&mut rust);
        assert_eq!(rust, ir);

        let mut zena = ir.clone();
        Naming::preset(Preset::Zena).apply(&mut zena);
        let text = ts_gen_abi::text::emit(&zena);
        assert_eq!(ts_gen_abi::text::parse(&text).as_ref(), Ok(&zena));
        let expected = r#"ir 1;

class déjà as d_ue9_j_ue0_ {
  f(type__2: String, type_: f64, _ue9_: d_ue9_j_ue0_): type_2;
}

class type as type_2 {}

class type_ {}

class Ns.match as Ns.match_ {
  g(x: Ns.match_);
}

kind type as t_ue9_ {
  d_ue9_j_ue0_2 = String alias d_ue0_j_ue9_;
  d_ue9_j_ue0_ = Number;
}

function h(k: t_ue9_, c: [type_2, (type_) => Ns.match_]);
"#;
        assert_eq!(text, expected);
    }
}
