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

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::primitive_unions;
use crate::codegen::typemap::{self, CodegenContext, TypePosition};
use crate::ir::{Param, SharedMemory, Throws, TypeRef};
use crate::parse::scope::ScopeId;
use crate::util::naming::{ident_chars, snake_ident, to_snake_case, LocalScope, Suffix};

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

    /// Whether the binding catches by default, without `--fallible`
    /// (constructors do; `@throws {never}` still opts out).
    pub fn always_catches(self) -> bool {
        matches!(self, Self::Constructor)
    }

    /// Whether overloads distinguished only by string literals become a
    /// literal-keyed cohort. A setter takes the value itself, so its
    /// literal arms stay one `&str` binding.
    pub fn allows_literal_cohort(self) -> bool {
        !matches!(self, Self::Setter | Self::StaticSetter)
    }
}

/// Compute the default Rust name for a callable's primary variant before
/// any `_with_X` suffix is appended.
///
/// Symbol-keyed methods (`js_name = "[Symbol.iterator]"`) drop the
/// `[Symbol.` prefix and trailing `]` so the Rust name reads
/// naturally — `[Symbol.iterator]` becomes `iterator`, not
/// `symboliterator`. The symbol nature is conveyed by `js_name`; the
/// Rust name doesn't need to repeat it.
pub fn base_rust_name(js_name: &str, kind: SignatureKind) -> String {
    let raw = js_name
        .strip_prefix("[Symbol.")
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(js_name);
    match kind {
        SignatureKind::Constructor => "new".to_string(),
        SignatureKind::Setter | SignatureKind::StaticSetter => {
            format!("set_{}", snake_ident(raw))
        }
        _ => snake_ident(raw),
    }
}

/// Convert a generated public function/method name into the Rust API spelling.
///
/// Raw identifiers are valid Rust, but public wasm-bindgen bindings follow the
/// web-sys convention of suffixing keywords instead: `type` -> `type_`, while
/// parameter names still go through `make_ident` and can use `r#type`.
pub fn public_rust_name(name: &str) -> String {
    if is_rust_keyword_or_path_name(name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

fn is_rust_keyword_or_path_name(name: &str) -> bool {
    matches!(
        name,
        "Self"
            | "abstract"
            | "as"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}

/// A single concrete parameter in an expanded signature.
#[derive(Clone, Debug, PartialEq)]
pub struct ConcreteParam {
    pub name: String,
    pub type_ref: TypeRef,
    /// Whether this param is variadic (only the last param can be).
    pub variadic: bool,
    /// `{@slice mode}` hint from the source [`crate::ir::Param`], applied
    /// where this alternative's lowering is slice-shaped (see
    /// [`crate::codegen::typemap::param_slice_mode`]).
    pub slice_mode: Option<crate::ir::SliceModeHint>,
}

/// One concrete parameter-list variant produced by overload expansion,
/// paired with a suffix that disambiguates it from its siblings in Rust.
///
/// The caller adds the suffix to a base name of its choosing (`new`,
/// `set_X`, `to_snake_case(js_name)`, `try_<name>`, etc.) and dedups
/// against the surrounding extern block's used-name set.
#[derive(Clone, Debug)]
pub struct ExpandedSignature {
    /// Index of the overload it expands.
    pub overload: usize,
    /// `_with_X` / `_with_X_and_Y` suffix — empty for the primary
    /// (most-truncated) variant when only one signature survives, or when
    /// the variant is the shortest in its cohort.
    pub name_suffix: String,
    /// Concrete params (no optional flags — those have been resolved by
    /// truncation; no union types — those have been cartesian-expanded).
    pub params: Vec<ConcreteParam>,
    /// Sibling expansions that differ from this one only in the value of
    /// string-literal parameters, this one included and first. Two or
    /// more arms make a literal-keyed cohort (see
    /// [`FunctionSignature::literal`]); the literal positions render as
    /// `&str` in `params`.
    pub literal_arms: Vec<LiteralArm>,
}

/// One value assignment for the string-literal parameters of an
/// expansion: `(position, literal)` pairs plus the overload it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct LiteralArm {
    pub values: Vec<(usize, String)>,
    pub overload: usize,
}

/// A literal-keyed cohort attached to an extern binding: the binding takes
/// `&str` at `positions`, and the emitter adds one inline wrapper per arm
/// that bakes the arm's literals in and carries the arm's own name.
#[derive(Clone, Debug, PartialEq)]
pub struct LiteralSpec {
    pub positions: Vec<usize>,
    /// `(literals in `positions` order, wrapper rust name)`.
    pub arms: Vec<(Vec<String>, String)>,
}

impl LiteralSpec {
    /// A derived form (`_promise`, `try_`, `_js_string`) names its wrappers
    /// off each arm's wrapper name, so the extern keeps pairing with its arms.
    fn derive(&self, names: &mut LocalScope, f: &dyn Fn(&str) -> String) -> LiteralSpec {
        LiteralSpec {
            positions: self.positions.clone(),
            arms: self
                .arms
                .iter()
                .map(|(lits, name)| (lits.clone(), names.declare(&f(name), Suffix::Snake)))
                .collect(),
        }
    }
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
    /// Lower eligible string leaves to `JsString` for the additive
    /// `_js_string` variant. Keeping this as metadata avoids representing a
    /// codegen-only choice as a user-resolvable [`TypeRef::Reference`].
    pub js_string_return: bool,
    /// Custom error type for the `Result` wrapper. `None` falls back to
    /// `JsValue` in [`to_return_type`].
    pub error_type: Option<TypeRef>,
    /// Doc comment — copied to every signature in the cohort.
    pub doc: Option<String>,
    /// Lexical body scope of the source-side method or function, used
    /// by codegen to resolve type-parameter references during emit.
    /// For non-generic callables this is just the enclosing scope.
    pub body_scope: ScopeId,
    /// Set when this binding stands for several overloads distinguished
    /// only by string-literal arguments; `rust_name` is then the first
    /// arm's wrapper name and the extern itself is emitted privately.
    pub literal: Option<LiteralSpec>,
}

/// Recursively collect every in-scope type-parameter name reachable
/// from `ty`, in source-appearance order, deduped.
///
/// A name in `ty` is treated as a type parameter when the scope chain
/// resolves it to [`crate::parse::scope::Binding::TypeParam`]. Used
/// by `classes.rs` / `functions.rs` to compute the `<T: JsGeneric, …>`
/// declaration for each emitted method or type.
pub fn collect_type_params(
    ty: &TypeRef,
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    out: &mut Vec<String>,
) {
    use crate::parse::scope::Binding;
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            // Bare single-segment refs may resolve to a type parameter.
            if segments.len() == 1 && generic_args.is_empty() {
                let name = &segments[0];
                if matches!(
                    cgctx.gctx.scopes.resolve_binding(scope, name),
                    Some(Binding::TypeParam)
                ) {
                    if !out.iter().any(|n| n == name) {
                        out.push(name.clone());
                    }
                    return;
                }
            }
            // Walk into generic args even when the head is a real type
            // (e.g. `Iterator<T>` — `T` may be a type param).
            for a in generic_args {
                collect_type_params(a, cgctx, scope, out);
            }
        }
        TypeRef::Array(inner) | TypeRef::Nullable(inner, _) => {
            collect_type_params(inner, cgctx, scope, out);
        }
        TypeRef::Union(members) | TypeRef::Intersection(members) | TypeRef::Tuple(members) => {
            for m in members {
                collect_type_params(m, cgctx, scope, out);
            }
        }
        TypeRef::Function(sig) => {
            for p in &sig.params {
                collect_type_params(&p.type_ref, cgctx, scope, out);
            }
            collect_type_params(&sig.return_type, cgctx, scope, out);
        }
        // Leaves with no nested types — no-op.
        TypeRef::Boolean
        | TypeRef::Number
        | TypeRef::String
        | TypeRef::BigInt
        | TypeRef::Void
        | TypeRef::Undefined
        | TypeRef::Null
        | TypeRef::Any
        | TypeRef::Unknown
        | TypeRef::Object
        | TypeRef::Symbol
        | TypeRef::ArrayBufferView
        | TypeRef::PropertyKey
        | TypeRef::TypedSlice(..)
        | TypeRef::RustScalar(_)
        | TypeRef::WideScalar(_)
        | TypeRef::StringLiteral(_)
        | TypeRef::NumberLiteral(_)
        | TypeRef::BooleanLiteral(_)
        | TypeRef::Unresolved(_) => {}
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
///
/// `literal_cohorts` keeps literal-only duplicates as arms of the kept
/// signature (see [`ExpandedSignature::literal_arms`]); off, they are
/// dropped as plain duplicates.
pub fn expand_signatures(
    overloads: &[Overload<'_>],
    cgctx: Option<&CodegenContext<'_>>,
    literal_cohorts: bool,
) -> Vec<ExpandedSignature> {
    // Phase 1: Per-overload expansion — optional truncation + union cartesian product.
    let mut all_sigs: Vec<(usize, Vec<ConcreteParam>)> = Vec::new();
    let mut first_overload_sigs = 0;
    for (index, overload) in overloads.iter().enumerate() {
        let sigs = expand_single_overload(overload.params, cgctx, overload.body_scope);
        if index == 0 {
            first_overload_sigs = sigs.len();
        }
        all_sigs.extend(sigs.into_iter().map(|sig| (index, sig)));
    }

    // Phase 2: Cross-overload dedup — remove signatures that are identical
    // structurally OR after lowering to Rust type tokens (so `"inline"` and
    // `"attachment"` literal arms both collapse to a single `&str` overload).
    let from_module = crate::ir::ModuleContext::Global;
    let render = |sig: &[ConcreteParam], scope: ScopeId| -> Vec<String> {
        sig.iter()
            .map(|p| {
                let ty = render_param_type(p, cgctx, scope, &from_module);
                format!("{}: {}", p.name, ty)
            })
            .collect()
    };

    let mut seen_concrete: Vec<&Vec<ConcreteParam>> = Vec::new();
    let mut seen_rendered: Vec<Vec<String>> = Vec::new();
    let mut deduped: Vec<Vec<ConcreteParam>> = Vec::new();
    let mut origins: Vec<usize> = Vec::new();
    // `None` marks a cohort that also absorbed a non-literal duplicate
    // (`"a" | "b" | string`): the plain form covers every arm, so it is
    // not literal-keyed.
    let mut arms: Vec<Option<Vec<LiteralArm>>> = Vec::new();
    // The first overload's expansions are never dropped as duplicates of a
    // later overload's, so its survivors are a contiguous prefix of `deduped`.
    let mut first_overload_len = 0;
    for (i, (overload, sig)) in all_sigs.iter().enumerate() {
        if seen_concrete.iter().any(|s| concrete_params_eq(s, sig)) {
            continue;
        }
        let rendered = render(sig, overloads[*overload].body_scope);
        if let Some(pos) = seen_rendered.iter().position(|r| *r == rendered) {
            // A duplicate that differs only in literal values is an arm of
            // the kept signature rather than a dropped one.
            match (literal_arm(&deduped[pos], sig, *overload), &mut arms[pos]) {
                (Some(arm), Some(arms)) => arms.push(arm),
                (None, slot) => *slot = None,
                _ => {}
            }
            continue;
        }
        seen_concrete.push(sig);
        seen_rendered.push(rendered);
        deduped.push(sig.clone());
        origins.push(*overload);
        arms.push(literal_cohorts.then(|| literal_arm(sig, sig, *overload).into_iter().collect()));
        if i < first_overload_sigs {
            first_overload_len += 1;
        }
    }
    let mut arms: Vec<Vec<LiteralArm>> = arms.into_iter().map(Option::unwrap_or_default).collect();

    // Phase 3: Suffix — compute `_with_X` disambiguators across the cohort.
    // The base name goes to the first overload's shortest expansion, or to
    // a shorter expansion that is a prefix of it: with
    // `get(key, options: Required)` declared before `get(key, options?)`,
    // `(key)` owns `get` rather than the two-arg form. A shorter expansion
    // with different leading params (`put(entries)` next to
    // `put(key, value)`) does not displace the first overload.
    if let Some(primary) = primary_expansion(&deduped, first_overload_len) {
        deduped[..=primary].rotate_right(1);
        origins[..=primary].rotate_right(1);
        arms[..=primary].rotate_right(1);
    }
    // A literal-keyed cohort is named as if the literal parameters were
    // not there: its wrappers take the remaining parameters, so the
    // suffix is the one those parameters would earn among the plain
    // expansions. The plain expansions are suffixed among themselves.
    let keyed: Vec<bool> = arms.iter().map(|a| a.len() > 1).collect();
    let plain: Vec<Vec<ConcreteParam>> = deduped
        .iter()
        .zip(&keyed)
        .filter(|(_, k)| !**k)
        .map(|(sig, _)| sig.clone())
        .collect();
    let mut plain_suffixes = compute_suffixes(&plain).into_iter();
    let suffixes: Vec<String> = deduped
        .iter()
        .zip(&keyed)
        .zip(&arms)
        .map(|((sig, keyed), arms)| {
            if !*keyed {
                return plain_suffixes.next().unwrap_or_default();
            }
            let positions: Vec<usize> = arms[0].values.iter().map(|(i, _)| *i).collect();
            let stripped: Vec<ConcreteParam> = sig
                .iter()
                .enumerate()
                .filter(|(i, _)| !positions.contains(i))
                .map(|(_, p)| p.clone())
                .collect();
            suffix_among(&plain, stripped)
        })
        .collect();
    suffixes
        .into_iter()
        .zip(deduped)
        .zip(origins)
        .zip(arms)
        .map(
            |(((name_suffix, params), overload), literal_arms)| ExpandedSignature {
                overload,
                name_suffix,
                params,
                literal_arms,
            },
        )
        .collect()
}

/// The suffix `extra` would get as a member of `cohort`: its sibling's
/// when a sibling has the same parameters, else what the naming pass
/// assigns it appended to the cohort.
fn suffix_among(cohort: &[Vec<ConcreteParam>], extra: Vec<ConcreteParam>) -> String {
    if let Some(pos) = cohort.iter().position(|s| concrete_params_eq(s, &extra)) {
        return compute_suffixes(cohort).swap_remove(pos);
    }
    let mut all = cohort.to_vec();
    all.push(extra);
    compute_suffixes(&all).pop().unwrap_or_default()
}

/// `candidate` as a literal arm of `kept`: both carry string literals at
/// the same positions and agree everywhere else. `None` when `kept` has
/// no literal parameter or the shapes differ.
fn literal_arm(
    kept: &[ConcreteParam],
    candidate: &[ConcreteParam],
    index: usize,
) -> Option<LiteralArm> {
    if kept.len() != candidate.len() {
        return None;
    }
    let mut values = Vec::new();
    for (i, (k, c)) in kept.iter().zip(candidate).enumerate() {
        match (&k.type_ref, &c.type_ref) {
            (TypeRef::StringLiteral(_), TypeRef::StringLiteral(lit)) if k.name == c.name => {
                values.push((i, lit.clone()));
            }
            _ if k.name == c.name && k.type_ref == c.type_ref && k.variadic == c.variadic => {}
            _ => return None,
        }
    }
    (!values.is_empty()).then_some(LiteralArm {
        values,
        overload: index,
    })
}

/// Index of the expansion that owns the unsuffixed name; see the Phase 3
/// comment in [`expand_signatures`]. The first `first_overload_len` entries
/// of `sigs` are the first overload's expansions. `None` when there is
/// nothing to move.
fn primary_expansion(sigs: &[Vec<ConcreteParam>], first_overload_len: usize) -> Option<usize> {
    let anchor = (0..first_overload_len).min_by_key(|&i| sigs[i].len())?;
    let is_prefix_of_anchor = |sig: &[ConcreteParam]| {
        sig.len() < sigs[anchor].len() && concrete_params_eq(sig, &sigs[anchor][..sig.len()])
    };
    let primary = (0..sigs.len())
        .filter(|&i| is_prefix_of_anchor(&sigs[i]))
        .min_by_key(|&i| sigs[i].len())
        .unwrap_or(anchor);
    (primary != 0).then_some(primary)
}

/// One declared signature of a callable. A function's or method's
/// overloads are separate declarations, each with its own return type,
/// type parameters, JSDoc and `@throws`.
#[derive(Clone, Copy, Debug)]
pub struct Overload<'a> {
    pub params: &'a [Param],
    pub return_type: &'a TypeRef,
    /// The scope holding its `<T, ...>` bindings, which its types lower in.
    /// For non-generic callables this is just the enclosing scope.
    pub body_scope: ScopeId,
    /// Failure-mode info from its `@throws` JSDoc — see [`Throws`].
    ///
    /// Drives three orthogonal decisions:
    /// * `Throws::None` → catching variants use the default `JsValue`
    ///   error type. Sync callables get a `try_<name>` companion.
    /// * `Throws::Type(T)` → catching variants use `Result<_, T>`. Sync
    ///   callables still get the `try_<name>` companion (with the typed
    ///   error).
    /// * `Throws::Never` → caller declares the callable never throws.
    ///   Sync callables get *no* `try_` companion; async callables drop
    ///   `catch` and return `T` directly; a constructor returns the type
    ///   bare.
    pub throws: &'a Throws,
    /// Its doc comment, attached to each signature it expands to.
    pub doc: &'a Option<String>,
}

/// Per-callable input describing a JS callable that needs to be emitted as
/// one or more wasm-bindgen extern signatures. Pairs with [`build_signatures`].
///
/// All decisions about `try_` companions, `async fn` collapse, and `catch`
/// fall out of the kind + return type + throws annotation once these inputs
/// are available.
#[derive(Clone, Debug)]
pub struct CallableSpec<'a> {
    pub js_name: &'a str,
    /// The Rust name, when it is not derived from `js_name`, as for a
    /// default export.
    pub rust_name: Option<&'a str>,
    pub kind: SignatureKind,
    /// All overloads sharing this `js_name` — typically one element, but TS
    /// allows multiple overloaded declarations.
    pub overloads: &'a [Overload<'a>],
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
///   A sync `<name>_promise` sibling returns the `Promise<T>` itself.
/// * **`try_` companion**: for sync, non-setter, non-constructor callables,
///   each primary `name` gets a fallible `try_name` sibling.
/// * **Constructors catch by default**: `new` can throw in JS; the primary
///   signature is `catch: true` and there's no `try_` variant.
/// * **`Throws::Never`**: the caller declared the callable never throws
///   (`@throws {never}`). Sync callables get *no* `try_` companion; async
///   callables drop `catch` and return `T` directly; constructors drop
///   `catch` and return the type bare.
/// * **Naming**: each surviving expansion gets `base + suffix` deduped against
///   `names`; `try_` variants prepend `try_` and declare again.
pub fn build_signatures(
    spec: &CallableSpec<'_>,
    names: &mut LocalScope,
    cgctx: Option<&CodegenContext<'_>>,
) -> Vec<FunctionSignature> {
    let base = spec
        .rust_name
        .map_or_else(|| base_rust_name(spec.js_name, spec.kind), ident_chars);
    let expansions = expand_signatures(spec.overloads, cgctx, spec.kind.allows_literal_cohort());

    // Under the `fallible` policy the primary binding itself catches, so
    // the `try_` companion has nothing to add.
    let fallible = cgctx.is_some_and(|c| c.fallible);
    let per_mono = CodegenContext::generic_mono(cgctx);
    let mut out = Vec::with_capacity(expansions.len());

    // One emitted binding per expansion, except a literal-keyed cohort,
    // which emits one binding per distinct return type among its arms,
    // each wrapped once per arm. Wrapper names bake the literal in after
    // the base name (`get_json`, `get_json_with_array`).
    struct Unit {
        params: Vec<ConcreteParam>,
        overload: usize,
        primary_name: String,
        literal: Option<LiteralSpec>,
    }
    let mut units: Vec<Unit> = Vec::new();
    for exp in expansions {
        if exp.literal_arms.len() < 2 {
            let primary_candidate = public_rust_name(&format!("{base}{}", exp.name_suffix));
            units.push(Unit {
                params: exp.params,
                overload: exp.overload,
                primary_name: names.declare(&primary_candidate, Suffix::Snake),
                literal: None,
            });
            continue;
        }
        let positions: Vec<usize> = exp.literal_arms[0].values.iter().map(|(i, _)| *i).collect();
        let mut params = exp.params;
        for &i in &positions {
            params[i].type_ref = TypeRef::String;
        }
        type Arms = Vec<(Vec<String>, String)>;
        let mut groups: Vec<(&TypeRef, usize, Arms)> = Vec::new();
        for arm in &exp.literal_arms {
            let ret = spec.overloads[arm.overload].return_type;
            let literals: Vec<String> = arm.values.iter().map(|(_, v)| v.clone()).collect();
            let tag = literals
                .iter()
                .map(|l| to_snake_case(l))
                .collect::<Vec<_>>()
                .join("_");
            let candidate = public_rust_name(&format!("{base}_{tag}{}", exp.name_suffix));
            let name = names.declare(&candidate, Suffix::Snake);
            match groups.iter_mut().find(|(r, _, _)| *r == ret) {
                Some((_, _, arms)) => arms.push((literals, name)),
                None => groups.push((ret, arm.overload, vec![(literals, name)])),
            }
        }
        for (_, overload, arms) in groups {
            units.push(Unit {
                params: params.clone(),
                overload,
                primary_name: arms[0].1.clone(),
                literal: Some(LiteralSpec {
                    positions: positions.clone(),
                    arms,
                }),
            });
        }
    }

    for unit in units {
        let Unit {
            params: exp_params,
            overload,
            primary_name,
            literal,
        } = unit;
        let overload = spec.overloads[overload];
        let scope = overload.body_scope;
        // `Throws::Never` suppresses every fallible form, constructors
        // included: `new` can throw in JS, so the default is to catch, but a
        // declaration saying it does not is honored like any other member.
        let nothrow = overload.throws.is_never();
        let error_type = overload.throws.as_type();
        let (is_async, return_type) = match (
            overload.return_type.as_promise_inner(),
            spec.kind.allows_async(),
        ) {
            (Some(inner), true) => (true, inner.clone()),
            _ => (false, overload.return_type.clone()),
        };
        let allow_try = !is_async && !nothrow && !fallible && spec.kind.allows_try_variant();
        out.reserve(match (allow_try, per_mono) {
            (true, true) => 4,
            (true, false) | (false, true) => 2,
            (false, false) => 1,
        });
        let derive = |names: &mut LocalScope, f: &dyn Fn(&str) -> String| {
            literal.as_ref().map(|spec| spec.derive(names, f))
        };
        let primary_literal = literal.clone();

        // Primary variant catches when:
        // * the kind catches by default (constructors), OR
        // * it's async, or sync under the `fallible` policy —
        // unless `Throws::Never` opts out. Setters never catch.
        let primary_catches = !nothrow
            && (spec.kind.always_catches()
                || is_async
                || (fallible && spec.kind.allows_try_variant()));
        let primary = FunctionSignature {
            rust_name: primary_name.clone(),
            js_name: spec.js_name.to_string(),
            params: exp_params.clone(),
            catch: primary_catches,
            is_async,
            return_type: return_type.clone(),
            js_string_return: false,
            error_type: if primary_catches {
                error_type.cloned()
            } else {
                None
            },
            doc: overload.doc.clone(),
            body_scope: scope,
            literal: primary_literal,
        };
        push_with_js_string_return(&mut out, primary, names, cgctx, scope);

        // An `async fn` import only calls JS when first polled, so a
        // caller that needs the call started now (handing the promise
        // to `waitUntil`, gating a constructor on it, racing it) gets
        // the un-awaited form too. Rejection travels inside the
        // returned promise; `catch` here covers only a synchronous
        // throw, which the sync-callable policy decides.
        if is_async {
            let promise_literal = derive(names, &|n| format!("{n}_promise"));
            let promise_name = match &promise_literal {
                Some(l) => l.arms[0].1.clone(),
                None => names.declare(&format!("{primary_name}_promise"), Suffix::Snake),
            };
            let promise_catches = !nothrow && fallible && spec.kind.allows_try_variant();
            out.push(FunctionSignature {
                rust_name: promise_name,
                js_name: spec.js_name.to_string(),
                params: exp_params.clone(),
                catch: promise_catches,
                is_async: false,
                return_type: overload.return_type.clone(),
                js_string_return: false,
                error_type: if promise_catches {
                    error_type.cloned()
                } else {
                    None
                },
                doc: overload.doc.clone(),
                body_scope: scope,
                literal: promise_literal,
            });
        }

        if allow_try {
            let try_literal = derive(names, &|n| format!("try_{n}"));
            let try_name = match &try_literal {
                Some(l) => l.arms[0].1.clone(),
                None => names.declare(&format!("try_{primary_name}"), Suffix::Snake),
            };
            let try_sig = FunctionSignature {
                rust_name: try_name,
                js_name: spec.js_name.to_string(),
                params: exp_params,
                catch: true,
                is_async: false,
                return_type: return_type.clone(),
                js_string_return: false,
                error_type: error_type.cloned(),
                doc: overload.doc.clone(),
                body_scope: scope,
                literal: try_literal,
            };
            push_with_js_string_return(&mut out, try_sig, names, cgctx, scope);
        }
    }

    out
}

fn push_with_js_string_return(
    out: &mut Vec<FunctionSignature>,
    sig: FunctionSignature,
    names: &mut LocalScope,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) {
    let has_js_return = cgctx.is_some_and(|ctx| {
        ctx.experimental_generic_mono
            && (typemap::has_mono_js_string_return(&sig.return_type, ctx, scope)
                || typemap::is_mono_string_list_return(&sig.return_type, ctx, scope))
    });
    out.push(sig.clone());
    if has_js_return {
        let mut js_sig = sig;
        let js_string = |n: &str| format!("{n}_js_string");
        match js_sig.literal.take() {
            Some(spec) => {
                let spec = spec.derive(names, &js_string);
                js_sig.rust_name = spec.arms[0].1.clone();
                js_sig.literal = Some(spec);
            }
            None => {
                js_sig.rust_name = names.declare(&js_string(&js_sig.rust_name), Suffix::Snake);
            }
        }
        js_sig.js_string_return = true;
        out.push(js_sig);
    }
}

/// Compute `_with_X` / `_with_X_and_Y` suffixes across a cohort of expanded
/// signatures. Returns one suffix string per input. The first variant
/// (or the only one if there's a single signature) gets `""`.
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

    // Pre-flatten every param's alternatives once. Reused below in
    // the cartesian-size projection and in the expansion loop, so
    // we never re-flatten the same `TypeRef`.
    let per_param_alts: Vec<Vec<TypeRef>> = non_variadic
        .iter()
        .map(|p| flatten_type(&p.type_ref, cgctx, scope))
        .collect();

    // Cap the cartesian product across params. Each optional param
    // contributes +1 (the truncation case) on top of its alt count;
    // required params multiply. Above the cap, every param's
    // alternative list collapses to a single LUB so the cartesian
    // degenerates to one signature.
    let projected: usize = non_variadic
        .iter()
        .zip(per_param_alts.iter())
        .map(|(p, alts)| {
            let n = alts.len().max(1);
            if p.optional {
                n + 1
            } else {
                n
            }
        })
        .fold(1usize, |acc, n| acc.saturating_mul(n));
    let collapse_each = projected > CARTESIAN_PRODUCT_LIMIT;
    if collapse_each {
        if let Some(c) = cgctx {
            let param_summary = non_variadic
                .iter()
                .map(|p| format!("{}: {}", p.name, p.type_ref.format_ts()))
                .collect::<Vec<_>>()
                .join(", ");
            c.warn(format!(
                "callable cartesian product is {projected} signatures (above the \
                 {CARTESIAN_PRODUCT_LIMIT}-variant limit) — collapsing each \
                 union-typed parameter to its LUB. Params: ({param_summary})",
            ));
        }
    }

    // Build expanded signatures via cartesian product.
    // Start with one empty signature, iterate params left to right.
    let mut sigs: Vec<Vec<ConcreteParam>> = vec![vec![]];

    for (i, (param, mut type_alternatives)) in non_variadic.iter().zip(per_param_alts).enumerate() {
        if collapse_each && type_alternatives.len() > 1 {
            type_alternatives = vec![flatten_collapse_to_lub(&param.type_ref, cgctx, scope)];
        }

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
                    slice_mode: param.slice_mode,
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
                    slice_mode: param.slice_mode,
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
                slice_mode: None,
            });
        }
    }

    sigs
}

/// [`flatten_type`] for dictionary-factory fields. Literal members stay
/// separate alternatives even under per-monomorphization primitive-union
/// grouping, because the factory bakes each literal into its own
/// `new_<literal>` constructor rather than taking it as an argument.
pub fn flatten_dictionary_field(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> Vec<TypeRef> {
    flatten_type_with(ty, cgctx, scope, true)
}

/// Hard cap on the number of alternatives any single [`flatten_type`]
/// call may produce. Above this, the entire input collapses to a
/// single LUB alternative — the per-callable cartesian downstream
/// then degrades to a single signature rather than exploding.
///
/// 32 is enough for every realistic TS union the codegen pipeline
/// has seen (the largest fixture in the test suite tops out around
/// a dozen). The Cloudflare worker-types dictionary that motivated
/// this limit had a union of 68 string literals on one field —
/// well above 32 — and that case is exactly what the saturation
/// is here to handle.
const FLATTEN_ALTERNATIVE_LIMIT: usize = 8;

/// Hard cap on the per-callable cartesian product size in
/// [`expand_single_overload`]. When the product of per-param
/// alternative counts (each clamped by [`FLATTEN_ALTERNATIVE_LIMIT`])
/// exceeds this cap, every union-typed param collapses to its LUB
/// and ABI fan-out is suppressed for the callable — the cartesian
/// degenerates to a single signature. A warning surfaces the
/// dropped overloads through the codegen context.
///
/// 64 is enough room for a 6-param fn whose every arg is a fan-out
/// terminal (2^6 = 64), or a 5-param fn with one binary union arg
/// (2^5 × 2 = 64). Beyond that the rendered API is already an
/// unreadable wall of `_with_…` function names.
const CARTESIAN_PRODUCT_LIMIT: usize = 64;

/// Flatten a type into its concrete codegen alternatives.
///
/// `Union` / `Nullable` / single-segment aliases unfold into each
/// member, mirroring TS's narrowing semantics. `Foo | string` arg
/// fans into `[Foo, string]`. The per-overload cartesian step in
/// [`expand_single_overload`] then handles the cross-param product.
///
/// Fan-out terminates at the param boundary: `Promise<string>` /
/// `Array<string>` don't recurse into their type arguments — the
/// inner `string` keeps its source form and goes through
/// inner-position lowering at codegen time.
///
/// ## Saturation
///
/// The output is capped at [`FLATTEN_ALTERNATIVE_LIMIT`]. A union
/// of 68 string literals would explode the cartesian product of
/// any signature it appears in. The recursion **short-circuits as
/// soon as the cap is hit** — downstream members aren't even
/// visited — and the entire input is replaced with a single LUB
/// alternative computed by [`flatten_collapse_to_lub`]. A
/// warning surfaces through the codegen context so the user sees
/// that some source-level alternatives were dropped from the
/// public API.
///
/// ## Primitive-union grouping
///
/// Under per-monomorphization codegen, primitive alternatives spanning
/// two or more categories (`string | number`) merge into one grouped
/// alternative that lowers to a `js_sys` marker-trait bound; see
/// [`primitive_unions::group_alternatives`].
fn flatten_type(ty: &TypeRef, cgctx: Option<&CodegenContext<'_>>, scope: ScopeId) -> Vec<TypeRef> {
    flatten_type_with(ty, cgctx, scope, false)
}

fn flatten_type_with(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    keep_literals: bool,
) -> Vec<TypeRef> {
    let mut out: Vec<TypeRef> = Vec::new();
    if flatten_into(ty, cgctx, scope, &mut out) {
        pair_typed_array_slice(&mut out, cgctx, scope);
        if cgctx.is_some_and(|ctx| ctx.experimental_generic_mono) {
            return primitive_unions::group_alternatives(out, keep_literals);
        }
        return out;
    }
    if let Some(c) = cgctx {
        c.warn(format!(
            "flattening `{}` exceeded the {}-alternative limit \
             — collapsing to the union LUB and suppressing \
             per-member overloads for this site",
            ty.format_ts(),
            FLATTEN_ALTERNATIVE_LIMIT,
        ));
    }
    vec![flatten_collapse_to_lub(ty, cgctx, scope)]
}

/// Pair a typed array with the `number[]` (or `bigint[]` for a 64-bit
/// array) beside it. A callee accepting `Uint8Array | number[]` reads the
/// values during the call, so the array alternative becomes the typed
/// array's element slice (`&[u8]`), passed as a zero-copy view of that
/// typed array, and goes first. The typed array keeps its handle
/// alternative. With more than one typed array the element is ambiguous
/// and nothing pairs, except for a `Uint8Array` beside a
/// `Uint8ClampedArray`: both hold `u8`, so the slice is a `Uint8Array`.
/// A slice has no `null` carrier, so a nullable view keeps only the
/// `undefined` arm (`Option<&[u8]>`); `null` stays reachable through the
/// typed array's handle overload. `Clamped` has no `Option` form, so a
/// clamped view keeps neither.
fn pair_typed_array_slice(
    alts: &mut Vec<TypeRef>,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) {
    use crate::ir::TypedArrayElement as E;
    fn peel_nullable(t: &TypeRef) -> (&TypeRef, Option<crate::ir::Nullability>) {
        match t {
            TypeRef::Nullable(inner, n) => (inner, Some(*n)),
            t => (t, None),
        }
    }
    let typed: Vec<_> = alts
        .iter()
        .filter_map(|t| match peel_nullable(t).0 {
            TypeRef::Reference {
                segments,
                generic_args,
            } if segments.len() == 1 => E::of_typed_array(&segments[0])
                .map(|elem| (elem, SharedMemory::of_typed_array_args(generic_args))),
            _ => None,
        })
        .collect();
    let (elem, shared) = match typed[..] {
        [only] => only,
        [a, b] if a.0 == E::U8 && b.0 == E::U8Clamped => a,
        [a, b] if b.0 == E::U8 && a.0 == E::U8Clamped => b,
        _ => return,
    };
    let pairs = |inner: &TypeRef| {
        let inner = match cgctx {
            Some(c) => typemap::peel_mono(inner, c, scope, &mut Default::default()),
            None => std::borrow::Cow::Borrowed(inner),
        };
        match inner.as_ref() {
            TypeRef::BigInt => elem.is_bigint(),
            TypeRef::Number | TypeRef::NumberLiteral(_) => !elem.is_bigint(),
            _ => false,
        }
    };
    let Some(i) = alts
        .iter()
        .position(|t| matches!(peel_nullable(t).0, TypeRef::Array(inner) if pairs(inner)))
    else {
        return;
    };
    let view = match peel_nullable(&alts.remove(i)).1 {
        Some(n) if n.undefined && !elem.is_clamped() => TypeRef::Nullable(
            Box::new(TypeRef::TypedSlice(elem, shared)),
            crate::ir::Nullability::UNDEFINED,
        ),
        _ => TypeRef::TypedSlice(elem, shared),
    };
    alts.insert(0, view);
}

/// Push every flatten-alternative for `ty` into `out`. Returns
/// `false` the first time the running total exceeds
/// [`FLATTEN_ALTERNATIVE_LIMIT`], short-circuiting the recursion
/// so a 1000-member union doesn't materialise every member just
/// to be discarded.
///
/// `out` may contain garbage on early-return (partial fan-out).
/// Callers ignore `out` entirely when `false` is returned.
fn flatten_into(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    out: &mut Vec<TypeRef>,
) -> bool {
    if out.len() > FLATTEN_ALTERNATIVE_LIMIT {
        return false;
    }
    match ty {
        // Unions fan out into each member recursively. Short-circuit
        // as soon as the limit trips so a huge union doesn't pay for
        // members beyond the cap.
        TypeRef::Union(members) => {
            for m in members {
                if !flatten_into(m, cgctx, scope, out) {
                    return false;
                }
            }
            true
        }

        // References resolve through aliases (generic instantiations
        // substitute their arguments), then re-flatten.
        TypeRef::Reference { .. } => {
            if let Some(c) = cgctx {
                if let Some(target) = c.resolve_alias_instance(ty, scope) {
                    return flatten_into(&target, cgctx, scope, out);
                }
            }
            out.push(ty.clone());
            true
        }

        // Nullable: fan out the inner and keep each alternative
        // nullable, so `T | U | null` becomes `Option<&T>` / `Option<&U>`
        // overloads rather than a `_with_null(val: &Null)` variant.
        TypeRef::Nullable(inner, n) => {
            let mut alts = Vec::new();
            if !flatten_into(inner, cgctx, scope, &mut alts) {
                return false;
            }
            out.extend(
                alts.into_iter()
                    .map(|alt| TypeRef::Nullable(Box::new(alt), *n)),
            );
            true
        }

        // `PropertyKey` stays whole for per-mono grouping so it keeps
        // the `js_sys::PropertyKey` spelling; otherwise it fans out like
        // the union it abbreviates.
        TypeRef::PropertyKey if !cgctx.is_some_and(|ctx| ctx.experimental_generic_mono) => {
            flatten_into(
                &TypeRef::Union(TypeRef::property_key_members()),
                cgctx,
                scope,
                out,
            )
        }

        // Everything else is terminal — generic containers are not
        // distributive (`Array<A | B>` and `Record<K, A | B>` are
        // single parameter shapes, not overloads), and TS spelling
        // controls Rust outer lowering rather than fanning out into
        // alternative forms.
        _ => {
            out.push(ty.clone());
            true
        }
    }
}

/// Compute the LUB-collapsed single alternative for a `TypeRef`
/// whose normal flatten would exceed
/// [`FLATTEN_ALTERNATIVE_LIMIT`].
///
/// Returns [`crate::codegen::subtyping::lub_union`] for unions
/// (and aliases that chase through to one); otherwise returns the
/// input unchanged. A `Nullable` stays nullable around the collapsed
/// inner.
fn flatten_collapse_to_lub(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TypeRef {
    match ty {
        TypeRef::Union(members) => {
            crate::codegen::subtyping::lub_union(members, cgctx, scope).unwrap_or(TypeRef::Any)
        }
        TypeRef::Nullable(inner, n) => {
            TypeRef::Nullable(Box::new(flatten_collapse_to_lub(inner, cgctx, scope)), *n)
        }
        TypeRef::PropertyKey => TypeRef::Any,
        TypeRef::Reference { .. } => {
            if let Some(c) = cgctx {
                if let Some(target) = c.resolve_alias_instance(ty, scope) {
                    return flatten_collapse_to_lub(&target, cgctx, scope);
                }
            }
            ty.clone()
        }
        other => other.clone(),
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
/// These are candidate names — the caller declares them in the block's `LocalScope` for
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

    // Greedy single-pass naming: the first signature with a given
    // candidate suffix keeps it; subsequent siblings whose
    // type-name-driven suffix would collide retry the offending
    // position using the param-name path instead. This replaces
    // numeric dedup (`_with_record` + `_with_record_2`) with
    // semantic disambiguation (`_with_record` + `_with_context`).
    let mut names = Vec::with_capacity(signatures.len());
    let mut used = LocalScope::default();

    for (sig_idx, sig) in signatures.iter().enumerate() {
        // The first signature gets the base name without any suffix;
        // `expand_signatures` has already moved the primary to the front.
        if sig_idx == 0 {
            names.push(base_name.to_string());
            used.claim(base_name);
            continue;
        }

        // Compute per-position suffix parts, each tagged with whether
        // the suffix used the type-name (which is collision-prone) or
        // the param-name (uniquely identifying). On collision the
        // type-name-tagged parts get rewritten to param-name during
        // the post-collision retry.
        let mut parts: Vec<SuffixPart> = Vec::new();

        let end = if sig.len() >= trim_end {
            sig.len() - trim_end
        } else {
            // Signature is shorter than the shared suffix — don't trim
            sig.len()
        };
        let start = trim_start.min(end);

        for (param_idx, param) in sig[start..end].iter().enumerate() {
            let abs_idx = start + param_idx;

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

            let param_snake = snake_ident(&param.name);
            let (text, from_type) = if any_same_name_different_type {
                (type_snake_name(&param.type_ref), true)
            } else {
                (param_snake.clone(), false)
            };
            parts.push(SuffixPart {
                text,
                from_type,
                param_snake,
            });
        }

        // Assemble the candidate name. If it collides with a sibling
        // already in `used`, retry the type-name parts by prepending
        // the param name. The retry runs at most twice — once
        // upgrading the entire suffix to the param-name path, once
        // upgrading param-name to param-name + flavor. Anything past
        // that returns to numeric dedup as the last-resort safety
        // net.
        let mut name = compose_name(base_name, &parts, NamingStrategy::Greedy);
        if used.contains(&name) {
            // Strategy 1: every type-name part adopts its param-name
            // prefix. The fanned side keeps its flavor as a suffix.
            name = compose_name(base_name, &parts, NamingStrategy::ParamNamePlusFlavor);
        }
        // Strategy 2: numeric fallback. Unlikely to fire in practice —
        // the param-name + flavor form is already very specific.
        names.push(used.declare(&name, Suffix::Snake));
    }

    names
}

/// How [`compute_rust_names`] assembles a name from its per-position
/// [`SuffixPart`] list. Used to retry name composition when a
/// greedy first-pass name collides with a sibling.
enum NamingStrategy {
    /// Use each part's chosen `text` as-is (`_with_record`,
    /// `_with_context`). The greedy default.
    Greedy,
    /// Prefer the **param name** for every part that originally came
    /// from the type-name path, suffixed with the ABI flavor when
    /// the part marks itself as a fan-out variant. This is the
    /// collision-rescue path: `_with_context_record` /
    /// `_with_default_value_js_string` over numeric dedup.
    ParamNamePlusFlavor,
}

fn compose_name(
    base_name: &str,
    parts: &[impl SuffixPartView],
    strategy: NamingStrategy,
) -> String {
    let mut name = base_name.to_string();
    let mut first = true;
    for p in parts {
        if first {
            name.push_str("_with_");
            first = false;
        } else {
            name.push_str("_and_");
        }
        let chunk = match strategy {
            NamingStrategy::Greedy => p.greedy_text().to_string(),
            NamingStrategy::ParamNamePlusFlavor => p.param_name_plus_flavor(),
        };
        name.push_str(&chunk);
    }
    name
}

/// One position's contribution to a `_with_<…>_and_<…>` suffix.
/// Created by [`compute_rust_names`] and consumed by [`compose_name`]
/// for greedy-then-retry naming.
struct SuffixPart {
    /// The greedy first-pass token (`record`, `context`, `flag_key`,
    /// …). Comes from `type_snake_name` for union differences, from
    /// the param name for optional / overload differences.
    text: String,
    /// `true` when [`text`] came from `type_snake_name`. Type-name
    /// suffixes (e.g. two `Record<X,Y>` / `Record<X,Z>` args both
    /// snake-casing to `record`) are the collision-prone ones and
    /// get rewritten on retry.
    from_type: bool,
    /// Param name in snake_case — the retry fallback that replaces
    /// the type-name path on greedy-collision.
    param_snake: String,
}

impl SuffixPartView for SuffixPart {
    fn greedy_text(&self) -> &str {
        &self.text
    }
    fn param_name_plus_flavor(&self) -> String {
        if !self.from_type {
            // Originally param-name driven — no upgrade needed.
            self.text.clone()
        } else {
            // Type-name driven and lost on collision — swap in the
            // param name. The original type-name token (`record`,
            // `js_string`, …) becomes the secondary modifier so the
            // resulting suffix carries both axes.
            format!("{}_{}", self.param_snake, self.text)
        }
    }
}

/// Read-only view of the suffix part fields needed by
/// [`compose_name`]. Lets the assembly step stay strategy-agnostic.
trait SuffixPartView {
    fn greedy_text(&self) -> &str;
    fn param_name_plus_flavor(&self) -> String;
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
///
/// Names mirror the **Rust** lowering of the type rather than the
/// TypeScript spelling, so the resulting suffix matches what callers
/// see in their function signatures:
///
/// | TypeScript      | Rust (arg)      | Suffix          |
/// | --------------- | --------------- | --------------- |
/// | `string`, `"x"` | `&str`          | `str`           |
/// | `number`, `1`   | `f64`           | `f64`           |
/// | `boolean`, `true` | `bool`        | `bool`          |
/// | `Array<T>`      | `&[T]`          | `slice`         |
/// | `T[]`           | `&[T]`          | `slice`         |
/// | `Uint8Array`    | `&Uint8Array`   | `uint8_array`   |
/// | `MyType`        | `&MyType`       | `my_type`       |
/// | `A.X`           | `&a::X`         | `a_x`           |
///
/// Named references (built-ins and user types) snake-case their path,
/// `A.X` → `a_x` — type args don't participate in the suffix. The named
/// JS types (`Uint8Array`, `Headers`, …) double as Rust types in
/// the generated bindings, so the snake-cased JS name *is* the
/// Rust spelling.
///
/// `ArrayBufferView` is a synthetic union that lowers to `&Object` /
/// `Uint8Array` depending on position; `typed_array` is short for
/// "any concrete typed-array" which captures both arms.
fn type_snake_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::String | TypeRef::StringLiteral(_) => "str".to_string(),
        TypeRef::Number | TypeRef::NumberLiteral(_) => "f64".to_string(),
        TypeRef::Boolean | TypeRef::BooleanLiteral(_) => "bool".to_string(),
        TypeRef::BigInt => "big_int".to_string(),
        TypeRef::Symbol => "symbol".to_string(),
        TypeRef::Void | TypeRef::Undefined => "undefined".to_string(),
        TypeRef::Null => "null".to_string(),
        TypeRef::Any | TypeRef::Unknown => "js_value".to_string(),
        TypeRef::Object => "object".to_string(),
        TypeRef::ArrayBufferView => "typed_array".to_string(),
        // Grouped per-mono primitive unions (`number_or_string`,
        // `property_key`). Outside that mode `PropertyKey` has already
        // fanned out into its members.
        TypeRef::Union(_) | TypeRef::PropertyKey => primitive_unions::PrimitiveUnion::classify(ty)
            .map_or_else(|| "js_value".to_string(), |u| u.suffix()),
        // Snake-case every segment of a named reference, so `A.X` is
        // `a_x` like its Rust path `a::X`. Type args don't participate in
        // `_with_` suffixes. A synthesised `Array<T>` / `ReadonlyArray<T>`
        // reference lowers to the slice form like the parse-normalised
        // `TypeRef::Array`, so it shares the `slice` suffix.
        TypeRef::Reference { segments, .. } => {
            if let [head] = segments.as_slice() {
                if head == "Array" || head == "ReadonlyArray" {
                    return "slice".to_string();
                }
            }
            segments
                .iter()
                .map(|s| to_snake_case(s))
                .collect::<Vec<_>>()
                .join("_")
        }
        TypeRef::Array(_) | TypeRef::TypedSlice(..) => "slice".to_string(),
        TypeRef::Nullable(inner, _) => type_snake_name(inner),
        TypeRef::Function(_) => "function".to_string(),
        _ => "js_value".to_string(),
    }
}

// ─── Shared codegen helpers ─────────────────────────────────────────

/// Convert concrete params to a `fn` parameter token stream.
///
/// Handles variadic params with `&[JsValue]`. The output is for extern
/// declarations: a parameter with a non-default slice mode carries
/// `#[wasm_bindgen(slice = ..)]` (see [`typemap::param_slice_mode`]).
pub fn generate_concrete_params(
    params: &[ConcreteParam],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &crate::ir::ModuleContext,
) -> TokenStream {
    let items: Vec<_> = params
        .iter()
        .zip(param_idents(params, cgctx))
        .map(|(p, name)| {
            let attr = param_attr(p, cgctx, scope);
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
            quote! { #attr #name: #ty }
        })
        .collect();

    quote! { #(#items),* }
}

/// One ident per parameter, numbered where names collide once sanitised
/// (`a` / `$a` → `a` / `a2`).
fn param_idents(params: &[ConcreteParam], cgctx: Option<&CodegenContext<'_>>) -> Vec<syn::Ident> {
    use syn::ext::IdentExt;
    let mut names = LocalScope::default();
    params
        .iter()
        .map(|p| {
            let ident = typemap::make_ident(&p.name).unraw().to_string();
            let ident = CodegenContext::param_name(cgctx, &ident);
            typemap::make_ident(&names.declare(&ident, Suffix::Param))
        })
        .collect()
}

/// The ident a dictionary helper's parameter binds. One named like the
/// helpers' `inner` local takes a trailing underscore, as does one named
/// like a static.
pub(crate) fn dictionary_param_ident(name: &str, cgctx: Option<&CodegenContext<'_>>) -> syn::Ident {
    let name = if name == "inner" { "inner_" } else { name };
    typemap::make_ident(&CodegenContext::param_name(cgctx, name))
}

/// The per-argument wasm-bindgen attribute for an extern parameter, or
/// empty tokens. Extern-only: plain Rust helpers can't carry custom
/// attributes on their parameters.
fn param_attr(
    p: &ConcreteParam,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TokenStream {
    // The string-literal spelling rather than the bare ident: rustfmt
    // only normalises `name = <literal>` attribute arms, leaving
    // `slice = array` token-spaced verbatim.
    match typemap::param_slice_mode(p, cgctx, scope) {
        Some(typemap::SliceMode::View) => quote! { #[wasm_bindgen(slice = "view")] },
        Some(typemap::SliceMode::UnsharedView) => {
            let marker = super::UNSHARED_VIEW;
            quote! { #[wasm_bindgen(slice = #marker)] }
        }
        Some(typemap::SliceMode::Array) => quote! { #[wasm_bindgen(slice = "array")] },
        Some(typemap::SliceMode::Copy) => quote! { #[wasm_bindgen(slice = "copy")] },
        None => quote! {},
    }
}

/// Convert parameters, using argument-position `impl Trait` bounds for
/// strings and primitive unions in per-monomorphization mode (see
/// [`mono_argument_type`]).
pub(crate) fn generate_concrete_params_with_mono_bounds(
    params: &[ConcreteParam],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &crate::ir::ModuleContext,
) -> TokenStream {
    let per_mono = CodegenContext::generic_mono(cgctx);
    if !per_mono {
        return generate_concrete_params(params, cgctx, scope, from_module);
    }

    let items = params
        .iter()
        .zip(param_idents(params, cgctx))
        .map(|(param, name)| {
            let attr = param_attr(param, cgctx, scope);
            let ty = render_param_type(param, cgctx, scope, from_module);
            quote! { #attr #name: #ty }
        })
        .collect::<Vec<_>>();

    quote! { #(#items),* }
}

/// The Rust type of one concrete parameter, exactly as the emitters render
/// it. Cross-overload dedup compares these so two expansions collapse only
/// when their rendered signatures coincide.
pub(crate) fn render_param_type(
    param: &ConcreteParam,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &crate::ir::ModuleContext,
) -> TokenStream {
    if param.variadic {
        return quote! { &[JsValue] };
    }
    cgctx
        .filter(|ctx| ctx.experimental_generic_mono)
        .and_then(|ctx| {
            mono_argument_type(&param.type_ref)
                .or_else(|| typemap::mono_string_list_argument(&param.type_ref, ctx, scope))
        })
        .unwrap_or_else(|| {
            typemap::to_syn_type(
                &param.type_ref,
                TypePosition::ARGUMENT,
                cgctx,
                scope,
                from_module,
            )
        })
}

/// Per-monomorphization argument bounds: strings become `impl
/// JsStringLike`, and grouped primitive unions become the matching `impl
/// js_sys::Js…Like` marker trait. Each occurrence is an independent
/// anonymous type parameter.
fn mono_argument_type(ty: &TypeRef) -> Option<TokenStream> {
    let bound = |ty: &TypeRef| match ty {
        TypeRef::String | TypeRef::StringLiteral(_) => {
            Some(quote! { ::wasm_bindgen::JsStringLike })
        }
        _ => primitive_unions::PrimitiveUnion::classify(ty).map(|u| u.trait_path()),
    };
    match ty {
        // `Option<impl Bound>` crosses `None` as `undefined`, so it is only
        // the right shape when `undefined` is the sole empty; a `null`
        // arm goes through the regular `JsNullable` lowering.
        TypeRef::Nullable(inner, n) if !n.null => bound(inner).map(|b| quote! { Option<impl #b> }),
        TypeRef::Nullable(..) => None,
        _ => bound(ty).map(|b| quote! { impl #b }),
    }
}

/// Rendered parameters for a dictionary factory, builder method, or setter.
pub struct DictionaryParams {
    /// One `Tn: ::js_sys::TypedArray` bound per `ArrayBufferView` param.
    pub bounds: Vec<TokenStream>,
    /// Extra `where` clause for Rust helper bodies. Extern declarations must
    /// not use it; see [`generate_dictionary_params`].
    pub helper_where_clause: TokenStream,
    /// The rendered `name: Type, …` list for Rust helper bodies.
    pub params: TokenStream,
    /// The same list for extern declarations, with per-argument
    /// wasm-bindgen attributes (`slice = ..`). Helpers must use
    /// [`Self::params`]: custom attributes don't parse on an ordinary
    /// `fn`'s parameters.
    pub extern_params: TokenStream,
}

/// Render dictionary factory params into a [`DictionaryParams`].
///
/// `ArrayBufferView` switches the helper into a generic signature
/// `<Tn: js_sys::TypedArray>` and a `&Tn` parameter, so callers can pass any
/// concrete typed-array (`Uint8Array`, `Int32Array`, etc.) without explicit
/// casts. All other types pass through `generate_concrete_params`.
///
/// Each synthesised ABV bound is a separate `TokenStream` (e.g.
/// `T: ::js_sys::TypedArray`). Callers compose the declarations with type-level generic bounds
/// (`T: ::wasm_bindgen::JsGeneric`) via [`render_generic_bounds`] so a single
/// `<...>` declaration carries every bound the `fn` needs.
///
/// Under per-monomorphization codegen, ordinary Rust helpers that call an
/// imported `&T` parameter must repeat the macro-generated reference ABI bound.
/// Extern declarations ignore the returned clause because wasm-bindgen adds
/// that requirement to their generated call shim itself.
pub fn generate_dictionary_params(
    params: &[ConcreteParam],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &crate::ir::ModuleContext,
) -> DictionaryParams {
    generate_dictionary_params_in(
        params,
        &vec![scope; params.len()],
        cgctx,
        scope,
        from_module,
    )
}

/// [`generate_dictionary_params`] for params each resolving in its own
/// scope, as a dictionary's fields from different declarations do.
/// `scope` is the dictionary's body scope.
pub fn generate_dictionary_params_in(
    params: &[ConcreteParam],
    scopes: &[ScopeId],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &crate::ir::ModuleContext,
) -> DictionaryParams {
    let mut generic_idents: Vec<syn::Ident> = Vec::new();
    let mut extern_items: Vec<TokenStream> = Vec::new();
    // Skips names already bound in scope, such as the dictionary's own `<T>`.
    let mut letters = (0..)
        .map(generic_letter)
        .filter(|g| cgctx.is_none_or(|c| c.gctx.scopes.resolve_binding(scope, g).is_none()));
    let items: Vec<_> = params
        .iter()
        .zip(scopes)
        .map(|(p, &scope)| {
            let name = dictionary_param_ident(&p.name, cgctx);
            let ty = if !p.variadic && matches!(p.type_ref, TypeRef::ArrayBufferView) {
                let g = syn::Ident::new(
                    &letters.next().expect("unbounded"),
                    proc_macro2::Span::call_site(),
                );
                generic_idents.push(g.clone());
                quote! { &#g }
            } else {
                render_param_type(p, cgctx, scope, from_module)
            };
            let attr = param_attr(p, cgctx, scope);
            extern_items.push(quote! { #attr #name: #ty });
            quote! { #name: #ty }
        })
        .collect();

    let bounds = generic_idents
        .iter()
        .map(|g| quote! { #g: ::js_sys::TypedArray })
        .collect::<Vec<_>>();

    // This must mirror wasm-bindgen's macro-generated import-shim bound: the
    // ordinary Rust dictionary helper calls that shim with `&T` directly.
    let helper_where_clause = if CodegenContext::generic_mono(cgctx) && !generic_idents.is_empty() {
        quote! {
            where
                #(for<'__wbg> &'__wbg #generic_idents: ::wasm_bindgen::convert::IntoWasmAbi),*
        }
    } else {
        quote! {}
    };

    DictionaryParams {
        bounds,
        helper_where_clause,
        params: quote! { #(#items),* },
        extern_params: quote! { #(#extern_items),* },
    }
}

/// Wrap a list of `<T: Bound>` token streams into a `<T: Bound, U: Bound>`
/// declaration. Empty input renders as empty tokens (not `<>`), which is
/// what `quote!` interpolation expects for "no generics".
pub fn render_generic_bounds(bounds: &[TokenStream]) -> TokenStream {
    if bounds.is_empty() {
        quote! {}
    } else {
        quote! { <#(#bounds),*> }
    }
}

/// Generic identifier names, walking `T, U, V, W, X, Y, Z, A, B, C, ...`
/// and primed (`T2`, `U2`, ...) when more than 26 are needed.
fn generic_letter(index: usize) -> String {
    // T..Z then A..S — covers all 26 single-letter slots without repeats.
    const ORDER: &[u8] = b"TUVWXYZABCDEFGHIJKLMNOPQRS";
    let letter = ORDER[index % ORDER.len()] as char;
    let primes = index / ORDER.len();
    if primes == 0 {
        letter.to_string()
    } else {
        format!("{letter}{}", primes + 1)
    }
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
    use crate::ir::{Nullability, TypeRef};

    fn no_used() -> LocalScope {
        LocalScope::default()
    }

    #[test]
    fn mono_strings_use_anonymous_impl_trait() {
        assert_eq!(
            mono_argument_type(&TypeRef::String).unwrap().to_string(),
            "impl :: wasm_bindgen :: JsStringLike"
        );
        assert_eq!(
            mono_argument_type(&TypeRef::Nullable(
                Box::new(TypeRef::String),
                Nullability::UNDEFINED
            ))
            .unwrap()
            .to_string(),
            "Option < impl :: wasm_bindgen :: JsStringLike >"
        );
        // A `null` arm has no `impl Trait` carrier; the JsNullable lowering applies.
        assert!(mono_argument_type(&TypeRef::Nullable(
            Box::new(TypeRef::String),
            Nullability::NULL
        ))
        .is_none());
        assert!(mono_argument_type(&TypeRef::Number).is_none());
    }

    #[test]
    fn mono_primitive_unions_use_marker_traits() {
        let union = TypeRef::Union(vec![TypeRef::String, TypeRef::Number]);
        assert_eq!(
            mono_argument_type(&union).unwrap().to_string(),
            "impl :: js_sys :: JsNumberOrStringLike"
        );
        assert_eq!(
            mono_argument_type(&TypeRef::Nullable(Box::new(union), Nullability::UNDEFINED))
                .unwrap()
                .to_string(),
            "Option < impl :: js_sys :: JsNumberOrStringLike >"
        );
        assert_eq!(
            mono_argument_type(&TypeRef::PropertyKey)
                .unwrap()
                .to_string(),
            "impl :: js_sys :: PropertyKey"
        );
    }

    #[test]
    fn param_idents_number_sanitised_collisions_without_underscores() {
        let params = ["a", "$a", "type", "type", "a2"].map(|n| concrete(n, TypeRef::Any, false));
        let idents: Vec<String> = param_idents(&params, None)
            .iter()
            .map(|i| i.to_string())
            .collect();
        assert_eq!(idents, ["a", "a2", "r#type", "type2", "a22"]);
    }

    fn string_param(name: &str) -> Param {
        Param {
            name: name.into(),
            type_ref: TypeRef::String,
            optional: false,
            variadic: false,
            slice_mode: None,
        }
    }

    fn concrete(name: &str, type_ref: TypeRef, variadic: bool) -> ConcreteParam {
        ConcreteParam {
            slice_mode: None,
            name: name.into(),
            type_ref,
            variadic,
        }
    }

    /// Build signatures for one overload with per-monomorphization on or off.
    fn expand_mode(
        mono: bool,
        js: &str,
        params: &[Param],
        ret: &TypeRef,
        used: &mut LocalScope,
    ) -> Vec<FunctionSignature> {
        let (gctx, scope) = test_ctx();
        let mut cgctx = CodegenContext::empty(&gctx, scope);
        cgctx.experimental_generic_mono = mono;
        build_signatures(
            &CallableSpec {
                js_name: js,
                rust_name: None,
                kind: SignatureKind::Function,
                overloads: &[Overload {
                    params,
                    return_type: ret,
                    body_scope: scope,
                    throws: &Throws::None,
                    doc: &None,
                }],
            },
            used,
            Some(&cgctx),
        )
    }

    #[test]
    fn mono_string_returns_add_js_string_variants() {
        let sigs = expand_mode(
            true,
            "value",
            &[string_param("fallback")],
            &TypeRef::String,
            &mut no_used(),
        );
        let names: Vec<(&str, bool)> = sigs
            .iter()
            .map(|s| (s.rust_name.as_str(), s.js_string_return))
            .collect();
        assert!(names.contains(&("value", false)));
        assert!(names.contains(&("value_js_string", true)));
        assert!(names.contains(&("try_value", false)));
        assert!(names.contains(&("try_value_js_string", true)));
        // The variant binds the same JS function with the same params.
        let js = sigs
            .iter()
            .find(|s| s.rust_name == "value_js_string")
            .unwrap();
        assert_eq!(js.js_name, "value");
        assert_eq!(js.params.len(), 1);
    }

    fn fallible_sigs(
        kind: SignatureKind,
        throws: &Throws,
        ret: &TypeRef,
    ) -> Vec<FunctionSignature> {
        let (gctx, scope) = test_ctx();
        let mut cgctx = CodegenContext::empty(&gctx, scope);
        cgctx.fallible = true;
        build_signatures(
            &CallableSpec {
                js_name: "op",
                rust_name: None,
                kind,
                overloads: &[Overload {
                    params: &[],
                    return_type: ret,
                    body_scope: scope,
                    throws,
                    doc: &None,
                }],
            },
            &mut no_used(),
            Some(&cgctx),
        )
    }

    #[test]
    fn fallible_policy_emits_one_catching_primary() {
        for kind in [
            SignatureKind::Method,
            SignatureKind::StaticMethod,
            SignatureKind::Function,
            SignatureKind::Constructor,
        ] {
            let sigs = fallible_sigs(kind, &Throws::None, &TypeRef::String);
            assert_eq!(sigs.len(), 1, "{kind:?}");
            assert!(sigs[0].catch && !sigs[0].rust_name.starts_with("try_"));
        }
        let never = fallible_sigs(SignatureKind::Method, &Throws::Never, &TypeRef::String);
        assert_eq!(never.len(), 1);
        assert!(!never[0].catch);
        let ctor_never =
            fallible_sigs(SignatureKind::Constructor, &Throws::Never, &TypeRef::String);
        assert!(!ctor_never[0].catch);
        let setter = fallible_sigs(SignatureKind::Setter, &Throws::None, &TypeRef::Void);
        assert_eq!(setter.len(), 1);
        assert!(!setter[0].catch);
        let typed = fallible_sigs(
            SignatureKind::Method,
            &Throws::Type(TypeRef::ident("TypeError")),
            &TypeRef::Void,
        );
        assert_eq!(typed[0].error_type, Some(TypeRef::ident("TypeError")));
    }

    #[test]
    fn fallible_policy_promise_form_catches_sync_throws() {
        let promise = TypeRef::generic("Promise", vec![TypeRef::String]);
        let sigs = fallible_sigs(SignatureKind::Method, &Throws::None, &promise);
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[1].rust_name, "op_promise");
        assert!(!sigs[1].is_async && sigs[1].catch);
        assert_eq!(sigs[1].return_type, promise);
        let typed = fallible_sigs(
            SignatureKind::Method,
            &Throws::Type(TypeRef::ident("TypeError")),
            &promise,
        );
        assert_eq!(typed[1].error_type, Some(TypeRef::ident("TypeError")));
        let never = fallible_sigs(SignatureKind::Method, &Throws::Never, &promise);
        assert!(!never[1].catch && never[1].error_type.is_none());
    }

    #[test]
    fn js_string_variants_need_mono_and_a_string_return() {
        let default = expand_mode(false, "value", &[], &TypeRef::String, &mut no_used());
        assert!(default.iter().all(|s| !s.js_string_return));
        let number = expand_mode(true, "count", &[], &TypeRef::Number, &mut no_used());
        assert!(number.iter().all(|s| !s.js_string_return));
    }

    #[test]
    fn js_string_variant_names_are_deduped() {
        let mut used = LocalScope::with_claimed(["value_js_string"]);
        let sigs = expand_mode(true, "value", &[], &TypeRef::String, &mut used);
        assert!(sigs.iter().any(|s| s.rust_name == "value_js_string_2"));
    }

    #[test]
    fn mono_bounds_params_switch_strings_only_in_mono_mode() {
        let (gctx, scope) = test_ctx();
        let mut cgctx = CodegenContext::empty(&gctx, scope);
        let params = [
            concrete("name", TypeRef::String, false),
            concrete("count", TypeRef::Number, false),
            concrete("rest", TypeRef::Any, true),
        ];
        let render = |cgctx: &CodegenContext<'_>| {
            generate_concrete_params_with_mono_bounds(
                &params,
                Some(cgctx),
                scope,
                &crate::ir::ModuleContext::Global,
            )
            .to_string()
        };
        assert_eq!(
            render(&cgctx),
            "name : & str , count : f64 , rest : & [JsValue]"
        );
        cgctx.experimental_generic_mono = true;
        assert_eq!(
            render(&cgctx),
            "name : impl :: wasm_bindgen :: JsStringLike , count : f64 , rest : & [JsValue]"
        );
    }

    #[test]
    fn dictionary_params_helper_where_clause_only_in_mono_mode() {
        let (gctx, scope) = test_ctx();
        let mut cgctx = CodegenContext::empty(&gctx, scope);
        let params = [
            concrete("view", TypeRef::ArrayBufferView, false),
            concrete("label", TypeRef::String, false),
        ];
        let render = |cgctx: &CodegenContext<'_>| {
            generate_dictionary_params(
                &params,
                Some(cgctx),
                scope,
                &crate::ir::ModuleContext::Global,
            )
        };

        let default = render(&cgctx);
        assert_eq!(default.bounds.len(), 1);
        assert_eq!(default.bounds[0].to_string(), "T : :: js_sys :: TypedArray");
        assert!(default.helper_where_clause.is_empty());
        assert_eq!(default.params.to_string(), "view : & T , label : & str");

        cgctx.experimental_generic_mono = true;
        let mono = render(&cgctx);
        assert_eq!(
            mono.helper_where_clause.to_string(),
            "where for < '__wbg > & '__wbg T : :: wasm_bindgen :: convert :: IntoWasmAbi"
        );
        assert_eq!(
            mono.params.to_string(),
            "view : & T , label : impl :: wasm_bindgen :: JsStringLike"
        );
    }

    #[test]
    fn dictionary_params_typed_array_generics_skip_names_in_scope() {
        let (mut gctx, root) = test_ctx();
        let scope = gctx.scopes.create_child(root);
        gctx.scopes.insert_type_param(scope, "T".to_string());
        gctx.scopes.insert_type_param(root, "V".to_string());
        let cgctx = CodegenContext::empty(&gctx, root);
        let params = [
            concrete("a", TypeRef::ArrayBufferView, false),
            concrete("b", TypeRef::ArrayBufferView, false),
        ];
        let rendered = generate_dictionary_params(
            &params,
            Some(&cgctx),
            scope,
            &crate::ir::ModuleContext::Global,
        );
        assert_eq!(rendered.params.to_string(), "a : & U , b : & W");
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
        used: &mut LocalScope,
    ) -> Vec<FunctionSignature> {
        expand_throws(js, params, ret, kind, &Throws::None, doc, used)
    }

    /// Like [`expand`] but with an explicit [`Throws`] state.
    fn expand_throws(
        js: &str,
        params: &[Param],
        ret: &TypeRef,
        kind: SignatureKind,
        throws: &Throws,
        doc: &Option<String>,
        used: &mut LocalScope,
    ) -> Vec<FunctionSignature> {
        let (gctx, scope) = test_ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        build_signatures(
            &CallableSpec {
                js_name: js,
                rust_name: None,
                kind,
                overloads: &[Overload {
                    params,
                    return_type: ret,
                    body_scope: scope,
                    throws,
                    doc,
                }],
            },
            used,
            Some(&cgctx),
        )
    }

    /// Shorthand: build signatures for multiple overloads.
    fn expand_overloads(
        js: &str,
        overloads: &[&[Param]],
        ret: &TypeRef,
        kind: SignatureKind,
        doc: &Option<String>,
        used: &mut LocalScope,
    ) -> Vec<FunctionSignature> {
        let (gctx, scope) = test_ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let overloads: Vec<Overload> = overloads
            .iter()
            .map(|&params| Overload {
                params,
                return_type: ret,
                body_scope: scope,
                throws: &Throws::None,
                doc,
            })
            .collect();
        build_signatures(
            &CallableSpec {
                js_name: js,
                rust_name: None,
                kind,
                overloads: &overloads,
            },
            used,
            Some(&cgctx),
        )
    }

    fn param(name: &str) -> Param {
        Param {
            name: name.to_string(),
            type_ref: TypeRef::Any,
            optional: false,
            variadic: false,
            slice_mode: None,
        }
    }

    fn typed_param(name: &str, ty: TypeRef) -> Param {
        Param {
            name: name.to_string(),
            type_ref: ty,
            optional: false,
            variadic: false,
            slice_mode: None,
        }
    }

    fn opt_param(name: &str) -> Param {
        Param {
            name: name.to_string(),
            type_ref: TypeRef::Any,
            optional: true,
            variadic: false,
            slice_mode: None,
        }
    }

    fn opt_typed_param(name: &str, ty: TypeRef) -> Param {
        Param {
            name: name.to_string(),
            type_ref: ty,
            optional: true,
            variadic: false,
            slice_mode: None,
        }
    }

    fn variadic_param(name: &str) -> Param {
        Param {
            name: name.to_string(),
            type_ref: TypeRef::Any,
            optional: false,
            variadic: true,
            slice_mode: None,
        }
    }

    #[test]
    fn qualified_reference_suffix_uses_the_full_path() {
        let qualified = |path: &str| TypeRef::Reference {
            segments: path.split('.').map(String::from).collect(),
            generic_args: vec![],
        };
        assert_eq!(type_snake_name(&qualified("A.X")), "a_x");
        assert_eq!(
            type_snake_name(&qualified("NodeJS.TypedArray")),
            "node_js_typed_array"
        );
        assert_eq!(type_snake_name(&qualified("Array")), "slice");
    }

    #[test]
    fn public_rust_names_suffix_keywords() {
        assert_eq!(public_rust_name("type"), "type_");
        assert_eq!(public_rust_name("use"), "use_");
        assert_eq!(base_rust_name("type", SignatureKind::Setter), "set_type");
        assert_eq!(public_rust_name("type_with_null"), "type_with_null");

        let mut used = no_used();
        let sigs = expand(
            "do",
            &[param("name"), opt_param("config")],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        let non_try: Vec<_> = sigs.iter().filter(|s| !s.catch).collect();
        assert_eq!(non_try[0].rust_name, "do_");
        assert_eq!(non_try[1].rust_name, "do_with_config");
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
            &TypeRef::ident("Console"),
            SignatureKind::Constructor,
            &None,
            &mut used,
        );
        // Constructor: only 1 signature, catches by default, no try_ variant
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
            &TypeRef::generic("Promise", vec![TypeRef::String]),
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2, "async primary plus its `_promise` form");
        assert_eq!(sigs[0].rust_name, "foo");
        assert!(sigs[0].is_async, "promise method must be async");
        assert!(sigs[0].catch, "async promise method must catch");
        // Return type is unwrapped from Promise<T> — wasm-bindgen rewraps as a
        // Promise on the JS side automatically when `async fn` is used.
        assert_eq!(sigs[0].return_type, TypeRef::String);
        // The sync form hands back the promise itself, un-unwrapped, and
        // does not catch under the default policy.
        assert_eq!(sigs[1].rust_name, "foo_promise");
        assert!(!sigs[1].is_async);
        assert!(!sigs[1].catch);
        assert_eq!(
            sigs[1].return_type,
            TypeRef::generic("Promise", vec![TypeRef::String])
        );
    }

    #[test]
    fn test_promise_function_collapses_to_async() {
        let mut used = no_used();
        let sigs = expand(
            "fetch",
            &[param("url")],
            &TypeRef::generic("Promise", vec![TypeRef::ident("Response")]),
            SignatureKind::Function,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "fetch");
        assert!(sigs[0].is_async);
        assert!(sigs[0].catch);
        assert_eq!(sigs[0].return_type, TypeRef::ident("Response"));
        assert_eq!(sigs[1].rust_name, "fetch_promise");
    }

    #[test]
    fn test_promise_static_method_collapses_to_async() {
        let mut used = no_used();
        let sigs = expand(
            "all",
            &[param("promises")],
            &TypeRef::generic("Promise", vec![TypeRef::Any]),
            SignatureKind::StaticMethod,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2);
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
                &[typed_param("message", TypeRef::ident("EmailMessage"))],
                &[typed_param("builder", TypeRef::Object)],
            ],
            &TypeRef::generic("Promise", vec![TypeRef::ident("EmailSendResult")]),
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(
            sigs.len(),
            4,
            "two overloads → two async sigs + two `_promise`"
        );
        for sig in &sigs {
            assert!(
                !sig.rust_name.starts_with("try_"),
                "no try_ variant expected for promise overloads, got {}",
                sig.rust_name,
            );
        }
        for sig in sigs.iter().filter(|s| s.is_async) {
            assert!(sig.catch, "{} should catch", sig.rust_name);
        }
        // Names should disambiguate via the standard `_with_<param>` suffix;
        // the promise form appends to the disambiguated name.
        assert_eq!(sigs[0].rust_name, "send");
        assert_eq!(sigs[1].rust_name, "send_promise");
        assert_eq!(sigs[2].rust_name, "send_with_builder");
        assert_eq!(sigs[3].rust_name, "send_with_builder_promise");
    }

    #[test]
    fn overloads_keep_their_own_return_type_and_scope() {
        let (mut gctx, outer) = test_ctx();
        let inner = gctx.create_child_scope(outer);
        let cgctx = CodegenContext::empty(&gctx, outer);
        let promise = TypeRef::generic("Promise", vec![TypeRef::String]);
        let by_string = [typed_param("x", TypeRef::String)];
        let by_number = [typed_param("x", TypeRef::Number)];
        let sigs = build_signatures(
            &CallableSpec {
                js_name: "f",
                rust_name: None,
                kind: SignatureKind::Function,
                overloads: &[
                    Overload {
                        params: &by_string,
                        return_type: &promise,
                        body_scope: outer,
                        throws: &Throws::None,
                        doc: &None,
                    },
                    Overload {
                        params: &by_number,
                        return_type: &TypeRef::Number,
                        body_scope: inner,
                        throws: &Throws::None,
                        doc: &None,
                    },
                ],
            },
            &mut no_used(),
            Some(&cgctx),
        );
        let summary: Vec<_> = sigs
            .iter()
            .map(|s| {
                (
                    s.rust_name.as_str(),
                    s.is_async,
                    &s.return_type,
                    s.body_scope,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("f", true, &TypeRef::String, outer),
                ("f_promise", false, &promise, outer),
                ("f_with_f64", false, &TypeRef::Number, inner),
                ("try_f_with_f64", false, &TypeRef::Number, inner),
            ]
        );
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
                TypeRef::generic("Promise", vec![TypeRef::String]),
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
            &TypeRef::ident("Console"),
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

    /// `(name, type)` of each param of the signature owning the bare `js`
    /// name, across `overloads` of one method.
    fn primary_params(js: &str, overloads: &[&[Param]]) -> Vec<(String, TypeRef)> {
        let sigs = expand_overloads(
            js,
            overloads,
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut no_used(),
        );
        let primary = sigs.iter().find(|s| s.rust_name == js).unwrap();
        primary
            .params
            .iter()
            .map(|p| (p.name.clone(), p.type_ref.clone()))
            .collect()
    }

    fn named(params: &[(&str, TypeRef)]) -> Vec<(String, TypeRef)> {
        params
            .iter()
            .map(|(name, ty)| (name.to_string(), ty.clone()))
            .collect()
    }

    #[test]
    fn primary_is_shorter_prefix_of_required_options_overload() {
        let primary = primary_params(
            "get",
            &[
                &[
                    string_param("key"),
                    typed_param("options", TypeRef::ident("Conditional")),
                ],
                &[
                    string_param("key"),
                    opt_typed_param("options", TypeRef::ident("GetOptions")),
                ],
            ],
        );
        assert_eq!(primary, named(&[("key", TypeRef::String)]));
    }

    #[test]
    fn primary_is_later_zero_argument_overload() {
        let primary = primary_params("first", &[&[string_param("column")], &[]]);
        assert_eq!(primary, named(&[]));
    }

    #[test]
    fn primary_stays_with_first_overload_on_different_leading_names() {
        let primary = primary_params(
            "put",
            &[
                &[
                    string_param("key"),
                    string_param("value"),
                    opt_param("options"),
                ],
                &[param("entries"), opt_param("options")],
            ],
        );
        assert_eq!(
            primary,
            named(&[("key", TypeRef::String), ("value", TypeRef::String)])
        );
    }

    #[test]
    fn primary_stays_with_first_overload_on_different_leading_types() {
        let opts = TypeRef::ident("Options");
        let primary = primary_params(
            "find",
            &[
                &[string_param("key"), typed_param("opts", opts.clone())],
                &[typed_param("key", TypeRef::Number)],
            ],
        );
        assert_eq!(primary, named(&[("key", TypeRef::String), ("opts", opts)]));
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
    fn test_throws_never_suppresses_try_variant() {
        // `@throws {never}` on a sync method: only the primary signature is
        // emitted — no `try_count` companion. The primary stays non-catching.
        let mut used = no_used();
        let sigs = expand_throws(
            "count",
            &[],
            &TypeRef::Void,
            SignatureKind::Method,
            &Throws::Never,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 1, "nothrow methods get no try_ companion");
        assert_eq!(sigs[0].rust_name, "count");
        assert!(!sigs[0].catch);
        assert!(!sigs[0].is_async);
    }

    #[test]
    fn test_throws_never_suppresses_try_for_function() {
        let mut used = no_used();
        let sigs = expand_throws(
            "id",
            &[param("x")],
            &TypeRef::Any,
            SignatureKind::Function,
            &Throws::Never,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].rust_name, "id");
        assert!(!sigs[0].catch);
    }

    #[test]
    fn test_throws_never_async_drops_catch() {
        // `@throws {never}` on a Promise-returning method: emit
        // `async fn -> T` (no `Result`, no `catch`).
        let mut used = no_used();
        let sigs = expand_throws(
            "loaded",
            &[],
            &TypeRef::generic("Promise", vec![TypeRef::String]),
            SignatureKind::Method,
            &Throws::Never,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "loaded");
        assert!(sigs[0].is_async);
        assert!(
            !sigs[0].catch,
            "nothrow async drops catch — return type is T directly"
        );
        assert!(sigs[0].error_type.is_none());
        assert_eq!(sigs[0].return_type, TypeRef::String);
        assert_eq!(sigs[1].rust_name, "loaded_promise");
        assert!(!sigs[1].catch);
    }

    #[test]
    fn test_throws_never_optional_method_no_try_pairs() {
        // Optional params still expand the parameter axis, but each
        // expansion has no `try_` sibling.
        let mut used = no_used();
        let sigs = expand_throws(
            "count",
            &[opt_param("label")],
            &TypeRef::Void,
            SignatureKind::Method,
            &Throws::Never,
            &None,
            &mut used,
        );
        // Two expansions × 1 (no try_) = 2
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "count");
        assert_eq!(sigs[1].rust_name, "count_with_label");
        assert!(sigs.iter().all(|s| !s.catch));
        assert!(sigs.iter().all(|s| !s.rust_name.starts_with("try_")));
    }

    #[test]
    fn test_throws_never_constructor_is_plain() {
        let mut used = no_used();
        let sigs = expand_throws(
            "Foo",
            &[param("x")],
            &TypeRef::ident("Foo"),
            SignatureKind::Constructor,
            &Throws::Never,
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].rust_name, "new");
        assert!(!sigs[0].catch);
        assert!(sigs[0].error_type.is_none());
        assert!(!sigs[0].is_async);
    }

    #[test]
    fn test_throws_typed_keeps_try_pair() {
        // Typed `Throws::Type(...)` behaves like the existing `@throws {T}`
        // — sync still gets the `try_` companion, with the typed error.
        let err = TypeRef::ident("ImagesError");
        let mut used = no_used();
        let sigs = expand_throws(
            "upload",
            &[param("file")],
            &TypeRef::Void,
            SignatureKind::Method,
            &Throws::Type(err.clone()),
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].rust_name, "upload");
        assert!(!sigs[0].catch);
        assert_eq!(sigs[1].rust_name, "try_upload");
        assert!(sigs[1].catch);
        assert_eq!(sigs[1].error_type.as_ref(), Some(&err));
    }

    #[test]
    fn test_throws_typed_async_carries_error_type() {
        let err = TypeRef::ident("ImagesError");
        let mut used = no_used();
        let sigs = expand_throws(
            "upload",
            &[],
            &TypeRef::generic("Promise", vec![TypeRef::String]),
            SignatureKind::Method,
            &Throws::Type(err.clone()),
            &None,
            &mut used,
        );
        assert_eq!(sigs.len(), 2);
        assert!(sigs[0].is_async);
        assert!(sigs[0].catch);
        assert_eq!(sigs[0].error_type.as_ref(), Some(&err));
        // Default policy: the sync promise form does not catch, so the
        // typed error does not apply to it.
        assert_eq!(sigs[1].rust_name, "upload_promise");
        assert!(!sigs[1].catch);
        assert!(sigs[1].error_type.is_none());
    }

    #[test]
    fn test_try_collision_deduped() {
        // If "try_count" is already taken, the try_ variant gets a numeric suffix.
        let mut used = LocalScope::with_claimed(["try_count"]);
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
        assert_eq!(sigs[1].rust_name, "try_count_2");
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
        assert_eq!(sigs2[0].rust_name, "foo_2");
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
        // foo and foo_with_promise.
        let mut used = no_used();
        let overload1 = [typed_param("x", TypeRef::String)];
        let overload2 = [typed_param(
            "x",
            TypeRef::generic("Promise", vec![TypeRef::String]),
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
        assert_eq!(non_try[0].rust_name, "foo");
        assert_eq!(non_try[1].rust_name, "foo_with_promise");
    }

    fn lit(name: &str, value: &str) -> Param {
        typed_param(name, TypeRef::StringLiteral(value.to_string()))
    }

    #[test]
    fn literal_only_overloads_form_a_cohort_per_return_type() {
        let (gctx, scope) = test_ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        let mut used = no_used();
        let plain = [typed_param("key", TypeRef::String)];
        let text = [typed_param("key", TypeRef::String), lit("type", "text")];
        let json = [typed_param("key", TypeRef::String), lit("type", "json")];
        let buf = [
            typed_param("key", TypeRef::String),
            lit("type", "arrayBuffer"),
        ];
        let keys = [
            typed_param("key", TypeRef::Array(Box::new(TypeRef::String))),
            lit("type", "text"),
        ];
        let returns = [
            TypeRef::String,
            TypeRef::String,
            TypeRef::Any,
            TypeRef::String,
            TypeRef::Any,
        ];
        let params: [&[Param]; 5] = [&plain, &text, &json, &buf, &keys];
        let overloads: Vec<Overload> = params
            .iter()
            .zip(&returns)
            .map(|(params, return_type)| Overload {
                params,
                return_type,
                body_scope: scope,
                throws: &Throws::Never,
                doc: &None,
            })
            .collect();
        let sigs = build_signatures(
            &CallableSpec {
                js_name: "get",
                rust_name: None,
                kind: SignatureKind::Method,
                overloads: &overloads,
            },
            &mut used,
            Some(&cgctx),
        );
        let names: Vec<&str> = sigs.iter().map(|s| s.rust_name.as_str()).collect();
        // `text` and `arrayBuffer` share a return and so one extern; `json`
        // gets its own. The `string[]` form has a single literal arm, so it
        // is not a cohort and keeps the plain `&str` binding.
        assert_eq!(
            names,
            ["get", "get_text", "get_json", "get_with_slice_and_type"]
        );
        assert!(sigs[0].literal.is_none());
        let text_spec = sigs[1].literal.as_ref().unwrap();
        assert_eq!(text_spec.positions, [1]);
        assert_eq!(
            text_spec.arms,
            [
                (vec!["text".to_string()], "get_text".to_string()),
                (
                    vec!["arrayBuffer".to_string()],
                    "get_array_buffer".to_string()
                ),
            ]
        );
        assert_eq!(sigs[1].params[1].type_ref, TypeRef::String);
        assert_eq!(sigs[2].return_type, TypeRef::Any);
        assert!(sigs[3].literal.is_none());
    }

    #[test]
    fn lone_literal_and_absorbed_plain_string_stay_a_str_binding() {
        let mut used = no_used();
        let only = [lit("name", "Set-Cookie")];
        let sigs = expand_overloads(
            "getAll",
            &[&only],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs[0].rust_name, "get_all");
        assert!(sigs[0].literal.is_none());

        let mut used = no_used();
        let a = [lit("mode", "a")];
        let b = [lit("mode", "b")];
        let any = [typed_param("mode", TypeRef::String)];
        let sigs = expand_overloads(
            "setMode",
            &[&a, &b, &any],
            &TypeRef::Void,
            SignatureKind::Method,
            &None,
            &mut used,
        );
        assert_eq!(sigs[0].rust_name, "set_mode");
        assert!(sigs[0].literal.is_none());

        let mut used = no_used();
        let sigs = expand_overloads(
            "disposition",
            &[&[lit("val", "inline")], &[lit("val", "attachment")]],
            &TypeRef::Void,
            SignatureKind::Setter,
            &None,
            &mut used,
        );
        assert_eq!(sigs[0].rust_name, "set_disposition");
        assert!(sigs[0].literal.is_none());
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

    // Generic containers are opaque to signature flattening.

    fn flatten_no_ctx(ty: &TypeRef) -> Vec<TypeRef> {
        let (gctx, scope) = test_ctx();
        let cgctx = CodegenContext::empty(&gctx, scope);
        flatten_type(ty, Some(&cgctx), scope)
    }

    #[test]
    fn typed_array_pairs_with_its_element_array() {
        use crate::ir::TypedArrayElement as E;
        let array = |elem: TypeRef| TypeRef::Array(Box::new(elem));
        let flat = |members: Vec<TypeRef>| flatten_no_ctx(&TypeRef::Union(members));
        assert_eq!(
            flat(vec![TypeRef::ident("Uint8Array"), array(TypeRef::Number)]),
            vec![
                TypeRef::TypedSlice(E::U8, SharedMemory::Accepted),
                TypeRef::ident("Uint8Array")
            ]
        );
        assert_eq!(
            flat(vec![
                TypeRef::String,
                array(TypeRef::BigInt),
                TypeRef::generic("BigInt64Array", vec![TypeRef::ident("ArrayBuffer")]),
            ]),
            vec![
                TypeRef::TypedSlice(E::I64, SharedMemory::Rejected),
                TypeRef::String,
                TypeRef::generic("BigInt64Array", vec![TypeRef::ident("ArrayBuffer")]),
            ]
        );
        assert_eq!(
            flat(vec![
                TypeRef::ident("Uint8ClampedArray"),
                array(TypeRef::Number)
            ]),
            vec![
                TypeRef::TypedSlice(E::U8Clamped, SharedMemory::Accepted),
                TypeRef::ident("Uint8ClampedArray")
            ]
        );
        assert_eq!(
            flat(vec![
                TypeRef::ident("Uint8ClampedArray"),
                TypeRef::generic("Uint8Array", vec![TypeRef::ident("ArrayBuffer")]),
                array(TypeRef::Number),
            ]),
            vec![
                TypeRef::TypedSlice(E::U8, SharedMemory::Rejected),
                TypeRef::ident("Uint8ClampedArray"),
                TypeRef::generic("Uint8Array", vec![TypeRef::ident("ArrayBuffer")]),
            ]
        );
        let bytes = TypeRef::Union(vec![TypeRef::ident("Uint8Array"), array(TypeRef::Number)]);
        let nullable = |n| flatten_no_ctx(&TypeRef::Nullable(Box::new(bytes.clone()), n));
        let view = TypeRef::TypedSlice(E::U8, SharedMemory::Accepted);
        let handle = |n| TypeRef::Nullable(Box::new(TypeRef::ident("Uint8Array")), n);
        assert_eq!(
            nullable(Nullability::NULL),
            vec![view.clone(), handle(Nullability::NULL)]
        );
        for n in [Nullability::UNDEFINED, Nullability::BOTH] {
            assert_eq!(
                nullable(n),
                vec![
                    TypeRef::Nullable(Box::new(view.clone()), Nullability::UNDEFINED),
                    handle(n)
                ]
            );
        }
        let clamped = TypeRef::Union(vec![
            TypeRef::ident("Uint8ClampedArray"),
            array(TypeRef::Number),
        ]);
        assert_eq!(
            flatten_no_ctx(&TypeRef::Nullable(
                Box::new(clamped),
                Nullability::UNDEFINED
            )),
            vec![
                TypeRef::TypedSlice(E::U8Clamped, SharedMemory::Accepted),
                TypeRef::Nullable(
                    Box::new(TypeRef::ident("Uint8ClampedArray")),
                    Nullability::UNDEFINED
                ),
            ]
        );
        for unpaired in [
            vec![TypeRef::ident("Uint8Array"), array(TypeRef::BigInt)],
            vec![TypeRef::ident("BigInt64Array"), array(TypeRef::Number)],
            vec![
                TypeRef::ident("Uint8Array"),
                TypeRef::ident("Int8Array"),
                array(TypeRef::Number),
            ],
        ] {
            assert_eq!(flat(unpaired.clone()), unpaired);
        }
    }

    #[test]
    fn record_with_mixed_primitive_union_does_not_distribute() {
        let union_v = TypeRef::Union(vec![TypeRef::String, TypeRef::Number, TypeRef::Boolean]);
        let ty = TypeRef::generic("Record", vec![TypeRef::String, union_v.clone()]);
        let alts = flatten_no_ctx(&ty);
        assert_eq!(alts.len(), 1);
        match alts[0].as_generic_head("Record") {
            Some([_, v]) => assert_eq!(*v, union_v),
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_with_union_key_does_not_distribute() {
        let union_k = TypeRef::Union(vec![TypeRef::String, TypeRef::Number]);
        let ty = TypeRef::generic("Record", vec![union_k.clone(), TypeRef::String]);
        let alts = flatten_no_ctx(&ty);
        assert_eq!(alts.len(), 1);
        match alts[0].as_generic_head("Record") {
            Some([k, _]) => assert_eq!(*k, union_k),
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn record_with_single_typed_value_keeps_typed_phantom() {
        let ty = TypeRef::generic("Record", vec![TypeRef::String, TypeRef::String]);
        let alts = flatten_no_ctx(&ty);
        assert_eq!(alts.len(), 1);
        match alts[0].as_generic_head("Record") {
            Some([_, v]) => assert_eq!(*v, TypeRef::String),
            other => panic!("expected Record, got {other:?}"),
        }
    }

    #[test]
    fn array_with_mixed_primitive_union_does_not_distribute() {
        let union_v = TypeRef::Union(vec![TypeRef::String, TypeRef::Number]);
        let ty = TypeRef::Array(Box::new(union_v.clone()));
        let alts = flatten_no_ctx(&ty);
        assert_eq!(alts.len(), 1);
        match &alts[0] {
            TypeRef::Array(inner) => assert_eq!(**inner, union_v),
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn map_with_union_type_args_does_not_distribute() {
        let k = TypeRef::Union(vec![TypeRef::String, TypeRef::Number]);
        let v = TypeRef::Union(vec![TypeRef::String, TypeRef::Boolean]);
        let ty = TypeRef::generic("Map", vec![k.clone(), v.clone()]);
        let alts = flatten_no_ctx(&ty);
        assert_eq!(alts.len(), 1);
        match alts[0].as_generic_head("Map") {
            Some([map_k, map_v]) => {
                assert_eq!(*map_k, k);
                assert_eq!(*map_v, v);
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    #[test]
    fn promise_with_union_does_not_distribute() {
        let union_v = TypeRef::Union(vec![TypeRef::String, TypeRef::Number]);
        let ty = TypeRef::generic("Promise", vec![union_v.clone()]);
        let alts = flatten_no_ctx(&ty);
        assert_eq!(alts.len(), 1);
        match alts[0].as_promise_inner() {
            Some(inner) => assert_eq!(*inner, union_v),
            None => panic!("expected Promise, got {:?}", alts[0]),
        }
    }
}
