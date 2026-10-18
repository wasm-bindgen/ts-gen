//! Inline wrappers over extern bindings whose declared type the extern ABI
//! cannot express.
//!
//! wasm-bindgen crosses `u64` / `i64` as a JS `BigInt`, so a `@rust u64` on
//! a `number` cannot be the extern type: the binding keeps `f64` under a
//! private `<name>_js` name and a `pub fn` of the public name converts with
//! `as` on the way in and out (exact to 2^53, as the JS number is). The
//! type emitters mark such leaves with a placeholder path (see
//! [`marker_tokens`]); this pass runs on the finished `syn::File`, rewrites
//! the extern signature, emits the wrapper beside it and resolves every
//! remaining marker (builder methods, factories) to the public scalar.
//!
//! A marker in a position the wrapper cannot convert (`Promise<f64>` in a
//! `_promise` companion, a callback's `Function<fn(f64)>`) is simply `f64`:
//! those forms are JS-typed by design.
//!
//! Overloads distinguished only by a string-literal argument are the second
//! use: the extern takes `&str` and the signature layer tags it (see
//! [`literal_attr`]) with one wrapper per literal, each baking its value in.
//! A literal type argument of a dictionary parameter is the third: the
//! wrapper sets the field it types on a copy of the caller's options.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::visit_mut::VisitMut;
use syn::{
    FnArg, ForeignItem, ForeignItemFn, GenericArgument, GenericParam, Generics, Ident, Item,
    ItemForeignMod, Pat, PathArguments, ReturnType, Type,
};

use crate::ir::RustScalar;

const MARKER_PREFIX: &str = "__TsGenWide";
const LITERAL_ATTR: &str = "ts_gen_literal";
const FIELD_LITERAL_ATTR: &str = "ts_gen_field_literal";
const DETERMINED_ATTR: &str = "ts_gen_determined";
const NULL_MARKER: &str = "__TsGenNull";

/// The attribute the signature layer puts on a literal-keyed extern fn:
/// `#[ts_gen_literal(positions(1), arm("text", get_text), arm("json", get_json))]`.
/// Positions index the parameters after `this`.
pub(crate) fn literal_attr(sig: &crate::codegen::signatures::FunctionSignature) -> TokenStream {
    let determined = (!sig.determined.is_empty()).then(|| {
        let attr = format_ident!("{DETERMINED_ATTR}");
        let positions = sig
            .determined
            .iter()
            .map(|p| proc_macro2::Literal::usize_unsuffixed(*p));
        quote! { #[#attr(#(#positions),*)] }
    });
    let attr = literal_attr_without_determined(sig);
    quote! { #determined #attr }
}

fn literal_attr_without_determined(
    sig: &crate::codegen::signatures::FunctionSignature,
) -> TokenStream {
    if let Some(field) = &sig.field_literal {
        let attr = format_ident!("{FIELD_LITERAL_ATTR}");
        let position = proc_macro2::Literal::usize_unsuffixed(field.position);
        let (name, value) = (&field.field, &field.value);
        let head = field.head.as_ref().map(|h| {
            let h = format_ident!("{h}");
            quote! { , #h }
        });
        return quote! { #[#attr(#position, #name, #value #head)] };
    }
    let Some(spec) = &sig.literal else {
        return quote! {};
    };
    let attr = format_ident!("{LITERAL_ATTR}");
    let positions = spec
        .positions
        .iter()
        .map(|p| proc_macro2::Literal::usize_unsuffixed(*p));
    let arms = spec.arms.iter().map(|(lits, name)| {
        let name = format_ident!("{name}");
        quote! { arm(#(#lits),*, #name) }
    });
    quote! { #[#attr(positions(#(#positions),*), #(#arms),*)] }
}

struct LiteralArms {
    positions: Vec<usize>,
    arms: Vec<(Vec<String>, Ident)>,
}

fn take_literal_attr(item: &mut ForeignItemFn) -> Option<LiteralArms> {
    let idx = item
        .attrs
        .iter()
        .position(|a| a.path().is_ident(LITERAL_ATTR))?;
    let attr = item.attrs.remove(idx);
    let syn::Meta::List(list) = attr.meta else {
        return None;
    };
    let parsed: LiteralArmsTokens = syn::parse2(list.tokens).ok()?;
    Some(LiteralArms {
        positions: parsed.positions,
        arms: parsed.arms,
    })
}

/// `#[ts_gen_field_literal(position, "field", "value", Head?)]`; see
/// [`crate::codegen::signatures::FieldLiteral`].
struct FieldLiteral {
    position: usize,
    field: String,
    value: String,
    head: Option<Ident>,
}

impl syn::parse::Parse for FieldLiteral {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let position: syn::LitInt = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let field: syn::LitStr = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let value: syn::LitStr = input.parse()?;
        let head = if input.parse::<Option<syn::Token![,]>>()?.is_some() {
            Some(input.parse()?)
        } else {
            None
        };
        Ok(Self {
            position: position.base10_parse()?,
            field: field.value(),
            value: value.value(),
            head,
        })
    }
}

/// `#[ts_gen_determined(positions…)]`: dictionaries the wrapper builds
/// with their factory; see [`crate::codegen::signatures::Determined`].
fn take_determined_attr(item: &mut ForeignItemFn) -> Vec<usize> {
    let Some(idx) = item
        .attrs
        .iter()
        .position(|a| a.path().is_ident(DETERMINED_ATTR))
    else {
        return Vec::new();
    };
    let attr = item.attrs.remove(idx);
    attr.parse_args_with(
        syn::punctuated::Punctuated::<syn::LitInt, syn::Token![,]>::parse_terminated,
    )
    .map(|list| list.iter().filter_map(|l| l.base10_parse().ok()).collect())
    .unwrap_or_default()
}

fn take_field_literal_attr(item: &mut ForeignItemFn) -> Option<FieldLiteral> {
    let idx = item
        .attrs
        .iter()
        .position(|a| a.path().is_ident(FIELD_LITERAL_ATTR))?;
    let attr = item.attrs.remove(idx);
    let syn::Meta::List(list) = attr.meta else {
        return None;
    };
    syn::parse2(list.tokens).ok()
}

struct LiteralArmsTokens {
    positions: Vec<usize>,
    arms: Vec<(Vec<String>, Ident)>,
}

impl syn::parse::Parse for LiteralArmsTokens {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut positions = Vec::new();
        let mut arms = Vec::new();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let content;
            syn::parenthesized!(content in input);
            if key == "positions" {
                let list = content.parse_terminated(syn::LitInt::parse, syn::Token![,])?;
                for lit in list {
                    positions.push(lit.base10_parse()?);
                }
            } else {
                let mut lits = Vec::new();
                while content.peek(syn::LitStr) {
                    let lit: syn::LitStr = content.parse()?;
                    lits.push(lit.value());
                    content.parse::<syn::Token![,]>()?;
                }
                let name: Ident = content.parse()?;
                arms.push((lits, name));
            }
            if input.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(Self { positions, arms })
    }
}

/// The placeholder type the emitters use for a 64-bit scalar on a `number`.
pub(crate) fn marker_tokens(scalar: RustScalar) -> TokenStream {
    let ident = format_ident!("{MARKER_PREFIX}{}", scalar.as_str().to_uppercase());
    quote! { #ident }
}

/// The placeholder type of an argument that can only be `null`: the extern
/// takes `&JsValue` and the wrapper drops the parameter, passing `null`.
pub(crate) fn null_marker_tokens() -> TokenStream {
    let ident = format_ident!("{NULL_MARKER}");
    quote! { #ident }
}

fn is_null_marker(ty: &Type) -> bool {
    let ty = match ty {
        Type::Reference(r) => &*r.elem,
        ty => ty,
    };
    matches!(ty, Type::Path(p) if p.path.is_ident(NULL_MARKER))
}

fn marker_scalar(ty: &Type) -> Option<RustScalar> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if p.path.segments.len() != 1 || !seg.arguments.is_none() {
        return None;
    }
    let name = seg.ident.to_string();
    let rest = name.strip_prefix(MARKER_PREFIX)?;
    RustScalar::parse(&rest.to_lowercase())
}

fn scalar_ident(scalar: RustScalar) -> Ident {
    Ident::new(scalar.as_str(), Span::call_site())
}

/// How a type converts between its public and extern spelling.
enum Shape {
    Marker(RustScalar),
    Option(Box<Shape>),
    Vec(Box<Shape>),
    /// `&[T]` in parameter position.
    Slice(Box<Shape>),
    Result(Box<Shape>),
    /// A parameter that can only be `null`, dropped from the wrapper.
    Null,
    /// No convertible marker inside; any marker is just `f64`.
    Plain,
}

impl Shape {
    fn converts(&self) -> bool {
        !matches!(self, Shape::Plain)
    }
}

fn shape(ty: &Type) -> Shape {
    if let Some(s) = marker_scalar(ty) {
        return Shape::Marker(s);
    }
    if is_null_marker(ty) {
        return Shape::Null;
    }
    match ty {
        Type::Reference(r) => {
            if let Type::Slice(slice) = &*r.elem {
                let inner = shape(&slice.elem);
                return if inner.converts() {
                    Shape::Slice(Box::new(inner))
                } else {
                    Shape::Plain
                };
            }
            Shape::Plain
        }
        Type::Path(p) => {
            let Some(seg) = p.path.segments.last() else {
                return Shape::Plain;
            };
            let PathArguments::AngleBracketed(args) = &seg.arguments else {
                return Shape::Plain;
            };
            let first = args.args.iter().find_map(|a| match a {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            });
            let Some(first) = first else {
                return Shape::Plain;
            };
            let inner = shape(first);
            if !inner.converts() {
                return Shape::Plain;
            }
            match seg.ident.to_string().as_str() {
                "Option" => Shape::Option(Box::new(inner)),
                "Vec" => Shape::Vec(Box::new(inner)),
                "Result" => Shape::Result(Box::new(inner)),
                _ => Shape::Plain,
            }
        }
        _ => Shape::Plain,
    }
}

/// `expr` of the public type → the extern type.
fn to_extern(expr: TokenStream, shape: &Shape) -> TokenStream {
    match shape {
        Shape::Marker(_) => quote! { #expr as f64 },
        Shape::Option(inner) => {
            let body = to_extern(quote! { v }, inner);
            quote! { #expr.map(|v| #body) }
        }
        Shape::Vec(inner) => {
            let body = to_extern(quote! { v }, inner);
            quote! { #expr.into_iter().map(|v| #body).collect::<Vec<_>>() }
        }
        Shape::Slice(inner) => {
            let body = to_extern(quote! { *v }, inner);
            quote! { &#expr.iter().map(|v| #body).collect::<Vec<_>>() }
        }
        Shape::Null => quote! { &::wasm_bindgen::JsValue::NULL },
        Shape::Result(_) | Shape::Plain => expr,
    }
}

/// `expr` of the extern type → the public type.
fn from_extern(expr: TokenStream, shape: &Shape) -> TokenStream {
    match shape {
        Shape::Marker(s) => {
            let s = scalar_ident(*s);
            quote! { #expr as #s }
        }
        Shape::Option(inner) | Shape::Result(inner) => {
            let body = from_extern(quote! { v }, inner);
            quote! { #expr.map(|v| #body) }
        }
        Shape::Vec(inner) => {
            let body = from_extern(quote! { v }, inner);
            quote! { #expr.into_iter().map(|v| #body).collect::<Vec<_>>() }
        }
        Shape::Slice(_) | Shape::Null | Shape::Plain => expr,
    }
}

struct Resolve {
    to_f64: bool,
}

impl VisitMut for Resolve {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if matches!(ty, Type::Path(p) if p.path.is_ident(NULL_MARKER)) {
            *ty = syn::parse_quote!(JsValue);
            return;
        }
        if let Some(s) = marker_scalar(ty) {
            let ident = if self.to_f64 {
                Ident::new("f64", Span::call_site())
            } else {
                scalar_ident(s)
            };
            *ty = syn::parse_quote!(#ident);
            return;
        }
        syn::visit_mut::visit_type_mut(self, ty);
    }
}

fn mentions_marker(tokens: &impl ToTokens) -> bool {
    let s = tokens.to_token_stream().to_string();
    s.contains(MARKER_PREFIX) || s.contains(NULL_MARKER)
}

/// Whether the block's `#[wasm_bindgen(...)]` sets `experimental_generic_mono`.
fn mentions_mono(block: &ItemForeignMod) -> bool {
    block.attrs.iter().any(|a| {
        a.path().is_ident("wasm_bindgen")
            && a.to_token_stream()
                .to_string()
                .contains("experimental_generic_mono")
    })
}

fn is_wrapper_attr(attr: &syn::Attribute) -> bool {
    [LITERAL_ATTR, FIELD_LITERAL_ATTR, DETERMINED_ATTR]
        .iter()
        .any(|name| attr.path().is_ident(name))
}

fn mentions_literal(block: &ItemForeignMod) -> bool {
    block.items.iter().any(|i| match i {
        ForeignItem::Fn(f) => f.attrs.iter().any(is_wrapper_attr),
        _ => false,
    })
}

/// What the extern fn binds, read off its `#[wasm_bindgen(...)]` attribute.
struct Binding {
    method: bool,
    setter: bool,
    constructor: bool,
    static_of: Option<Ident>,
    has_js_name: bool,
}

/// Reads the `#[wasm_bindgen(...)]` attribute, adding an empty one to a
/// bare free function so a `js_name` can be appended to it.
fn read_binding(item: &mut ForeignItemFn) -> Option<(usize, Binding)> {
    let mut binding = Binding {
        method: false,
        setter: false,
        constructor: false,
        static_of: None,
        has_js_name: false,
    };
    let Some(idx) = item
        .attrs
        .iter()
        .position(|a| a.path().is_ident("wasm_bindgen"))
    else {
        item.attrs.push(syn::parse_quote!(#[wasm_bindgen()]));
        return Some((item.attrs.len() - 1, binding));
    };
    let attr = &item.attrs[idx];
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("method") {
            binding.method = true;
        } else if meta.path.is_ident("setter") {
            binding.setter = true;
        } else if meta.path.is_ident("constructor") {
            binding.constructor = true;
        } else if meta.path.is_ident("static_method_of") {
            let value = meta.value()?;
            binding.static_of = Some(value.parse()?);
        } else if meta.path.is_ident("js_name") {
            binding.has_js_name = true;
            let _ = meta.value()?.parse::<syn::Lit>()?;
        } else if meta.input.peek(syn::Token![=]) {
            let _ = meta.value()?.parse::<syn::Expr>()?;
        }
        Ok(())
    });
    Some((idx, binding))
}

/// The receiver type of a `method` binding (`this: &Foo<A, B>`).
fn this_type(item: &ForeignItemFn) -> Option<&Type> {
    match item.sig.inputs.first()? {
        FnArg::Typed(pat) => match &*pat.ty {
            Type::Reference(r) => Some(&r.elem),
            _ => None,
        },
        FnArg::Receiver(_) => None,
    }
}

/// `Foo<A, B>` → (`Foo`, [`A`, `B`]) for a path whose arguments are bare idents.
fn type_head(ty: &Type) -> Option<(Ident, Vec<Ident>)> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    let args = match &seg.arguments {
        PathArguments::None => Vec::new(),
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .map(|a| match a {
                GenericArgument::Type(Type::Path(tp)) if tp.path.segments.len() == 1 => {
                    Some(tp.path.segments[0].ident.clone())
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
        PathArguments::Parenthesized(_) => return None,
    };
    Some((seg.ident.clone(), args))
}

/// Unwraps `Result<T, E>` to `T`.
fn result_ok(ty: &Type) -> &Type {
    if let Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            if seg.ident == "Result" {
                if let PathArguments::AngleBracketed(a) = &seg.arguments {
                    if let Some(GenericArgument::Type(t)) = a.args.first() {
                        return t;
                    }
                }
            }
        }
    }
    ty
}

/// Splits a fn's generics into those naming the impl target's arguments
/// (moved onto the `impl`) and the rest (kept on the wrapper fn).
fn split_generics(generics: &Generics, impl_args: &[Ident]) -> (Generics, Generics) {
    let mut on_impl = Generics::default();
    let mut on_fn = Generics::default();
    for param in &generics.params {
        let is_impl = match param {
            GenericParam::Type(t) => impl_args.contains(&t.ident),
            _ => false,
        };
        if is_impl {
            on_impl.params.push(param.clone());
        } else {
            on_fn.params.push(param.clone());
        }
    }
    (on_impl, on_fn)
}

/// The generics declared on the foreign `type` of this name, defaults stripped.
fn declared_generics(block: &ItemForeignMod, name: &Ident) -> Option<Generics> {
    block.items.iter().find_map(|item| match item {
        ForeignItem::Type(t) if &t.ident == name => {
            let mut g = t.generics.clone();
            for p in g.params.iter_mut() {
                if let GenericParam::Type(tp) = p {
                    tp.eq_token = None;
                    tp.default = None;
                }
            }
            Some(g)
        }
        _ => None,
    })
}

fn generic_args(generics: &Generics) -> TokenStream {
    if generics.params.is_empty() {
        return quote! {};
    }
    let idents = generics.params.iter().map(|p| match p {
        GenericParam::Type(t) => t.ident.to_token_stream(),
        GenericParam::Lifetime(l) => l.lifetime.to_token_stream(),
        GenericParam::Const(c) => c.ident.to_token_stream(),
    });
    quote! { <#(#idents),*> }
}

/// Where a wrapper lands.
#[derive(Clone)]
enum Target {
    /// `impl<G> Ty<Args> { pub fn f(&self, ..) }`
    Method {
        ty: Ident,
        generics: Generics,
        args: TokenStream,
    },
    /// `impl<G> Ty<Args> { pub fn f(..) }`
    Associated {
        ty: Ident,
        generics: Generics,
        args: TokenStream,
    },
    Free,
}

struct Wrapper {
    target: Target,
    item: TokenStream,
}

/// Whether `ty` names one of `params`.
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
/// type other than `Option`/`Vec`. A handle's ABI holds for every
/// instantiation (`&Foo<T>`, `&Function<fn(T)>`, `Promise<T>`), so only a
/// bare mention needs a bound.
fn mentions_bare(ty: &Type, params: &[Ident]) -> bool {
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

/// `&[E]` or `Option<&[E]>` → `E`.
fn slice_elem(ty: &Type) -> Option<&Type> {
    let ty = match ty {
        Type::Path(p) => {
            let seg = p.path.segments.last()?;
            let PathArguments::AngleBracketed(args) = &seg.arguments else {
                return None;
            };
            match (seg.ident == "Option", args.args.first()) {
                (true, Some(GenericArgument::Type(t))) => t,
                _ => return None,
            }
        }
        other => other,
    };
    match ty {
        Type::Reference(r) if r.mutability.is_none() => match &*r.elem {
            Type::Slice(slice) => Some(&slice.elem),
            _ => None,
        },
        _ => None,
    }
}

/// The `where` predicates `experimental_generic_mono` puts on an extern fn
/// for each argument and return type that names a type parameter. Its
/// wrapper calls it with those parameters, so it must state the ones that
/// do not hold for every instantiation (see [`mentions_bare`]). An awaited
/// result also has to be `'static`. The receiver (`skip`) holds for every
/// instantiation.
fn mono_bounds(sig: &syn::Signature, skip: usize, params: &[Ident]) -> Vec<syn::WherePredicate> {
    let mut out = mono_arg_bounds(sig.inputs.iter().skip(skip), params);
    if let ReturnType::Type(_, ty) = &sig.output {
        let ok = result_ok(ty);
        if sig.asyncness.is_some() && mentions(ok, params) {
            out.push(syn::parse_quote!(#ok: ::wasm_bindgen::convert::FromWasmAbi + 'static));
        } else if mentions_bare(ok, params) {
            out.push(syn::parse_quote!(#ok: ::wasm_bindgen::convert::FromWasmAbi + ::wasm_bindgen::describe::WasmDescribe));
        }
    }
    out
}

/// The argument half of [`mono_bounds`]: one predicate per typed argument
/// naming a type parameter bare. Receivers are skipped. A borrow nested in
/// the type (`Option<&T>`) is bound for every lifetime.
pub(crate) fn mono_arg_bounds<'a>(
    inputs: impl Iterator<Item = &'a FnArg>,
    params: &[Ident],
) -> Vec<syn::WherePredicate> {
    let mut out: Vec<syn::WherePredicate> = Vec::new();
    for arg in inputs {
        let FnArg::Typed(pat) = arg else { continue };
        let ty = &*pat.ty;
        if !mentions_bare(ty, params) {
            continue;
        }
        out.push(match (slice_elem(ty), ty) {
            (Some(elem), _) => {
                syn::parse_quote!(#elem: ::wasm_bindgen::convert::VectorRefIntoWasmAbi)
            }
            (None, Type::Reference(r)) if r.mutability.is_none() => {
                let elem = &r.elem;
                syn::parse_quote!(for<'__wbg> &'__wbg #elem: ::wasm_bindgen::convert::IntoWasmAbi + ::wasm_bindgen::describe::WasmDescribe)
            }
            _ => {
                let mut ty = ty.clone();
                if name_elided_lifetimes(&mut ty) {
                    syn::parse_quote!(for<'__wbg> #ty: ::wasm_bindgen::convert::IntoWasmAbi + ::wasm_bindgen::describe::WasmDescribe)
                } else {
                    syn::parse_quote!(#ty: ::wasm_bindgen::convert::IntoWasmAbi + ::wasm_bindgen::describe::WasmDescribe)
                }
            }
        });
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.to_token_stream().to_string()));
    out
}

/// Gives every elided borrow in `ty` the lifetime `'__wbg`; whether any
/// was found.
fn name_elided_lifetimes(ty: &mut Type) -> bool {
    struct Name(bool);
    impl VisitMut for Name {
        fn visit_type_reference_mut(&mut self, r: &mut syn::TypeReference) {
            if r.lifetime.is_none() {
                r.lifetime = Some(syn::Lifetime::new("'__wbg", Span::call_site()));
                self.0 = true;
            }
            syn::visit_mut::visit_type_reference_mut(self, r);
        }
    }
    let mut v = Name(false);
    v.visit_type_mut(ty);
    v.0
}

fn type_param_idents(generics: &Generics) -> impl Iterator<Item = Ident> + '_ {
    generics.params.iter().filter_map(|p| match p {
        GenericParam::Type(t) => Some(t.ident.clone()),
        _ => None,
    })
}

fn wrap(block: &ItemForeignMod, item: &mut ForeignItemFn, mono: bool) -> Vec<Wrapper> {
    let literal = take_literal_attr(item);
    let field = take_field_literal_attr(item);
    let determined = take_determined_attr(item);
    let Some((attr_idx, binding)) = read_binding(item) else {
        return Vec::new();
    };
    let return_shape = match &item.sig.output {
        ReturnType::Default => Shape::Plain,
        ReturnType::Type(_, ty) => shape(ty),
    };
    let skip = if binding.method { 1 } else { 0 };
    let param_shapes: Vec<(Ident, Shape)> = item
        .sig
        .inputs
        .iter()
        .skip(skip)
        .filter_map(|arg| match arg {
            FnArg::Typed(pat) => match &*pat.pat {
                Pat::Ident(id) => Some((id.ident.clone(), shape(&pat.ty))),
                _ => None,
            },
            FnArg::Receiver(_) => None,
        })
        .collect();
    let converts = return_shape.converts() || param_shapes.iter().any(|(_, s)| s.converts());
    if !converts && literal.is_none() && field.is_none() && determined.is_empty() {
        return Vec::new();
    }

    let public_name = item.sig.ident.clone();
    let extern_name = format_ident!("{public_name}_js");
    let doc_attrs: Vec<_> = item
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .cloned()
        .collect();

    // Public signature: convertible positions resolve to the scalar; a
    // marker the wrapper cannot convert (inside a callback type, a
    // `Promise<…>`) stays `f64` on both sides.
    // Parameter attributes (`slice = ..`) are the extern binding's.
    let mut public_sig = item.sig.clone();
    for arg in public_sig.inputs.iter_mut() {
        if let FnArg::Typed(pat) = arg {
            pat.attrs.clear();
            let to_f64 = !shape(&pat.ty).converts();
            Resolve { to_f64 }.visit_type_mut(&mut pat.ty);
        }
    }
    if let ReturnType::Type(_, ty) = &mut public_sig.output {
        let to_f64 = !return_shape.converts();
        Resolve { to_f64 }.visit_type_mut(ty);
    }
    // The caller's options take the dictionary's `Partial`, whose `field`
    // the wrapper sets.
    let field_arg = field.as_ref().and_then(|f| {
        let head = f.head.as_ref()?;
        match public_sig.inputs.iter_mut().nth(skip + f.position)? {
            FnArg::Typed(pat) => Some((head, &mut *pat.ty)),
            FnArg::Receiver(_) => None,
        }
    });
    if let Some((head, Type::Reference(r))) = field_arg {
        if let Type::Path(p) = &mut *r.elem {
            if let Some(last) = p.path.segments.last_mut() {
                last.ident = head.clone();
            }
        }
    }

    // Extern binding: markers resolve to f64, private name, explicit JS name
    // since wasm-bindgen would otherwise infer it from the new ident.
    Resolve { to_f64: true }.visit_signature_mut(&mut item.sig);
    item.sig.ident = extern_name.clone();
    item.vis = syn::Visibility::Inherited;
    if !binding.has_js_name && !binding.constructor {
        let js_name = if binding.setter {
            public_name
                .to_string()
                .strip_prefix("set_")
                .map(str::to_owned)
                .unwrap_or_else(|| public_name.to_string())
        } else {
            public_name.to_string()
        };
        let attr = &mut item.attrs[attr_idx];
        let syn::Meta::List(list) = &mut attr.meta else {
            return Vec::new();
        };
        let extra = if list.tokens.is_empty() {
            quote! { js_name = #js_name }
        } else {
            quote! { , js_name = #js_name }
        };
        list.tokens.extend(extra);
    }
    item.attrs.retain(|a| !a.path().is_ident("doc"));

    let is_async = public_sig.asyncness.is_some();
    let await_tokens = is_async.then(|| quote! { .await });
    let (target, callee) = if binding.method {
        let Some((ty, args)) = this_type(item).and_then(type_head) else {
            return Vec::new();
        };
        let (on_impl, on_fn) = split_generics(&public_sig.generics, &args);
        public_sig.generics = on_fn;
        let self_arg: FnArg = syn::parse_quote!(&self);
        match public_sig.inputs.first_mut() {
            Some(first) => *first = self_arg,
            None => return Vec::new(),
        }
        let args_tokens = generic_args(&on_impl);
        (
            Target::Method {
                ty,
                generics: on_impl,
                args: args_tokens,
            },
            quote! { self.#extern_name },
        )
    } else if let Some(ty) = binding.static_of {
        let generics = declared_generics(block, &ty).unwrap_or_default();
        let args = generic_args(&generics);
        (
            Target::Associated { ty, generics, args },
            quote! { Self::#extern_name },
        )
    } else if binding.constructor {
        let ReturnType::Type(_, ret) = &public_sig.output else {
            return Vec::new();
        };
        let Some((ty, args)) = type_head(result_ok(ret)) else {
            return Vec::new();
        };
        let (on_impl, on_fn) = split_generics(&public_sig.generics, &args);
        public_sig.generics = on_fn;
        let args_tokens = generic_args(&on_impl);
        (
            Target::Associated {
                ty,
                generics: on_impl,
                args: args_tokens,
            },
            quote! { Self::#extern_name },
        )
    } else {
        (Target::Free, quote! { #extern_name })
    };

    // One wrapper per literal arm (the literal parameters dropped from
    // the public signature, their values passed through), else one.
    let arms: Vec<(Vec<(usize, String)>, Ident)> = match &literal {
        Some(l) => l
            .arms
            .iter()
            .map(|(lits, name)| {
                (
                    l.positions
                        .iter()
                        .copied()
                        .zip(lits.iter().cloned())
                        .collect(),
                    name.clone(),
                )
            })
            .collect(),
        None => vec![(Vec::new(), public_name.clone())],
    };
    if mono {
        let impl_generics = match &target {
            Target::Method { generics, .. } | Target::Associated { generics, .. } => Some(generics),
            Target::Free => None,
        };
        let params: Vec<Ident> = type_param_idents(&public_sig.generics)
            .chain(impl_generics.into_iter().flat_map(type_param_idents))
            .collect();
        let bounds = mono_bounds(&item.sig, skip, &params);
        if !bounds.is_empty() {
            public_sig
                .generics
                .make_where_clause()
                .predicates
                .extend(bounds);
        }
    }
    let typed_inputs: Vec<FnArg> = public_sig.inputs.iter().skip(skip).cloned().collect();
    let receiver: Vec<FnArg> = public_sig.inputs.iter().take(skip).cloned().collect();
    arms.into_iter()
        .map(|(values, name)| {
            let call_args = param_shapes.iter().enumerate().map(|(i, (pname, s))| {
                match values.iter().find(|(pos, _)| *pos == i) {
                    Some((_, lit)) => quote! { #lit },
                    None if determined.contains(&i) => match typed_inputs.get(i) {
                        Some(FnArg::Typed(pat)) => {
                            let ty = match &*pat.ty {
                                Type::Reference(r) => &*r.elem,
                                ty => ty,
                            };
                            quote! { &<#ty>::new() }
                        }
                        _ => quote! { #pname },
                    },
                    None if field.as_ref().is_some_and(|f| f.position == i) => {
                        quote! { ::wasm_bindgen::JsCast::unchecked_ref(&#pname) }
                    }
                    None => to_extern(quote! { #pname }, s),
                }
            });
            let call = quote! { #callee(#(#call_args),*)#await_tokens };
            let body = from_extern(call, &return_shape);
            let body = match field
                .as_ref()
                .and_then(|f| Some((f, &param_shapes.get(f.position)?.0)))
            {
                Some((f, pname)) => {
                    let (name, value) = (&f.field, &f.value);
                    quote! {
                        let #pname: ::js_sys::Object = ::js_sys::Object::assign(
                            &::js_sys::Object::new(),
                            ::wasm_bindgen::JsCast::unchecked_ref(#pname),
                        );
                        let _ = ::js_sys::Reflect::set_str(
                            &#pname,
                            &::js_sys::JsString::from(#name),
                            &::wasm_bindgen::JsValue::from_str(#value),
                        );
                        #body
                    }
                }
                None => body,
            };
            let mut sig = public_sig.clone();
            sig.ident = name;
            sig.inputs = receiver
                .iter()
                .cloned()
                .chain(
                    typed_inputs
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| {
                            !values.iter().any(|(pos, _)| pos == i)
                                && !determined.contains(i)
                                && !matches!(param_shapes.get(*i), Some((_, Shape::Null)))
                        })
                        .map(|(_, a)| a.clone()),
                )
                .collect();
            let item = quote! {
                #(#doc_attrs)*
                pub #sig { #body }
            };
            Wrapper {
                target: target.clone(),
                item,
            }
        })
        .collect()
}

fn process_items(items: &mut Vec<Item>) {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        match &mut item {
            Item::Mod(m) => {
                if let Some((_, content)) = &mut m.content {
                    process_items(content);
                }
                out.push(item);
            }
            Item::ForeignMod(block) if mentions_marker(block) || mentions_literal(block) => {
                let snapshot = block.clone();
                let mono = mentions_mono(block);
                let mut wrappers = Vec::new();
                for fi in block.items.iter_mut() {
                    if let ForeignItem::Fn(f) = fi {
                        if mentions_marker(&f.sig) || f.attrs.iter().any(is_wrapper_attr) {
                            wrappers.extend(wrap(&snapshot, f, mono));
                        }
                    }
                }
                Resolve { to_f64: true }.visit_item_foreign_mod_mut(block);
                out.push(item);
                out.extend(render_wrappers(wrappers));
            }
            _ => {
                Resolve { to_f64: false }.visit_item_mut(&mut item);
                out.push(item);
            }
        }
    }
    *items = out;
}

/// One `impl` block per target, in first-seen order.
fn render_wrappers(wrappers: Vec<Wrapper>) -> Vec<Item> {
    let mut groups: Vec<(String, TokenStream, Vec<TokenStream>)> = Vec::new();
    let mut free = Vec::new();
    for w in wrappers {
        let header = match &w.target {
            Target::Method { ty, generics, args } | Target::Associated { ty, generics, args } => {
                quote! { impl #generics #ty #args }
            }
            Target::Free => {
                free.push(w.item);
                continue;
            }
        };
        let key = header.to_string();
        match groups.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, _, items)) => items.push(w.item),
            None => groups.push((key, header, vec![w.item])),
        }
    }
    let mut items = Vec::new();
    for (_, header, fns) in groups {
        items.push(syn::parse_quote! { #header { #(#fns)* } });
    }
    for f in free {
        items.push(syn::parse_quote! { #f });
    }
    items
}

/// Rewrite every marker in `file`, emitting wrappers for extern fns that
/// carry one in a convertible position.
pub(crate) fn apply(file: &mut syn::File) {
    let text = file.to_token_stream().to_string();
    if ![
        MARKER_PREFIX,
        NULL_MARKER,
        LITERAL_ATTR,
        FIELD_LITERAL_ATTR,
        DETERMINED_ATTR,
    ]
    .iter()
    .any(|m| text.contains(m))
    {
        return;
    }
    process_items(&mut file.items);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> String {
        let mut file: syn::File = syn::parse_str(src).unwrap();
        apply(&mut file);
        prettyplease::unparse(&file)
    }

    #[test]
    fn getter_and_setter_wrap_and_keep_the_js_name() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Foo;
                #[wasm_bindgen(method, getter)]
                pub fn size(this: &Foo) -> __TsGenWideU64;
                #[wasm_bindgen(method, setter)]
                pub fn set_size(this: &Foo, val: __TsGenWideU64);
            }");
        assert!(out.contains("#[wasm_bindgen(method, getter, js_name = \"size\")]"));
        assert!(out.contains("fn size_js(this: &Foo) -> f64;"));
        assert!(out.contains("#[wasm_bindgen(method, setter, js_name = \"size\")]"));
        assert!(out.contains("fn set_size_js(this: &Foo, val: f64);"));
        assert!(out.contains("pub fn size(&self) -> u64 {\n        self.size_js() as u64\n    }"));
        assert!(out.contains(
            "pub fn set_size(&self, val: u64) {\n        self.set_size_js(val as f64)\n    }"
        ));
        assert!(!out.contains("__TsGenWide"));
    }

    #[test]
    fn containers_convert_elementwise() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Foo;
                #[wasm_bindgen(method, catch)]
                pub fn sizes(this: &Foo, parts: &[__TsGenWideI64], n: Option<__TsGenWideU64>)
                    -> Result<Vec<__TsGenWideU64>, JsValue>;
            }");
        assert!(out.contains("parts: &[f64], n: Option<f64>"));
        assert!(out.contains("parts: &[i64], n: Option<u64>"));
        assert!(out.contains("&parts.iter().map(|v| *v as f64).collect::<Vec<_>>()"));
        assert!(out.contains("n.map(|v| v as f64)"));
        assert!(out.contains(".map(|v| v.into_iter().map(|v| v as u64).collect::<Vec<_>>())"));
    }

    #[test]
    fn unconvertible_positions_resolve_to_f64_without_a_wrapper() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Foo;
                #[wasm_bindgen(method)]
                pub fn count_promise(this: &Foo) -> Promise<__TsGenWideU64>;
            }
            impl FooBuilder {
                pub fn size(self, val: __TsGenWideU64) -> Self { self }
            }");
        assert!(out.contains("pub fn count_promise(this: &Foo) -> Promise<f64>;"));
        assert!(!out.contains("count_promise_js"));
        assert!(out.contains("pub fn size(self, val: u64) -> Self"));
    }

    #[test]
    fn literal_arms_drop_the_parameter_and_bake_the_value() {
        let out = run(
            "#[wasm_bindgen] extern \"C\" {
                pub type Kv;
                #[ts_gen_literal(positions(1), arm(\"text\", get_text), arm(\"arrayBuffer\", get_array_buffer))]
                #[wasm_bindgen(method, catch, js_name = \"get\")]
                pub async fn get_text(this: &Kv, key: &str, r#type: &str) -> Result<JsValue, JsValue>;
                #[ts_gen_literal(positions(0), arm(\"gzip\", new_gzip), arm(\"deflate-raw\", new_deflate_raw))]
                #[wasm_bindgen(constructor, catch)]
                pub fn new_gzip(format: &str) -> Result<Kv, JsValue>;
            }",
        );
        assert!(!out.contains("ts_gen_literal"));
        assert!(out.contains("async fn get_text_js(this: &Kv, key: &str, r#type: &str)"));
        assert!(out.contains("pub async fn get_text(&self, key: &str) -> Result<JsValue, JsValue> {\n        self.get_text_js(key, \"text\").await\n    }"));
        assert!(out.contains("pub async fn get_array_buffer(&self, key: &str) -> Result<JsValue, JsValue> {\n        self.get_text_js(key, \"arrayBuffer\").await\n    }"));
        assert!(out.contains("pub fn new_deflate_raw() -> Result<Kv, JsValue> {\n        Self::new_gzip_js(\"deflate-raw\")\n    }"));
    }

    #[test]
    fn mono_wrappers_restate_the_extern_bounds() {
        let out = run("#[wasm_bindgen(experimental_generic_mono)] extern \"C\" {
                pub type Kv;
                #[ts_gen_literal(positions(1), arm(\"json\", get_json))]
                #[wasm_bindgen(method, catch, js_name = \"get\")]
                pub async fn get_json<T>(this: &Kv, key: &str, r#type: &str, rows: &[T], one: &T)
                    -> Result<JsNullable<T>, JsValue>;
            }");
        assert!(out.contains("T: ::wasm_bindgen::convert::VectorRefIntoWasmAbi,"));
        assert!(out.contains("for<'__wbg> &'__wbg T: ::wasm_bindgen::convert::IntoWasmAbi"));
        assert!(out.contains("JsNullable<T>: ::wasm_bindgen::convert::FromWasmAbi + 'static,"));
        assert!(!out.contains("&'__wbg Kv"));
    }

    #[test]
    fn static_constructor_and_free_forms() {
        let out = run(
            "#[wasm_bindgen] extern \"C\" {
                pub type Foo<T: ::wasm_bindgen::JsGeneric = JsValue>;
                #[wasm_bindgen(static_method_of = Foo, js_name = \"maxSize\")]
                pub fn max_size<T: ::wasm_bindgen::JsGeneric>() -> __TsGenWideU64;
                #[wasm_bindgen(constructor, catch)]
                pub fn new<T: ::wasm_bindgen::JsGeneric>(n: __TsGenWideU64) -> Result<Foo<T>, JsValue>;
            }
            #[wasm_bindgen] extern \"C\" {
                pub fn now() -> __TsGenWideU64;
            }",
        );
        assert!(out.contains("impl<T: ::wasm_bindgen::JsGeneric> Foo<T>"));
        assert!(out.contains("Self::max_size_js() as u64"));
        assert!(out.contains("pub fn new(n: u64) -> Result<Foo<T>, JsValue> {\n        Self::new_js(n as f64)\n    }"));
        assert!(out.contains("#[wasm_bindgen(js_name = \"now\")]\n    fn now_js() -> f64;"));
        assert!(out.contains("pub fn now() -> u64 {\n    now_js() as u64\n}"));
    }
}
