//! Callables bound from their IR overloads.
//!
//! The producer expands a callable's overloads, so the Rust signature
//! pipeline takes them from the IR: each IR overload is one expansion,
//! named apart by the Rust `_with_` vocabulary. A callable stays on the
//! parse when the IR doesn't yet carry what its Rust form reads.

use crate::codegen::signatures::{
    build_signatures, build_signatures_from, expand_signatures, CallableSpec, ConcreteParam,
    ExpandedSignature, FunctionSignature, LiteralArm, Overload,
};
use crate::codegen::typemap::CodegenContext;
use crate::ir::{Throws, TypeRef};
use crate::parse::scope::ScopeId;
use crate::util::naming::LocalScope;

/// Which members of a container are the callable's overloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Function,
    /// A top-level getter.
    Variable,
    Method,
    Static,
    Constructor,
    /// A constructor `new`ing the JS value at the path `js_name` names
    /// (`@js(new = Wrap)`), on the type it returns.
    Value,
    Get,
    StaticGet,
    Set,
    StaticSet,
}

/// Where a callable's IR overloads are declared.
#[derive(Clone, Copy)]
pub struct Site<'a> {
    pub module: Option<&'a str>,
    /// The enclosing namespaces, then the class for a member.
    pub path: &'a [String],
    pub form: Form,
}

/// Where the zip takes the parse's type for the IR's beyond naming it:
/// each is a reading emitting from the IR changes.
#[derive(Default)]
pub struct Zip {
    /// Of the site being restated, dropped unless it is.
    pending: std::cell::RefCell<Vec<String>>,
    pub diffs: std::cell::RefCell<Vec<String>>,
}

fn mark(cgctx: &CodegenContext<'_>) -> usize {
    cgctx.zip.as_ref().map_or(0, |z| z.pending.borrow().len())
}

fn unmark(cgctx: &CodegenContext<'_>, mark: usize) {
    if let Some(z) = &cgctx.zip {
        z.pending.borrow_mut().truncate(mark);
    }
}

fn pending(cgctx: &CodegenContext<'_>, reading: String) {
    if let Some(z) = &cgctx.zip {
        z.pending.borrow_mut().push(reading);
    }
}

/// `f`, whose readings are a trial.
fn quiet<T>(cgctx: &CodegenContext<'_>, f: impl FnOnce() -> T) -> T {
    let at = mark(cgctx);
    let r = f();
    unmark(cgctx, at);
    r
}

/// `r`, committing the readings since `at` to `site`'s `js_name` when it
/// restates.
fn settled<T>(
    cgctx: &CodegenContext<'_>,
    at: usize,
    site: &Site<'_>,
    js_name: &str,
    r: Option<T>,
) -> Option<T> {
    if let Some(z) = &cgctx.zip {
        let pending: Vec<String> = z.pending.borrow_mut().drain(at..).collect();
        if r.is_some() {
            let module = site.module.map(|m| format!("{m}:")).unwrap_or_default();
            let path = [site.path.join("."), js_name.to_string()].join(".");
            let mut diffs = z.diffs.borrow_mut();
            for n in pending {
                diffs.push(format!("{module}{path} ({:?}) {n}", site.form));
            }
        }
    }
    r
}

/// [`build_signatures`], with the overloads from the IR at `site` when it
/// states every one.
pub fn build(
    spec: &CallableSpec<'_>,
    site: Option<Site<'_>>,
    names: &mut LocalScope,
    cgctx: Option<&CodegenContext<'_>>,
) -> Vec<FunctionSignature> {
    let (cgctx, site) = match (cgctx, site) {
        (Some(c), Some(site)) if c.ir.is_some() => (c, site),
        // No IR declares it: the parse's reading (a model without IR).
        (cgctx, _) => return build_signatures(spec, names, cgctx),
    };
    let Some((ir, scopes, expansions)) = overloads(cgctx, spec, &site) else {
        let at = [site.path, &[spec.js_name.to_string()]].concat().join(".");
        let module = site.module.map(|m| format!("{m}:")).unwrap_or_default();
        cgctx.warn(format!(
            "`{module}{at}`: the IR doesn't state the parse's reading; skipped"
        ));
        return Vec::new();
    };
    let cgctx = Some(cgctx);
    let first = &spec.overloads[0];
    let overloads: Vec<Overload> = ir
        .iter()
        .zip(scopes)
        .map(|(o, body_scope)| Overload {
            params: &o.params,
            // A constructor's result is its class, which the IR leaves out.
            return_type: match spec.kind {
                crate::codegen::signatures::SignatureKind::Constructor => first.return_type,
                _ => &o.return_type,
            },
            body_scope,
            throws: &o.throws,
            doc: &o.doc,
        })
        .collect();
    build_signatures_from(
        &CallableSpec {
            overloads: &overloads,
            ..*spec
        },
        expansions,
        names,
        cgctx,
    )
}

type Restated = (
    Vec<super::ir_types::Overload>,
    Vec<ScopeId>,
    Vec<ExpandedSignature>,
);

/// The IR overloads at `site`, each with the scope of the parse overload it
/// restates, and the parse's expansions restated over them, when the
/// parse's reading of the callable is one the IR carries.
///
/// The Rust forms (the `_with_` names, literal-keyed cohorts, `null` arms,
/// slice hints) are still the parse's expansions; each of their calls
/// must be exactly one IR overload, whose types it takes, and what it
/// throws: a named throws enum is typed, `err` untyped. `@throws {never}`
/// is the parse's.
fn overloads(
    cgctx: &CodegenContext<'_>,
    spec: &CallableSpec<'_>,
    site: &Site<'_>,
) -> Option<Restated> {
    let at = mark(cgctx);
    let r = restated(cgctx, spec, site);
    settled(cgctx, at, site, spec.js_name, r)
}

fn restated(
    cgctx: &CodegenContext<'_>,
    spec: &CallableSpec<'_>,
    site: &Site<'_>,
) -> Option<Restated> {
    let ir = cgctx.ir.as_ref()?;
    let first = spec.overloads.first()?;
    let scope = first.body_scope;
    let legacy = expand_signatures(spec.overloads, spec.kind, Some(cgctx));
    let bound: Vec<ScopeId> = spec.overloads.iter().map(|o| o.body_scope).collect();
    let mut overloads = ir.callable(cgctx.gctx, site, spec.js_name, scope, &bound)?;
    // Each overload's typed array parameters, beside which Rust reads an
    // element array as a view of one.
    let typed: Vec<Vec<Option<crate::ir::TypedArrayElement>>> = overloads
        .iter()
        .map(|o| {
            o.params
                .iter()
                .map(|p| match &p.type_ref {
                    TypeRef::Reference { segments, .. } if segments.len() == 1 => {
                        crate::ir::TypedArrayElement::of_typed_array(&segments[0])
                    }
                    _ => None,
                })
                .collect()
        })
        .collect();
    let mut used = vec![false; overloads.len()];
    let mut scopes = vec![scope; overloads.len()];
    let mut covered = Vec::new();
    let mut expansions = Vec::with_capacity(legacy.len());
    let mut collapsed = false;
    'legacy: for e in legacy {
        let arms: Vec<Option<&LiteralArm>> = match e.literal_arms.len() {
            0 | 1 => vec![None],
            _ => e.literal_arms.iter().map(Some).collect(),
        };
        let mut matched = Vec::with_capacity(arms.len());
        // The IR's enums of literals, which Rust takes whole where the
        // parse expands an arm per literal.
        let mut enums: Vec<(usize, usize, TypeRef)> = Vec::new();
        // An expansion's types for the parameters of an enum it takes one
        // arm of.
        let mut arm_types = Vec::new();
        // The parameters the parse takes whole where the IR expands them,
        // with the IR's arms.
        let mut splits: Vec<(usize, Vec<TypeRef>)> = Vec::new();
        for arm in arms {
            let args = legacy_args(&e.params, arm);
            let source = &spec.overloads[arm.map_or(e.overload, |a| a.overload)];
            let parsed = source.return_type;
            // Each overload of merged declarations resolves in its own.
            let scope = source.body_scope;
            let Some((i, merged)) = quiet(cgctx, || {
                matching(cgctx, scope, &overloads, &e.params, &args, &used)
            }) else {
                // A `null` arm of `T | null | undefined`, which the IR reads
                // as `T?`, writing `undefined` for either.
                if args.iter().any(|a| matches!(a, LegacyArg::Null)) {
                    pending(cgctx, "null arm the IR collapses into `undefined`".into());
                    continue 'legacy;
                }
                return None;
            };
            covered.extend(merged);
            scopes[i] = scope;
            let o = &mut overloads[i];
            if source.throws.is_never() && matches!(o.throws, Throws::None) {
                o.throws = Throws::Never;
            }
            // Rust erases a type parameter no argument infers, to its default.
            let mut erased: Vec<(String, TypeRef)> = Vec::new();
            if !o.tparams.is_empty() {
                let mut kept = Vec::new();
                for t in e.params.iter().map(|p| &p.type_ref).chain([parsed]) {
                    crate::codegen::signatures::collect_type_params(t, cgctx, scope, &mut kept);
                }
                let (tparams, defaults) = (
                    std::mem::take(&mut o.tparams),
                    std::mem::take(&mut o.defaults),
                );
                for (t, d) in tparams.into_iter().zip(defaults) {
                    match kept.contains(&t) {
                        true => {
                            o.tparams.push(t);
                            o.defaults.push(d);
                        }
                        false => erased.push((t, d.unwrap_or(TypeRef::Any))),
                    }
                }
            }
            let default = |t: &TypeRef| match t {
                TypeRef::Reference {
                    segments,
                    generic_args,
                } if segments.len() == 1 && generic_args.is_empty() => erased
                    .iter()
                    .find(|(n, _)| *n == segments[0])
                    .map(|(_, d)| d.clone()),
                _ => None,
            };
            let named = |t: &TypeRef| default(t).is_some();
            let erase = |ours: &TypeRef, _: &TypeRef| erased_to(cgctx, scope, ours, &default);
            let native = erase;
            state(cgctx, scope, &erase(&o.return_type, parsed), parsed)?;
            o.return_type = emit(cgctx, scope, &native(&o.return_type, parsed), parsed);
            for (j, p) in o.params.iter().enumerate() {
                let declared = source.params.iter().find(|d| d.name == p.name);
                if let Some(t) = declared.and_then(|d| literal_enum(&p.type_ref, &d.type_ref)) {
                    if o.whole[j] && !enums.iter().any(|e| (e.0, e.1) == (i, j)) {
                        enums.push((i, j, t));
                    }
                }
                // A generic alias's instance the IR takes whole as its enum.
                let core = |t: &TypeRef| t.split_empties().map_or(t.clone(), |(c, _)| c);
                if let (true, Some(d), Some((name, []))) = (
                    o.whole[j],
                    declared,
                    super::ir_types::synthesised(&core(&p.type_ref)),
                ) {
                    let d = core(&d.type_ref);
                    if matches!(&d, TypeRef::Reference { generic_args, .. } if !generic_args.is_empty())
                        && !enums.iter().any(|e| (e.0, e.1) == (i, j))
                        && cgctx.take_alias_instance(&d, scope, name)
                    {
                        enums.push((i, j, d));
                    }
                }
            }
            for (ours, theirs) in o.args.iter().zip(&args) {
                if let (super::ir_types::Arg::Param(j), LegacyArg::Param(k)) = (ours, theirs) {
                    if enums.iter().any(|e| (e.0, e.1) == (i, *j)) {
                        continue;
                    }
                    let theirs = &e.params[*k].type_ref;
                    let mut ours = erase(&o.params[*j].type_ref, theirs);
                    let mut t = state(cgctx, scope, &ours, theirs);
                    // A `T?` the parse writes as `T`, passing fewer states:
                    // the IR's `T | null | undefined` beside the overload
                    // omitting it.
                    if t.is_none()
                        && ours
                            .split_empties()
                            .is_some_and(|(_, n)| n == crate::ir::Nullability::Undefined)
                    {
                        let plain = ours.without_undefined();
                        if let Some(stated) = state(cgctx, scope, &plain, theirs) {
                            o.params[*j].type_ref = o.params[*j].type_ref.without_undefined();
                            (ours, t) = (plain, Some(stated));
                        }
                    }
                    // Whole or expanded, as `Overload::whole` says; the parse decides
                    // alike unless listed.
                    if let Some(members) = &o.members[*j] {
                        let declared = source.params.iter().find(|d| d.name == o.params[*j].name);
                        let core = |t: &TypeRef| t.split_empties().map_or(t.clone(), |(c, _)| c);
                        // An arm's type, or one the enum's doesn't state.
                        let expands = t.as_ref() != Some(theirs)
                            || members.iter().any(|m| {
                                quiet(cgctx, || state(cgctx, scope, m, theirs)).as_ref()
                                    == Some(theirs)
                            });
                        let sentinel = |t: &TypeRef| {
                            super::ir_types::declared(t).is_some()
                                || super::ir_types::synthesised(t).is_some()
                        };
                        // An anonymous union of values is expanded
                        // (`PropertyKey` too), as only a named one is one
                        // Rust type.
                        let anonymous = declared.is_some_and(|d| {
                            !matches!(core(&d.type_ref), TypeRef::Reference { .. })
                        });
                        let whole = o.whole[*j] && !anonymous;
                        // The parse's type undivided, not an arm it narrowed to.
                        let undivided = declared.is_some_and(|d| d.type_ref == *theirs);
                        let split = !whole
                            && !expands
                            && arm.is_none()
                            && undivided
                            && members
                                .iter()
                                .all(|m| !m.admits_undefined() && !mentions(m, &sentinel));
                        if split {
                            splits.push((*k, members.clone()));
                        } else if whole == expands && (expands || undivided) {
                            let (r12, parse) = match expands {
                                true => ("whole", "expand"),
                                false => ("expand", "whole"),
                            };
                            let name = &o.params[*j].name;
                            pending(
                                cgctx,
                                format!(
                                    "r12: `{name}` {r12}, parse {parse}: {}",
                                    theirs.format_ts()
                                ),
                            );
                        }
                    }
                    match &o.members[*j] {
                        // The parse takes the enum whole, or one arm of it.
                        Some(members) if t.as_ref() != Some(theirs) => {
                            let m = members.iter().find(|m| {
                                quiet(cgctx, || state(cgctx, scope, m, theirs)).as_ref()
                                    == Some(theirs)
                            })?;
                            if matched.is_empty() {
                                let t = match (m, theirs) {
                                    // The array beside a typed array, which Rust
                                    // reads as a view of it.
                                    (TypeRef::Array(_), TypeRef::TypedSlice(e, _))
                                        if members.iter().any(|m| matches!(m,
                                            TypeRef::Reference { segments, .. } if segments.len() == 1
                                                && crate::ir::TypedArrayElement::of_typed_array(&segments[0]) == Some(*e))) =>
                                    {
                                        theirs.clone()
                                    }
                                    _ => emit(cgctx, scope, m, theirs),
                                };
                                arm_types.push((*j, t));
                            }
                        }
                        // The array beside a typed array, which Rust reads
                        // as a view of it.
                        _ if matches!((&o.params[*j].type_ref, theirs), (TypeRef::Array(_), TypeRef::TypedSlice(e, _))
                            if typed.iter().enumerate().any(|(k, ps)| k != i && ps.get(*j) == Some(&Some(*e)))) =>
                        {
                            t?;
                            o.params[*j].type_ref = theirs.clone();
                        }
                        _ => {
                            t?;
                            let ours = inferred_native(
                                cgctx,
                                scope,
                                &native(&o.params[*j].type_ref, theirs),
                            );
                            let t = emit(cgctx, scope, &ours, theirs);
                            let at = [site.path, &[spec.js_name.to_string()]].concat().join(".");
                            let at = format!("`{at}` parameter `{}`", o.params[*j].name);
                            o.params[*j].type_ref = writes(cgctx, scope, &at, &t, theirs);
                        }
                    }
                }
            }
            if std::iter::once(&o.return_type)
                .chain(o.params.iter().map(|p| &p.type_ref))
                .any(|t| mentions(t, &named))
            {
                return None;
            }
            used[i] = true;
            matched.push(i);
        }
        let o = &overloads[matched[0]];
        let param = |k: &usize| match o.args.get(*k) {
            Some(super::ir_types::Arg::Param(j)) => Some(*j),
            _ => None,
        };
        let mut enums: Vec<(usize, TypeRef)> = enums
            .into_iter()
            .filter(|e| e.0 == matched[0])
            .map(|(_, j, t)| (j, t))
            .collect();
        // A cohort the IR takes as one enum is one binding; one it splits
        // across overloads keeps its wrappers, passing literals.
        let positions: Vec<usize> = e
            .literal_arms
            .first()
            .map_or(Vec::new(), |a| a.values.iter().map(|(k, _)| *k).collect());
        let whole = matched.iter().all(|&i| i == matched[0])
            && positions
                .iter()
                .all(|k| param(k).is_some_and(|j| enums.iter().any(|(e, _)| *e == j)));
        let literal_arms = match whole {
            true => {
                collapsed |= e.literal_arms.len() > 1;
                Vec::new()
            }
            false => {
                enums.retain(|(j, _)| !positions.iter().any(|k| param(k) == Some(*j)));
                e.literal_arms.clone()
            }
        };
        let first = legacy_args(
            &e.params,
            e.literal_arms.first().filter(|_| e.literal_arms.len() > 1),
        );
        let params = e
            .params
            .iter()
            .zip(&o.args)
            .zip(&first)
            .map(|((p, a), theirs)| match (a, theirs) {
                (super::ir_types::Arg::Param(j), _) if enums.iter().any(|(e, _)| e == j) => {
                    let t = enums.iter().find(|(e, _)| e == j).unwrap().1.clone();
                    ConcreteParam {
                        type_ref: p
                            .type_ref
                            .split_empties()
                            .map_or(t.clone(), |(_, n)| t.with_empties(n)),
                        ..p.clone()
                    }
                }
                (super::ir_types::Arg::Param(j), LegacyArg::Param(_)) => ConcreteParam {
                    type_ref: arm_types
                        .iter()
                        .find(|(k, _)| k == j)
                        .map_or(&o.params[*j].type_ref, |(_, t)| t)
                        .clone(),
                    ..p.clone()
                },
                _ => p.clone(),
            })
            .collect();
        let literal_arms = match literal_arms.len() {
            0 | 1 => Vec::new(),
            _ => literal_arms
                .iter()
                .zip(&matched)
                .map(|(arm, &overload)| LiteralArm {
                    values: arm.values.clone(),
                    overload,
                })
                .collect(),
        };
        let mut each = vec![ExpandedSignature {
            overload: matched[0],
            name_suffix: e.name_suffix,
            params,
            literal_arms,
        }];
        for (k, members) in splits {
            collapsed = true;
            each = each
                .iter()
                .flat_map(|x| {
                    members.iter().map(move |m| {
                        let mut x = x.clone();
                        x.params[k].type_ref = m.clone();
                        x
                    })
                })
                .collect();
        }
        for x in each {
            // An expansion the IR takes whole, as an earlier one.
            let alike = |y: &ExpandedSignature| {
                y.overload == x.overload
                    && y.literal_arms.is_empty()
                    && x.literal_arms.is_empty()
                    && y.params == x.params
            };
            match expansions.iter().any(alike) {
                true => collapsed = true,
                false => expansions.push(x),
            }
        }
    }
    // A cohort taken whole is named as a plain expansion.
    if collapsed {
        let plain: Vec<usize> = (0..expansions.len())
            .filter(|&k| expansions[k].literal_arms.is_empty())
            .collect();
        let params: Vec<_> = plain
            .iter()
            .map(|&k| expansions[k].params.clone())
            .collect();
        let suffixes = crate::codegen::signatures::compute_suffixes_pub(&params);
        for (k, s) in plain.into_iter().zip(suffixes) {
            expansions[k].name_suffix = s;
        }
    }
    // The IR pairs each typed array with its copy (`u8[]`), which the
    // parse binds only beside an element array.
    let copy = |o: &super::ir_types::Overload, k: &super::ir_types::Overload| {
        o.params.len() == k.params.len()
            && o.params.iter().zip(&k.params).all(|(a, b)| {
                a.name == b.name
                    && (a.type_ref == b.type_ref
                        || matches!((&a.type_ref, &b.type_ref), (
                            TypeRef::TypedSlice(e, _),
                            TypeRef::Reference { segments, .. },
                        ) if segments.len() == 1
                            && crate::ir::TypedArrayElement::of_typed_array(&segments[0]) == Some(*e)))
            })
    };
    let unbound =
        |i: usize| (0..overloads.len()).any(|k| used[k] && copy(&overloads[i], &overloads[k]));
    if (0..overloads.len()).any(|i| !used[i] && !unbound(i) && !covered.contains(&i)) {
        return None;
    }
    Some((overloads, scopes, expansions))
}

/// A JS argument of a parse expansion.
enum LegacyArg<'a> {
    /// The expansion's parameter at this index.
    Param(usize),
    Str(&'a str),
    Null,
}

/// The JS arguments of `params`, with `arm`'s literals in place.
fn legacy_args<'a>(params: &'a [ConcreteParam], arm: Option<&'a LiteralArm>) -> Vec<LegacyArg<'a>> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let literal = arm.and_then(|a| a.values.iter().find(|(at, _)| *at == i));
            match (literal, &p.type_ref) {
                (Some((_, v)), _) | (None, TypeRef::StringLiteral(v)) => LegacyArg::Str(v),
                (None, TypeRef::Null) => LegacyArg::Null,
                _ => LegacyArg::Param(i),
            }
        })
        .collect()
}

/// The one unused IR overload making the call `args` over `params`: the
/// same constants, and parameters of the same names in the same places.
/// Overloads that differ only in their types are told apart by the
/// parse's, as the IR's states them, beside those the parse merged into it.
fn matching(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    overloads: &[super::ir_types::Overload],
    params: &[ConcreteParam],
    args: &[LegacyArg<'_>],
    used: &[bool],
) -> Option<(usize, Vec<usize>)> {
    use super::ir_types::Arg;
    // A slice the callee views, rather than copies.
    let view = |t: &TypeRef, mode| {
        matches!(t, TypeRef::TypedSlice(..)) && mode == Some(crate::ir::SliceModeHint::View)
    };
    let candidates: Vec<usize> = (0..overloads.len())
        .filter(|&i| {
            let o = &overloads[i];
            // An overload taking an enum is one per arm the parse expands.
            (!used[i] || o.members.iter().any(Option::is_some))
                && o.args.len() == args.len()
                && o.args
                    .iter()
                    .zip(args)
                    .all(|(ours, theirs)| match (ours, theirs) {
                        (Arg::Param(j), LegacyArg::Param(k)) => {
                            o.params[*j].name == params[*k].name
                                && o.params[*j].variadic == params[*k].variadic
                                && view(&o.params[*j].type_ref, o.params[*j].slice_mode)
                                    == view(&params[*k].type_ref, params[*k].slice_mode)
                        }
                        (Arg::Param(j), LegacyArg::Str(v)) => o.members[*j]
                            .as_ref()
                            .is_some_and(|m| m.contains(&TypeRef::StringLiteral((*v).into()))),
                        (Arg::Str(a), LegacyArg::Str(b)) => a == b,
                        (Arg::Null, LegacyArg::Null) => true,
                        _ => false,
                    })
        })
        .collect();
    if let [i] = candidates[..] {
        return Some((i, Vec::new()));
    }
    let same: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&i| {
            let o = &overloads[i];
            o.args
                .iter()
                .zip(args)
                .all(|(ours, theirs)| match (ours, theirs) {
                    (Arg::Param(j), LegacyArg::Param(k)) => {
                        let theirs = &params[*k].type_ref;
                        // An argument the IR erases (`Options<Str>` for
                        // `Options<"text">`), or reads as its enum, binds
                        // alike.
                        let states = |t: &TypeRef| {
                            state(cgctx, scope, t, theirs).is_some_and(|t| {
                                t == *theirs
                                    || erased_literals(&t, theirs)
                                    || literal_args(&t, theirs)
                            })
                        };
                        states(&o.params[*j].type_ref)
                            || o.members[*j].as_ref().is_some_and(|m| m.iter().any(states))
                    }
                    _ => true,
                })
        })
        .collect();
    // Of those alike, the one no expansion took yet, else the one the
    // parse writes as is, else the first; the others bind alike, and the
    // parse merged them.
    let unused = |c: &[usize]| -> Vec<usize> { c.iter().copied().filter(|&i| !used[i]).collect() };
    let exact: Vec<usize> = unused(&same)
        .into_iter()
        .filter(|&i| {
            let o = &overloads[i];
            o.args
                .iter()
                .zip(args)
                .all(|(ours, theirs)| match (ours, theirs) {
                    (Arg::Param(j), LegacyArg::Param(k)) => {
                        o.params[*j].type_ref == params[*k].type_ref
                    }
                    _ => true,
                })
        })
        .collect();
    match (
        &same[..],
        &unused(&same)[..],
        &exact[..],
        &unused(&candidates)[..],
    ) {
        ([i], ..) | (_, [i], ..) | ([], _, _, [i]) => Some((*i, Vec::new())),
        (_, _, [i], _) | (_, [i, ..], [], _) => {
            Some((*i, same.iter().copied().filter(|k| k != i).collect()))
        }
        _ => None,
    }
}

/// Whether the IR, declaring `site`, leaves the accessor `js_name` unread
/// (an accessor it drops, or doesn't state as the parse reads it), so the
/// binding is skipped.
pub fn unread(cgctx: Option<&CodegenContext<'_>>, site: Option<Site<'_>>, js_name: &str) -> bool {
    let (Some(c), Some(site)) = (cgctx.filter(|c| c.ir.is_some()), site) else {
        return false;
    };
    let at = [site.path, &[js_name.to_string()]].concat().join(".");
    let module = site.module.map(|m| format!("{m}:")).unwrap_or_default();
    c.warn(format!(
        "`{module}{at}` ({:?}): the IR doesn't state the parse's reading; skipped",
        site.form
    ));
    true
}

/// The type of the accessor at `site`, when the IR states the parse's
/// reading of `parsed`: no record, `object`, `symbol`, intersection, type
/// alias or named union, and no slice hint.
pub fn accessor(
    cgctx: Option<&CodegenContext<'_>>,
    site: Option<Site<'_>>,
    js_name: &str,
    parsed: &TypeRef,
    scope: ScopeId,
) -> Option<TypeRef> {
    let (cgctx, site) = (cgctx?, site?);
    let at = mark(cgctx);
    let r = accessor_at(cgctx, &site, js_name, parsed, scope);
    let r = settled(cgctx, at, &site, js_name, r)?;
    Some(match site.form {
        Form::Set => {
            let at = [site.path, &[js_name.to_string()]].concat().join(".");
            writes(cgctx, scope, &format!("`{at}`"), &r, parsed)
        }
        _ => r,
    })
}

/// `ours` where the parse's `theirs` writes an intersection the IR reads
/// as one part: `JsValue`, unless that part is the whole intersection
/// (`Derived & Base`), since a write must be every part. `at` locates it.
fn writes(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    at: &str,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> TypeRef {
    take(ours, theirs, &|o, t| {
        let TypeRef::Intersection(ts) = t else {
            return None;
        };
        // Every object is an open record (`Record<string, unknown>`).
        let open = |t: &TypeRef| {
            matches!(t, TypeRef::Object)
                || matches!(t, TypeRef::Reference { segments, generic_args }
                    if segments == &["Record"]
                        && matches!(&generic_args[..], [TypeRef::String, TypeRef::Unknown | TypeRef::Any]))
        };
        let ts: Vec<TypeRef> = ts.iter().filter(|t| !open(t)).cloned().collect();
        let whole = match &ts[..] {
            [one] => Some(one.clone()),
            ts => super::subtyping::glb_intersection(ts, Some(cgctx), scope),
        };
        if *o == TypeRef::Any || whole.as_ref() == Some(o) {
            return None;
        }
        cgctx.warn(format!(
            "{at}: the intersection `{}` it writes is read as one part, so it is erased to JsValue",
            t.format_ts()
        ));
        Some(TypeRef::Any)
    })
}

fn accessor_at(
    cgctx: &CodegenContext<'_>,
    site: &Site<'_>,
    js_name: &str,
    parsed: &TypeRef,
    scope: ScopeId,
) -> Option<TypeRef> {
    if parsed == &TypeRef::Never {
        return None;
    }
    // The IR writes a union arm by arm, a setter each.
    let (arms, declared) = cgctx
        .ir
        .as_ref()?
        .accessors(cgctx.gctx, site, js_name, scope)?;
    // A typed array's copy (`u64[]`), which the parse writes only beside
    // an element array.
    let copy = |t: &TypeRef| {
        matches!(t, TypeRef::TypedSlice(e, _) if arms.iter().any(|a| matches!(a,
            TypeRef::Reference { segments, .. } if segments.len() == 1
                && crate::ir::TypedArrayElement::of_typed_array(&segments[0]) == Some(*e))))
    };
    let core = parsed.split_empties().map_or(parsed.clone(), |(c, _)| c);
    let slices = matches!(core, TypeRef::Union(_))
        || mentions(parsed, &|t| {
            matches!(t, TypeRef::Array(_) | TypeRef::TypedSlice(..))
        });
    let arms: Vec<TypeRef> = arms
        .iter()
        .filter(|a| slices || !copy(a))
        .cloned()
        .collect();
    let ours = TypeRef::union(arms);
    // The parse narrows a parent's member at the child's arguments, which
    // the parent's accessor doesn't take.
    if declared {
        // A union the IR writes arm by arm reads as the parse's, as the
        // parent's own does.
        let union = matches!(
            ours.split_empties().map_or(ours.clone(), |(core, _)| core),
            TypeRef::Union(_)
        );
        let settled = !mentions(&ours, &|t| super::ir_types::declared(t).is_some());
        return stated(cgctx, scope, &ours, parsed)
            .or_else(|| synthesised(cgctx, scope, &ours, parsed))
            .or_else(|| {
                literals(&ours, parsed).then(|| emit_literals(cgctx, site, scope, &ours, parsed))
            })
            .or((!union && settled).then_some(ours));
    }
    stated(cgctx, scope, &ours, parsed)
        .or_else(|| {
            literals(&ours, parsed).then(|| emit_literals(cgctx, site, scope, &ours, parsed))
        })
        // An optional property, whose setters write it present.
        .or_else(|| {
            let optional = TypeRef::union([ours.clone(), TypeRef::Undefined]);
            (parsed.admits_undefined() && optional != ours)
                .then(|| stated(cgctx, scope, &optional, parsed))
                .flatten()
        })
        // An optional `T | null`, which the IR reads as `T?` but under
        // `--strict-nullable`.
        .or_else(|| {
            let absent = TypeRef::union([parsed.clone(), TypeRef::Undefined]);
            (absent != *parsed)
                .then(|| stated(cgctx, scope, &ours, &absent))
                .flatten()
        })
}

/// Whether the IR's `ours` reads the parse's literal union `theirs` at
/// its primitives or enums (`number | "auto"` for `"auto" | 0 | 1`,
/// `boolean` for `false | true`): each member of either covers or is
/// covered by one of the other's.
fn literals(ours: &TypeRef, theirs: &TypeRef) -> bool {
    let members = |t: &TypeRef| match t {
        TypeRef::Union(u) => u.members().to_vec(),
        t => vec![t.clone()],
    };
    let covers = |o: &TypeRef, t: &TypeRef| {
        o == t
            || matches!(
                (o, t),
                (TypeRef::Number, TypeRef::NumberLiteral(_))
                    | (TypeRef::String, TypeRef::StringLiteral(_))
                    | (TypeRef::Boolean, TypeRef::BooleanLiteral(_))
            )
            // An enum of literals, whose arms the IR names.
            || matches!(t, TypeRef::StringLiteral(_))
                && super::ir_types::synthesised(o).is_some_and(|(_, l)| l.is_empty() || l.contains(t))
    };
    // A literal union the IR declares as an enum (`StreamDownloadStatus`).
    if matches!(ours.without_undefined(), TypeRef::Reference { .. })
        && members(theirs)
            .iter()
            .all(|t| *t == TypeRef::Undefined || matches!(t, TypeRef::StringLiteral(_)))
    {
        return true;
    }
    let (ours, theirs) = (members(ours), members(theirs));
    let literal = |t: &TypeRef| {
        matches!(
            t,
            TypeRef::NumberLiteral(_) | TypeRef::StringLiteral(_) | TypeRef::BooleanLiteral(_)
        )
    };
    theirs.iter().any(literal)
        // An optional property's setters write it present.
        && theirs
            .iter()
            .all(|t| *t == TypeRef::Undefined || ours.iter().any(|o| covers(o, t)))
        && ours.iter().all(|o| theirs.iter().any(|t| covers(o, t)))
}

/// The IR's `ours` standing for the parse's `theirs`: [`restate`]d, unless
/// the parse reads more than the IR says there (see [`lossy_type`]) and
/// the two differ. An enum the producer synthesised stands for the
/// parse's union.
fn state(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    quiet(cgctx, || state_at(cgctx, scope, ours, theirs))
}

/// [`emit`], where the zip states the parse's reading.
fn stated(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    state(cgctx, scope, ours, theirs)?;
    Some(emit(cgctx, scope, ours, theirs))
}

/// The Rust reading of the IR's `ours`, where the zip matched it to the
/// parse's `theirs`: the IR's, its declarations and enums named as the
/// parse spells them.
fn emit(cgctx: &CodegenContext<'_>, scope: ScopeId, ours: &TypeRef, theirs: &TypeRef) -> TypeRef {
    let t = take(ours, theirs, &|o, t| {
        declared(cgctx, scope, o, t)
            .or_else(|| unaliased(cgctx, scope, o, t))
            // A union or literal the IR reads as a synthesised enum inside
            // another type, named as the parse spells it.
            .or_else(|| {
                let core = |t: &TypeRef| t.split_empties().map_or(t.clone(), |(c, _)| c);
                let nested = !std::ptr::eq(o, ours)
                    && matches!(
                        core(t),
                        TypeRef::Union(_) | TypeRef::StringLiteral(_) | TypeRef::NumberLiteral(_)
                    )
                    && super::ir_types::synthesised(&core(o)).is_some_and(|(_, l)| l.is_empty());
                nested.then(|| synthesised(cgctx, scope, o, t)).flatten()
            })
    });
    let t = synthesised(cgctx, scope, &t, theirs).unwrap_or_else(|| settle(&t, theirs));
    let t = unmerged(cgctx, scope, &t, ours);
    defaults(cgctx, scope, &t)
}

/// `t` without the `null` an alias it names holds, where the IR's `ours`
/// doesn't admit it (`ReasoningEffort?` with `type ReasoningEffort = "low"
/// | null`): its target's members, which read anywhere alike.
fn unmerged(cgctx: &CodegenContext<'_>, scope: ScopeId, t: &TypeRef, ours: &TypeRef) -> TypeRef {
    let own = ours.split_empties().map(|(_, n)| n);
    if own.is_some_and(|n| n.admits_null()) {
        return t.clone();
    }
    match super::typemap::split_empties_in(t, Some(cgctx), scope) {
        Some((core, n, _))
            if n.admits_null() && !mentions(&core, &|m| matches!(m, TypeRef::Reference { .. })) =>
        {
            own.map_or(core.clone(), |n| core.with_empties(n))
        }
        _ => t.clone(),
    }
}

/// The IR's reading of the parse's literal union `theirs`: its primitives
/// or its enum, named as the parse spells it. The IR writes a union arm by
/// arm, a literal as a setter of its own, which Rust's setter takes whole
/// as the enum its getter reads.
fn emit_literals(
    cgctx: &CodegenContext<'_>,
    site: &Site<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> TypeRef {
    let arms = matches!(ours.without_undefined(), TypeRef::Union(_));
    match site.form {
        Form::Set if arms => theirs.clone(),
        _ => emit(cgctx, scope, ours, theirs),
    }
}

/// The IR's enum `ours` of the parse's literal union `theirs`, which Rust
/// takes whole rather than as the primitive it widens to.
fn literal_enum(ours: &TypeRef, theirs: &TypeRef) -> Option<TypeRef> {
    let core = |t: &TypeRef| t.split_empties().map_or(t.clone(), |(c, _)| c);
    let ours = core(ours);
    let (name, _) = super::ir_types::synthesised(&ours)?;
    let TypeRef::Union(u) = core(theirs) else {
        return None;
    };
    let members = u.members();
    if !members
        .iter()
        .all(|m| matches!(m, TypeRef::StringLiteral(_)))
        && !super::typemap::is_integer_literal_union(members)
    {
        return None;
    }
    Some(TypeRef::Reference {
        segments: vec![super::ir_types::SYNTHESISED.into(), name.into()],
        generic_args: members.to_vec(),
    })
}

/// `t` without the trailing `any` arguments its declaration defaults to
/// `undefined` or another open type (`Fetcher<any>` for `Fetcher`), which
/// Rust otherwise reads apart; one equal to its default the parse keeps
/// too.
fn defaults(cgctx: &CodegenContext<'_>, scope: ScopeId, t: &TypeRef) -> TypeRef {
    let TypeRef::Reference {
        segments,
        generic_args,
    } = t
    else {
        return map_types(t, &|t| defaults(cgctx, scope, t));
    };
    let mut args: Vec<TypeRef> = generic_args
        .iter()
        .map(|a| defaults(cgctx, scope, a))
        .collect();
    if let Some(params) = type_params(cgctx, scope, segments) {
        let open = |t: &TypeRef| matches!(t, TypeRef::Any | TypeRef::Unknown | TypeRef::Undefined);
        while let Some(last) = args.last() {
            let default = params.get(args.len() - 1).and_then(|p| p.default.as_ref());
            match default {
                Some(d) if open(d) && open(last) => args.pop(),
                _ => break,
            };
        }
    }
    TypeRef::Reference {
        segments: segments.clone(),
        generic_args: args,
    }
}

/// `t` with each type parameter Rust erases (that `f` reads) read as the
/// default of the declaration it is an argument of, unless that names the
/// declaration's own, else as `f` reads it.
fn erased_to(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    t: &TypeRef,
    f: &impl Fn(&TypeRef) -> Option<TypeRef>,
) -> TypeRef {
    if let Some(r) = f(t) {
        return r;
    }
    match t {
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            let mut args = generic_args.clone();
            if let Some(params) = type_params(cgctx, scope, segments) {
                let own = |t: &TypeRef| {
                    mentions(t, &|t| {
                        matches!(t, TypeRef::Reference { segments, .. }
                            if segments.len() == 1 && params.iter().any(|p| p.name == segments[0]))
                    })
                };
                for (a, p) in args.iter_mut().zip(params) {
                    if let Some(d) = p.default.as_ref().filter(|d| f(a).is_some() && !own(d)) {
                        *a = d.clone();
                    }
                }
            }
            TypeRef::Reference {
                segments: segments.clone(),
                generic_args: args.iter().map(|a| erased_to(cgctx, scope, a, f)).collect(),
            }
        }
        t => map_types(t, &|t| erased_to(cgctx, scope, t, f)),
    }
}

/// `t` with `f` applied to each type it directly holds.
fn map_types(t: &TypeRef, f: &impl Fn(&TypeRef) -> TypeRef) -> TypeRef {
    match t {
        TypeRef::Union(u) => TypeRef::union(u.members().iter().map(f)),
        TypeRef::Array(x) => TypeRef::Array(Box::new(f(x))),
        TypeRef::Tuple(ts) => TypeRef::Tuple(ts.iter().map(f).collect()),
        TypeRef::Function(sig) => TypeRef::Function(crate::ir::FunctionSig {
            params: sig
                .params
                .iter()
                .map(|p| crate::ir::Param {
                    type_ref: f(&p.type_ref),
                    ..p.clone()
                })
                .collect(),
            return_type: Box::new(f(&sig.return_type)),
        }),
        t => t.clone(),
    }
}

/// A leading omittable parameter (`T | undefined`, the IR's optional
/// before a later argument) naming a type parameter, which Rust keeps
/// plain, as `None` would leave it to infer.
fn inferred_native(cgctx: &CodegenContext<'_>, scope: ScopeId, t: &TypeRef) -> TypeRef {
    let plain = t.without_undefined();
    let mut found = Vec::new();
    crate::codegen::signatures::collect_type_params(&plain, cgctx, scope, &mut found);
    match plain != *t && !found.is_empty() {
        true => plain,
        false => t.clone(),
    }
}

fn state_at(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    exact(cgctx, scope, ours, theirs).or_else(|| {
        // The IR reads `T | null | undefined` as `T?`, or `@js(null) T?`
        // nested, but under `--strict-nullable`.
        [TypeRef::Null, TypeRef::Undefined].iter().find_map(|drop| {
            let c = collapsed(cgctx, scope, theirs, drop);
            (c != *theirs)
                .then(|| exact(cgctx, scope, ours, &c))
                .flatten()
        })
    })
}

fn exact(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let ours = &take(ours, theirs, &|o, t| {
        declared(cgctx, scope, o, t)
            .or_else(|| unaliased(cgctx, scope, o, t))
            .or_else(|| defaulted(cgctx, scope, o, t))
            .or_else(|| aliased(cgctx, scope, o, t))
            .or_else(|| inferred(cgctx, scope, o, t))
            .or_else(|| family(cgctx, scope, o, t))
            // An inline type the IR shares with an alias of its shape.
            .or_else(|| {
                let split = |t: &TypeRef| {
                    t.split_empties()
                        .map_or((t.clone(), None), |(c, n)| (c, Some(n)))
                };
                let ((o_core, o_empty), (t_core, t_empty)) = (split(o), split(t));
                let TypeRef::Reference {
                    segments,
                    generic_args,
                } = &o_core
                else {
                    return None;
                };
                let (target, _) = cgctx.resolve_alias(&segments.join("."), scope)?;
                (generic_args.is_empty() && *target == t_core && o_empty == t_empty && t != o)
                    .then(|| t.clone())
            })
            // A union the IR reads as a synthesised enum inside another
            // type (`[string, File | string]`), nullable or not.
            .or_else(|| {
                let core = |t: &TypeRef| t.split_empties().map_or(t.clone(), |(c, _)| c);
                let nested = !std::ptr::eq(o, ours)
                    && matches!(core(t), TypeRef::Union(_))
                    && super::ir_types::synthesised(&core(o)).is_some_and(|(_, l)| l.is_empty());
                nested.then(|| synthesised(cgctx, scope, o, t)).flatten()
            })
    });
    if let Some(t) = synthesised(cgctx, scope, ours, theirs) {
        return Some(t);
    }
    let t = restate(&settle(ours, theirs), theirs)?;
    let lossy = lossy_type(theirs, cgctx, scope) || cgctx.union_alias(theirs, scope).is_some();
    // An instantiation at a literal union reads as the IR's enum of it.
    let instance = mentions(&t, &|t| super::ir_types::synthesised(t).is_some());
    (t == *theirs
        || !lossy
        || instance
        || projects(theirs, cgctx, scope)
        || assoc(&t, theirs)
        || literal_args(&t, theirs)
        || erased_literals(&t, theirs)
        || widens(&t, theirs))
    .then_some(t)
}

/// `t` with `drop` left out of each union admitting both empties, its
/// aliases' included.
fn collapsed(cgctx: &CodegenContext<'_>, scope: ScopeId, t: &TypeRef, drop: &TypeRef) -> TypeRef {
    let both = |n| n == crate::ir::Nullability::Both;
    let t = match super::typemap::split_empties_in(t, Some(cgctx), scope) {
        Some((core, n, _)) if both(n) && t.split_empties().is_none_or(|(_, m)| !both(m)) => {
            TypeRef::union([core, TypeRef::Null, TypeRef::Undefined])
        }
        _ => t.clone(),
    };
    let t = match &t {
        TypeRef::Reference {
            segments,
            generic_args,
        } => TypeRef::Reference {
            segments: segments.clone(),
            generic_args: generic_args
                .iter()
                .map(|a| collapsed(cgctx, scope, a, drop))
                .collect(),
        },
        t => map_types(t, &|t| collapsed(cgctx, scope, t, drop)),
    };
    match &t {
        TypeRef::Union(u)
            if u.members().contains(&TypeRef::Null)
                && u.members().contains(&TypeRef::Undefined) =>
        {
            TypeRef::union(u.members().iter().filter(|m| *m != drop).cloned())
        }
        _ => t,
    }
}

/// `theirs` where it instantiates the family `ours` names at a type
/// parameter (`PluralizeUnit<U>`), or at any where the parse reads it as an
/// enum, which the IR reads erased at its widest row, the enum of the
/// family's name.
fn family(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let (
        TypeRef::Reference {
            segments: a,
            generic_args: x,
        },
        TypeRef::Reference {
            segments: b,
            generic_args: y,
        },
    ) = (ours, theirs)
    else {
        return None;
    };
    if a != b || !x.is_empty() || y.is_empty() {
        return None;
    }
    let mut found = Vec::new();
    crate::codegen::signatures::collect_type_params(theirs, cgctx, scope, &mut found);
    // The parse lowers a family to its widest row's enum, whatever its
    // arguments.
    let whole = || {
        cgctx
            .gctx
            .resolve_path(scope, &b.join("."))
            .is_some_and(|id| {
                matches!(
                    cgctx.gctx.get_type(id).kind,
                    crate::ir::TypeKind::StringEnum(_)
                )
            })
    };
    (!found.is_empty() || whole()).then(|| theirs.clone())
}

/// `theirs` where the IR's `ours` is it or `undefined`, and it names a type
/// parameter: the parse keeps such a leading omittable plain, as `None`
/// would leave it to infer.
fn inferred(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let mut found = Vec::new();
    crate::codegen::signatures::collect_type_params(theirs, cgctx, scope, &mut found);
    (ours != theirs && ours.without_undefined() == *theirs && !found.is_empty())
        .then(|| theirs.clone())
}

/// Whether `ours` is the primitive of the one literal `theirs` reads
/// (`"message"` as `string`), which the IR widens a result to.
fn widens(ours: &TypeRef, theirs: &TypeRef) -> bool {
    let (ours, theirs) = match (ours.split_empties(), theirs.split_empties()) {
        (Some((a, x)), Some((b, y))) if x == y => (a, b),
        (None, None) => (ours.clone(), theirs.clone()),
        _ => return false,
    };
    let numbers = |u: &crate::ir::Union| {
        u.members()
            .iter()
            .all(|m| matches!(m, TypeRef::Number | TypeRef::NumberLiteral(_)))
    };
    matches!(
        (&ours, &theirs),
        (TypeRef::String, TypeRef::StringLiteral(_))
            | (TypeRef::Number, TypeRef::NumberLiteral(_))
            | (TypeRef::Boolean, TypeRef::BooleanLiteral(_))
    ) || matches!((&ours, &theirs), (TypeRef::Number, TypeRef::Union(u)) if numbers(u))
}

/// Whether `ours` is `theirs` with its literal arguments read as `string`.
fn erased_literals(ours: &TypeRef, theirs: &TypeRef) -> bool {
    match (ours, theirs) {
        (
            TypeRef::Reference {
                segments: a,
                generic_args: x,
            },
            TypeRef::Reference {
                segments: b,
                generic_args: y,
            },
        ) => {
            a == b
                && x.len() == y.len()
                && x.iter().zip(y).all(|(x, y)| {
                    x == y || *x == TypeRef::String && matches!(y, TypeRef::StringLiteral(_))
                })
        }
        _ => false,
    }
}

/// Whether `ours` instantiates what `theirs` does, at an enum wherever
/// `theirs` passes string literals (`Rounding<"hour">` as `Rounding<Hour>`).
fn literal_args(ours: &TypeRef, theirs: &TypeRef) -> bool {
    let literals = |t: &TypeRef| match t {
        TypeRef::StringLiteral(_) => true,
        TypeRef::Union(u) => u
            .members()
            .iter()
            .all(|m| matches!(m, TypeRef::StringLiteral(_))),
        _ => false,
    };
    match (ours, theirs) {
        (
            TypeRef::Reference {
                segments: a,
                generic_args: x,
            },
            TypeRef::Reference {
                segments: b,
                generic_args: y,
            },
        ) => {
            a == b
                && x.len() == y.len()
                && x.iter().zip(y).any(|(x, y)| x != y)
                && x.iter()
                    .zip(y)
                    .all(|(x, y)| x == y || literals(y) && matches!(x, TypeRef::Reference { .. }))
        }
        _ => false,
    }
}

/// Whether `ours` is an associated type of a parameter `theirs` reads
/// beside literals (`"auto" | U`), which the IR tabulates per argument.
fn assoc(ours: &TypeRef, theirs: &TypeRef) -> bool {
    let core = ours.split_empties().map_or(ours.clone(), |(core, _)| core);
    let TypeRef::Reference { segments, .. } = &core else {
        return false;
    };
    let [param, _] = segments.as_slice() else {
        return false;
    };
    mentions(theirs, &|t| t.as_ident() == Some(param.as_str()))
}

/// `ours` with each enum the producer synthesised inside it, an element,
/// a type argument or a callback's parameter or result, read as the
/// parse's union at the same place. Unlike a whole type's (see
/// [`synthesised`]), it doesn't name the union, which another use may
/// share. An instantiation argument carrying its literals stays for the
/// typemap to declare.
fn settle(ours: &TypeRef, theirs: &TypeRef) -> TypeRef {
    let core = ours.split_empties().map_or(ours.clone(), |(core, _)| core);
    if super::ir_types::synthesised(&core).is_some_and(|(_, literals)| literals.is_empty()) {
        return theirs.clone();
    }
    match (ours, theirs) {
        (TypeRef::Array(x), TypeRef::Array(y)) => TypeRef::Array(Box::new(settle(x, y))),
        (
            TypeRef::Reference {
                segments: a,
                generic_args: x,
            },
            TypeRef::Reference {
                segments: b,
                generic_args: y,
            },
        ) if a == b && x.len() == y.len() => TypeRef::Reference {
            segments: a.clone(),
            generic_args: x
                .iter()
                .zip(y)
                .map(|(x, y)| match super::ir_types::synthesised(x) {
                    Some((_, literals)) if !literals.is_empty() => x.clone(),
                    _ => settle(x, y),
                })
                .collect(),
        },
        (TypeRef::Function(f), TypeRef::Function(g)) if f.params.len() == g.params.len() => {
            TypeRef::Function(crate::ir::FunctionSig {
                params: f
                    .params
                    .iter()
                    .zip(&g.params)
                    .map(|(p, q)| crate::ir::Param {
                        type_ref: settle(&p.type_ref, &q.type_ref),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(settle(&f.return_type, &g.return_type)),
            })
        }
        (TypeRef::Union(_), TypeRef::Union(_)) => {
            match (ours.split_empties(), theirs.split_empties()) {
                (Some((x, n)), Some((y, _))) => settle(&x, &y).with_empties(n),
                _ => ours.clone(),
            }
        }
        _ => ours.clone(),
    }
}

/// Whether the parse reads `t` through a projection, which the IR states
/// as an associated type.
fn projects(t: &TypeRef, cgctx: &CodegenContext<'_>, scope: ScopeId) -> bool {
    let gctx = cgctx.gctx;
    if gctx.projections.is_empty() {
        return false;
    }
    mentions(t, &|t| {
        matches!(t, TypeRef::Reference { .. }) && {
            let key = t.format_ts();
            gctx.scopes
                .chain(scope)
                .into_iter()
                .any(|s| gctx.projections.contains_key(&(s, key.clone())))
        }
    })
}

/// The IR's `ours` standing for the parse's `theirs`, or `None` when it
/// can't. The IR reads a type it can't name as `Any`, fills in a generic's
/// omitted arguments, the default `Any?` dropped here, and reads a typed
/// array as a vector, which the parse's slice keeps clamping and shared
/// memory on.
fn restate(ours: &TypeRef, theirs: &TypeRef) -> Option<TypeRef> {
    // The IR lets a leading optional argument be `undefined` before a later
    // one; the parse doesn't.
    let undefined =
        |t: &TypeRef| matches!(t, TypeRef::Union(u) if u.members().contains(&TypeRef::Undefined));
    if undefined(ours) && !undefined(theirs) && !matches!(theirs, TypeRef::Any | TypeRef::Unknown) {
        return None;
    }
    let ours = adopt(ours, theirs);
    ((!has_any(&ours) || has_any(theirs)) && !pads(&ours, theirs) && !unsettled(&ours, true))
        .then_some(ours)
}

/// Whether `t` reads an enum the producer synthesised that the typemap
/// can't declare: anywhere but an instantiation's argument with its
/// literals.
fn unsettled(t: &TypeRef, top: bool) -> bool {
    if super::ir_types::declared(t).is_some() {
        return true;
    }
    match super::ir_types::synthesised(t) {
        Some((_, literals)) => top || literals.is_empty(),
        None => match t {
            TypeRef::Reference { generic_args, .. } => {
                generic_args.iter().any(|a| unsettled(a, false))
            }
            t => mentions(t, &|m| m != t && unsettled(m, true)),
        },
    }
}

fn adopt(ours: &TypeRef, theirs: &TypeRef) -> TypeRef {
    // What Rust reads alike: the IR's `any` for a type it can't state,
    // which the parse lowers to `JsValue` too; its `T?` for a nested
    // `T | null`; and the class it names for `this`.
    let opaque = matches!(
        theirs,
        TypeRef::Symbol
            | TypeRef::KeyOf(_)
            | TypeRef::IndexedAccess(..)
            | TypeRef::Mapped(_)
            | TypeRef::ObjectLiteral(_)
    ) || matches!(theirs, TypeRef::Unresolved(s) if s != "this");
    let opaque = opaque
        || matches!(theirs.split_empties(), Some((core, _)) if matches!(core, TypeRef::Symbol));
    // The IR's `Any?` for a type of only empties.
    let empty = |t: &TypeRef| matches!(t, TypeRef::Null | TypeRef::Undefined);
    let opaque = opaque
        || empty(theirs)
        || matches!(theirs, TypeRef::Union(u) if u.members().iter().all(empty));
    match (ours, theirs) {
        (TypeRef::Any, _) if opaque => return theirs.clone(),
        (TypeRef::Any, TypeRef::Unknown) => return theirs.clone(),
        // `any | undefined` is `any`.
        (TypeRef::Any, TypeRef::Union(u)) if u.members().contains(&TypeRef::Any) => {
            return theirs.clone();
        }
        (TypeRef::Array(x), TypeRef::Any | TypeRef::Unknown) if **x == TypeRef::Any => {
            return theirs.clone();
        }
        (TypeRef::Undefined, TypeRef::Null) => return theirs.clone(),
        // The IR's `unit` is `undefined`.
        (TypeRef::Void, TypeRef::Undefined) => return theirs.clone(),
        // A copy reads the buffer whatever its sharing, which only a view
        // states (`@js(unshared)`).
        (
            TypeRef::TypedSlice(a, crate::ir::SharedMemory::Accepted),
            TypeRef::TypedSlice(b, crate::ir::SharedMemory::Rejected),
        ) if a.rust() == b.rust() => return theirs.clone(),
        (TypeRef::BigInt, TypeRef::AbiScalar(crate::ir::AbiScalar::I64)) => return theirs.clone(),
        (TypeRef::Reference { .. }, TypeRef::Unresolved(s)) if s == "this" => {
            return theirs.clone();
        }
        // The IR reads an intersection as one of its members.
        (_, TypeRef::Intersection(ts)) if ts.contains(ours) => return theirs.clone(),
        _ => {}
    }
    match (ours, theirs) {
        (
            TypeRef::Reference {
                segments: a,
                generic_args: x,
            },
            TypeRef::Reference {
                segments: b,
                generic_args: y,
            },
        ) if a == b => TypeRef::Reference {
            segments: a.clone(),
            generic_args: if y.is_empty() && x.iter().all(|t| *t == TypeRef::Any) {
                Vec::new()
            } else if x.len() == y.len() {
                x.iter().zip(y).map(|(x, y)| adopt(x, y)).collect()
            } else {
                x.clone()
            },
        },
        (TypeRef::TypedSlice(a, _), TypeRef::TypedSlice(b, _)) if a.rust() == b.rust() => {
            theirs.clone()
        }
        // The parse reads the array beside a typed array as a view of it.
        (TypeRef::Array(x), TypeRef::TypedSlice(e, _))
            if e.is_bigint() && **x == TypeRef::BigInt
                || !e.is_bigint() && matches!(**x, TypeRef::Number | TypeRef::AbiScalar(_)) =>
        {
            theirs.clone()
        }
        (TypeRef::Array(x), TypeRef::Array(y)) => TypeRef::Array(Box::new(adopt(x, y))),
        // The IR names a callback's parameters by position.
        (TypeRef::Function(f), TypeRef::Function(g)) if f.params.len() == g.params.len() => {
            TypeRef::Function(crate::ir::FunctionSig {
                params: f
                    .params
                    .iter()
                    .zip(&g.params)
                    .map(|(p, q)| crate::ir::Param {
                        name: q.name.clone(),
                        type_ref: adopt(&p.type_ref, &q.type_ref),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(adopt(&f.return_type, &g.return_type)),
            })
        }
        (TypeRef::Union(u), TypeRef::Union(v)) if u.members().len() == v.members().len() => {
            TypeRef::union(
                u.members()
                    .iter()
                    .zip(v.members())
                    .map(|(x, y)| adopt(x, y)),
            )
        }
        _ => ours.clone(),
    }
}

fn pads(ours: &TypeRef, theirs: &TypeRef) -> bool {
    match (ours, theirs) {
        (
            TypeRef::Reference {
                segments: a,
                generic_args: x,
            },
            TypeRef::Reference {
                segments: b,
                generic_args: y,
            },
        ) if a == b => x.len() != y.len() || x.iter().zip(y).any(|(x, y)| pads(x, y)),
        (TypeRef::Array(x), TypeRef::Array(y)) => pads(x, y),
        (TypeRef::Union(u), theirs) => u.members().iter().any(|m| match theirs {
            TypeRef::Union(v) => v.members().iter().any(|n| pads(m, n)),
            t => pads(m, t),
        }),
        _ => false,
    }
}

/// `ours`, with `at`'s reading of a part of it against the same part of
/// `theirs` wherever it has one.
fn take(
    ours: &TypeRef,
    theirs: &TypeRef,
    at: &impl Fn(&TypeRef, &TypeRef) -> Option<TypeRef>,
) -> TypeRef {
    if let Some(t) = at(ours, theirs).filter(|t| t != ours) {
        return take(&t, theirs, at);
    }
    let each = |x: &[TypeRef], y: &[TypeRef]| -> Option<Vec<TypeRef>> {
        (x.len() == y.len()).then(|| x.iter().zip(y).map(|(x, y)| take(x, y, at)).collect())
    };
    match (ours, theirs) {
        (
            TypeRef::Reference {
                segments: a,
                generic_args: x,
            },
            TypeRef::Reference {
                segments: b,
                generic_args: y,
            },
            // Past the parse's arguments, the IR's are the declaration's
            // defaults, which the parse leaves to it.
        ) if a == b && x.len() >= y.len() => match each(&x[..y.len()], y) {
            Some(generic_args) => TypeRef::Reference {
                segments: a.clone(),
                generic_args,
            },
            None => ours.clone(),
        },
        (TypeRef::Array(x), TypeRef::Array(y)) => TypeRef::Array(Box::new(take(x, y, at))),
        (TypeRef::Tuple(x), TypeRef::Tuple(y)) => each(x, y).map_or(ours.clone(), TypeRef::Tuple),
        (TypeRef::Union(u), TypeRef::Union(v)) => {
            each(u.members(), v.members()).map_or(ours.clone(), TypeRef::union)
        }
        (TypeRef::Function(f), TypeRef::Function(g)) if f.params.len() == g.params.len() => {
            TypeRef::Function(crate::ir::FunctionSig {
                params: f
                    .params
                    .iter()
                    .zip(&g.params)
                    .map(|(p, q)| crate::ir::Param {
                        type_ref: take(&p.type_ref, &q.type_ref, at),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(take(&f.return_type, &g.return_type, at)),
            })
        }
        _ => ours.clone(),
    }
}

/// The IR's alias `ours` expanded, where the parse reads what it names.
/// The IR declares only function aliases; a name the parse aliases
/// otherwise is the IR's enum of it.
fn unaliased(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let TypeRef::Reference { segments, .. } = ours else {
        return None;
    };
    if matches!(theirs, TypeRef::Reference { segments: b, .. } if b == segments) {
        return None;
    }
    let id = cgctx.gctx.resolve_path(scope, &segments.join("."))?;
    if !matches!(
        cgctx.gctx.get_type(id).kind,
        crate::ir::TypeKind::TypeAlias(_)
    ) {
        return None;
    }
    cgctx
        .resolve_alias_instance(ours, scope)
        .filter(|t| matches!(t, TypeRef::Function(_) | TypeRef::Reference { .. }))
}

/// The parse's alias `theirs`, where `ours` states what it names: the
/// IR expands aliases, which the parse keeps by name.
fn aliased(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let target = |t: &TypeRef| -> Option<(TypeRef, ScopeId)> {
        let TypeRef::Reference {
            segments,
            generic_args,
        } = t
        else {
            return None;
        };
        match generic_args.is_empty() {
            true => {
                let (t, at) = cgctx.resolve_alias(&segments.join("."), scope)?;
                Some((t.into_owned(), at))
            }
            false => match cgctx.resolve_alias_instance(t, scope) {
                Some(t) => Some((t, scope)),
                // A target that reads only in the alias's own scope
                // (`H.Triple<number>` over a module-local `Hidden`), at
                // arguments that read alike anywhere.
                None if !generic_args
                    .iter()
                    .any(|a| mentions(a, &|t| matches!(t, TypeRef::Reference { .. }))) =>
                {
                    let id = cgctx.gctx.resolve_path(scope, &segments.join("."))?;
                    let crate::ir::TypeKind::TypeAlias(alias) = &cgctx.gctx.get_type(id).kind
                    else {
                        return None;
                    };
                    Some((
                        crate::codegen::typemap::alias_instance(alias, generic_args),
                        alias.body_scope,
                    ))
                }
                None => None,
            },
        }
    };
    let (target, at) = match theirs {
        // An alias among a union's members (`MaybeStr | undefined`), where
        // the IR doesn't read the union as an enum of its own.
        TypeRef::Union(u) if !mentions(ours, &|t| super::ir_types::synthesised(t).is_some()) => {
            let mut any = false;
            let members = u.members().iter().map(|m| match target(m) {
                Some((t, _)) => {
                    any = true;
                    t
                }
                None => m.clone(),
            });
            let t = TypeRef::union(members.collect::<Vec<_>>());
            (any.then_some(t)?, scope)
        }
        t => target(t)?,
    };
    // An alias of a union with empties, which the parse's null arm and
    // omission take (`MaybeDay = DayLike | null | undefined`).
    let core = target
        .split_empties()
        .map(|(c, _)| c)
        .filter(|_| ours.split_empties().is_none());
    let (t, target) = match state(cgctx, at, ours, &target) {
        Some(t) => (t, target),
        None => {
            let core = core?;
            (state(cgctx, at, ours, &core)?, core)
        }
    };
    // A name the alias's own scope reads alike (`Events.Options` for
    // `Options`).
    let named = |t: &TypeRef, scope| match t {
        TypeRef::Reference {
            segments,
            generic_args,
        } => Some((
            cgctx.gctx.resolve_path(scope, &segments.join("."))?,
            generic_args.clone(),
        )),
        _ => None,
    };
    (t == target || named(&t, scope).is_some_and(|n| Some(n) == named(&target, at)))
        .then(|| theirs.clone())
}

/// `ours` without the arguments the parse leaves to the declaration's
/// defaults (`Envelope` for `Envelope<any, Loose>`), or with the buffer
/// a typed array's parse names, which the IR leaves out.
fn defaulted(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let (
        TypeRef::Reference {
            segments: a,
            generic_args: x,
        },
        TypeRef::Reference {
            segments: b,
            generic_args: y,
        },
    ) = (ours, theirs)
    else {
        return None;
    };
    if a != b {
        return None;
    }
    if x.is_empty() && crate::ir::TypedArrayElement::of_typed_array(&a.join(".")).is_some() {
        return Some(theirs.clone());
    }
    // The parse's arguments the IR leaves to their defaults.
    if x.is_empty() && !y.is_empty() {
        let params = type_params(cgctx, scope, b)?;
        return y
            .iter()
            .zip(params)
            .all(|(y, p)| {
                matches!(y, TypeRef::Any | TypeRef::Unknown) || p.default.as_ref() == Some(y)
            })
            .then(|| theirs.clone());
    }
    if x.len() <= y.len() {
        return None;
    }
    let params = type_params(cgctx, scope, b)?;
    x[y.len()..]
        .iter()
        .zip(params.get(y.len()..)?)
        .all(|(x, p)| match (x, &p.default) {
            (TypeRef::Any, _) => true,
            // The default named bare (`CfProperties` for
            // `CfProperties<HostMetadata>`) takes its own defaults.
            (
                TypeRef::Reference {
                    segments: a,
                    generic_args,
                },
                Some(TypeRef::Reference { segments: b, .. }),
            ) if generic_args.is_empty() => a == b,
            (x, d) => d.as_ref() == Some(x),
        })
        .then(|| TypeRef::Reference {
            segments: a.clone(),
            generic_args: x[..y.len()].to_vec(),
        })
}

/// The type parameters of the declaration `segments` names.
fn type_params<'a>(
    cgctx: &'a CodegenContext<'_>,
    scope: ScopeId,
    segments: &[String],
) -> Option<&'a [crate::ir::TypeParam]> {
    let id = cgctx.gctx.resolve_path(scope, &segments.join("."))?;
    match &cgctx.gctx.get_type(id).kind {
        crate::ir::TypeKind::Class(c) => Some(&c.type_params),
        crate::ir::TypeKind::Interface(i) => Some(&i.type_params),
        crate::ir::TypeKind::TypeAlias(a) => Some(&a.type_params),
        _ => None,
    }
}

/// The parse's name for a declaration the IR names by its id, `ours`,
/// where `theirs` names the same one.
fn declared(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let id = super::ir_types::declared(ours)?;
    let (TypeRef::Reference { generic_args, .. }, TypeRef::Reference { segments, .. }) =
        (ours, theirs)
    else {
        return None;
    };
    let named = cgctx.gctx.resolve_path(scope, &segments.join("."))?;
    (named.index().to_string() == id).then(|| TypeRef::Reference {
        segments: segments.clone(),
        generic_args: generic_args.clone(),
    })
}

fn has_any(t: &TypeRef) -> bool {
    mentions(t, &|t| matches!(t, TypeRef::Any | TypeRef::Unknown))
}

pub(crate) fn mentions(t: &TypeRef, f: &impl Fn(&TypeRef) -> bool) -> bool {
    f(t) || match t {
        TypeRef::Union(u) => u.members().iter().any(|t| mentions(t, f)),
        TypeRef::Array(t) => mentions(t, f),
        TypeRef::Tuple(ts) | TypeRef::Intersection(ts) => ts.iter().any(|t| mentions(t, f)),
        TypeRef::Reference { generic_args, .. } => generic_args.iter().any(|t| mentions(t, f)),
        TypeRef::Function(sig) => {
            sig.params.iter().any(|p| mentions(&p.type_ref, f)) || mentions(&sig.return_type, f)
        }
        _ => false,
    }
}

/// The parse's `theirs`, where the IR reads it as an enum the producer
/// synthesised (bare, nullable, or awaited), which then takes the IR's
/// name.
fn synthesised(
    cgctx: &CodegenContext<'_>,
    scope: ScopeId,
    ours: &TypeRef,
    theirs: &TypeRef,
) -> Option<TypeRef> {
    let (ours_core, theirs_core) = match (ours.as_promise_inner(), theirs.as_promise_inner()) {
        (Some(ours), Some(theirs)) => (ours, theirs),
        (None, None) => (ours, theirs),
        _ => return None,
    };
    // The parse's union, admitting the empties the IR's does.
    let empties = ours_core.split_empties().map(|(_, n)| n);
    let rebased = {
        let core = theirs_core
            .split_empties()
            .map_or(theirs_core.clone(), |(c, _)| c);
        let t = empties.map_or(core.clone(), |n| core.with_empties(n));
        match theirs.as_promise_inner() {
            Some(_) => TypeRef::Reference {
                segments: vec!["Promise".into()],
                generic_args: vec![t],
            },
            None => t,
        }
    };
    let ours_core = ours_core
        .split_empties()
        .map_or(ours_core.clone(), |(core, _)| core);
    let (name, literals) = super::ir_types::synthesised(&ours_core)?;
    if literals.is_empty() || theirs.as_promise_inner().is_some() {
        cgctx.name_synthesised_union(theirs_core, scope, name);
        return Some(rebased);
    }
    // An associated type's row, whose literals the parse reads through a
    // projection it keeps whole.
    let mut members = literals.to_vec();
    if let Some(n) = empties {
        if n.admits_null() {
            members.push(TypeRef::Null);
        }
        if n.admits_undefined() {
            members.push(TypeRef::Undefined);
        }
    }
    let read = TypeRef::union(members);
    cgctx.name_synthesised_union(&read, scope, name);
    Some(read)
}

/// Whether the parse reads `t` as more than its IR type says: a record,
/// `object` or `symbol` (all `Any` in the IR), a type alias, which the IR
/// resolves, `this`, or a callback with a rest parameter. Inside another
/// type, `null` too: the IR's `@js(null)` marks only a parameter or result.
fn lossy_type(t: &TypeRef, cgctx: &CodegenContext<'_>, scope: ScopeId) -> bool {
    lossy_at(t, cgctx, scope, true)
}

fn lossy_at(t: &TypeRef, cgctx: &CodegenContext<'_>, scope: ScopeId, top: bool) -> bool {
    let lossy = |t: &TypeRef| lossy_at(t, cgctx, scope, false);
    let absent = |t: &TypeRef| matches!(t, TypeRef::Null | TypeRef::Undefined);
    match t {
        // The IR reads a literal as its primitive, and `null` or
        // `undefined` alone as `Any?` or `void`.
        TypeRef::StringLiteral(_) | TypeRef::NumberLiteral(_) | TypeRef::BooleanLiteral(_) => true,
        TypeRef::Null | TypeRef::Undefined => true,
        TypeRef::Union(u) if u.members().iter().all(absent) => true,
        TypeRef::Union(u) if !top && u.members().contains(&TypeRef::Null) => true,
        TypeRef::Union(u) if top => u
            .members()
            .iter()
            .any(|m| !absent(m) && lossy_at(m, cgctx, scope, true)),
        TypeRef::Unresolved(name) if name == "this" => true,
        TypeRef::Function(f) => {
            f.params.iter().any(|p| p.variadic || lossy(&p.type_ref)) || lossy(&f.return_type)
        }
        // The IR reads an intersection as one of its members.
        TypeRef::Object | TypeRef::Symbol | TypeRef::PropertyKey | TypeRef::Intersection(_) => true,
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            let name = segments.join(".");
            let alias = cgctx.gctx.resolve_path(scope, &name).is_some_and(|id| {
                matches!(
                    cgctx.gctx.get_type(id).kind,
                    crate::ir::TypeKind::TypeAlias(_)
                )
            });
            alias || cgctx.resolve_alias(&name, scope).is_some() || generic_args.iter().any(lossy)
        }
        TypeRef::Union(u) => u.members().iter().any(lossy),
        TypeRef::Array(t) => lossy(t),
        TypeRef::Tuple(ts) => ts.iter().any(lossy),
        _ => false,
    }
}
