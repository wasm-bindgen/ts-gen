//! Evaluate TS `enum` members once every file is parsed.
//!
//! An initialiser can reference another enum's members, including one in a
//! namespace or another file, and a merged enum's members span all its
//! declarations, which can be in different files too. So population leaves
//! each enum as a [`TsEnumDecl`] and this pass evaluates them together,
//! member by member as references reach them, then replaces each enum's
//! declarations with one string or numeric enum.

use std::collections::{HashMap, HashSet};

use crate::context::{GlobalContext, TypeId};
use crate::ir::{
    EnumInit, ModuleContext, NumericEnumDecl, NumericEnumVariant, StringEnumDecl,
    StringEnumVariant, TsEnumDecl, TypeAliasDecl, TypeKind, TypeRef,
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
    /// The index of its declaration among the enum's.
    decl: usize,
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
        let (init, scope, decl) = (member.init, member.scope, member.decl);
        // Each declaration's first member starts from 0.
        let first = index == 0 || self.members[&key][index - 1].decl != decl;
        let value = match init {
            Some(init) => self.eval(init, key, index, scope),
            None if first => Some(EnumValue::Num(0.0)),
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

    /// A bare name is a member of the same enum. A qualified one names an
    /// enum through the scope chain. Either can only reach an earlier member
    /// of its own declaration, but any member of another declaration, which
    /// TS allows from another file.
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
        let own = (target == key).then(|| members[index].decl);
        let found = (0..members.len()).rev().find(|&i| {
            members[i].name == name.as_str() && (own != Some(members[i].decl) || i < index)
        })?;
        self.value(target, found)
    }
}

/// The scope a merged enum's declarations are in.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Root {
    Global,
    Module(ModuleContext),
    File(ScopeId),
}

/// Evaluate every TS enum and replace its declarations with one string or
/// numeric enum, numeric when any member is a number.
///
/// Declarations merge when they declare one name in the same namespace path
/// of the same scope: the global scope of every file, one module's
/// `declare module` blocks, or a module file's top level. The first takes
/// the merged enum. The others are dropped from `roots` and every other list of
/// declarations, and the names bound to them are rebound to the first.
pub(crate) fn evaluate_ts_enums(gctx: &mut GlobalContext, roots: &mut Vec<TypeId>) {
    // Phase 1 placeholders are in the arena but in no list.
    let listed: HashSet<TypeId> = roots.iter().copied().chain(gctx.listed_types()).collect();
    let ids: Vec<TypeId> = gctx
        .iter_types()
        .filter(|(id, decl)| matches!(decl.kind, TypeKind::TsEnum(_)) && listed.contains(id))
        .map(|(id, _)| id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let namespaces: HashMap<ScopeId, (ScopeId, &str)> = gctx
        .iter_types()
        .filter_map(|(_, decl)| match &decl.kind {
            TypeKind::Namespace(ns) => Some((ns.child_scope, (decl.scope_id, ns.name.as_str()))),
            _ => None,
        })
        .collect();
    // The namespace path from the scope a namespace chain starts in.
    let namespace_path = |mut scope: ScopeId| {
        let mut path = Vec::new();
        while let Some(&(parent, name)) = namespaces.get(&scope) {
            path.push(name.to_string());
            scope = parent;
        }
        path.reverse();
        (path, scope)
    };
    let mut keys: Vec<TypeId> = Vec::new();
    let mut groups: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
    let ambient: HashSet<ScopeId> = gctx
        .ambient_module_scopes
        .values()
        .flatten()
        .copied()
        .collect();
    let mut by_binding: HashMap<(Root, Vec<String>, String), TypeId> = HashMap::new();
    for &id in &ids {
        let decl = gctx.get_type(id);
        let TypeKind::TsEnum(e) = &decl.kind else {
            continue;
        };
        let (path, scope) = namespace_path(decl.scope_id);
        let root = if e.global {
            Root::Global
        } else if ambient.contains(&scope) {
            Root::Module(decl.module_context.clone())
        } else {
            Root::File(scope)
        };
        let binding = (root, path, e.name.clone());
        let key = *by_binding.entry(binding).or_insert_with(|| {
            keys.push(id);
            id
        });
        groups.entry(key).or_default().push(id);
    }

    let mut diag = DiagnosticCollector::new();
    let results: Vec<(Vec<TypeId>, TypeKind, Option<String>)> = {
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
                let index = members.last().map_or(0, |m: &Member<'_>| m.decl + 1);
                members.extend(e.members.iter().map(|m| Member {
                    name: &m.name,
                    init: m.init.as_ref(),
                    doc: &m.doc,
                    scope: decl.scope_id,
                    decl: index,
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
                let decl = gctx.get_type(key);
                let TypeKind::TsEnum(TsEnumDecl { name, .. }) = &decl.kind else {
                    unreachable!("grouped under a TS enum declaration");
                };
                let kind = finish(
                    name,
                    decl.scope_id,
                    &enums.members[&key],
                    &values,
                    &mut diag,
                );
                let decls = &groups[&key];
                let doc = decls.iter().find_map(|&id| gctx.get_type(id).doc.clone());
                (decls.clone(), kind, doc)
            })
            .collect()
    };
    gctx.diagnostics.diagnostics.extend(diag.diagnostics);

    let mut absorbed: HashMap<TypeId, TypeId> = HashMap::new();
    for (decls, kind, doc) in results {
        let decl = gctx.get_type_mut(decls[0]);
        decl.kind = kind;
        decl.doc = doc;
        absorbed.extend(decls[1..].iter().map(|&id| (id, decls[0])));
    }
    if !absorbed.is_empty() {
        let dropped: HashSet<TypeId> = absorbed.keys().copied().collect();
        roots.retain(|id| !dropped.contains(id));
        gctx.drop_types(&dropped);
        gctx.scopes.rebind(&absorbed);
    }
}

/// The string or numeric enum `members` evaluate to. A member whose value
/// doesn't fit the kind is skipped with a warning: a guessed discriminant
/// could repeat a real member's value and make that member an alias of it.
/// wasm-bindgen can't export an enum without variants, so one left with
/// none is erased to `JsValue`.
fn finish(
    name: &str,
    scope: ScopeId,
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
    if kept.is_empty() {
        diag.warn(format!(
            "Enum `{name}` has no member with a constant value, erasing it to JsValue"
        ));
        return TypeKind::TypeAlias(TypeAliasDecl {
            name,
            type_params: vec![],
            target: TypeRef::Any,
            from_module: None,
            body_scope: scope,
        });
    }
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

    /// Every numeric enum's name and variants, sorted.
    fn numeric_enums(gctx: &crate::context::GlobalContext) -> Vec<(String, Vec<(String, i64)>)> {
        let mut enums: Vec<_> = gctx
            .iter_types()
            .filter_map(|(_, decl)| match &decl.kind {
                TypeKind::NumericEnum(e) => Some((
                    e.name.clone(),
                    e.variants
                        .iter()
                        .map(|v| (v.js_name.clone(), v.value))
                        .collect(),
                )),
                _ => None,
            })
            .collect();
        enums.sort();
        enums
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
    fn each_declaration_counts_from_zero() {
        let src = "declare enum M { A = 5 } declare enum M { B, C } declare enum M { D = A + C }";
        assert_eq!(
            numeric(src, "M"),
            pairs(&[("A", 5), ("B", 0), ("C", 1), ("D", 6)])
        );
    }

    #[test]
    fn declarations_merge_across_files() {
        let dir = std::env::temp_dir().join(format!("ts-gen-enum-merge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.d.ts"), dir.join("b.d.ts"));
        std::fs::write(
            &a,
            r#"
                declare enum G { A = 1, C = B + 1 }
                declare namespace N { enum NE { A = 1 } }
                declare module "m" { export enum ME { A = 1 } }
            "#,
        )
        .unwrap();
        std::fs::write(
            &b,
            r#"
                declare enum G { B = A + 1 }
                declare enum H { X = G.A }
                declare namespace N { enum NE { B = A + 1 } }
                declare module "m" { export enum ME { B = A + 1 } }
            "#,
        )
        .unwrap();
        let (_, gctx) = crate::parse::parse_dts_files(&[&a, &b], None).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let merged = pairs(&[("A", 1), ("B", 2)]);
        assert_eq!(
            numeric_enums(&gctx),
            vec![
                ("G".to_string(), pairs(&[("A", 1), ("C", 3), ("B", 2)])),
                ("H".to_string(), pairs(&[("X", 1)])),
                ("ME".to_string(), merged.clone()),
                ("NE".to_string(), merged),
            ]
        );
    }

    #[test]
    fn module_files_keep_their_own_enums() {
        let dir = std::env::temp_dir().join(format!("ts-gen-enum-module-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (x, y) = (dir.join("x.d.ts"), dir.join("y.d.ts"));
        std::fs::write(
            &x,
            "export declare enum Kind { A = 9 } export declare enum F { Q = Kind.A }",
        )
        .unwrap();
        std::fs::write(
            &y,
            r#"
                import { F } from "./x";
                export declare enum Kind { A = 1 }
                export declare enum Uses { U = Kind.A }
                export declare function use(k: Kind, u: Uses, f: F): void;
            "#,
        )
        .unwrap();
        let (_, gctx) = crate::parse::parse_dts_files(&[&y], Some("y")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(
            numeric_enums(&gctx),
            vec![
                ("F".to_string(), pairs(&[("Q", 9)])),
                ("Kind".to_string(), pairs(&[("A", 1)])),
                ("Kind".to_string(), pairs(&[("A", 9)])),
                ("Uses".to_string(), pairs(&[("U", 1)])),
            ]
        );
    }

    #[test]
    fn an_enum_without_values_is_erased() {
        let src = "declare enum E { A = Undeclared.X }";
        let (module, gctx) = crate::parse::parse_single_source(src, None).unwrap();
        let kind = &gctx.get_type(module.types[0]).kind;
        assert!(
            matches!(kind, TypeKind::TypeAlias(a) if a.target == crate::ir::TypeRef::Any),
            "{kind:?}"
        );
    }

    #[test]
    fn cycles_have_no_value() {
        let src = "declare enum Cycle { A = Loop.B, Fixed = 1 } \
                   declare enum Loop { B = Cycle.A, C = Cycle.Fixed }";
        assert_eq!(numeric(src, "Cycle"), pairs(&[("Fixed", 1)]));
        assert_eq!(numeric(src, "Loop"), pairs(&[("C", 1)]));
    }
}
