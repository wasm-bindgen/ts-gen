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
//! A marker in a position the wrapper cannot convert (a callback's
//! `Function<fn(f64)>`) is simply `f64`: those forms are JS-typed by design.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::visit_mut::VisitMut;
use syn::{
    FnArg, ForeignItem, ForeignItemFn, GenericArgument, GenericParam, Generics, Ident, Item,
    ItemForeignMod, Pat, PathArguments, ReturnType, Type,
};

use crate::ir::RustScalar;

const MARKER_PREFIX: &str = "__TsGenWide";

/// The placeholder type the emitters use for a 64-bit scalar on a `number`.
pub(crate) fn marker_tokens(scalar: RustScalar) -> TokenStream {
    let ident = format_ident!("{MARKER_PREFIX}{}", scalar.as_str().to_uppercase());
    quote! { #ident }
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
    /// `Option<T>` or `Nullable<T>`, the read carriers of `T | undefined`
    /// and `T | null`, which both convert through `map`.
    Option(Box<Shape>),
    Vec(Box<Shape>),
    /// `&[T]` in parameter position.
    Slice(Box<Shape>),
    Result(Box<Shape>),
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
                "Option" | "Nullable" => Shape::Option(Box::new(inner)),
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
        Shape::Slice(_) | Shape::Plain => expr,
    }
}

struct Resolve {
    to_f64: bool,
}

impl VisitMut for Resolve {
    fn visit_type_mut(&mut self, ty: &mut Type) {
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
    tokens.to_token_stream().to_string().contains(MARKER_PREFIX)
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

fn wrap(block: &ItemForeignMod, item: &mut ForeignItemFn) -> Option<Wrapper> {
    let (attr_idx, binding) = read_binding(item)?;
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
    if !return_shape.converts() && !param_shapes.iter().any(|(_, s)| s.converts()) {
        return None;
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
            return None;
        };
        let extra = if list.tokens.is_empty() {
            quote! { js_name = #js_name }
        } else {
            quote! { , js_name = #js_name }
        };
        list.tokens.extend(extra);
    }
    item.attrs.retain(|a| !a.path().is_ident("doc"));

    let call_args = param_shapes
        .iter()
        .map(|(name, s)| to_extern(quote! { #name }, s));
    let is_async = public_sig.asyncness.is_some();
    let await_tokens = is_async.then(|| quote! { .await });
    let (target, call) = if binding.method {
        let (ty, args) = type_head(this_type(item)?)?;
        let (on_impl, on_fn) = split_generics(&public_sig.generics, &args);
        public_sig.generics = on_fn;
        let self_arg: FnArg = syn::parse_quote!(&self);
        *public_sig.inputs.first_mut()? = self_arg;
        let args_tokens = generic_args(&on_impl);
        (
            Target::Method {
                ty,
                generics: on_impl,
                args: args_tokens,
            },
            quote! { self.#extern_name(#(#call_args),*)#await_tokens },
        )
    } else if let Some(ty) = binding.static_of {
        let generics = declared_generics(block, &ty).unwrap_or_default();
        let args = generic_args(&generics);
        (
            Target::Associated { ty, generics, args },
            quote! { Self::#extern_name(#(#call_args),*)#await_tokens },
        )
    } else if binding.constructor {
        let ReturnType::Type(_, ret) = &public_sig.output else {
            return None;
        };
        let (ty, args) = type_head(result_ok(ret))?;
        let (on_impl, on_fn) = split_generics(&public_sig.generics, &args);
        public_sig.generics = on_fn;
        let args_tokens = generic_args(&on_impl);
        (
            Target::Associated {
                ty,
                generics: on_impl,
                args: args_tokens,
            },
            quote! { Self::#extern_name(#(#call_args),*)#await_tokens },
        )
    } else {
        (
            Target::Free,
            quote! { #extern_name(#(#call_args),*)#await_tokens },
        )
    };
    let body = from_extern(call, &return_shape);
    let item = quote! {
        #(#doc_attrs)*
        pub #public_sig { #body }
    };
    Some(Wrapper { target, item })
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
            Item::ForeignMod(block) if mentions_marker(block) => {
                let snapshot = block.clone();
                let mut wrappers = Vec::new();
                for fi in block.items.iter_mut() {
                    if let ForeignItem::Fn(f) = fi {
                        if mentions_marker(&f.sig) {
                            if let Some(w) = wrap(&snapshot, f) {
                                wrappers.push(w);
                            }
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
    if !mentions_marker(file) {
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
    fn nullable_reads_convert_through_map() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Foo;
                #[wasm_bindgen(method, getter)]
                pub fn at(this: &Foo) -> Nullable<__TsGenWideU64>;
                #[wasm_bindgen(method, getter)]
                pub fn maybe_at(this: &Foo) -> Option<Nullable<__TsGenWideU64>>;
            }");
        assert!(out.contains("fn at_js(this: &Foo) -> Nullable<f64>;"));
        assert!(out.contains(
            "pub fn at(&self) -> Nullable<u64> {\n        self.at_js().map(|v| v as u64)"
        ));
        assert!(out.contains("pub fn maybe_at(&self) -> Option<Nullable<u64>>"));
        assert!(out.contains("self.maybe_at_js().map(|v| v.map(|v| v as u64))"));
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
