//! A written `null` is an ordinary union arm: `f(x: T | null)` binds
//! `f(x)` and `f_with_null()`. The extern binding of a `null` arm keeps the
//! parameter as `&JsValue` under a private `__<name>` name, and a wrapper
//! of the public name drops it, passing `null` itself.
//!
//! The signature emitters render such a parameter as a placeholder type
//! (see [`marker`]); this pass runs on the finished `syn::File`, rewrites
//! each extern fn carrying one and emits its wrapper beside the block.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::ext::IdentExt;
use syn::{
    FnArg, ForeignItem, ForeignItemFn, GenericArgument, GenericParam, Generics, Ident, Item, Pat,
    PathArguments, ReturnType, Type, WherePredicate,
};

const MARKER: &str = "__TsGenNull";

/// The placeholder type of a parameter that can only be `null`.
pub(crate) fn marker() -> TokenStream {
    let ident = format_ident!("{MARKER}");
    quote! { &#ident }
}

pub(crate) fn is_marker(ty: &Type) -> bool {
    match ty {
        Type::Reference(r) => matches!(&*r.elem, Type::Path(p) if p.path.is_ident(MARKER)),
        _ => false,
    }
}

/// Rewrite every extern fn with a `null` arm, emitting its wrapper.
pub(crate) fn apply(items: &mut Vec<Item>) {
    let mut out = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        match &mut item {
            Item::Mod(m) => {
                if let Some((_, content)) = &mut m.content {
                    apply(content);
                }
                out.push(item);
            }
            Item::ForeignMod(block) => {
                let wrappers: Vec<Wrapper> = block
                    .items
                    .iter_mut()
                    .filter_map(|fi| match fi {
                        ForeignItem::Fn(f) => wrap(f),
                        _ => None,
                    })
                    .collect();
                out.push(item);
                out.extend(render(wrappers));
            }
            _ => out.push(item),
        }
    }
    *items = out;
}

/// What the extern fn binds, read off its `#[wasm_bindgen(...)]` attribute.
#[derive(Default)]
struct Binding {
    method: bool,
    setter: bool,
    constructor: bool,
    static_of: Option<Ident>,
    has_js_name: bool,
}

fn read_binding(item: &mut ForeignItemFn) -> (usize, Binding) {
    let mut binding = Binding::default();
    let Some(idx) = item
        .attrs
        .iter()
        .position(|a| a.path().is_ident("wasm_bindgen"))
    else {
        item.attrs.push(syn::parse_quote!(#[wasm_bindgen()]));
        return (item.attrs.len() - 1, binding);
    };
    let _ = item.attrs[idx].parse_nested_meta(|meta| {
        if meta.path.is_ident("method") {
            binding.method = true;
        } else if meta.path.is_ident("setter") {
            binding.setter = true;
        } else if meta.path.is_ident("constructor") {
            binding.constructor = true;
        } else if meta.path.is_ident("static_method_of") {
            binding.static_of = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("js_name") {
            binding.has_js_name = true;
            let _ = meta.value()?.parse::<syn::Lit>()?;
        } else if meta.input.peek(syn::Token![=]) {
            let _ = meta.value()?.parse::<syn::Expr>()?;
        }
        Ok(())
    });
    (idx, binding)
}

/// Where a wrapper lands: an `impl` block of a type, or the module.
enum Target {
    Impl {
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
                GenericArgument::Type(Type::Path(tp)) => tp.path.get_ident().cloned(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
        PathArguments::Parenthesized(_) => return None,
    };
    Some((seg.ident.clone(), args))
}

/// `T` of a `Result<T, E>`, else the type itself.
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
        match param {
            GenericParam::Type(t) if impl_args.contains(&t.ident) => {
                on_impl.params.push(param.clone())
            }
            _ => on_fn.params.push(param.clone()),
        }
    }
    on_fn.where_clause = generics.where_clause.clone();
    (on_impl, on_fn)
}

fn generic_args(generics: &Generics) -> TokenStream {
    let idents: Vec<&Ident> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(&t.ident),
            _ => None,
        })
        .collect();
    if idents.is_empty() {
        quote! {}
    } else {
        quote! { <#(#idents),*> }
    }
}

fn mentions(tokens: TokenStream, ident: &Ident) -> bool {
    tokens.into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == *ident,
        proc_macro2::TokenTree::Group(g) => mentions(g.stream(), ident),
        _ => false,
    })
}

/// Drops the type parameters only the `null` arm's parameter mentioned,
/// with the `where` predicates bounding them.
pub(crate) fn drop_unused_params(sig: &mut syn::Signature) {
    let used = |ident: &Ident, sig: &syn::Signature| {
        let others = sig.generics.params.iter().filter_map(|p| match p {
            GenericParam::Type(t) if t.ident != *ident => Some(t.to_token_stream()),
            _ => None,
        });
        sig.inputs
            .iter()
            .any(|i| mentions(i.to_token_stream(), ident))
            || mentions(sig.output.to_token_stream(), ident)
            || others.into_iter().any(|t| mentions(t, ident))
    };
    let unused: Vec<Ident> = sig
        .generics
        .type_params()
        .map(|t| t.ident.clone())
        .filter(|i| !used(i, sig))
        .collect();
    if unused.is_empty() {
        return;
    }
    sig.generics.params = std::mem::take(&mut sig.generics.params)
        .into_iter()
        .filter(|p| !matches!(p, GenericParam::Type(t) if unused.contains(&t.ident)))
        .collect();
    if let Some(w) = &mut sig.generics.where_clause {
        w.predicates = std::mem::take(&mut w.predicates)
            .into_iter()
            .filter(|p| match p {
                WherePredicate::Type(t) => !unused
                    .iter()
                    .any(|i| mentions(t.bounded_ty.to_token_stream(), i)),
                _ => true,
            })
            .collect();
        if w.predicates.is_empty() {
            sig.generics.where_clause = None;
        }
    }
}

fn wrap(item: &mut ForeignItemFn) -> Option<Wrapper> {
    let is_null_arm = |arg: &FnArg| matches!(arg, FnArg::Typed(pat) if is_marker(&pat.ty));
    if !item.sig.inputs.iter().any(is_null_arm) {
        return None;
    }
    let (attr_idx, binding) = read_binding(item);
    let public_name = item.sig.ident.clone();
    let extern_name = format_ident!("__{}", public_name.unraw());
    let skip = usize::from(binding.method);

    let call_args: Vec<TokenStream> = item
        .sig
        .inputs
        .iter()
        .skip(skip)
        .map(|arg| match arg {
            FnArg::Typed(pat) if is_marker(&pat.ty) => quote! { &::wasm_bindgen::JsValue::NULL },
            FnArg::Typed(pat) => match &*pat.pat {
                Pat::Ident(id) => id.ident.to_token_stream(),
                other => other.to_token_stream(),
            },
            FnArg::Receiver(r) => r.self_token.to_token_stream(),
        })
        .collect();

    // Public signature: the `null` parameter dropped; parameter attributes
    // (`slice = ..`) are the extern binding's.
    let mut public_sig = item.sig.clone();
    public_sig.inputs = public_sig
        .inputs
        .into_iter()
        .filter(|a| !is_null_arm(a))
        .map(|mut a| {
            if let FnArg::Typed(pat) = &mut a {
                pat.attrs.clear();
            }
            a
        })
        .collect();
    drop_unused_params(&mut public_sig);

    // Extern binding: `&JsValue`, private name, explicit JS name since
    // wasm-bindgen would otherwise infer it from the new ident.
    for arg in item.sig.inputs.iter_mut() {
        if let FnArg::Typed(pat) = arg {
            if is_marker(&pat.ty) {
                *pat.ty = syn::parse_quote!(&::wasm_bindgen::JsValue);
            }
        }
    }
    item.sig.generics = public_sig.generics.clone();
    item.sig.ident = extern_name.clone();
    item.vis = syn::Visibility::Inherited;
    if !binding.has_js_name && !binding.constructor {
        let name = public_name.to_string();
        let js_name = match binding.setter {
            true => name.strip_prefix("set_").unwrap_or(&name).to_string(),
            false => name,
        };
        if let syn::Meta::List(list) = &mut item.attrs[attr_idx].meta {
            let sep = (!list.tokens.is_empty()).then(|| quote! { , });
            list.tokens.extend(quote! { #sep js_name = #js_name });
        }
    }
    let docs: Vec<syn::Attribute> = item
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .cloned()
        .collect();
    let cfgs: Vec<syn::Attribute> = item
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .cloned()
        .collect();
    item.attrs.retain(|a| !a.path().is_ident("doc"));

    let await_tokens = public_sig.asyncness.is_some().then(|| quote! { .await });
    let assoc = |ty: Ident, args: &[Ident], sig: &mut syn::Signature| {
        let (on_impl, on_fn) = split_generics(&sig.generics, args);
        sig.generics = on_fn;
        Target::Impl {
            ty,
            args: generic_args(&on_impl),
            generics: on_impl,
        }
    };
    let (target, callee) = if binding.method {
        let this = match item.sig.inputs.first()? {
            FnArg::Typed(pat) => match &*pat.ty {
                Type::Reference(r) => (*r.elem).clone(),
                _ => return None,
            },
            FnArg::Receiver(_) => return None,
        };
        let (ty, args) = type_head(&this)?;
        let target = assoc(ty, &args, &mut public_sig);
        *public_sig.inputs.first_mut()? = syn::parse_quote!(&self);
        (target, quote! { self.#extern_name })
    } else if let Some(ty) = binding.static_of.clone() {
        // A bare `static_method_of` binds the type's defaults, unless the
        // static returns the type itself, which hoists its arguments.
        let args = match &public_sig.output {
            ReturnType::Type(_, ret) => type_head(result_ok(ret))
                .filter(|(head, _)| *head == ty)
                .map(|(_, args)| args)
                .unwrap_or_default(),
            ReturnType::Default => Vec::new(),
        };
        (
            assoc(ty, &args, &mut public_sig),
            quote! { Self::#extern_name },
        )
    } else if binding.constructor {
        let ReturnType::Type(_, ret) = &public_sig.output else {
            return None;
        };
        let (ty, args) = type_head(result_ok(ret))?;
        (
            assoc(ty, &args, &mut public_sig),
            quote! { Self::#extern_name },
        )
    } else {
        (Target::Free, quote! { #extern_name })
    };
    let body = quote! { #callee(#(#call_args),*)#await_tokens };
    Some(Wrapper {
        target,
        item: quote! {
            #(#docs)*
            #(#cfgs)*
            pub #public_sig { #body }
        },
    })
}

/// One `impl` block per target, in first-seen order, then free functions.
fn render(wrappers: Vec<Wrapper>) -> Vec<Item> {
    let mut groups: Vec<(String, TokenStream, Vec<TokenStream>)> = Vec::new();
    let mut free = Vec::new();
    for w in wrappers {
        let Target::Impl { ty, generics, args } = &w.target else {
            free.push(w.item);
            continue;
        };
        let header = quote! { impl #generics #ty #args };
        let key = header.to_string();
        match groups.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, _, items)) => items.push(w.item),
            None => groups.push((key, header, vec![w.item])),
        }
    }
    let mut items: Vec<Item> = groups
        .into_iter()
        .map(|(_, header, fns)| syn::parse_quote! { #header { #(#fns)* } })
        .collect();
    items.extend(free.into_iter().map(|f| -> Item {
        syn::parse_quote! { #f }
    }));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> String {
        let mut file: syn::File = syn::parse_str(src).unwrap();
        apply(&mut file.items);
        prettyplease::unparse(&file)
    }

    #[test]
    fn null_arms_bind_privately_and_wrap_without_the_parameter() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Foo;
                #[wasm_bindgen(method, setter, js_name = \"label\")]
                pub fn set_label_with_null(this: &Foo, val: &__TsGenNull);
                #[wasm_bindgen(js_name = \"put\")]
                pub fn put_with_null<V>(key: &str, value: &__TsGenNull);
                #[wasm_bindgen(constructor, catch)]
                pub fn new_with_null<T>(body: &__TsGenNull) -> Result<Holder<T>, JsValue>;
            }");
        assert!(
            out.contains("fn __set_label_with_null(this: &Foo, val: &::wasm_bindgen::JsValue);")
        );
        assert!(out.contains("pub fn set_label_with_null(&self) {"));
        assert!(out.contains("self.__set_label_with_null(&::wasm_bindgen::JsValue::NULL)"));
        assert!(out.contains("fn __put_with_null(key: &str, value: &::wasm_bindgen::JsValue);"));
        assert!(out.contains("pub fn put_with_null(key: &str) {"));
        assert!(out.contains("impl<T> Holder<T> {"));
        assert!(out.contains("pub fn new_with_null() -> Result<Holder<T>, JsValue> {"));
        assert!(!out.contains(MARKER));
    }
}
