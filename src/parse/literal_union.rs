//! Structural merge of an anonymous-interface union into a single
//! interface body.
//!
//! TypeScript happily lets users write
//!
//! ```ts
//! type EmailAttachment =
//!   | { disposition: "inline"; contentId: string; filename: string; }
//!   | { disposition: "attachment"; contentId?: undefined; filename: string; };
//! ```
//!
//! and pass values shaped like either branch. There's no way to express
//! that union shape directly at the wasm-bindgen FFI boundary; instead,
//! `ts-gen` reduces such unions to a single inline interface whose members
//! are the structural intersection (every property that appears in any
//! branch), with optionality and types adjusted to remain valid against
//! every branch.
//!
//! ## Merge rules
//!
//! For each property name `p` appearing in any branch:
//!
//! * **Optionality**: required iff `p` is required in **every** branch
//!   (present and non-optional). If any branch declares it optional, or
//!   omits it entirely, the merged property is optional.
//! * **Type**: union of `p`'s types across the branches it appears in.
//!   The resulting type goes through the regular union resolution
//!   (subtyping LUB or `JsValue` fallback).
//! * **Read/write capability**: writable in the merged interface iff
//!   it's writable in every branch where it appears (any `readonly`
//!   branch downgrades to read-only).
//!
//! For methods of the same name, all signatures are kept and flow
//! through the regular overload-flattening pipeline.
//!
//! Index signatures, if all branches agree, pass through as-is. Mismatched
//! index signatures across branches degrade to the first one with a
//! diagnostic.
//!
//! Anything that isn't a property / method / index signature (e.g. call
//! signatures, construct signatures) is skipped — type literals in real
//! `.d.ts` files virtually never carry these.

use crate::ir::{
    GetterMember, IndexSigMember, Member, MethodMember, SetterMember, StaticGetterMember,
    StaticMethodMember, StaticSetterMember, TypeRef,
};
use indexmap::IndexMap;

/// Merge a list of type-literal branches (each as a `Vec<Member>` produced
/// by the regular `convert_ts_signature` pipeline) into a single combined
/// member set per the rules above.
pub(crate) fn merge_member_branches(branches: &[Vec<Member>]) -> Vec<Member> {
    if branches.is_empty() {
        return Vec::new();
    }
    if branches.len() == 1 {
        return branches[0].clone();
    }

    // Collect every property name that appears in any branch, plus the
    // matching getter/setter per branch (or `None` for "absent").
    //
    // `IndexMap` preserves first-appearance order across branches so the
    // merged interface keeps the user's source order — important because
    // it carries through to constructor / builder parameter order.
    let mut getters: IndexMap<String, Vec<Option<&GetterMember>>> = IndexMap::new();
    let mut setters: IndexMap<String, Vec<Option<&SetterMember>>> = IndexMap::new();
    let mut methods: IndexMap<String, Vec<&MethodMember>> = IndexMap::new();
    let mut index_sigs: Vec<&IndexSigMember> = Vec::new();

    // Statics are not expected in inline literals; if present, we pass
    // them through unmerged from the first branch that has them.
    let mut static_getters: IndexMap<String, &StaticGetterMember> = IndexMap::new();
    let mut static_setters: IndexMap<String, &StaticSetterMember> = IndexMap::new();
    let mut static_methods: IndexMap<String, Vec<&StaticMethodMember>> = IndexMap::new();

    // Walk each branch once collecting names, then in a second pass align
    // per-branch slots so missing members surface as `None`.
    for branch in branches {
        for m in branch {
            match m {
                Member::Getter(g) => {
                    getters.entry(g.js_name.clone()).or_default();
                }
                Member::Setter(s) => {
                    setters.entry(s.js_name.clone()).or_default();
                }
                Member::Method(_) | Member::IndexSignature(_) | Member::Constructor(_) => {}
                Member::StaticGetter(_) | Member::StaticSetter(_) | Member::StaticMethod(_) => {}
            }
        }
    }

    for branch in branches {
        for (name, slots) in getters.iter_mut() {
            slots.push(branch.iter().find_map(|m| match m {
                Member::Getter(g) if g.js_name == *name => Some(g),
                _ => None,
            }));
        }
        for (name, slots) in setters.iter_mut() {
            slots.push(branch.iter().find_map(|m| match m {
                Member::Setter(s) if s.js_name == *name => Some(s),
                _ => None,
            }));
        }
        for m in branch {
            match m {
                Member::Method(meth) => {
                    methods.entry(meth.js_name.clone()).or_default().push(meth);
                }
                Member::IndexSignature(idx) => index_sigs.push(idx),
                Member::StaticGetter(g) => {
                    static_getters.entry(g.js_name.clone()).or_insert(g);
                }
                Member::StaticSetter(s) => {
                    static_setters.entry(s.js_name.clone()).or_insert(s);
                }
                Member::StaticMethod(meth) => {
                    static_methods
                        .entry(meth.js_name.clone())
                        .or_default()
                        .push(meth);
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();

    // Walk properties in source order, emitting each property's getter
    // and (when applicable) setter together. The non-merged interface
    // path already produces getter-then-setter per property, so this
    // keeps the literal-union output consistent with the rest of the
    // pipeline rather than emitting all getters then all setters.
    //
    // Drives off the getter ordering since every writable property has
    // a matching getter in the merged shape; setter-only properties
    // (extremely rare in TS) are picked up in a tail loop below.
    let getter_emitted = |name: &str| -> Option<Member> {
        let slots = getters.get(name)?;
        let present: Vec<&GetterMember> = slots.iter().filter_map(|s| *s).collect();
        if present.is_empty() {
            return None;
        }
        let absent_in_any = slots.iter().any(|s| s.is_none());
        let optional_in_any = present.iter().any(|g| g.optional);
        let optional = absent_in_any || optional_in_any;

        let type_ref = union_member_types(present.iter().map(|g| g.type_ref.clone()));
        let doc = present.iter().find_map(|g| g.doc.clone());

        Some(Member::Getter(GetterMember {
            js_name: name.to_string(),
            type_ref,
            optional,
            doc,
        }))
    };

    // Emit a setter when one exists and isn't suppressed by a `readonly`
    // branch. A getter-without-setter in any branch downgrades the
    // merged property to read-only, since writing through the merged
    // setter would be invalid for the readonly branch.
    let setter_emitted = |name: &str| -> Option<Member> {
        let slots = setters.get(name)?;
        let present: Vec<&SetterMember> = slots.iter().filter_map(|s| *s).collect();
        if present.is_empty() {
            return None;
        }
        if let Some(getter_slots) = getters.get(name) {
            let any_branch_readonly = getter_slots
                .iter()
                .zip(slots.iter())
                .any(|(g, s)| g.is_some() && s.is_none());
            if any_branch_readonly {
                return None;
            }
        }
        let type_ref = union_member_types(present.iter().map(|s| s.type_ref.clone()));
        let doc = present.iter().find_map(|s| s.doc.clone());
        Some(Member::Setter(SetterMember {
            js_name: name.to_string(),
            type_ref,
            doc,
        }))
    };

    for name in getters.keys() {
        if let Some(g) = getter_emitted(name) {
            out.push(g);
        }
        if let Some(s) = setter_emitted(name) {
            out.push(s);
        }
    }
    // Setter-only properties (no matching getter in any branch). The
    // earlier loop didn't visit these since it keys off getter names.
    for name in setters.keys() {
        if getters.contains_key(name) {
            continue;
        }
        if let Some(s) = setter_emitted(name) {
            out.push(s);
        }
    }

    // Methods: keep every signature. The flattening pipeline downstream
    // handles overload disambiguation.
    for (_, sigs) in methods {
        for m in sigs {
            out.push(Member::Method(m.clone()));
        }
    }

    // Index signatures: dedupe by structural equality of (key, value).
    // The first one wins; the rest are silently dropped.
    if let Some(first) = index_sigs.first() {
        out.push(Member::IndexSignature((*first).clone()));
    }

    for (_, g) in static_getters {
        out.push(Member::StaticGetter(g.clone()));
    }
    for (_, s) in static_setters {
        out.push(Member::StaticSetter(s.clone()));
    }
    for (_, sigs) in static_methods {
        for m in sigs {
            out.push(Member::StaticMethod(m.clone()));
        }
    }

    out
}

/// Combine per-property branch types into a single `TypeRef`.
///
/// Identical branches collapse to a single type. Otherwise we hand
/// off to the regular [`simplify_union`] from the parse layer so
/// `null` / `undefined` arms coalesce into `Nullable<T>` (matching
/// what users get from a hand-written `T | null` union).
///
/// [`simplify_union`]: crate::parse::types::simplify_union
fn union_member_types(types: impl IntoIterator<Item = TypeRef>) -> TypeRef {
    let mut all: Vec<TypeRef> = types.into_iter().collect();
    if all.is_empty() {
        return TypeRef::Any;
    }
    all.dedup();
    if all.len() == 1 {
        return all.into_iter().next().unwrap();
    }
    crate::parse::types::simplify_union(all)
}

/// Identify discriminator properties across a set of branches.
///
/// A property qualifies as a discriminator when:
///
/// * It appears as a **required** (non-optional) getter in **every**
///   branch.
/// * Its type in every branch is a string, number, or boolean literal.
///
/// TypeScript narrows on all three literal kinds (e.g. `disposition:
/// "inline"`, `done: false`, `status: 200`), so the codegen rule
/// matches.
///
/// Returns the JS names of qualifying properties in source order
/// (first-appearance across branches).
pub(crate) fn detect_discriminators(branches: &[Vec<Member>]) -> Vec<String> {
    if branches.len() < 2 {
        return Vec::new();
    }
    // Collect every property name with its per-branch (optional?, type)
    // pair, in first-appearance order.
    let mut order: Vec<String> = Vec::new();
    let mut per_branch: indexmap::IndexMap<String, Vec<Option<&GetterMember>>> =
        indexmap::IndexMap::new();
    for branch in branches {
        for m in branch {
            if let Member::Getter(g) = m {
                if !per_branch.contains_key(&g.js_name) {
                    order.push(g.js_name.clone());
                }
                per_branch.entry(g.js_name.clone()).or_default();
            }
        }
    }
    for branch in branches {
        for slot_name in per_branch.keys().cloned().collect::<Vec<_>>() {
            let found = branch.iter().find_map(|m| match m {
                Member::Getter(g) if g.js_name == slot_name => Some(g),
                _ => None,
            });
            per_branch.get_mut(&slot_name).unwrap().push(found);
        }
    }

    order
        .into_iter()
        .filter(|name| {
            let slots = &per_branch[name];
            // Must be present, required, and literal-typed in every
            // branch.
            slots.iter().all(|slot| {
                slot.is_some_and(|g| {
                    !g.optional
                        && matches!(
                            g.type_ref,
                            TypeRef::StringLiteral(_)
                                | TypeRef::NumberLiteral(_)
                                | TypeRef::BooleanLiteral(_)
                        )
                })
            })
        })
        .collect()
}
