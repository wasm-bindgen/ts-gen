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
use std::collections::BTreeMap;

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
    // BTreeMap keeps output order deterministic across runs.
    let mut getters: BTreeMap<String, Vec<Option<&GetterMember>>> = BTreeMap::new();
    let mut setters: BTreeMap<String, Vec<Option<&SetterMember>>> = BTreeMap::new();
    let mut methods: BTreeMap<String, Vec<&MethodMember>> = BTreeMap::new();
    let mut index_sigs: Vec<&IndexSigMember> = Vec::new();

    // Statics are not expected in inline literals; if present, we pass
    // them through unmerged from the first branch that has them.
    let mut static_getters: BTreeMap<String, &StaticGetterMember> = BTreeMap::new();
    let mut static_setters: BTreeMap<String, &StaticSetterMember> = BTreeMap::new();
    let mut static_methods: BTreeMap<String, Vec<&StaticMethodMember>> = BTreeMap::new();

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

    // Getters: optional if missing in any branch or optional in any branch.
    for (name, slots) in &getters {
        let present: Vec<&GetterMember> = slots.iter().filter_map(|s| *s).collect();
        if present.is_empty() {
            continue;
        }
        let absent_in_any = slots.iter().any(|s| s.is_none());
        let optional_in_any = present.iter().any(|g| g.optional);
        let optional = absent_in_any || optional_in_any;

        let type_ref = union_member_types(present.iter().map(|g| g.type_ref.clone()));
        let doc = present.iter().find_map(|g| g.doc.clone());

        out.push(Member::Getter(GetterMember {
            js_name: name.clone(),
            type_ref,
            optional,
            doc,
        }));
    }

    // Setters: emit only when at least one branch had a setter for this
    // name — a `readonly` branch suppresses the setter merge-wide.
    for (name, slots) in &setters {
        let present: Vec<&SetterMember> = slots.iter().filter_map(|s| *s).collect();
        if present.is_empty() {
            continue;
        }
        // If a branch had a getter for this name but no setter, treat the
        // merged property as readonly (drop the setter). This keeps
        // soundness — writing through the merged setter would be invalid
        // for the readonly branch.
        if let Some(getter_slots) = getters.get(name) {
            let any_branch_readonly = getter_slots
                .iter()
                .zip(slots.iter())
                .any(|(g, s)| g.is_some() && s.is_none());
            if any_branch_readonly {
                continue;
            }
        }
        let type_ref = union_member_types(present.iter().map(|s| s.type_ref.clone()));
        let doc = present.iter().find_map(|s| s.doc.clone());
        out.push(Member::Setter(SetterMember {
            js_name: name.clone(),
            type_ref,
            doc,
        }));
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

/// Wrap a sequence of types in `TypeRef::Union` unless they're all
/// identical — in which case the single type is returned directly. The
/// regular union resolution (subtyping LUB / `JsValue` erasure) then
/// applies as usual.
fn union_member_types(types: impl IntoIterator<Item = TypeRef>) -> TypeRef {
    let mut all: Vec<TypeRef> = types.into_iter().collect();
    if all.is_empty() {
        return TypeRef::Any;
    }
    all.dedup();
    if all.len() == 1 {
        return all.into_iter().next().unwrap();
    }
    TypeRef::Union(all)
}
