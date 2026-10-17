//! Evaluate TS `enum` members once every file is parsed.
//!
//! An initialiser can reference another enum's members, including one
//! declared later, in a namespace or in another file, and a merged enum's
//! members span all its declarations. So population leaves each enum as a
//! [`TsEnumDecl`] and this pass evaluates them together, member by member as
//! references reach them, then replaces each enum's declarations with one
//! string or numeric enum.

use std::collections::{HashMap, HashSet};

use crate::context::{GlobalContext, TypeId};
use crate::ir::{
    EnumInit, NumericEnumDecl, NumericEnumVariant, StringEnumDecl, StringEnumVariant, TsEnumDecl,
    TypeKind,
};
use crate::parse::scope::ScopeId;
use crate::util::diagnostics::DiagnosticCollector;

/// The value a TS enum member evaluates to.
#[derive(Clone, Debug, PartialEq)]
enum EnumValue {
    Num(f64),
    Str(String),
}

enum State {
    Pending,
    InProgress,
    Done(Option<EnumValue>),
}

struct Member<'g> {
    name: &'g str,
    init: Option<&'g EnumInit>,
    doc: &'g Option<String>,
    /// The enum declaration's scope, where qualified references resolve.
    scope: ScopeId,
    state: State,
}

struct Enums<'g> {
    gctx: &'g GlobalContext,
    /// Each enum's members across its declarations, in declaration order,
    /// by the declaration its name binds to.
    members: HashMap<TypeId, Vec<Member<'g>>>,
    /// The enum each declaration belongs to.
    enum_of: HashMap<TypeId, TypeId>,
}

impl<'g> Enums<'g> {
    /// A member's value, evaluated on first use. One that's reached again
    /// while being evaluated is part of a cycle, which has no value.
    fn value(&mut self, key: TypeId, index: usize) -> Option<EnumValue> {
        let member = &mut self.members.get_mut(&key)?[index];
        match &member.state {
            State::Done(value) => return value.clone(),
            State::InProgress => return None,
            State::Pending => member.state = State::InProgress,
        }
        let (init, scope) = (member.init, member.scope);
        let value = match init {
            Some(init) => self.eval(init, key, index, scope),
            None if index == 0 => Some(EnumValue::Num(0.0)),
            None => match self.value(key, index - 1) {
                Some(EnumValue::Num(prev)) => Some(EnumValue::Num(prev + 1.0)),
                _ => None,
            },
        };
        self.members.get_mut(&key)?[index].state = State::Done(value.clone());
        value
    }

    /// Evaluate an initialiser the way TypeScript does, with JS numeric
    /// semantics (bitwise operators on int32, `>>>` on uint32).
    fn eval(
        &mut self,
        init: &'g EnumInit,
        key: TypeId,
        index: usize,
        scope: ScopeId,
    ) -> Option<EnumValue> {
        match init {
            EnumInit::Num(n) => Some(EnumValue::Num(*n)),
            EnumInit::Str(s) => Some(EnumValue::Str(s.clone())),
            EnumInit::Ref(path) => self.reference(path, key, index, scope),
            EnumInit::Unary(op, arg) => {
                let EnumValue::Num(v) = self.eval(arg, key, index, scope)? else {
                    return None;
                };
                Some(EnumValue::Num(match op.as_str() {
                    "-" => -v,
                    "+" => v,
                    "~" => f64::from(!to_int32(v)),
                    _ => return None,
                }))
            }
            EnumInit::Binary(op, l, r) => {
                let l = self.eval(l, key, index, scope)?;
                let r = self.eval(r, key, index, scope)?;
                let (l, r) = match (l, r) {
                    (EnumValue::Str(l), EnumValue::Str(r)) if op == "+" => {
                        return Some(EnumValue::Str(l + &r));
                    }
                    (EnumValue::Num(l), EnumValue::Num(r)) => (l, r),
                    _ => return None,
                };
                let (li, ri) = (to_int32(l), to_int32(r));
                Some(EnumValue::Num(match op.as_str() {
                    "+" => l + r,
                    "-" => l - r,
                    "*" => l * r,
                    "/" => l / r,
                    "%" => l % r,
                    "**" => l.powf(r),
                    "&" => f64::from(li & ri),
                    "|" => f64::from(li | ri),
                    "^" => f64::from(li ^ ri),
                    "<<" => f64::from(li.wrapping_shl(ri as u32 & 31)),
                    ">>" => f64::from(li.wrapping_shr(ri as u32 & 31)),
                    ">>>" => f64::from((li as u32).wrapping_shr(ri as u32 & 31)),
                    _ => return None,
                }))
            }
            EnumInit::Unknown => None,
        }
    }

    /// A bare name is an earlier member of the same enum. A qualified one
    /// names an enum through the scope chain, and can only reach an earlier
    /// member when that's the same enum.
    fn reference(
        &mut self,
        path: &[String],
        key: TypeId,
        index: usize,
        scope: ScopeId,
    ) -> Option<EnumValue> {
        let (target, name) = match path {
            [name] => (key, name),
            [enum_path @ .., name] => {
                let id = self.gctx.resolve_path(scope, &enum_path.join("."))?;
                (*self.enum_of.get(&id)?, name)
            }
            [] => return None,
        };
        let members = self.members.get(&target)?;
        let reachable = if target == key { index } else { members.len() };
        let found = members[..reachable]
            .iter()
            .rposition(|m| m.name == name.as_str())?;
        self.value(target, found)
    }
}

/// Evaluate every TS enum and replace its declarations with one string or
/// numeric enum, numeric when any member is a number. A merged enum keeps
/// the declaration its name binds to, and the others are dropped from
/// `roots` and every other list of declarations.
pub(crate) fn evaluate_ts_enums(gctx: &mut GlobalContext, roots: &mut Vec<TypeId>) {
    let ids: Vec<TypeId> = gctx
        .iter_types()
        .filter(|(_, decl)| matches!(decl.kind, TypeKind::TsEnum(_)))
        .map(|(id, _)| id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let mut keys: Vec<TypeId> = Vec::new();
    let mut groups: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
    for &id in &ids {
        let decl = gctx.get_type(id);
        let TypeKind::TsEnum(e) = &decl.kind else {
            continue;
        };
        let key = gctx
            .scopes
            .get(decl.scope_id)
            .get(&e.name)
            .filter(|bound| matches!(gctx.get_type(*bound).kind, TypeKind::TsEnum(_)))
            .unwrap_or(id);
        if !groups.contains_key(&key) {
            keys.push(key);
        }
        groups.entry(key).or_default().push(id);
    }

    let mut diag = DiagnosticCollector::new();
    let results: Vec<(TypeId, Vec<TypeId>, TypeKind, Option<String>)> = {
        let gctx: &GlobalContext = gctx;
        let mut enums = Enums {
            gctx,
            members: HashMap::new(),
            enum_of: HashMap::new(),
        };
        for (&key, decls) in &groups {
            let mut members = Vec::new();
            for &id in decls {
                enums.enum_of.insert(id, key);
                let decl = gctx.get_type(id);
                let TypeKind::TsEnum(e) = &decl.kind else {
                    continue;
                };
                members.extend(e.members.iter().map(|m| Member {
                    name: &m.name,
                    init: m.init.as_ref(),
                    doc: &m.doc,
                    scope: decl.scope_id,
                    state: State::Pending,
                }));
            }
            enums.members.insert(key, members);
        }
        keys.iter()
            .map(|&key| {
                let values: Vec<Option<EnumValue>> = (0..enums.members[&key].len())
                    .map(|i| enums.value(key, i))
                    .collect();
                let TypeKind::TsEnum(TsEnumDecl { name, .. }) = &gctx.get_type(key).kind else {
                    unreachable!("grouped under a TS enum declaration");
                };
                let kind = finish(name, &enums.members[&key], &values, &mut diag);
                let decls = &groups[&key];
                let doc = decls.iter().find_map(|&id| gctx.get_type(id).doc.clone());
                (key, decls.clone(), kind, doc)
            })
            .collect()
    };
    gctx.diagnostics.diagnostics.extend(diag.diagnostics);

    let mut dropped = HashSet::new();
    for (key, decls, kind, doc) in results {
        let decl = gctx.get_type_mut(key);
        decl.kind = kind;
        decl.doc = doc;
        dropped.extend(decls.into_iter().filter(|&id| id != key));
    }
    if !dropped.is_empty() {
        roots.retain(|id| !dropped.contains(id));
        gctx.drop_types(&dropped);
    }
}

/// The string or numeric enum `members` evaluate to. A member whose value
/// doesn't fit the kind is skipped with a warning: a guessed discriminant
/// could repeat a real member's value and make that member an alias of it.
fn finish(
    name: &str,
    members: &[Member<'_>],
    values: &[Option<EnumValue>],
    diag: &mut DiagnosticCollector,
) -> TypeKind {
    let numeric = values.iter().any(|v| matches!(v, Some(EnumValue::Num(_))));
    let mut warn_skipped = |member: &str, value: &Option<EnumValue>| {
        let reason = match value {
            Some(EnumValue::Str(_)) => "is a string in a numeric enum",
            Some(EnumValue::Num(_)) => "is a number in a string enum",
            None => "has no constant value",
        };
        diag.warn(format!("Enum `{name}::{member}` {reason}, skipping it"));
    };
    let mut kept: Vec<(&Member<'_>, &EnumValue)> = Vec::new();
    for (member, value) in members.iter().zip(values) {
        match value {
            Some(v @ EnumValue::Num(_)) if numeric => kept.push((member, v)),
            Some(v @ EnumValue::Str(_)) if !numeric => kept.push((member, v)),
            _ => warn_skipped(member.name, value),
        }
    }
    let mut rust_names: Vec<String> = kept
        .iter()
        .map(|(m, _)| crate::util::naming::to_enum_variant(m.name))
        .collect();
    crate::util::naming::dedup_names(&mut rust_names);
    let name = name.to_string();
    if numeric {
        let variants = kept
            .into_iter()
            .zip(rust_names)
            .map(|((member, value), rust_name)| {
                let EnumValue::Num(value) = value else {
                    unreachable!("kept by kind");
                };
                NumericEnumVariant {
                    rust_name,
                    js_name: member.name.to_string(),
                    value: f64_to_i64(*value, member.name, &name, diag),
                    doc: member.doc.clone(),
                }
            })
            .collect();
        TypeKind::NumericEnum(NumericEnumDecl { name, variants })
    } else {
        let variants = kept
            .into_iter()
            .zip(rust_names)
            .map(|((_, value), rust_name)| {
                let EnumValue::Str(js_value) = value else {
                    unreachable!("kept by kind");
                };
                StringEnumVariant {
                    rust_name,
                    js_value: js_value.clone(),
                }
            })
            .collect();
        TypeKind::StringEnum(StringEnumDecl { name, variants })
    }
}

/// ECMAScript `ToInt32`: modular wrap, not saturation.
fn to_int32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    (v.trunc().rem_euclid(4_294_967_296.0) as u32) as i32
}

/// Convert f64 (JS number) to i64 with a diagnostic if the value is not
/// an exact integer or is out of i64 range.
fn f64_to_i64(
    value: f64,
    member_name: &str,
    enum_name: &str,
    diag: &mut DiagnosticCollector,
) -> i64 {
    if value.fract() != 0.0 {
        diag.warn(format!(
            "Enum `{enum_name}::{member_name}` has non-integer value {value}, truncating to {}",
            value as i64
        ));
    } else if value > i64::MAX as f64 || value < i64::MIN as f64 {
        diag.warn(format!(
            "Enum `{enum_name}::{member_name}` value {value} is out of i64 range, truncating"
        ));
    }
    let result = value as i64;
    // Warn if value won't fit in the codegen repr (i32 for signed, u32 for unsigned)
    if i32::try_from(result).is_err() && u32::try_from(result).is_err() {
        diag.warn(format!(
            "Enum `{enum_name}::{member_name}` value {result} exceeds i32/u32 range, \
             will be truncated in generated code"
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::ir::TypeKind;

    /// The enums `src` declares at its top level, and its enum warnings.
    fn parse(src: &str) -> (Vec<TypeKind>, Vec<String>) {
        let (module, gctx) = crate::parse::parse_single_source(src, None).unwrap();
        let kinds = module
            .types
            .iter()
            .map(|id| gctx.get_type(*id).kind.clone())
            .filter(|kind| matches!(kind, TypeKind::StringEnum(_) | TypeKind::NumericEnum(_)))
            .collect();
        let warnings = gctx
            .diagnostics
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .filter(|m| m.starts_with("Enum `"))
            .collect();
        (kinds, warnings)
    }

    fn find<'k>(kinds: &'k [TypeKind], name: &str) -> &'k TypeKind {
        kinds
            .iter()
            .find(|k| crate::codegen::typemap::type_decl_name(k) == Some(name))
            .unwrap_or_else(|| panic!("no enum `{name}`"))
    }

    fn numeric(src: &str, name: &str) -> Vec<(String, i64)> {
        match find(&parse(src).0, name) {
            TypeKind::NumericEnum(e) => e
                .variants
                .iter()
                .map(|v| (v.js_name.clone(), v.value))
                .collect(),
            other => panic!("`{name}` is not numeric: {other:?}"),
        }
    }

    fn string(src: &str, name: &str) -> Vec<(String, String)> {
        match find(&parse(src).0, name) {
            TypeKind::StringEnum(e) => e
                .variants
                .iter()
                .map(|v| (v.rust_name.clone(), v.js_value.clone()))
                .collect(),
            other => panic!("`{name}` is not a string enum: {other:?}"),
        }
    }

    fn pairs<V: Clone>(items: &[(&str, V)]) -> Vec<(String, V)> {
        items
            .iter()
            .map(|(n, v)| (n.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn member_references_take_the_referenced_value() {
        let values = numeric("declare enum E { A = 1, B = 2, C = B, D = E.A, F }", "E");
        assert_eq!(
            values,
            pairs(&[("A", 1), ("B", 2), ("C", 2), ("D", 1), ("F", 2)])
        );
    }

    #[test]
    fn constant_expressions_use_js_numeric_semantics() {
        let values = numeric(
            "declare enum F { R = 1 << 0, W = 1 << 1, RW = R | W, All = RW | 4, Mask = ~0, \
             Neg = -R, Big = 1 << 31, Shr = -8 >> 1, Ushr = -1 >>> 28, Pow = 2 ** 3 }",
            "F",
        );
        assert_eq!(
            values,
            pairs(&[
                ("R", 1),
                ("W", 2),
                ("RW", 3),
                ("All", 7),
                ("Mask", -1),
                ("Neg", -1),
                ("Big", -2147483648),
                ("Shr", -4),
                ("Ushr", 15),
                ("Pow", 8),
            ])
        );
    }

    #[test]
    fn members_without_a_constant_value_are_skipped() {
        let values = numeric(
            "declare enum E { A = 1, B = Other.X, C = 2, D = B, F }",
            "E",
        );
        assert_eq!(values, pairs(&[("A", 1), ("C", 2)]));
        let values = string("declare enum K { A = \"B\", B = Other.X, C = \"c\" }", "K");
        assert_eq!(values, pairs(&[("A", "B".to_string()), ("C", "c".into())]));
        // Only an earlier member of the same enum is in scope.
        assert_eq!(
            numeric("declare enum E { A = B, B = 1 }", "E"),
            pairs(&[("B", 1)])
        );
    }

    #[test]
    fn skip_warnings_distinguish_mismatched_kind_from_unknown_value() {
        assert_eq!(
            parse("declare enum H { A = 1, B = \"b\", C = Other.X }").1,
            [
                "Enum `H::B` is a string in a numeric enum, skipping it",
                "Enum `H::C` has no constant value, skipping it",
            ]
        );
        assert_eq!(
            parse("declare enum K { A = \"a\", B = Other.X }").1,
            ["Enum `K::B` has no constant value, skipping it"]
        );
    }

    #[test]
    fn classification_follows_evaluated_values() {
        let (kinds, _) = parse(
            "declare enum F { R = 1 << 0, W = 1 << 1, RW = R | W } \
             declare enum K { A = \"a\" + \"b\", B = A } \
             declare enum E { A, B } \
             declare enum S { A = Other.A, B = K.A }",
        );
        assert!(matches!(find(&kinds, "F"), TypeKind::NumericEnum(_)));
        assert!(matches!(find(&kinds, "K"), TypeKind::StringEnum(_)));
        assert!(matches!(find(&kinds, "E"), TypeKind::NumericEnum(_)));
        assert!(matches!(find(&kinds, "S"), TypeKind::StringEnum(_)));
    }

    #[test]
    fn string_members_reference_and_concatenate() {
        let values = string(
            "declare enum K { A = \"a\", B = A, C = A + \"c\", D = `d`, E = K.B }",
            "K",
        );
        assert_eq!(
            values,
            pairs(&[
                ("A", "a".to_string()),
                ("B", "a".into()),
                ("C", "ac".into()),
                ("D", "d".into()),
                ("E", "a".into())
            ])
        );
    }

    #[test]
    fn references_reach_other_enums() {
        let src = "declare enum Base { Default = 2, Other = 4 } \
                   declare enum Mode { A = Base.Default, B, C = A | 8, D = Later.X + Mode.B, \
                   E = Base[\"Other\"] } \
                   declare enum Later { X = 16 } \
                   declare namespace Ns { enum Inner { Value = \"inner\" } } \
                   declare enum Label { Inner = Ns.Inner.Value, Joined = Ns.Inner.Value + \"-x\" }";
        assert_eq!(
            numeric(src, "Mode"),
            pairs(&[("A", 2), ("B", 3), ("C", 10), ("D", 19), ("E", 4)])
        );
        assert_eq!(
            string(src, "Label"),
            pairs(&[("Inner", "inner".to_string()), ("Joined", "inner-x".into())])
        );
    }

    #[test]
    fn merged_declarations_evaluate_as_one() {
        let src = "/** Merged. */ declare enum E { A = 1 } declare enum E { B = A + 1, C }";
        let (kinds, warnings) = parse(src);
        assert_eq!(kinds.len(), 1);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(numeric(src, "E"), pairs(&[("A", 1), ("B", 2), ("C", 3)]));
    }

    #[test]
    fn cycles_have_no_value() {
        let src = "declare enum Cycle { A = Loop.B, Fixed = 1 } \
                   declare enum Loop { B = Cycle.A, C = Cycle.Fixed }";
        assert_eq!(numeric(src, "Cycle"), pairs(&[("Fixed", 1)]));
        assert_eq!(numeric(src, "Loop"), pairs(&[("C", 1)]));
    }
}
