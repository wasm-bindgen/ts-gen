//! Inline wrappers over extern bindings whose declared type the extern ABI
//! cannot express.
//!
//! The extern binding is kept under a private `<name>_js` name and a
//! `pub fn` of the public name calls it. This pass runs on the finished
//! `syn::File`, rewrites the extern signature and emits the wrappers
//! beside its block.
//!
//! This module handles overloads distinguished only by a string-literal
//! argument (a lone `null` arm is [`null_arms`](super::null_arms)'): the
//! extern takes `&str` and the signature layer tags it (see
//! [`literal_attr`]) with one wrapper per literal, each baking its value in.
//! A cohort of `null` arms (`f(kind: "a" | "b", x: T | null)`) also drops
//! the `null` parameter here, since the arm wrappers are already the public
//! face of the extern.

use super::null_arms;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{
    FnArg, ForeignItem, ForeignItemFn, GenericArgument, GenericParam, Generics, Ident, ImplItem,
    Item, ItemForeignMod, Pat, PathArguments, ReturnType, Signature, Type, WherePredicate,
};

const LITERAL_ATTR: &str = "ts_gen_literal";
/// Names the extern a wrapper calls, so [`copy_abi_bounds`] can find it.
const WRAPS_ATTR: &str = "ts_gen_wraps";

/// The attribute the signature layer puts on a literal-keyed extern fn:
/// `#[ts_gen_literal(binding(get_js), positions(1), arm("text", get_text), arm("json", get_json))]`.
/// The extern is renamed to `binding`; positions index the parameters
/// after `this`.
pub(crate) fn literal_attr(sig: &crate::codegen::signatures::FunctionSignature) -> TokenStream {
    let Some(spec) = &sig.literal else {
        return quote! {};
    };
    let attr = format_ident!("{LITERAL_ATTR}");
    let binding = format_ident!("{}", spec.binding);
    let positions = spec
        .positions
        .iter()
        .map(|p| proc_macro2::Literal::usize_unsuffixed(*p));
    let arms = spec.arms.iter().map(|(lits, name)| {
        let name = format_ident!("{name}");
        quote! { arm(#(#lits),*, #name) }
    });
    quote! { #[#attr(binding(#binding), positions(#(#positions),*), #(#arms),*)] }
}

struct LiteralArms {
    binding: Ident,
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
    syn::parse2(list.tokens).ok()
}

impl syn::parse::Parse for LiteralArms {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut binding = None;
        let mut positions = Vec::new();
        let mut arms = Vec::new();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let content;
            syn::parenthesized!(content in input);
            if key == "binding" {
                binding = Some(content.parse()?);
            } else if key == "positions" {
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
        let binding = binding.ok_or_else(|| input.error("missing binding(..)"))?;
        Ok(Self {
            binding,
            positions,
            arms,
        })
    }
}

fn is_literal_keyed(item: &ForeignItemFn) -> bool {
    item.attrs.iter().any(|a| a.path().is_ident(LITERAL_ATTR))
}

fn mentions_literal(block: &ItemForeignMod) -> bool {
    block.items.iter().any(|i| match i {
        ForeignItem::Fn(f) => is_literal_keyed(f),
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

fn wrap(block: &ItemForeignMod, item: &mut ForeignItemFn) -> Vec<Wrapper> {
    let Some(literal) = take_literal_attr(item) else {
        return Vec::new();
    };
    let Some((attr_idx, binding)) = read_binding(item) else {
        return Vec::new();
    };
    let skip = if binding.method { 1 } else { 0 };
    let is_null_arm =
        |arg: &FnArg| matches!(arg, FnArg::Typed(pat) if null_arms::is_marker(&pat.ty));
    // Parameters that can only be `null`, indexed like the literal
    // positions: every arm drops them and passes `null` itself.
    let null_positions: Vec<usize> = item
        .sig
        .inputs
        .iter()
        .skip(skip)
        .enumerate()
        .filter(|(_, arg)| is_null_arm(arg))
        .map(|(i, _)| i)
        .collect();
    let param_names: Vec<Ident> = item
        .sig
        .inputs
        .iter()
        .skip(skip)
        .filter_map(|arg| match arg {
            FnArg::Typed(pat) => match &*pat.pat {
                Pat::Ident(id) => Some(id.ident.clone()),
                _ => None,
            },
            FnArg::Receiver(_) => None,
        })
        .collect();

    let public_name = item.sig.ident.clone();
    let extern_name = literal.binding.clone();
    let doc_attrs: Vec<_> = item
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .cloned()
        .collect();

    // Parameter attributes (`slice = ..`) are the extern binding's.
    let mut public_sig = item.sig.clone();
    for arg in public_sig.inputs.iter_mut() {
        if let FnArg::Typed(pat) = arg {
            pat.attrs.clear();
        }
    }
    if !null_positions.is_empty() {
        // Type parameters only the `null` parameter mentioned go with it,
        // on the wrappers and on the extern, which binds it as `&JsValue`.
        let mut pruned = public_sig.clone();
        pruned.inputs = pruned
            .inputs
            .into_iter()
            .filter(|a| !is_null_arm(a))
            .collect();
        null_arms::drop_unused_params(&mut pruned);
        public_sig.generics = pruned.generics;
        for arg in item.sig.inputs.iter_mut() {
            if let FnArg::Typed(pat) = arg {
                if null_arms::is_marker(&pat.ty) {
                    *pat.ty = syn::parse_quote!(&::wasm_bindgen::JsValue);
                }
            }
        }
        item.sig.generics = public_sig.generics.clone();
    }

    // Extern binding: private name, explicit JS name since wasm-bindgen
    // would otherwise infer it from the new ident.
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
        // wasm-bindgen puts a static method in `impl<G> Ty<G>`, binding
        // the type's parameters there rather than on the fn.
        let generics = declared_generics(block, &ty).unwrap_or_default();
        let declared: Vec<Ident> = generics.type_params().map(|t| t.ident.clone()).collect();
        public_sig.generics = split_generics(&public_sig.generics, &declared).1;
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

    // One wrapper per literal arm: the literal and `null` parameters
    // dropped from the public signature, their values passed through.
    let typed_inputs: Vec<FnArg> = public_sig.inputs.iter().skip(skip).cloned().collect();
    let receiver: Vec<FnArg> = public_sig.inputs.iter().take(skip).cloned().collect();
    literal
        .arms
        .into_iter()
        .map(|(lits, name)| {
            let values: Vec<(usize, String)> =
                literal.positions.iter().copied().zip(lits).collect();
            let call_args = param_names.iter().enumerate().map(|(i, pname)| {
                match values.iter().find(|(pos, _)| *pos == i) {
                    Some((_, lit)) => quote! { #lit },
                    None if null_positions.contains(&i) => {
                        quote! { &::wasm_bindgen::JsValue::NULL }
                    }
                    None => quote! { #pname },
                }
            });
            let body = quote! { #callee(#(#call_args),*)#await_tokens };
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
                            !values.iter().any(|(pos, _)| pos == i) && !null_positions.contains(i)
                        })
                        .map(|(_, a)| a.clone()),
                )
                .collect();
            let wraps = format_ident!("{WRAPS_ATTR}");
            let item = quote! {
                #(#doc_attrs)*
                #[#wraps(#extern_name)]
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
            Item::ForeignMod(block) if mentions_literal(block) => {
                let snapshot = block.clone();
                let mut wrappers = Vec::new();
                for fi in block.items.iter_mut() {
                    if let ForeignItem::Fn(f) = fi {
                        if is_literal_keyed(f) {
                            wrappers.extend(wrap(&snapshot, f));
                        }
                    }
                }
                out.push(item);
                out.extend(render_wrappers(wrappers));
            }
            _ => out.push(item),
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

/// Emit the wrappers for every extern fn in `file` that needs one.
pub(crate) fn apply(file: &mut syn::File) {
    let text = file.to_token_stream().to_string();
    if !text.contains(LITERAL_ATTR) {
        return;
    }
    process_items(&mut file.items);
}

/// Copy each extern's final `where` predicates onto the wrappers over it.
/// The ABI bounds (`T: FromWasmAbi`, `ExpectedValue: NullableFromWasmAbi`)
/// are stated on the extern items by
/// [`state_abi_bounds`](super::signatures::state_abi_bounds), which runs
/// after the wrappers are emitted; a wrapper calling the extern needs the
/// same bounds. Wrappers follow their extern block, so each is matched
/// against the block before it.
pub(crate) fn copy_abi_bounds(items: &mut [Item]) {
    let mut bounds: Vec<(Ident, Vec<WherePredicate>)> = Vec::new();
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, content)) = &mut m.content {
                    copy_abi_bounds(content);
                }
            }
            Item::ForeignMod(block) => {
                bounds = block
                    .items
                    .iter()
                    .filter_map(|fi| match fi {
                        ForeignItem::Fn(f) => Some((
                            f.sig.ident.clone(),
                            f.sig
                                .generics
                                .where_clause
                                .iter()
                                .flat_map(|w| w.predicates.iter().cloned())
                                .collect(),
                        )),
                        _ => None,
                    })
                    .collect();
            }
            Item::Impl(imp) => {
                for ii in &mut imp.items {
                    if let ImplItem::Fn(f) = ii {
                        bound_wrapper(&mut f.attrs, &mut f.sig, &bounds);
                    }
                }
            }
            Item::Fn(f) => bound_wrapper(&mut f.attrs, &mut f.sig, &bounds),
            _ => {}
        }
    }
}

fn bound_wrapper(
    attrs: &mut Vec<syn::Attribute>,
    sig: &mut Signature,
    bounds: &[(Ident, Vec<WherePredicate>)],
) {
    let Some(idx) = attrs.iter().position(|a| a.path().is_ident(WRAPS_ATTR)) else {
        return;
    };
    let Ok(extern_name) = attrs.remove(idx).parse_args::<Ident>() else {
        return;
    };
    // A `view` / `copy` split repeats the extern under one name, with the
    // same bounds.
    let Some((_, predicates)) = bounds.iter().find(|(name, _)| *name == extern_name) else {
        return;
    };
    if predicates.is_empty() {
        return;
    }
    let clause = sig.generics.make_where_clause();
    for p in predicates {
        if !clause.predicates.iter().any(|q| q == p) {
            clause.predicates.push(p.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> String {
        let mut file: syn::File = syn::parse_str(src).unwrap();
        apply(&mut file);
        copy_abi_bounds(&mut file.items);
        prettyplease::unparse(&file)
    }

    #[test]
    fn literal_arms_drop_the_parameter_and_bake_the_value() {
        let out = run(
            "#[wasm_bindgen] extern \"C\" {
                pub type Kv;
                #[ts_gen_literal(binding(get_text_js), positions(1), arm(\"text\", get_text), arm(\"arrayBuffer\", get_array_buffer))]
                #[wasm_bindgen(method, catch, js_name = \"get\")]
                pub async fn get_text(this: &Kv, key: &str, r#type: &str) -> Result<JsValue, JsValue>;
                #[ts_gen_literal(binding(new_js), positions(0), arm(\"gzip\", new_gzip), arm(\"deflate-raw\", new_deflate_raw))]
                #[wasm_bindgen(constructor, catch)]
                pub fn new_gzip(format: &str) -> Result<Kv, JsValue>;
            }",
        );
        assert!(!out.contains("ts_gen_literal"));
        assert!(out.contains("async fn get_text_js(this: &Kv, key: &str, r#type: &str)"));
        assert!(out.contains("pub async fn get_text(&self, key: &str) -> Result<JsValue, JsValue> {\n        self.get_text_js(key, \"text\").await\n    }"));
        assert!(out.contains("pub async fn get_array_buffer(&self, key: &str) -> Result<JsValue, JsValue> {\n        self.get_text_js(key, \"arrayBuffer\").await\n    }"));
        assert!(out.contains("pub fn new_deflate_raw() -> Result<Kv, JsValue> {\n        Self::new_js(\"deflate-raw\")\n    }"));
    }

    #[test]
    fn null_arm_of_a_literal_cohort_is_dropped_and_passed_as_null() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Picker;
                #[ts_gen_literal(binding(put_with_null_js), positions(0), arm(\"a\", put_a_with_null), arm(\"b\", put_b_with_null))]
                #[wasm_bindgen(method, js_name = \"put\")]
                pub fn put_a_with_null<T>(this: &Picker, kind: &str, x: &__TsGenNull, y: f64);
            }");
        assert!(!out.contains("__TsGenNull"), "{out}");
        assert!(out.contains(
            "fn put_with_null_js(this: &Picker, kind: &str, x: &::wasm_bindgen::JsValue, y: f64);"
        ), "{out}");
        assert!(out.contains("pub fn put_b_with_null(&self, y: f64) {\n        self.put_with_null_js(\"b\", &::wasm_bindgen::JsValue::NULL, y)\n    }"), "{out}");
        // A type parameter only the `null` parameter could name goes.
        assert!(!out.contains("<T>"), "{out}");

        // `null_arms` runs after and leaves the rewritten extern alone.
        let mut file: syn::File = syn::parse_str(&out).unwrap();
        crate::codegen::null_arms::apply(&mut file.items);
        assert_eq!(prettyplease::unparse(&file), out);
    }

    #[test]
    fn static_and_free_forms() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Foo<T: ::wasm_bindgen::JsGeneric = JsValue>;
                #[ts_gen_literal(binding(make_js), positions(0), arm(\"a\", make_a), arm(\"b\", make_b))]
                #[wasm_bindgen(static_method_of = Foo, js_name = \"make\")]
                pub fn make_a<T: ::wasm_bindgen::JsGeneric>(kind: &str) -> Foo<T>;
            }
            #[wasm_bindgen] extern \"C\" {
                #[ts_gen_literal(binding(pick_js), positions(0), arm(\"x\", pick_x), arm(\"y\", pick_y))]
                #[wasm_bindgen(js_name = \"pick\")]
                pub fn pick_x(axis: &str) -> f64;
            }");
        assert!(out.contains("impl<T: ::wasm_bindgen::JsGeneric> Foo<T>"));
        assert!(out.contains("pub fn make_b() -> Foo<T> {\n        Self::make_js(\"b\")\n    }"));
        assert!(
            out.contains("#[wasm_bindgen(js_name = \"pick\")]\n    fn pick_js(axis: &str) -> f64;")
        );
        assert!(out.contains("pub fn pick_y() -> f64 {\n    pick_js(\"y\")\n}"));
    }
}
