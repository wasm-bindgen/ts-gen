//! ABI bounds of per-monomorphization imports.
//!
//! An `experimental_generic_mono` import crosses each type parameter as
//! itself, so the conversion traits a position needs are part of its
//! declaration, and wasm-bindgen adds none. This pass states them on every
//! per-mono extern fn, from the shape of each argument and return naming a
//! type parameter, and then writes every bound on a type parameter inline
//! (`T: FromWasmAbi`) rather than in a `where` clause. A position whose type
//! marshals for every instantiation, such as `&Holder<T>` for an imported
//! `Holder<T>`, needs no bound.

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::visit_mut::VisitMut;
use syn::{
    FnArg, ForeignItem, GenericArgument, GenericParam, Generics, Ident, ImplItem, Item,
    PathArguments, ReturnType, Type, TypeParamBound, WherePredicate,
};

/// The traits a bound names, imported at the top of the file.
const TRAITS: &[&str] = &[
    "IntoWasmAbi",
    "RefIntoWasmAbi",
    "VectorRefIntoWasmAbi",
    "FromWasmAbi",
];

pub(crate) fn apply(file: &mut syn::File) {
    if !any_mono(&file.items) {
        return;
    }
    process(&mut file.items);
    let text = file.to_token_stream().to_string();
    let used: Vec<Ident> = TRAITS
        .iter()
        .filter(|t| {
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|w| w == **t)
        })
        .map(|t| Ident::new(t, proc_macro2::Span::call_site()))
        .collect();
    if !used.is_empty() {
        let at = file
            .items
            .iter()
            .position(|i| !matches!(i, Item::Use(_)))
            .unwrap_or(file.items.len());
        file.items.insert(
            at,
            syn::parse_quote! {
                #[allow(unused_imports)]
                use wasm_bindgen::convert::{#(#used),*};
            },
        );
    }
}

fn any_mono(items: &[Item]) -> bool {
    items.iter().any(|item| match item {
        Item::Mod(m) => m.content.as_ref().is_some_and(|(_, items)| any_mono(items)),
        Item::ForeignMod(block) => super::inline_wrappers::mentions_mono(block),
        _ => false,
    })
}

fn process(items: &mut [Item]) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, items)) = &mut m.content {
                    process(items);
                }
            }
            Item::ForeignMod(block) => {
                let mono = super::inline_wrappers::mentions_mono(block);
                for item in &mut block.items {
                    if let ForeignItem::Fn(f) = item {
                        if mono {
                            let params = type_params(&f.sig.generics);
                            let preds = signature_bounds(&f.sig, 0, &params);
                            add_predicates(&mut f.sig.generics, preds);
                        }
                        inline_bounds(&mut f.sig.generics, &[]);
                    }
                }
            }
            Item::Impl(imp) => {
                inline_bounds(&mut imp.generics, &[]);
                let outer = type_params(&imp.generics);
                for item in &mut imp.items {
                    if let ImplItem::Fn(f) = item {
                        inline_bounds(&mut f.sig.generics, &outer);
                    }
                }
            }
            Item::Fn(f) => inline_bounds(&mut f.sig.generics, &[]),
            _ => {}
        }
    }
}

pub(crate) fn type_params(generics: &Generics) -> Vec<Ident> {
    generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(t.ident.clone()),
            _ => None,
        })
        .collect()
}

/// The bounds a call through `sig` needs for `params`: each typed argument
/// after the first `skip`, and the return (a `catch` import's `Ok` type),
/// which an `async` import also needs `'static`.
pub(crate) fn signature_bounds(
    sig: &syn::Signature,
    skip: usize,
    params: &[Ident],
) -> Vec<WherePredicate> {
    let mut out = argument_bounds(sig.inputs.iter().skip(skip), params);
    if let ReturnType::Type(_, ty) = &sig.output {
        let ok = result_ok(ty);
        let mut ret = shape_bound(ok, params, Position::Return);
        if sig.asyncness.is_some() {
            match &mut ret {
                Some(WherePredicate::Type(p)) => p.bounds.push(syn::parse_quote!('static)),
                _ if mentions(ok, params) => {
                    let from = from();
                    ret = Some(syn::parse_quote!(#ok: #from + 'static));
                }
                _ => {}
            }
        }
        out.extend(ret);
    }
    dedup(out)
}

/// The argument half of [`signature_bounds`].
pub(crate) fn argument_bounds<'a>(
    inputs: impl Iterator<Item = &'a FnArg>,
    params: &[Ident],
) -> Vec<WherePredicate> {
    let out = inputs
        .filter_map(|arg| match arg {
            FnArg::Typed(pat) => shape_bound(&pat.ty, params, Position::Argument),
            FnArg::Receiver(_) => None,
        })
        .collect();
    dedup(out)
}

#[derive(Clone, Copy, PartialEq)]
enum Position {
    Argument,
    Return,
}

/// The bound for one position: on the type parameter itself where the
/// shape has an exact trait of its own (`&T`, `&[T]`), else on the whole
/// type. `Option<T>` is the whole type: `OptionFromWasmAbi` only covers the
/// types whose `None` fits their own ABI, which excludes `f64`.
fn shape_bound(ty: &Type, params: &[Ident], pos: Position) -> Option<WherePredicate> {
    if !mentions_bare(ty, params) {
        return None;
    }
    let bare = |ty: &Type| -> Option<Ident> {
        let Type::Path(p) = ty else { return None };
        let ident = p.path.get_ident()?;
        params.contains(ident).then(|| ident.clone())
    };
    let (param, bound): (Option<Ident>, TokenStream) = match (pos, shape(ty)) {
        (Position::Argument, Shape::Bare(t)) => (bare(t), into()),
        (Position::Argument, Shape::Ref(t)) => (bare(t), ref_into()),
        (Position::Argument, Shape::OptionRef(t)) => (bare(t), ref_into()),
        (Position::Argument, Shape::Slice(t)) => (bare(t), slice_into()),
        (Position::Return, Shape::Bare(t)) => (bare(t), from()),
        _ => (None, TokenStream::new()),
    };
    if let Some(param) = param {
        return Some(syn::parse_quote!(#param: #bound));
    }
    let mut ty = ty.clone();
    let named = name_elided_lifetimes(&mut ty);
    let whole = match pos {
        Position::Argument => into(),
        Position::Return => from(),
    };
    Some(if named {
        syn::parse_quote!(for<'a> #ty: #whole)
    } else {
        syn::parse_quote!(#ty: #whole)
    })
}

fn into() -> TokenStream {
    quote! { IntoWasmAbi }
}
fn ref_into() -> TokenStream {
    quote! { RefIntoWasmAbi }
}
fn slice_into() -> TokenStream {
    quote! { VectorRefIntoWasmAbi }
}
fn from() -> TokenStream {
    quote! { FromWasmAbi }
}

enum Shape<'t> {
    Bare(&'t Type),
    Ref(&'t Type),
    OptionRef(&'t Type),
    Slice(&'t Type),
    Other,
}

fn shape(ty: &Type) -> Shape<'_> {
    match ty {
        Type::Reference(r) if r.mutability.is_none() => match &*r.elem {
            Type::Slice(s) => Shape::Slice(&s.elem),
            elem => Shape::Ref(elem),
        },
        Type::Path(_) => match generic_of(ty, "Option") {
            Some(Type::Reference(r)) if r.mutability.is_none() => match &*r.elem {
                Type::Slice(s) => Shape::Slice(&s.elem),
                elem => Shape::OptionRef(elem),
            },
            Some(_) => Shape::Other,
            None => Shape::Bare(ty),
        },
        _ => Shape::Other,
    }
}

/// `inner` of `name<inner>`.
fn generic_of<'t>(ty: &'t Type, name: &str) -> Option<&'t Type> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    }
}

fn result_ok(ty: &Type) -> &Type {
    generic_of(ty, "Result").unwrap_or(ty)
}

fn mentions(ty: &Type, params: &[Ident]) -> bool {
    struct Find<'a> {
        params: &'a [Ident],
        found: bool,
    }
    impl VisitMut for Find<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            if path.leading_colon.is_none()
                && path.segments.len() == 1
                && self.params.contains(&path.segments[0].ident)
            {
                self.found = true;
            }
            syn::visit_mut::visit_path_mut(self, path);
        }
    }
    let mut find = Find {
        params,
        found: false,
    };
    find.visit_type_mut(&mut ty.clone());
    find.found
}

/// Whether `ty` names one of `params` outside the arguments of a generic
/// type other than `Option` / `Vec`: a handle's ABI holds for every
/// instantiation of its arguments.
pub(crate) fn mentions_bare(ty: &Type, params: &[Ident]) -> bool {
    struct Find<'a> {
        params: &'a [Ident],
        found: bool,
    }
    impl VisitMut for Find<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            if path.leading_colon.is_none()
                && path.segments.len() == 1
                && self.params.contains(&path.segments[0].ident)
            {
                self.found = true;
            }
            syn::visit_mut::visit_path_mut(self, path);
        }
        fn visit_path_segment_mut(&mut self, seg: &mut syn::PathSegment) {
            let transparent = seg.ident == "Option" || seg.ident == "Vec";
            if transparent || matches!(seg.arguments, PathArguments::None) {
                syn::visit_mut::visit_path_segment_mut(self, seg);
            }
        }
    }
    let mut find = Find {
        params,
        found: false,
    };
    find.visit_type_mut(&mut ty.clone());
    find.found
}

fn name_elided_lifetimes(ty: &mut Type) -> bool {
    struct Name(bool);
    impl VisitMut for Name {
        fn visit_type_reference_mut(&mut self, r: &mut syn::TypeReference) {
            if r.lifetime.is_none() {
                r.lifetime = Some(syn::Lifetime::new("'a", proc_macro2::Span::call_site()));
                self.0 = true;
            }
            syn::visit_mut::visit_type_reference_mut(self, r);
        }
    }
    let mut v = Name(false);
    v.visit_type_mut(ty);
    v.0
}

fn dedup(mut preds: Vec<WherePredicate>) -> Vec<WherePredicate> {
    let mut seen = std::collections::HashSet::new();
    preds.retain(|p| seen.insert(p.to_token_stream().to_string()));
    preds
}

pub(crate) fn add_predicates(generics: &mut Generics, preds: Vec<WherePredicate>) {
    if !preds.is_empty() {
        generics.make_where_clause().predicates.extend(preds);
    }
}

/// Moves each `where` predicate on a type parameter of `generics` into the
/// parameter's own bounds. Predicates on `outer` parameters (an impl's,
/// seen from one of its fns) and on other types stay.
pub(crate) fn inline_bounds(generics: &mut Generics, outer: &[Ident]) {
    let Some(clause) = generics.where_clause.take() else {
        return;
    };
    let mut kept = clause.clone();
    kept.predicates.clear();
    for pred in clause.predicates {
        let target = match &pred {
            WherePredicate::Type(p) if p.lifetimes.is_none() => match &p.bounded_ty {
                Type::Path(t) => t.path.get_ident().cloned(),
                _ => None,
            }
            .filter(|ident| !outer.contains(ident)),
            _ => None,
        };
        let param = target.and_then(|ident| {
            generics.params.iter_mut().find_map(|g| match g {
                GenericParam::Type(t) if t.ident == ident => Some(t),
                _ => None,
            })
        });
        match (param, pred) {
            (Some(param), WherePredicate::Type(p)) => {
                for bound in p.bounds {
                    let text = bound.to_token_stream().to_string();
                    if !param
                        .bounds
                        .iter()
                        .any(|b: &TypeParamBound| b.to_token_stream().to_string() == text)
                    {
                        param.bounds.push(bound);
                    }
                }
                if param.colon_token.is_none() {
                    param.colon_token = Some(Default::default());
                }
            }
            (_, pred) => kept.predicates.push(pred),
        }
    }
    if !kept.predicates.is_empty() {
        generics.where_clause = Some(kept);
    }
}
