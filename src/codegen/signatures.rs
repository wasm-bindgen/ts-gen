//! Signature expansion: expand TypeScript overloads (with optional/variadic/union
//! params) into multiple concrete Rust parameter lists, each tagged with a
//! disambiguating name suffix.
//!
//! This module is focused exclusively on the **parameter axis** — what
//! variants does a single JS callable produce based on its overloads, optionals,
//! and union-typed params? Decisions about `catch` / `try_` / `async fn` /
//! return types belong in the per-callable emitters (constructors, methods,
//! free functions, static methods) — those are the parts that vary by what
//! kind of callable we're emitting and what the source IR member tells us
//! about errors and async-ness.
//!
//! # Algorithm
//!
//! 1. **Per-overload expansion**: For each overload, generate optional truncation
//!    variants and cartesian-product union type alternatives.
//! 2. **Cross-overload dedup**: Remove expanded signatures with identical concrete
//!    param lists (e.g. two overloads both truncate to `(callback)`).
//! 3. **Suffix**: Compute `_with_X` / `_with_X_and_Y` suffixes across all surviving
//!    signatures as one cohort.
//!
//! # Expansion Rules
//!
//! Given `f(a, b?, c?)`:
//! - `f(a)`         — base signature, suffix `""`
//! - `f(a, b)`      — first optional included, suffix `"_with_b"`
//! - `f(a, b, c)`   — all params included, suffix `"_with_b_and_c"`
//!
//! # Variadic
//!
//! A variadic param uses `#[wasm_bindgen(variadic)]` with a `&[JsValue]` type.
//! Variadic params participate in `_with_`/`_and_` suffix computation — if a
//! signature differs from its siblings only by having a trailing variadic param,
//! the param name is used as a suffix (e.g. `_with_args`).
//!
//! # `try_` and `async` are not handled here
//!
//! The decision to emit `name` + `try_name` (sync, fallible companion) or just a
//! single `async fn name() -> Result<T, E>` (Promise-returning methods) is per-
//! callable and depends on return-type analysis. See `emit_callable_variants`
//! in the parent `codegen` module for that layer.

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::typemap::{self, CodegenContext, TypePosition};
use crate::ir::{Param, TypeRef};
use crate::parse::scope::ScopeId;
use crate::util::naming::to_snake_case;

/// What kind of callable an emitter is producing — drives default-naming
/// (`new`, `set_x`, snake-case, …) and which `wasm_bindgen` attributes to
/// stamp on the binding. Setters and constructors don't generate `try_`
/// companions or `async fn` forms.
///
/// `expand_signatures` itself is kind-agnostic: it only does parameter-axis
/// work. This enum is the contract between [`base_rust_name`] and the
/// per-callable emitters in `classes.rs` / `functions.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureKind {
    Constructor,
    Method,
    StaticMethod,
    Function,
    Setter,
    StaticSetter,
}

impl SignatureKind {
    /// Whether this kind can produce a `try_<name>` fallible companion.
    pub fn allows_try_variant(self) -> bool {
        !matches!(self, Self::Constructor | Self::Setter | Self::StaticSetter)
    }

    /// Whether this kind can collapse a `Promise<T>` return into `async fn`.
    pub fn allows_async(self) -> bool {
        !matches!(self, Self::Constructor | Self::Setter | Self::StaticSetter)
    }

    /// Whether the binding always requires `catch` (constructors do).
    pub fn always_catches(self) -> bool {
        matches!(self, Self::Constructor)
    }
}

/// Compute the default Rust name for a callable's primary variant before
/// any `_with_X` suffix is appended.
pub fn base_rust_name(js_name: &str, kind: SignatureKind) -> String {
    match kind {
        SignatureKind::Constructor => "new".to_string(),
        SignatureKind::Setter | SignatureKind::StaticSetter => {
            format!("set_{}", to_snake_case(js_name))
        }
        _ => to_snake_case(js_name),
    }
}

/// A single concrete parameter in an expanded signature.
#[derive(Clone, Debug, PartialEq)]
pub struct ConcreteParam {
    pub name: String,
    pub type_ref: TypeRef,
    /// Whether this param is variadic (only the last param can be).
    pub variadic: bool,
}

/// One concrete parameter-list variant produced by overload expansion,
/// paired with a suffix that disambiguates it from its siblings in Rust.
///
/// The caller adds the suffix to a base name of its choosing (`new`,
/// `set_X`, `to_snake_case(js_name)`, `try_<name>`, etc.) and dedups
/// against the surrounding extern block's used-name set.
#[derive(Clone, Debug)]
pub struct ExpandedSignature {
    /// `_with_X` / `_with_X_and_Y` suffix — empty for the primary
    /// (most-truncated) variant when only one signature survives, or when
    /// the variant is the shortest in its cohort.
    pub name_suffix: String,
    /// Concrete params (no optional flags — those have been resolved by
    /// truncation; no union types — those have been cartesian-expanded).
    pub params: Vec<ConcreteParam>,
}

/// A fully-resolved emitter input: an [`ExpandedSignature`] augmented with
/// the per-emit decisions (final Rust name, `catch`, `async`, return type,
/// doc, error type) that the per-callable layer made.
///
/// This is what concrete emitters (`generate_method`, etc.) consume. Build
/// via [`build_signatures`].
#[derive(Clone, Debug)]
pub struct FunctionSignature {
    /// Final Rust function name — already deduped against the extern block's
    /// used-names set.
    pub rust_name: String,
    /// JS function name, copied from the source for `js_name = "..."`.
    pub js_name: String,
    pub params: Vec<ConcreteParam>,
    /// Wrap return in `Result<T, ErrTy>`.
    pub catch: bool,
    /// Emit as `async fn`. The `return_type` here carries the *unwrapped*
    /// `T` — wasm-bindgen rewraps it as `Promise<T>` in the JS shim.
    pub is_async: bool,
    /// Return type (already Promise-unwrapped when `is_async`).
    pub return_type: TypeRef,
    /// Custom error type for the `Result` wrapper. `None` falls back to
    /// `JsValue` in [`to_return_type`].
    pub error_type: Option<TypeRef>,
    /// Doc comment — copied to every signature in the cohort.
    pub doc: Option<String>,
}

/// Assign a unique name within the extern block.
///
/// If `candidate` is already taken, appends `_1`, `_2`, etc. until a unique
/// name is found. The chosen name is inserted into `used_names`.
pub fn dedupe_name(candidate: &str, used_names: &mut HashSet<String>) -> String {
    let mut name = candidate.to_string();
    if !used_names.contains(&name) {
        used_names.insert(name.clone());
        return name;
    }
    let base = name.clone();
    let mut counter = 1u32;
    loop {
        name = format!("{base}_{counter}");
        if !used_names.contains(&name) {
            used_names.insert(name.clone());
            return name;
        }
        counter += 1;
    }
}

/// Expand all overloads of a single JS callable into concrete Rust parameter
/// variants, each tagged with a disambiguating suffix.
///
/// 1. **Per-overload expansion**: optional truncation + union cartesian product.
/// 2. **Cross-overload dedup**: drop expansions with identical concrete params.
/// 3. **Suffix**: assign `_with_X` / `_with_X_and_Y` suffixes across the cohort.
///
/// The caller is responsible for choosing a base name, applying the suffix,
/// and deduping the final names against the surrounding extern block.
pub fn expand_signatures(
    overloads: &[&[Param]],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> Vec<ExpandedSignature> {
    // Phase 1: Per-overload expansion — optional truncation + union cartesian product.
    let mut all_sigs: Vec<Vec<ConcreteParam>> = Vec::new();
    for params in overloads {
        all_sigs.extend(expand_single_overload(params, cgctx, scope));
    }

    // Phase 2: Cross-overload dedup — remove identical expanded signatures.
    let mut seen: Vec<&Vec<ConcreteParam>> = Vec::new();
    let mut deduped: Vec<Vec<ConcreteParam>> = Vec::new();
    for sig in &all_sigs {
        if !seen.iter().any(|s| concrete_params_eq(s, sig)) {
            seen.push(sig);
            deduped.push(sig.clone());
        }
    }

    // Phase 3: Suffix — compute `_with_X` disambiguators across the cohort.
    compute_suffixes(&deduped)
        .into_iter()
        .zip(deduped)
        .map(|(name_suffix, params)| ExpandedSignature {
            name_suffix,
            params,
        })
        .collect()
}

/// Per-callable input describing a JS callable that needs to be emitted as
/// one or more wasm-bindgen extern signatures. Pairs with [`build_signatures`].
///
/// All decisions about `try_` companions, `async fn` collapse, and `catch`
/// fall out of the kind + return type + error type once these inputs are
/// available.
#[derive(Clone, Debug)]
pub struct CallableSpec<'a> {
    pub js_name: &'a str,
    pub kind: SignatureKind,
    /// All overloads sharing this `js_name` — typically one element, but TS
    /// allows multiple overloaded declarations.
    pub overloads: &'a [&'a [Param]],
    pub return_type: &'a TypeRef,
    /// Optional error type from `@throws` — `None` means use the default
    /// `JsValue` for any catching variant.
    pub error_type: Option<&'a TypeRef>,
    /// Doc comment to attach to the primary signature only.
    pub doc: &'a Option<String>,
}

/// Resolve a [`CallableSpec`] into the full set of [`FunctionSignature`]s
/// that should be emitted in the surrounding extern block.
///
/// Decisions made here:
///
/// * **Promise → async**: when [`SignatureKind::allows_async`] and the return
///   type is `Promise<T>`, collapse to a single `async fn` with `catch: true`,
///   carrying the unwrapped `T` as `return_type`. No `try_` companion is
///   emitted in this case (`async + catch` is already the fallible form).
/// * **`try_` companion**: for sync, non-setter, non-constructor callables,
///   each primary `name` gets a fallible `try_name` sibling.
/// * **Constructor catches always**: per JS semantics; the primary signature
///   is `catch: true` and there's no `try_` variant.
/// * **Naming**: each surviving expansion gets `base + suffix` deduped against
///   `used_names`; `try_` variants prepend `try_` and dedup again.
pub fn build_signatures(
    spec: &CallableSpec<'_>,
    used_names: &mut HashSet<String>,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> Vec<FunctionSignature> {
    let base = base_rust_name(spec.js_name, spec.kind);
    let expansions = expand_signatures(spec.overloads, cgctx, scope);

    // Promise unwrap is per-callable, not per-overload — every expansion of a
    // given JS callable shares the same return type.
    let (is_async, return_type) = match (spec.return_type, spec.kind.allows_async()) {
        (TypeRef::Promise(inner), true) => (true, inner.as_ref().clone()),
        (other, _) => (false, other.clone()),
    };

    let allow_try = !is_async && spec.kind.allows_try_variant();
    let mut out = Vec::with_capacity(expansions.len() * if allow_try { 2 } else { 1 });

    for exp in expansions {
        let primary_candidate = format!("{base}{}", exp.name_suffix);
        let primary_name = dedupe_name(&primary_candidate, used_names);

        // Primary variant: catches if async (the catch encodes async failure)
        // or if the kind always catches (constructors).
        let primary_catches = is_async || spec.kind.always_catches();
        out.push(FunctionSignature {
            rust_name: primary_name.clone(),
            js_name: spec.js_name.to_string(),
            params: exp.params.clone(),
            catch: primary_catches,
            is_async,
            return_type: return_type.clone(),
            error_type: if primary_catches {
                spec.error_type.cloned()
            } else {
                None
            },
            doc: spec.doc.clone(),
        });

        if allow_try {
            let try_name = dedupe_name(&format!("try_{primary_name}"), used_names);
            out.push(FunctionSignature {
                rust_name: try_name,
                js_name: spec.js_name.to_string(),
                params: exp.params,
                catch: true,
                is_async: false,
                return_type: return_type.clone(),
                error_type: spec.error_type.cloned(),
                doc: spec.doc.clone(),
            });
        }
    }

    out
}

/// Compute `_with_X` / `_with_X_and_Y` suffixes across a cohort of expanded
/// signatures. Returns one suffix string per input. The shortest-arity
/// variant (or the only one if there's a single signature) gets `""`.
///
/// Wraps the existing [`compute_rust_names`] helper, then extracts the
/// suffixes by stripping a shared base prefix. We compute against a
/// non-empty placeholder base because `compute_rust_names` was originally
/// designed to produce full names.
/// Public alias of [`compute_suffixes`] for callers in sibling modules
/// that build their own concrete signature lists.
pub fn compute_suffixes_pub(expansions: &[Vec<ConcreteParam>]) -> Vec<String> {
    compute_suffixes(expansions)
}

fn compute_suffixes(expansions: &[Vec<ConcreteParam>]) -> Vec<String> {
    // Use a placeholder base — `BASE` is just a prefix that won't appear in
    // any param name, so we can safely strip it back off.
    const BASE: &str = "BASE";
    compute_rust_names(BASE, expansions)
        .into_iter()
        .map(|name| {
            // `compute_rust_names` returns either `BASE` (primary, no suffix)
            // or `BASE_with_X[_and_Y]` (variant). Strip the base; what's
            // left including the leading `_` is the suffix.
            name.strip_prefix(BASE).unwrap_or(&name).to_string()
        })
        .collect()
}

/// Check if two expanded concrete param lists are identical.
fn concrete_params_eq(a: &[ConcreteParam], b: &[ConcreteParam]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(pa, pb)| {
            pa.name == pb.name && pa.type_ref == pb.type_ref && pa.variadic == pb.variadic
        })
}

/// Expand a single overload's params into all concrete signature variants.
///
/// Handles optional truncation, union flattening (cartesian product), and
/// variadic param appending.
fn expand_single_overload(
    params: &[Param],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> Vec<Vec<ConcreteParam>> {
    // Separate trailing variadic param.
    let (non_variadic, variadic_param) = if params.last().is_some_and(|p| p.variadic) {
        (&params[..params.len() - 1], Some(&params[params.len() - 1]))
    } else {
        (params, None)
    };

    // Build expanded signatures via cartesian product.
    // Start with one empty signature, iterate params left to right.
    let mut sigs: Vec<Vec<ConcreteParam>> = vec![vec![]];

    for (i, param) in non_variadic.iter().enumerate() {
        let type_alternatives = flatten_type(&param.type_ref, cgctx, scope);

        if param.optional {
            // Only extend sigs that are "full" up to this point (len == i).
            // Shorter sigs are from earlier optional truncation — they stay frozen.
            let frozen: Vec<Vec<ConcreteParam>> =
                sigs.iter().filter(|s| s.len() < i).cloned().collect();
            let mut extendable: Vec<Vec<ConcreteParam>> =
                sigs.into_iter().filter(|s| s.len() == i).collect();
            let snapshot = extendable.clone(); // before extension (absent variants)

            let cur = extendable.len();
            for (j, alt) in type_alternatives.into_iter().enumerate() {
                let concrete = ConcreteParam {
                    name: param.name.clone(),
                    type_ref: alt,
                    variadic: false,
                };
                if j == 0 {
                    for sig in extendable.iter_mut().take(cur) {
                        sig.push(concrete.clone());
                    }
                } else {
                    for item in snapshot.iter().take(cur) {
                        let mut sig = item.clone();
                        sig.push(concrete.clone());
                        extendable.push(sig);
                    }
                }
            }

            // Reassemble: frozen + absent (snapshot) + extended
            sigs = frozen;
            sigs.extend(snapshot);
            sigs.extend(extendable);
        } else {
            // Required param: flatten and multiply.
            let cur = sigs.len();
            for (j, alt) in type_alternatives.into_iter().enumerate() {
                let concrete = ConcreteParam {
                    name: param.name.clone(),
                    type_ref: alt,
                    variadic: false,
                };
                if j == 0 {
                    for sig in sigs.iter_mut().take(cur) {
                        sig.push(concrete.clone());
                    }
                } else {
                    for k in 0..cur {
                        let mut sig = sigs[k].clone();
                        sig.truncate(i);
                        sig.push(concrete.clone());
                        sigs.push(sig);
                    }
                }
            }
        }
    }

    // Append variadic param to every signature.
    if let Some(vp) = variadic_param {
        for sig in &mut sigs {
            sig.push(ConcreteParam {
                name: vp.name.clone(),
                type_ref: vp.type_ref.clone(),
                variadic: true,
            });
        }
    }

    sigs
}

/// Recursively flatten a type into its concrete alternatives.
///
/// - `Union([A, B])` → flatten(A) ++ flatten(B)
/// - `Nullable(T)` → flatten(T) wrapped in Nullable
/// - `Named("Foo")` → resolve alias; if alias is a union, flatten it
/// - `Promise(T)` → for each flatten(T), wrap in Promise
/// - `Array(T)` → for each flatten(T), wrap in Array
/// - Everything else → single leaf
fn flatten_type(ty: &TypeRef, cgctx: Option<&CodegenContext<'_>>, scope: ScopeId) -> Vec<TypeRef> {
    match ty {
        // Unions fan out into each member, recursively
        TypeRef::Union(members) => members
            .iter()
            .flat_map(|m| flatten_type(m, cgctx, scope))
            .collect(),

        // Named types: resolve through aliases, then re-flatten
        TypeRef::Named(name) => {
            if let Some(c) = cgctx {
                if let Some(target) = c.resolve_alias(name, scope) {
                    let target = target.clone();
                    return flatten_type(&target, cgctx, scope);
                }
            }
            vec![ty.clone()]
        }

        // Nullable: flatten inner types unwrapped, then add a Null variant.
        // This expands `T | null` into separate overload variants for each T
        // plus an explicit `_with_null` variant, rather than wrapping every
        // alternative in `Option<T>`.
        TypeRef::Nullable(inner) => {
            let mut alts = flatten_type(inner, cgctx, scope);
            alts.push(TypeRef::Null);
            alts
        }

        // Generic containers: flatten inner, wrap each
        TypeRef::Promise(inner) => flatten_type(inner, cgctx, scope)
            .into_iter()
            .map(|t| TypeRef::Promise(Box::new(t)))
            .collect(),
        TypeRef::Array(inner) => flatten_type(inner, cgctx, scope)
            .into_iter()
            .map(|t| TypeRef::Array(Box::new(t)))
            .collect(),
        TypeRef::Set(inner) => flatten_type(inner, cgctx, scope)
            .into_iter()
            .map(|t| TypeRef::Set(Box::new(t)))
            .collect(),
        // Two-arg containers: cartesian product
        TypeRef::Record(k, v) => {
            let ks = flatten_type(k, cgctx, scope);
            let vs = flatten_type(v, cgctx, scope);
            let mut result = Vec::new();
            for k in &ks {
                for v in &vs {
                    result.push(TypeRef::Record(Box::new(k.clone()), Box::new(v.clone())));
                }
            }
            result
        }
        TypeRef::Map(k, v) => {
            let ks = flatten_type(k, cgctx, scope);
            let vs = flatten_type(v, cgctx, scope);
            let mut result = Vec::new();
            for k in &ks {
                for v in &vs {
                    result.push(TypeRef::Map(Box::new(k.clone()), Box::new(v.clone())));
                }
            }
            result
        }

        // Leaf types: no expansion
        _ => vec![ty.clone()],
    }
}

/// Compute candidate Rust names for a set of signatures sharing the same JS name.
///
/// Follows the wasm-bindgen webidl naming convention:
/// - The first signature gets the base name
/// - Other signatures get `_with_` / `_and_` suffixes
/// - When two sigs differ at a param position:
///   - If they have different param names (optional expansion / overload), use the param name
///   - If they have the same param name but different types (union expansion),
///     use the type's snake_case name
///
/// Variadic params participate in naming — if a signature has a trailing variadic
/// that others lack, the param name is used as a suffix.
///
/// These are candidate names — the caller runs them through `dedupe_name` for
/// final uniqueness within the extern block.
fn compute_rust_names(base_name: &str, signatures: &[Vec<ConcreteParam>]) -> Vec<String> {
    if signatures.len() == 1 {
        return vec![base_name.to_string()];
    }

    // Compute the number of params to trim from each end — params that are
    // identical across ALL signatures at the same offset don't disambiguate.
    //
    // This handles two cases that pure positional comparison misses:
    // - Variadic params anchored at the end (e.g. `(data)` vs `(label, data)`)
    // - Shared leading params (e.g. `(callback)` vs `(callback, msDelay)`)
    //
    // Only the "middle" params that differ contribute to naming suffixes.
    let (trim_start, trim_end) = compute_trim(signatures);

    let mut names = Vec::new();

    for (sig_idx, sig) in signatures.iter().enumerate() {
        // The first signature (shortest / most basic) gets the base name
        // without any suffix. This matches the convention that the most
        // common calling pattern uses the simplest name.
        if sig_idx == 0 {
            names.push(base_name.to_string());
            continue;
        }

        let mut name = base_name.to_string();
        let mut first_suffix = true;

        let end = if sig.len() >= trim_end {
            sig.len() - trim_end
        } else {
            // Signature is shorter than the shared suffix — don't trim
            sig.len()
        };
        let start = trim_start.min(end);

        for (param_idx, param) in sig[start..end].iter().enumerate() {
            let abs_idx = start + param_idx;

            // Check if this param position differs from any other signature
            // we need to disambiguate against (using original absolute indices).
            let mut any_different = false;
            let mut any_same_name_different_type = false;

            for (other_idx, other) in signatures.iter().enumerate() {
                if other_idx == sig_idx {
                    continue;
                }
                match other.get(abs_idx) {
                    Some(other_param) => {
                        if other_param.name == param.name && other_param.type_ref != param.type_ref
                        {
                            any_same_name_different_type = true;
                            any_different = true;
                        } else if other_param.name != param.name {
                            any_different = true;
                        }
                    }
                    None => {
                        // Other sig doesn't have this param
                        any_different = true;
                    }
                }
            }

            if !any_different {
                continue;
            }

            if first_suffix {
                name.push_str("_with_");
                first_suffix = false;
            } else {
                name.push_str("_and_");
            }

            if any_same_name_different_type {
                // Union expansion: use the type name
                name.push_str(&type_snake_name(&param.type_ref));
            } else {
                // Optional expansion or overload: use the param name
                name.push_str(&to_snake_case(&param.name));
            }
        }

        names.push(name);
    }

    names
}

/// Compute how many params to trim from the start and end of all signatures.
///
/// A param at offset `i` from the start is "shared" if ALL signatures have
/// length > i and the param at position `i` is identical (same name, type,
/// variadic) across all signatures. Similarly from the end.
fn compute_trim(signatures: &[Vec<ConcreteParam>]) -> (usize, usize) {
    let min_len = signatures.iter().map(|s| s.len()).min().unwrap_or(0);

    // Trim from start: count matching prefix across all signatures
    let mut trim_start = 0;
    for i in 0..min_len {
        let first = &signatures[0][i];
        if signatures[1..].iter().all(|sig| sig[i] == *first) {
            trim_start += 1;
        } else {
            break;
        }
    }

    // Trim from end: count matching suffix across all signatures
    let mut trim_end = 0;
    for i in 0..min_len {
        let first = &signatures[0][signatures[0].len() - 1 - i];
        if signatures[1..]
            .iter()
            .all(|sig| sig[sig.len() - 1 - i] == *first)
        {
            trim_end += 1;
        } else {
            break;
        }
    }

    // Don't let trims overlap
    if trim_start + trim_end > min_len {
        trim_end = min_len - trim_start;
    }

    (trim_start, trim_end)
}

/// Get a short snake_case name for a TypeRef, used in `_with_` suffixes.
fn type_snake_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::String => "str".to_string(),
        TypeRef::Number => "f64".to_string(),
        TypeRef::Boolean => "bool".to_string(),
        TypeRef::BigInt => "big_int".to_string(),
        TypeRef::Void | TypeRef::Undefined => "undefined".to_string(),
        TypeRef::Null => "null".to_string(),
        TypeRef::Any | TypeRef::Unknown => "js_value".to_string(),
        TypeRef::Object => "object".to_string(),
        TypeRef::Named(n) => to_snake_case(n),
        TypeRef::ArrayBuffer => "array_buffer".to_string(),
        TypeRef::Uint8Array => "uint8_array".to_string(),
        TypeRef::Int8Array => "int8_array".to_string(),
        TypeRef::Float32Array => "float32_array".to_string(),
        TypeRef::Float64Array => "float64_array".to_string(),
        TypeRef::Array(_) => "array".to_string(),
        TypeRef::Promise(_) => "promise".to_string(),
        TypeRef::Nullable(inner) => type_snake_name(inner),

        TypeRef::Function(_) => "function".to_string(),
        TypeRef::Date => "date".to_string(),
        TypeRef::RegExp => "reg_exp".to_string(),
        TypeRef::Error => "error".to_string(),
        TypeRef::Map(_, _) => "map".to_string(),
        TypeRef::Set(_) => "set".to_string(),
        TypeRef::Record(_, _) => "record".to_string(),
        _ => "js_value".to_string(),
    }
}

// ─── Shared codegen helpers ─────────────────────────────────────────

/// Convert concrete params to a `fn` parameter token stream.
///
/// Handles variadic params with `&[JsValue]`.
pub fn generate_concrete_params(
    params: &[ConcreteParam],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &crate::ir::ModuleContext,
) -> TokenStream {
    let items: Vec<_> = params
        .iter()
        .map(|p| {
            let name = typemap::make_ident(&p.name);
            let ty = if p.variadic {
                quote! { &[JsValue] }
            } else {
                typemap::to_syn_type(
                    &p.type_ref,
                    TypePosition::ARGUMENT,
                    cgctx,
                    scope,
                    from_module,
                )
            };
            quote! { #name: #ty }
        })
        .collect();

    quote! { #(#items),* }
}

/// Returns true if the return type is void (no return value in Rust).
pub fn is_void_return(ty: &TypeRef) -> bool {
    matches!(ty, TypeRef::Void | TypeRef::Undefined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::typemap::CodegenContext;
    use crate::context::GlobalContext;
    use crate::ir::TypeRef;

    fn no_used() -> HashSet<String> {
        HashSet::new()
    }

    /// Create a GlobalContext + scope + CodegenContext for tests.
    fn test_ctx() -> (GlobalContext, ScopeId) {
        let mut gctx = GlobalContext::new();
        let scope = gctx.create_root_scope();
        (gctx, scope)
    }

    /// Shorthand: build signatures for a single overload through the full
    /// per-callable pipeline (`build_signatures`).
    fn expand(
        js: &str,
        params: &[Param],
        ret: &TypeRef,
        kind: SignatureKind,
        doc: &Option<String>,
        used: &mut HashSet<String>,
    ) -> Vec<FunctionSignature> {
        let (gctx, scope) = test_ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        build_signatures(
            &CallableSpec {
                js_name: js,
                kind,
                overloads: &[params],
                return_type: ret,
                error_type: None,
                doc,
            },
            used,
            Some(&cgctx),
            scope,
        )
    }

    /// Shorthand: build signatures for multiple overloads.
    fn expand_overloads(
        js: &str,
        overloads: &[&[Param]],
        ret: &TypeRef,
        kind: SignatureKind,
        doc: &Option<String>,
        used: &mut HashSet<String>,
    ) -> Vec<FunctionSignature> {
        let (gctx, scope) = test_ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        build_signatures(
            &CallableSpec {
                js_name: js,
                kind,
                overloads,
                return_type: ret,
                error_type: None,
                doc,
            },
            used,
            Some(&cgctx),
            scope,
        )
    }

    fn param(name: &str) -> Param {
        Param {
            name: name.to_string(),
            type_ref: TypeRef::Any,
            optional: false,
            variadic: false,
        }
    }

    fn typed_param(name: &str, ty: TypeRef) -> Param {
        Param {
            name: name.to_string(),
            type_ref: ty,
            optional: false,
            variadic: false,
        }
    }

    fn opt_param(name: &str) -> Param {
        Param {
            name: name.to_string(),
            type_ref: TypeRef::Any,
            optional: true,
            variadic: false,
        }
    }

    fn opt_typed_param(name: &str, ty: TypeRef) -> Param {
        Param {
            name: name.to_string(),
            type_ref: ty,
            optional: true,
            variadic: false,
        }
    }

    fn variadic_param(name: &str) -> Param {
        Param {
            name: name.to_string(),
            type_ref: TypeRef::Any,
            optional: false,
            variadic: true,
        }
    }

    #[test]
    fn test_no_optional_params() {
        let mut used = no_used();
        let sigs = expand(
            "foo",
            &[param("a"), param("b")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        // Should produce 2: foo (no catch) + try_foo (catch)
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "foo");
        assert!(!sigs[0].catch);
        assert_eq!(sigs[0].params.len(), 2);
        assert_eq!(sigs[1].rust_name, "try_foo");
        assert!(sigs[1].catch);
    }

    #[test]
    fn test_constructor_no_try_variant() {
        let mut used = no_used();
        let sigs = expand(
            "Console",
            &[param("stdout")],
            &TypeRef::Named("Console".into()),
            SignatureKind::Constructor,
            &None,
            &mut used,
        );
        // Constructor: only 1 signature, always catch, no try_ variant
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].rust_name, "new");
        assert!(sigs[0].catch);
        assert!(!sigs[0].is_async);
    }

    #[test]
    fn test_promise_method_collapses_to_async() {
        // `foo(): Promise<string>` on a method should produce a single
        // `async fn foo() -> Result<string, JsValue>` signature — no `try_foo`
        // companion, since the async + catch form is already fallible.
        let mut used = no_used();
        let sigs = expand(
            "foo",
            &[],
            &TypeRef::Promise(Box::new(TypeRef::String)),
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 1, "promise method should not get a try_ pair");
        assert_eq!(sigs[0].rust_name, "foo");
        assert!(sigs[0].is_async, "promise method must be async");
        assert!(sigs[0].catch, "async promise method must catch");
        // Return type is unwrapped from Promise<T> — wasm-bindgen rewraps as a
        // Promise on the JS side automatically when `async fn` is used.
        assert_eq!(sigs[0].return_type, TypeRef::String);
    }

    #[test]
    fn test_promise_function_collapses_to_async() {
        let mut used = no_used();
        let sigs = expand(
            "fetch",
            &[param("url")],
            &TypeRef::Promise(Box::new(TypeRef::Named("Response".into()))),
            SignatureKind::Function,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].rust_name, "fetch");
        assert!(sigs[0].is_async);
        assert!(sigs[0].catch);
        assert_eq!(sigs[0].return_type, TypeRef::Named("Response".into()));
    }

    #[test]
    fn test_promise_static_method_collapses_to_async() {
        let mut used = no_used();
        let sigs = expand(
            "all",
            &[param("promises")],
            &TypeRef::Promise(Box::new(TypeRef::Any)),
            SignatureKind::StaticMethod,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 1);
        assert!(sigs[0].is_async);
        assert!(sigs[0].catch);
    }

    #[test]
    fn test_sync_method_keeps_try_pair() {
        // Regression guard — sync methods must still get the `name` + `try_name`
        // pair. The async collapse should only apply when the return type is
        // `Promise<T>`.
        let mut used = no_used();
        let sigs = expand(
            "compute",
            &[param("x")],
            &TypeRef::Number,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "compute");
        assert!(!sigs[0].is_async);
        assert!(!sigs[0].catch);
        assert_eq!(sigs[1].rust_name, "try_compute");
        assert!(!sigs[1].is_async);
        assert!(sigs[1].catch);
    }

    #[test]
    fn test_promise_overloads_each_collapse() {
        // Two overloads, both promise-returning. Each surviving expanded
        // signature should be a standalone async fn — no `try_` siblings, even
        // when overload disambiguation produces multiple bindings.
        let mut used = no_used();
        let sigs = expand_overloads(
            "send",
            &[
                &[typed_param(
                    "message",
                    TypeRef::Named("EmailMessage".into()),
                )],
                &[typed_param("builder", TypeRef::Object)],
            ],
            &TypeRef::Promise(Box::new(TypeRef::Named("EmailSendResult".into()))),
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2, "two overloads → two sigs, no try_ variants");
        for sig in &sigs {
            assert!(sig.is_async, "{} should be async", sig.rust_name);
            assert!(sig.catch, "{} should catch", sig.rust_name);
            assert!(
                !sig.rust_name.starts_with("try_"),
                "no try_ variant expected for promise overloads, got {}",
                sig.rust_name,
            );
        }
        // Names should disambiguate via the standard `_with_<param>` suffix.
        assert_eq!(sigs[0].rust_name, "send");
        assert_eq!(sigs[1].rust_name, "send_with_builder");
    }

    #[test]
    fn test_promise_setter_does_not_become_async() {
        // Setters are never async — even if (somehow) the prop type was a
        // Promise, the setter signature itself is sync `set_x(val)`.
        let mut used = no_used();
        let sigs = expand(
            "value",
            &[typed_param(
                "val",
                TypeRef::Promise(Box::new(TypeRef::String)),
            )],
            &TypeRef::Void,
            SignatureKind::Setter,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].rust_name, "set_value");
        assert!(!sigs[0].is_async);
        // Setters never get `try_` either.
    }

    #[test]
    fn test_optional_expansion() {
        let mut used = no_used();
        let sigs = expand(
            "Console",
            &[
                param("stdout"),
                opt_param("stderr"),
                opt_param("ignoreErrors"),
            ],
            &TypeRef::Named("Console".into()),
            SignatureKind::Constructor,
            &None,
            &mut used,
        );
        // 3 constructor signatures (no try_ variants)
        assert_eq!(sigs.len(), 3);
        assert_eq!(sigs[0].rust_name, "new");
        assert_eq!(sigs[0].params.len(), 1);
        assert_eq!(sigs[1].rust_name, "new_with_stderr");
        assert_eq!(sigs[1].params.len(), 2);
        assert_eq!(sigs[2].rust_name, "new_with_stderr_and_ignore_errors");
        assert_eq!(sigs[2].params.len(), 3);
    }

    #[test]
    fn test_optional_method_expansion() {
        let mut used = no_used();
        let sigs = expand(
            "count",
            &[opt_param("label")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        // 2 expansions × 2 (normal + try_) = 4
        assert_eq!(sigs.len(), 4);
        assert_eq!(sigs[0].rust_name, "count");
        assert_eq!(sigs[0].params.len(), 0);
        assert!(!sigs[0].catch);
        assert_eq!(sigs[1].rust_name, "try_count");
        assert!(sigs[1].catch);
        assert_eq!(sigs[2].rust_name, "count_with_label");
        assert_eq!(sigs[2].params.len(), 1);
        assert_eq!(sigs[3].rust_name, "try_count_with_label");
    }

    #[test]
    fn test_variadic_param() {
        let mut used = no_used();
        let sigs = expand(
            "log",
            &[variadic_param("data")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        // Variadic is always present — 1 signature × 2 (normal + try_) = 2
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "log");
        assert_eq!(sigs[0].params.len(), 1);
        assert!(sigs[0].params[0].variadic);
        assert_eq!(sigs[1].rust_name, "try_log");
    }

    #[test]
    fn test_optional_then_variadic() {
        let mut used = no_used();
        let sigs = expand(
            "timeLog",
            &[opt_param("label"), variadic_param("data")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        // Variadic always present. Optional label creates 2 truncation points.
        // 2 expansions × 2 (normal + try_) = 4
        assert_eq!(sigs.len(), 4);
        assert_eq!(sigs[0].rust_name, "time_log");
        assert_eq!(sigs[0].params.len(), 1); // just variadic data
        assert!(sigs[0].params[0].variadic);
        assert_eq!(sigs[1].rust_name, "try_time_log");
        // Variadic params participate in naming — data is present in both sigs,
        // but label differs, so suffix uses the label param name.
        assert_eq!(sigs[2].rust_name, "time_log_with_label");
        assert_eq!(sigs[2].params.len(), 2); // label + variadic data
        assert!(!sigs[2].params[0].variadic);
        assert!(sigs[2].params[1].variadic);
        assert_eq!(sigs[3].rust_name, "try_time_log_with_label");
    }

    #[test]
    fn test_doc_on_all_variants() {
        let doc = Some("Hello".to_string());
        let mut used = no_used();
        let sigs = expand(
            "count",
            &[opt_param("label")],
            &TypeRef::Void,
            SignatureKind::Method,
            &doc,
            &mut used,
        );
        assert_eq!(sigs[0].doc, Some("Hello".to_string()));
        assert_eq!(sigs[1].doc, Some("Hello".to_string())); // try_count
        assert_eq!(sigs[2].doc, Some("Hello".to_string())); // count_with_label
        assert_eq!(sigs[3].doc, Some("Hello".to_string())); // try_count_with_label
    }

    #[test]
    fn test_try_collision_deduped() {
        // If "try_count" is already taken, the try_ variant gets a numeric suffix.
        let mut used: HashSet<String> = ["try_count".to_string()].into_iter().collect();
        let sigs = expand(
            "count",
            &[param("x")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "count");
        assert!(!sigs[0].catch);
        assert_eq!(sigs[1].rust_name, "try_count_1");
        assert!(sigs[1].catch);
    }

    #[test]
    fn test_name_collision_deduped() {
        // Two separate expand calls with the same JS name — second gets numeric suffix.
        let mut used = no_used();
        let sigs1 = expand(
            "foo",
            &[param("a")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        let sigs2 = expand(
            "foo",
            &[param("a"), param("b")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs1[0].rust_name, "foo");
        assert_eq!(sigs2[0].rust_name, "foo_1");
    }

    #[test]
    fn test_overloads_with_variadic() {
        // setTimeout pattern:
        //   overload 1: (callback: Function, msDelay?: number)
        //   overload 2: (callback: Function, msDelay?: number, ...args: any[])
        let mut used = no_used();
        let overload1 = [
            typed_param("callback", TypeRef::Any),
            opt_typed_param("msDelay", TypeRef::Number),
        ];
        let overload2 = [
            typed_param("callback", TypeRef::Any),
            opt_typed_param("msDelay", TypeRef::Number),
            variadic_param("args"),
        ];
        let sigs = expand_overloads(
            "setTimeout",
            &[&overload1, &overload2],
            &TypeRef::Number,
            SignatureKind::Method,
            &None,
            &mut used,
        );

        // Expected (non-try_ only):
        //   set_timeout(callback)                        — from overload 1 truncation
        //   set_timeout_with_ms_delay(callback, msDelay) — from overload 1 full
        //   set_timeout_with_args(callback, args)        — from overload 2 truncation + variadic
        //   set_timeout_with_ms_delay_and_args(callback, msDelay, args) — from overload 2 full
        // Note: overload 2's truncated (callback) is deduped against overload 1's.
        let non_try: Vec<_> = sigs.iter().filter(|s| !s.catch).collect();
        assert_eq!(non_try.len(), 4);
        assert_eq!(non_try[0].rust_name, "set_timeout");
        assert_eq!(non_try[0].params.len(), 1);
        assert_eq!(non_try[1].rust_name, "set_timeout_with_ms_delay");
        assert_eq!(non_try[1].params.len(), 2);
        assert_eq!(non_try[2].rust_name, "set_timeout_with_args");
        assert_eq!(non_try[2].params.len(), 2);
        assert!(non_try[2].params[1].variadic);
        assert_eq!(non_try[3].rust_name, "set_timeout_with_ms_delay_and_args");
        assert_eq!(non_try[3].params.len(), 3);
        assert!(non_try[3].params[2].variadic);
    }

    #[test]
    fn test_overloads_with_different_types() {
        // foo(x: string) and foo(x: Promise<string>) should expand as
        // foo_with_str and foo_with_promise
        let mut used = no_used();
        let overload1 = [typed_param("x", TypeRef::String)];
        let overload2 = [typed_param(
            "x",
            TypeRef::Promise(Box::new(TypeRef::String)),
        )];
        let sigs = expand_overloads(
            "foo",
            &[&overload1, &overload2],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );

        let non_try: Vec<_> = sigs.iter().filter(|s| !s.catch).collect();
        assert_eq!(non_try.len(), 2);
        // First overload gets base name, second gets type suffix
        assert_eq!(non_try[0].rust_name, "foo");
        assert_eq!(non_try[1].rust_name, "foo_with_promise");
    }

    #[test]
    fn test_overloads_shared_truncation_deduped() {
        // Two overloads that share a truncation: both truncate to (a)
        //   overload 1: (a: any, b?: any)
        //   overload 2: (a: any, c?: any)
        let mut used = no_used();
        let overload1 = [param("a"), opt_param("b")];
        let overload2 = [param("a"), opt_param("c")];
        let sigs = expand_overloads(
            "foo",
            &[&overload1, &overload2],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );

        // Expected: foo(a), foo_with_b(a, b), foo_with_c(a, c)
        // The two (a) truncations are deduped.
        let non_try: Vec<_> = sigs.iter().filter(|s| !s.catch).collect();
        assert_eq!(non_try.len(), 3);
        assert_eq!(non_try[0].rust_name, "foo");
        assert_eq!(non_try[0].params.len(), 1);
        assert_eq!(non_try[1].rust_name, "foo_with_b");
        assert_eq!(non_try[1].params.len(), 2);
        assert_eq!(non_try[2].rust_name, "foo_with_c");
        assert_eq!(non_try[2].params.len(), 2);
    }
}
