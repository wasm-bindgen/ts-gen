//! Conflict renaming (IR.md §7).
//!
//! The constant-expansion convention (CONVENTIONS.md: Variant names, the
//! `_with_<type>` vocabulary and literal-keyed overloads), recased to
//! camelCase. The producer applies it to overloads whose keys collide once
//! hidden constants are moved out (`getText`, `setSignalWithNull`);
//! generators without overloading apply it to their own conflicts.
//!
//! A variant is named for what it adds beyond the primary: the positions
//! where it departs, by parameter name where the primary lacks the
//! parameter, by type where a sibling has the same parameter with another
//! type. A hidden string constant names the variant itself (`getJson`), and
//! a hidden `null` names its position (`withNull`, or `withANull` when
//! several positions take one).

use std::collections::{HashMap, HashSet};

use crate::ir::{Literal, Type};

/// One position of a variant's JS argument list, before constants are
/// hidden.
#[derive(Clone, Copy, Debug)]
pub enum Slot<'a> {
    Param {
        name: &'a str,
        ty: &'a Type,
        /// A trailing rest parameter (`...rest`).
        rest: bool,
    },
    /// A constant moved into `@js(args)`; `name` is the source parameter's.
    Const { name: &'a str, value: &'a Literal },
}

/// Names for a group of variants of one member, `variants[0]` being the
/// primary: it keeps `base` unless it has a constant of its own. Names are
/// unique within the group. Types are named by [`type_word`].
pub fn variant_names(base: &str, variants: &[Vec<Slot<'_>>]) -> Vec<String> {
    variant_names_with(base, variants, &type_word)
}

/// [`variant_names`] with a generator's own type vocabulary, so a generator
/// names a type the way its signatures spell it (Rust: `slice`, `js_value`).
/// `type_word` returns a PascalCase chunk.
pub fn variant_names_with(
    base: &str,
    variants: &[Vec<Slot<'_>>],
    type_word: &dyn Fn(&Type) -> String,
) -> Vec<String> {
    let parsed: Vec<Variant<'_>> = variants.iter().map(|v| Variant::new(base, v)).collect();
    if parsed.len() == 1 {
        return vec![parsed[0].stem.clone()];
    }
    let sigs: Vec<&[Pos<'_>]> = parsed.iter().map(|v| v.positions.as_slice()).collect();
    let (trim_start, trim_end) = trim(&sigs);
    let null_positions: HashSet<usize> = sigs
        .iter()
        .flat_map(|s| s.iter().enumerate().filter(|(_, p)| p.is_null()))
        .map(|(i, _)| i)
        .collect();

    let mut used = Used::default();
    let mut names = vec![parsed[0].stem.clone()];
    used.claim(&parsed[0].stem);

    let primary = sigs[0];
    let plan = |idx: usize| -> (Vec<Part>, Vec<Part>) {
        let sig = sigs[idx];
        let end = if sig.len() >= trim_end {
            sig.len() - trim_end
        } else {
            sig.len()
        };
        let start = trim_start.min(end);
        let mut parts = Vec::new();
        let mut added = Vec::new();
        for (abs, p) in sig.iter().enumerate().take(end).skip(start) {
            let mut different = false;
            let mut retyped = false;
            for (other_idx, other) in sigs.iter().enumerate() {
                if other_idx == idx {
                    continue;
                }
                match other.get(abs) {
                    // A `null` position names itself, not the ones beside it.
                    Some(o) if o.is_null() && !p.is_null() => {}
                    Some(o) if o.name == p.name && o.kind != p.kind => {
                        retyped = true;
                        different = true;
                    }
                    Some(o) if o.name != p.name => different = true,
                    Some(_) => {}
                    None => different = true,
                }
            }
            if !different {
                continue;
            }
            let param = pascal(p.name);
            let (text, from_type) = match p.kind {
                Kind::Null if null_positions.len() > 1 => (format!("{param}Null"), false),
                Kind::Null => ("Null".to_string(), false),
                Kind::Ty(ty, _) if retyped => (type_word(ty), true),
                Kind::Ty(..) => (param.clone(), false),
            };
            let part = Part {
                text,
                from_type,
                param,
            };
            match primary.get(abs) {
                // A position the primary lacks is named by its parameter, so
                // a type word never names different parameters in sibling
                // names.
                None if part.from_type => added.push(Part {
                    text: part.param.clone(),
                    from_type: false,
                    param: part.param.clone(),
                }),
                None => added.push(part.clone()),
                Some(q) if q != p => added.push(part.clone()),
                Some(_) => {}
            }
            parts.push(part);
        }
        (added, parts)
    };
    let plans: Vec<(Vec<Part>, Vec<Part>)> = (1..sigs.len()).map(plan).collect();

    // A name for what a variant adds is kept only where no sibling adds the
    // same: `compareWithAAndB` and `compareWithBAndA`, never a lone
    // `compareWithB`.
    let mut added_names: HashMap<String, usize> = HashMap::new();
    for (v, (added, _)) in parsed[1..].iter().zip(&plans) {
        if !added.is_empty() {
            *added_names
                .entry(compose(&v.stem, added, Strategy::Greedy))
                .or_default() += 1;
        }
    }
    for (v, (added, parts)) in parsed[1..].iter().zip(plans) {
        let mut name = compose(&v.stem, &added, Strategy::Greedy);
        let bare = added.is_empty() && v.stem != base;
        if !bare && (added.is_empty() || added_names[&name] > 1 || used.contains(&name)) {
            name = compose(&v.stem, &parts, Strategy::Greedy);
        }
        if used.contains(&name) {
            name = compose(&v.stem, &parts, Strategy::ParamAndType);
        }
        names.push(used.declare(&name));
    }
    names
}

/// The default type vocabulary: the IR spelling as one PascalCase chunk
/// (`Str`, `F64`, `ArrayBuffer`, `Array`). Nullability doesn't name.
pub fn type_word(t: &Type) -> String {
    match t {
        Type::Unit => "Unit".into(),
        Type::UnitBox => "Unit".into(),
        Type::Never => "Never".into(),
        Type::Scalar(s) | Type::ScalarBox(s) => pascal(s.name()),
        Type::Str | Type::StrBox => "Str".into(),
        Type::BigInt | Type::BigIntBox => "BigInt".into(),
        Type::Any => "Any".into(),
        Type::Vec(_) | Type::VecBox(_) | Type::MutVec(_) => "Vec".into(),
        Type::Array(_) | Type::ArrBox(_) => "Array".into(),
        Type::Tuple(_) | Type::TupleBox(_) => "Tuple".into(),
        Type::Future(..) => "Future".into(),
        Type::Fn(_) => "Fn".into(),
        Type::View { .. } => "View".into(),
        Type::Nullable(t, _) => type_word(t),
        Type::Named(r) => {
            let spec = match &r.path.base {
                crate::ir::Base::Module(spec) => Some(spec),
                _ => None,
            };
            spec.into_iter()
                .chain(&r.path.segments)
                .map(|s| pascal(s))
                .collect()
        }
    }
}

/// PascalCase over the words of `s`: `flagKey` → `FlagKey`, `deflate-raw`
/// → `DeflateRaw`, `node:buffer` → `NodeBuffer`.
pub fn pascal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split(|c: char| !c.is_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(c) = chars.next() {
            out.extend(c.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// The name a hidden constant adds to its variant.
fn literal_word(l: &Literal) -> String {
    match l {
        Literal::Str(s) if pascal(s).is_empty() => "Empty".into(),
        Literal::Str(s) => pascal(s),
        Literal::Int(n) if *n < 0 => format!("Neg{}", n.unsigned_abs()),
        Literal::Int(n) => n.to_string(),
        Literal::Bool(true) => "True".into(),
        Literal::Bool(false) => "False".into(),
        Literal::Null => "Null".into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind<'a> {
    Ty(&'a Type, bool),
    Null,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos<'a> {
    name: &'a str,
    kind: Kind<'a>,
}

impl Pos<'_> {
    fn is_null(&self) -> bool {
        self.kind == Kind::Null
    }
}

struct Variant<'a> {
    /// The base, plus each hidden non-null constant.
    stem: String,
    /// Parameters and hidden `null`s; other constants named the stem.
    positions: Vec<Pos<'a>>,
}

impl<'a> Variant<'a> {
    fn new(base: &str, slots: &[Slot<'a>]) -> Variant<'a> {
        let mut stem = base.to_string();
        let mut positions = Vec::new();
        for s in slots {
            match *s {
                Slot::Param { name, ty, rest } => positions.push(Pos {
                    name,
                    kind: Kind::Ty(ty, rest),
                }),
                Slot::Const {
                    name,
                    value: Literal::Null,
                } => positions.push(Pos {
                    name,
                    kind: Kind::Null,
                }),
                Slot::Const { value, .. } => stem.push_str(&literal_word(value)),
            }
        }
        Variant { stem, positions }
    }
}

/// Positions identical across every variant at the same offset from the
/// start or the end don't disambiguate, so they're left out of names.
fn trim(sigs: &[&[Pos<'_>]]) -> (usize, usize) {
    let min = sigs.iter().map(|s| s.len()).min().unwrap_or(0);
    let start = (0..min)
        .take_while(|&i| sigs[1..].iter().all(|s| s[i] == sigs[0][i]))
        .count();
    let end = (0..min)
        .take_while(|&i| {
            let first = sigs[0][sigs[0].len() - 1 - i];
            sigs[1..].iter().all(|s| s[s.len() - 1 - i] == first)
        })
        .count();
    (start, end.min(min - start))
}

#[derive(Clone)]
struct Part {
    text: String,
    /// `text` is a type word, which can collide between siblings.
    from_type: bool,
    param: String,
}

enum Strategy {
    Greedy,
    /// Every type word is prefixed with its parameter name:
    /// `withContextMap`.
    ParamAndType,
}

fn compose(stem: &str, parts: &[Part], strategy: Strategy) -> String {
    let mut name = stem.to_string();
    // A base that already reads `with` continues with `And`.
    let mut joiner = if ends_with_word(stem, "with") {
        "And"
    } else {
        "With"
    };
    for p in parts {
        name.push_str(joiner);
        joiner = "And";
        match strategy {
            Strategy::ParamAndType if p.from_type => {
                name.push_str(&p.param);
                name.push_str(&p.text);
            }
            _ => name.push_str(&p.text),
        }
    }
    name
}

/// `with`, `setWith`, `set_with`.
fn ends_with_word(name: &str, word: &str) -> bool {
    let n = name.len();
    if n < word.len() || !name[n - word.len()..].eq_ignore_ascii_case(word) {
        return false;
    }
    let head = &name[..n - word.len()];
    let first = name[n - word.len()..].chars().next().unwrap();
    head.is_empty()
        || (first.is_uppercase() && head.chars().last().is_some_and(char::is_lowercase))
        || head.ends_with('_')
}

#[derive(Default)]
struct Used(HashSet<String>);

impl Used {
    fn claim(&mut self, name: &str) {
        self.0.insert(name.to_string());
    }

    fn contains(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    /// `name`, or `name2`, `name3`, … when it's taken.
    fn declare(&mut self, name: &str) -> String {
        let mut out = name.to_string();
        let mut n = 2;
        while self.0.contains(&out) {
            out = format!("{name}{n}");
            n += 1;
        }
        self.0.insert(out.clone());
        out
    }
}
