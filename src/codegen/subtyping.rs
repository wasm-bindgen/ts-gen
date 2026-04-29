//! Subtyping lattice — walk parent chains for built-in JS/TS types and
//! user-declared `class`/`interface` types, and compute the most-specific
//! common ancestor (LUB) of a set of types.
//!
//! This is what powers union simplification (`T1 | T2 | …` → common
//! ancestor) including the special case of `@throws {T1 | T2 | …}` whose
//! LUB becomes the `Result<_, ErrTy>` error type. There is nothing
//! error-specific here; errors just happen to be the most common case
//! where multiple union members share a meaningful supertype.
//!
//! # Builtin coverage
//!
//! The static [`BUILTIN_PARENTS`] table captures inheritance for the
//! JS builtins ts-gen knows about — error types, collections, typed
//! arrays, DOM exceptions. Anything not listed here falls back to user
//! `extends` lookup, then to "no known parent" → resolves to `Object`.
//!
//! # User type coverage
//!
//! User `class`/`interface` declarations carry `extends` directly in the
//! IR; [`user_parents`] walks that via the codegen scope chain. Multi-
//! interface `extends` (TypeScript allows interfaces to extend several
//! other interfaces) takes the first parent — that's a simplification, but
//! a multi-parent LUB on the user side is rare in `.d.ts` files and would
//! complicate the algorithm without much benefit.

use crate::codegen::typemap::CodegenContext;
use crate::ir::{TypeKind, TypeRef};
use crate::parse::scope::ScopeId;

/// Built-in JS/TS supertype chains, deepest immediate parent first, ending
/// at `Object`. The chain implicitly extends to `Object` even when not
/// listed (the LUB algorithm treats anything reachable from `Object` as
/// having `Object` as a final fallback).
///
/// Sources: ECMA-262 Object hierarchy and `lib.dom.d.ts`.
const BUILTIN_PARENTS: &[(&str, &[&str])] = &[
    // JS error hierarchy — every named error subclass extends `Error`.
    ("TypeError", &["Error"]),
    ("RangeError", &["Error"]),
    ("SyntaxError", &["Error"]),
    ("ReferenceError", &["Error"]),
    ("EvalError", &["Error"]),
    ("URIError", &["Error"]),
    ("AggregateError", &["Error"]),
    // DOM exceptions inherit from `Error` per WebIDL spec.
    ("DOMException", &["Error"]),
    // Typed arrays all inherit from a hidden `TypedArray` base in the spec,
    // exposed as `Object`-extending in JS. Keeping them parallel siblings
    // here matches what consumers can actually do with the LUB.
    ("Int8Array", &["Object"]),
    ("Uint8Array", &["Object"]),
    ("Uint8ClampedArray", &["Object"]),
    ("Int16Array", &["Object"]),
    ("Uint16Array", &["Object"]),
    ("Int32Array", &["Object"]),
    ("Uint32Array", &["Object"]),
    ("Float32Array", &["Object"]),
    ("Float64Array", &["Object"]),
    ("BigInt64Array", &["Object"]),
    ("BigUint64Array", &["Object"]),
    // Collections.
    ("Array", &["Object"]),
    ("Map", &["Object"]),
    ("Set", &["Object"]),
    ("WeakMap", &["Object"]),
    ("WeakSet", &["Object"]),
    // Misc.
    ("Date", &["Object"]),
    ("RegExp", &["Object"]),
    ("Promise", &["Object"]),
    ("Error", &["Object"]),
];

/// Look up the immediate parent chain for a builtin type. Returns `&[]` for
/// unknown names — callers should then try [`user_parents`] before giving up.
pub fn builtin_parents(name: &str) -> &'static [&'static str] {
    for (n, parents) in BUILTIN_PARENTS {
        if *n == name {
            return parents;
        }
    }
    &[]
}

/// Walk the user-declared parent chain for a `Named` type via the scope.
/// Returns the immediate parent name(s) — typically zero or one for classes,
/// possibly several for interfaces (which can `extends` multiple).
fn user_parents(name: &str, ctx: &CodegenContext<'_>, scope: ScopeId) -> Vec<String> {
    let Some(type_id) = ctx.gctx.scopes.resolve(scope, name) else {
        return Vec::new();
    };
    let decl = ctx.gctx.get_type(type_id);
    match &decl.kind {
        TypeKind::Class(c) => match &c.extends {
            Some(parent) => parent_typeref_to_names(parent),
            None => Vec::new(),
        },
        TypeKind::Interface(i) => i.extends.iter().flat_map(parent_typeref_to_names).collect(),
        _ => Vec::new(),
    }
}

/// Pull a name out of an `extends` `TypeRef`, ignoring complex shapes
/// (generic instantiations, unions, etc.) that don't have a single
/// representative supertype name.
fn parent_typeref_to_names(ty: &TypeRef) -> Vec<String> {
    match ty {
        TypeRef::Named(n) => vec![n.clone()],
        _ => Vec::new(),
    }
}

/// Build the full ancestor chain for a type — starting at the type itself
/// and walking up via builtin → user lookup until reaching `Object` (or a
/// dead end). Order: self, immediate parent, …, `Object`. Cycles are
/// guarded against by tracking visited names.
pub fn ancestor_chain(name: &str, ctx: &CodegenContext<'_>, scope: ScopeId) -> Vec<String> {
    use std::collections::HashSet;

    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut current = name.to_string();

    while seen.insert(current.clone()) {
        chain.push(current.clone());

        // Try builtin first, fall through to user.
        let parents = builtin_parents(&current);
        if let Some(first) = parents.first() {
            current = first.to_string();
            continue;
        }

        let user = user_parents(&current, ctx, scope);
        if let Some(first) = user.into_iter().next() {
            current = first;
            continue;
        }

        // No more parents known — stop unless we haven't reached Object.
        if chain.last().is_some_and(|n| n != "Object") {
            chain.push("Object".to_string());
        }
        break;
    }
    chain
}

/// Compute the most-specific common ancestor of `names`.
///
/// Returns `None` for an empty input or when no common ancestor exists more
/// specific than `Object` (callers typically substitute `JsValue` in that
/// case — see `to_syn_type` for `TypeRef::Union`).
///
/// Returns `Some("Object")` only when `Object` itself is the deepest shared
/// ancestor; consumers may decide whether that's useful or whether to widen
/// to `JsValue` for the same reasons unions usually erase.
pub fn lub_types(names: &[&str], ctx: &CodegenContext<'_>, scope: ScopeId) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    if names.len() == 1 {
        return Some(names[0].to_string());
    }

    // Build each type's ancestor chain (deepest first), then find the first
    // ancestor that appears in all of them. Using a Vec for the first chain
    // and HashSets for subsequent ones gives O(n·k) where n = #types and
    // k = average chain depth — both small in practice.
    let chains: Vec<Vec<String>> = names
        .iter()
        .map(|n| ancestor_chain(n, ctx, scope))
        .collect();

    let first_chain = &chains[0];
    for ancestor in first_chain {
        if chains[1..].iter().all(|c| c.iter().any(|n| n == ancestor)) {
            return Some(ancestor.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::typemap::CodegenContext;
    use crate::context::GlobalContext;

    fn ctx() -> (GlobalContext, ScopeId) {
        let mut gctx = GlobalContext::new();
        let scope = gctx.create_root_scope();
        (gctx, scope)
    }

    #[test]
    fn builtin_chain_for_type_error() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let chain = ancestor_chain("TypeError", &cgctx, scope);
        assert_eq!(chain, vec!["TypeError", "Error", "Object"]);
    }

    #[test]
    fn builtin_chain_for_dom_exception() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let chain = ancestor_chain("DOMException", &cgctx, scope);
        assert_eq!(chain, vec!["DOMException", "Error", "Object"]);
    }

    #[test]
    fn unknown_name_falls_to_object() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let chain = ancestor_chain("ImagesError", &cgctx, scope);
        assert_eq!(chain, vec!["ImagesError", "Object"]);
    }

    #[test]
    fn lub_two_error_subclasses_is_error() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let lub = lub_types(&["TypeError", "RangeError"], &cgctx, scope);
        assert_eq!(lub, Some("Error".to_string()));
    }

    #[test]
    fn lub_three_error_subclasses_is_error() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let lub = lub_types(&["TypeError", "RangeError", "DOMException"], &cgctx, scope);
        assert_eq!(lub, Some("Error".to_string()));
    }

    #[test]
    fn lub_error_with_unknown_user_type_is_object() {
        // The unknown user type's chain only reaches `Object`, so the LUB
        // collapses to `Object`. Codegen will substitute `JsValue` for that.
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let lub = lub_types(&["TypeError", "ImagesError"], &cgctx, scope);
        assert_eq!(lub, Some("Object".to_string()));
    }

    #[test]
    fn lub_array_with_typed_array_is_object() {
        // Both extend Object directly with no closer common ancestor.
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let lub = lub_types(&["Array", "Uint8Array"], &cgctx, scope);
        assert_eq!(lub, Some("Object".to_string()));
    }

    #[test]
    fn lub_single_name_returns_self() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let lub = lub_types(&["TypeError"], &cgctx, scope);
        assert_eq!(lub, Some("TypeError".to_string()));
    }

    #[test]
    fn lub_empty_returns_none() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let lub = lub_types(&[], &cgctx, scope);
        assert!(lub.is_none());
    }

    #[test]
    fn lub_same_name_returns_that_name() {
        let (gctx, scope) = ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let lub = lub_types(&["TypeError", "TypeError"], &cgctx, scope);
        assert_eq!(lub, Some("TypeError".to_string()));
    }
}
