//! Inline wrappers over extern bindings whose declared type the extern ABI
//! cannot express.
//!
//! The extern binding is kept under a private `__<name>` name and a
//! `pub fn` of the public name calls it. This pass runs on the finished
//! `syn::File`, rewrites the extern signature and emits the wrappers
//! beside its block.
//!
//! This module handles overloads distinguished only by a string-literal
//! argument (a lone `null` arm is [`null_arms`](super::null_arms)'): the
//! extern takes `&str` and the signature layer tags it (see
//! [`literal_attr`]) with one wrapper per literal, each baking its value in.
//! A cohort of `null` arms (`f(kind: "a" | "b", x: T | null)`) also drops
//! the `null` parameter here, and its union alias arguments convert here
//! (see [`into_target`]), since the arm wrappers are already the public
//! face of the extern.

use super::null_arms;
use super::signatures::{FunctionSignature, PromiseForm};
use super::typemap::CodegenContext;
use crate::ir::TypeRef;
use crate::parse::scope::ScopeId;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{
    FnArg, ForeignItem, ForeignItemFn, GenericArgument, GenericParam, Generics, Ident, ImplItem,
    Item, ItemForeignMod, Pat, PathArguments, ReturnType, Signature, Type, WherePredicate,
};

const LITERAL_ATTR: &str = "ts_gen_literal";
const BUFFER_READ_ATTR: &str = "ts_gen_buffer_read";
/// Names the extern a wrapper calls, so [`copy_abi_bounds`] can find it.
const WRAPS_ATTR: &str = "ts_gen_wraps";

/// The attribute the signature layer puts on a literal-keyed extern fn:
/// `#[ts_gen_literal(binding(__get), positions(1), arm("text", get_text), arm("json", get_json))]`.
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
    let fields = &spec.fields;
    let fields = (!fields.is_empty()).then(|| quote! { fields(#(#fields),*), });
    let made = spec
        .made
        .iter()
        .map(|p| proc_macro2::Literal::usize_unsuffixed(*p));
    let made = (!spec.made.is_empty()).then(|| quote! { made(#(#made),*), });
    quote! { #[#attr(binding(#binding), positions(#(#positions),*), #fields #made #(#arms),*)] }
}

struct LiteralArms {
    binding: Ident,
    positions: Vec<usize>,
    /// `fields("type")`: the literals fill these fields of the parameters
    /// at the positions, which the wrapper keeps.
    fields: Vec<String>,
    /// `made(0)`: the parameters at these positions are the wrapper's to
    /// make, filled, rather than the caller's.
    made: Vec<usize>,
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
        let mut fields = Vec::new();
        let mut made = Vec::new();
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
            } else if key == "made" {
                let list = content.parse_terminated(syn::LitInt::parse, syn::Token![,])?;
                for lit in list {
                    made.push(lit.base10_parse()?);
                }
            } else if key == "fields" {
                let list = content
                    .parse_terminated(<syn::LitStr as syn::parse::Parse>::parse, syn::Token![,])?;
                fields.extend(list.iter().map(syn::LitStr::value));
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
            fields,
            made,
            arms,
        })
    }
}

/// The enum a parameter typed `impl ::core::convert::Into<Alias>`
/// converts to: the signature layer's spelling of a named union alias
/// argument (see [`union_alias_argument`]). The extern binding takes
/// `Alias` by value, as a wasm-bindgen enum crosses the ABI, and the
/// wrapper over it keeps the `impl Into` and converts.
///
/// [`union_alias_argument`]: super::typemap::union_alias_argument
pub(crate) fn into_target(ty: &Type) -> Option<Type> {
    let Type::ImplTrait(imp) = ty else {
        return None;
    };
    let [syn::TypeParamBound::Trait(bound)] = imp.bounds.iter().collect::<Vec<_>>()[..] else {
        return None;
    };
    let path = &bound.path;
    let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    if path.leading_colon.is_none() || names != ["core", "convert", "Into"] {
        return None;
    }
    match &path.segments.last()?.arguments {
        PathArguments::AngleBracketed(a) => match a.args.first()? {
            GenericArgument::Type(t) if a.args.len() == 1 => Some(t.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Rewrites every `impl Into<Alias>` parameter of an extern binding to
/// `Alias`, returning whether there was one.
pub(crate) fn bind_into_targets(sig: &mut Signature) -> bool {
    let mut any = false;
    for arg in sig.inputs.iter_mut() {
        if let FnArg::Typed(pat) = arg {
            if let Some(target) = into_target(&pat.ty) {
                *pat.ty = target;
                any = true;
            }
        }
    }
    any
}

fn is_literal_keyed(item: &ForeignItemFn) -> bool {
    item.attrs.iter().any(|a| a.path().is_ident(LITERAL_ATTR))
}

fn mentions_literal(block: &ItemForeignMod) -> bool {
    block.items.iter().any(|i| match i {
        ForeignItem::Fn(f) => is_literal_keyed(f) || reads_buffer(f),
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

fn wrap(item: &mut ForeignItemFn) -> Vec<Wrapper> {
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
    // Parameters taking a union alias whole, converted by every arm.
    let into_positions: Vec<usize> = item
        .sig
        .inputs
        .iter()
        .skip(skip)
        .enumerate()
        .filter(|(_, arg)| matches!(arg, FnArg::Typed(pat) if into_target(&pat.ty).is_some()))
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

    bind_into_targets(&mut item.sig);

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
    let Some((target, callee)) = place(item, &binding, &mut public_sig, &extern_name) else {
        return Vec::new();
    };

    // One wrapper per literal arm: the literal and `null` parameters
    // dropped from the public signature, their values passed through.
    let filled = !literal.fields.is_empty();
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
                    Some(_) if filled => {
                        quote! { ::wasm_bindgen::JsCast::unchecked_ref(&#pname) }
                    }
                    Some((_, lit)) => quote! { #lit },
                    None if null_positions.contains(&i) => {
                        quote! { &::wasm_bindgen::JsValue::NULL }
                    }
                    None if into_positions.contains(&i) => quote! { #pname.into() },
                    None => quote! { #pname },
                }
            });
            let body = quote! { #callee(#(#call_args),*)#await_tokens };
            // A filled parameter is passed as a copy with its fields set.
            let fills =
                values
                    .iter()
                    .zip(&literal.fields)
                    .enumerate()
                    .map(|(n, ((pos, lit), field))| {
                        let pname = &param_names[*pos];
                        let first = !values[..n].iter().any(|(p, _)| p == pos);
                        let copy = match (first, literal.made.contains(pos)) {
                            (false, _) => None,
                            (true, true) => Some(quote! {
                                let #pname = ::js_sys::Object::new();
                            }),
                            (true, false) => Some(quote! {
                                let #pname: ::js_sys::Object = ::js_sys::Object::assign(
                                    &::js_sys::Object::new(),
                                    ::wasm_bindgen::JsCast::unchecked_ref(#pname),
                                );
                            }),
                        };
                        quote! {
                            #copy
                            let _ = ::js_sys::Reflect::set_str(
                                &#pname,
                                &::js_sys::JsString::from(#field),
                                &::wasm_bindgen::JsValue::from_str(#lit),
                            );
                        }
                    });
            let body = quote! { #(#fills)* #body };
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
                            (filled || !values.iter().any(|(pos, _)| pos == i))
                                && !null_positions.contains(i)
                                && !literal.made.contains(i)
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

/// Where a wrapper over `item` goes and how it reaches the extern
/// `extern_name`. A method's receiver becomes `&self`, and the generics
/// its `impl` states leave `public_sig`.
fn place(
    item: &ForeignItemFn,
    binding: &Binding,
    public_sig: &mut Signature,
    extern_name: &Ident,
) -> Option<(Target, TokenStream)> {
    Some(if binding.method {
        let (ty, args) = this_type(item).and_then(type_head)?;
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
            quote! { self.#extern_name },
        )
    } else if let Some(ty) = binding.static_of.clone() {
        // A bare `static_method_of` binds the type's defaults, unless the
        // static returns the type itself, which hoists its arguments (as
        // in `null_arms`).
        let args = match &public_sig.output {
            ReturnType::Type(_, ret) => type_head(result_ok(ret))
                .filter(|(head, _)| *head == ty)
                .map(|(_, args)| args)
                .unwrap_or_default(),
            ReturnType::Default => Vec::new(),
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
            quote! { Self::#extern_name },
        )
    } else {
        (Target::Free, quote! { #extern_name })
    })
}

/// The attribute the signature layer puts on an extern fn reading a
/// `buffer`. The extern keeps the handle as `<name>_js_buffer` and a
/// wrapper of the public name reads its bytes.
pub(crate) fn buffer_read_attr(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TokenStream {
    let core = |t: &TypeRef| t.split_empties().map_or_else(|| t.clone(), |(c, _)| c);
    let mut t = core(ty);
    if let TypeRef::Reference {
        segments,
        generic_args,
    } = &t
    {
        if segments.len() == 1 && segments[0] == "Promise" && generic_args.len() == 1 {
            t = core(&generic_args[0]);
        }
    }
    if !super::signatures::names_buffer(&t, cgctx, scope) {
        return quote! {};
    }
    let attr = format_ident!("{BUFFER_READ_ATTR}");
    quote! { #[#attr] }
}

/// [`buffer_read_attr`] for a callable's result.
pub(crate) fn buffer_result_attr(
    sig: &FunctionSignature,
    cgctx: Option<&CodegenContext<'_>>,
) -> TokenStream {
    if sig.literal.is_some() || sig.js_string_return || matches!(sig.form, PromiseForm::Handle(_)) {
        return quote! {};
    }
    buffer_read_attr(&sig.return_type, cgctx, sig.body_scope)
}

fn reads_buffer(item: &ForeignItemFn) -> bool {
    item.attrs
        .iter()
        .any(|a| a.path().is_ident(BUFFER_READ_ATTR))
}

/// `ret` with its handle replaced by `Vec<u8>` and by `JsValue`, and
/// `call`, returning the latter, read as the former: under `Result<_, E>`,
/// `Option<_>` or both.
fn bytes_return(ret: &Type, call: TokenStream) -> Option<(Type, Type, TokenStream)> {
    fn args_of(ty: &Type, name: &str) -> Option<Vec<Type>> {
        let Type::Path(p) = ty else {
            return None;
        };
        let seg = p.path.segments.last()?;
        let PathArguments::AngleBracketed(args) = &seg.arguments else {
            return None;
        };
        (seg.ident == name).then(|| {
            args.args
                .iter()
                .filter_map(|a| match a {
                    GenericArgument::Type(t) => Some(t.clone()),
                    _ => None,
                })
                .collect()
        })
    }
    let (ok, err) = match args_of(ret, "Result").as_deref() {
        Some([ok, err]) => (ok.clone(), Some(err.clone())),
        _ => (ret.clone(), None),
    };
    let (handle, optional) = match args_of(&ok, "Option").as_deref() {
        Some([t]) => (t.clone(), true),
        _ => (ok, false),
    };
    // A class, or the enum of the two: a `Promise<T>` stays the handle.
    let Type::Path(p) = &handle else {
        return None;
    };
    if !p.path.segments.last()?.arguments.is_none() {
        return None;
    }
    let read = quote! { __ts_gen_buffer_bytes(&v) };
    let (mut bytes, mut value, read): (Type, Type, _) = match optional {
        true => (
            syn::parse_quote!(Option<Vec<u8>>),
            syn::parse_quote!(Option<JsValue>),
            quote! { v.map(|v| #read) },
        ),
        false => (syn::parse_quote!(Vec<u8>), syn::parse_quote!(JsValue), read),
    };
    let body = match &err {
        Some(_) => quote! { #call.map(|v| #read) },
        None => quote! { let v = #call; #read },
    };
    if let Some(err) = err {
        bytes = syn::parse_quote!(Result<#bytes, #err>);
        value = syn::parse_quote!(Result<#value, #err>);
    }
    Some((bytes, value, body))
}

/// The wrapper reading a `buffer` result's bytes, and the private extern
/// it calls, which takes the result as any value: the handle's enum admits
/// one view class. `item` becomes the `_js_buffer` twin.
fn wrap_buffer_read(item: &mut ForeignItemFn) -> Option<(Wrapper, ForeignItemFn)> {
    let idx = item
        .attrs
        .iter()
        .position(|a| a.path().is_ident(BUFFER_READ_ATTR))?;
    item.attrs.remove(idx);
    // A literal-keyed extern and one dropping a `null` parameter already
    // have their wrappers, which keep the handle.
    let marker = |arg: &FnArg| matches!(arg, FnArg::Typed(pat) if null_arms::is_marker(&pat.ty));
    if is_literal_keyed(item) || item.sig.inputs.iter().any(marker) {
        return None;
    }
    let (attr_idx, binding) = read_binding(item)?;
    if binding.constructor || binding.setter {
        return None;
    }
    let public_name = item.sig.ident.clone();
    let js_name = syn::ext::IdentExt::unraw(&public_name).to_string();
    let extern_name = format_ident!("__{js_name}");
    let mut public_sig = item.sig.clone();
    for arg in public_sig.inputs.iter_mut() {
        if let FnArg::Typed(pat) = arg {
            pat.attrs.clear();
        }
    }
    let args = item
        .sig
        .inputs
        .iter()
        .skip(usize::from(binding.method))
        .filter_map(|arg| match arg {
            FnArg::Typed(pat) => match &*pat.pat {
                Pat::Ident(id) => {
                    let name = &id.ident;
                    Some(match into_target(&pat.ty) {
                        Some(_) => quote! { #name.into() },
                        None => quote! { #name },
                    })
                }
                _ => None,
            },
            FnArg::Receiver(_) => None,
        });
    let call_args: Vec<TokenStream> = args.collect();
    let (target, callee) = place(item, &binding, &mut public_sig, &extern_name)?;
    let await_tokens = public_sig.asyncness.map(|_| quote! { .await });
    let ReturnType::Type(arrow, ret) = &public_sig.output else {
        return None;
    };
    let arrow = *arrow;
    let (bytes, value, body) = bytes_return(ret, quote! { #callee(#(#call_args),*)#await_tokens })?;
    public_sig.output = ReturnType::Type(arrow, Box::new(bytes));

    if !binding.has_js_name {
        let syn::Meta::List(list) = &mut item.attrs[attr_idx].meta else {
            return None;
        };
        let extra = match list.tokens.is_empty() {
            true => quote! { js_name = #js_name },
            false => quote! { , js_name = #js_name },
        };
        list.tokens.extend(extra);
    }
    let mut private = item.clone();
    private.sig.ident = extern_name.clone();
    private.sig.output = ReturnType::Type(arrow, Box::new(value));
    private.vis = syn::Visibility::Inherited;
    private.attrs.retain(|a| !a.path().is_ident("doc"));
    bind_into_targets(&mut private.sig);
    item.sig.ident = format_ident!("{js_name}_js_buffer");

    let doc_attrs = item.attrs.iter().filter(|a| a.path().is_ident("doc"));
    let wraps = format_ident!("{WRAPS_ATTR}");
    let wrapper = Wrapper {
        target,
        item: quote! {
            #(#doc_attrs)*
            #[#wraps(#extern_name)]
            pub #public_sig { #body }
        },
    };
    Some((wrapper, private))
}

/// The bytes of a `buffer` handle: a view contributes its own window of
/// its `ArrayBuffer`, whatever its element.
fn buffer_bytes_helper() -> Vec<Item> {
    let file: syn::File = syn::parse_quote! {
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_name = "Uint8Array")]
            type __TsGenBytes;
            #[wasm_bindgen(constructor, js_class = "Uint8Array")]
            fn new(buffer: &JsValue, offset: f64, length: f64) -> __TsGenBytes;
            #[wasm_bindgen(method, getter, js_class = "Uint8Array")]
            fn buffer(this: &__TsGenBytes) -> JsValue;
            #[wasm_bindgen(method, getter, js_class = "Uint8Array", js_name = "byteOffset")]
            fn byte_offset(this: &__TsGenBytes) -> f64;
            #[wasm_bindgen(method, getter, js_class = "Uint8Array", js_name = "byteLength")]
            fn byte_length(this: &__TsGenBytes) -> f64;
            #[wasm_bindgen(js_namespace = ArrayBuffer, js_name = "isView")]
            fn __ts_gen_is_view(value: &JsValue) -> bool;
        }
        fn __ts_gen_buffer_bytes(value: &JsValue) -> Vec<u8> {
            let view: &__TsGenBytes = ::wasm_bindgen::JsCast::unchecked_ref(value);
            let bytes = match __ts_gen_is_view(value) {
                true => __TsGenBytes::new(&view.buffer(), view.byte_offset(), view.byte_length()),
                false => __TsGenBytes::new(value, 0.0, view.byte_length()),
            };
            ::wasm_bindgen::JsCast::unchecked_into::<::js_sys::Uint8Array>(bytes).to_vec()
        }
    };
    file.items
}

fn process_items(items: &mut Vec<Item>) {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut reads = false;
    for mut item in items.drain(..) {
        match &mut item {
            Item::Mod(m) => {
                if let Some((_, content)) = &mut m.content {
                    process_items(content);
                }
                out.push(item);
            }
            Item::ForeignMod(block) if mentions_literal(block) => {
                let mut wrappers = Vec::new();
                let mut items = Vec::with_capacity(block.items.len());
                for mut fi in block.items.drain(..) {
                    let mut private = None;
                    if let ForeignItem::Fn(f) = &mut fi {
                        if let Some((w, p)) = wrap_buffer_read(f) {
                            wrappers.push(w);
                            private = Some(ForeignItem::Fn(p));
                            reads = true;
                        }
                        if is_literal_keyed(f) {
                            wrappers.extend(wrap(f));
                        }
                    }
                    items.push(fi);
                    items.extend(private);
                }
                block.items = items;
                out.push(item);
                out.extend(render_wrappers(wrappers));
            }
            _ => out.push(item),
        }
    }
    if reads {
        out.extend(buffer_bytes_helper());
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
    if !text.contains(LITERAL_ATTR) && !text.contains(BUFFER_READ_ATTR) {
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
                #[ts_gen_literal(binding(__get_text), positions(1), arm(\"text\", get_text), arm(\"arrayBuffer\", get_array_buffer))]
                #[wasm_bindgen(method, catch, js_name = \"get\")]
                pub async fn get_text(this: &Kv, key: &str, r#type: &str) -> Result<JsValue, JsValue>;
                #[ts_gen_literal(binding(__new), positions(0), arm(\"gzip\", new_gzip), arm(\"deflate-raw\", new_deflate_raw))]
                #[wasm_bindgen(constructor, catch)]
                pub fn new_gzip(format: &str) -> Result<Kv, JsValue>;
            }",
        );
        assert!(!out.contains("ts_gen_literal"));
        assert!(out.contains("async fn __get_text(this: &Kv, key: &str, r#type: &str)"));
        assert!(out.contains("pub async fn get_text(&self, key: &str) -> Result<JsValue, JsValue> {\n        self.__get_text(key, \"text\").await\n    }"));
        assert!(out.contains("pub async fn get_array_buffer(&self, key: &str) -> Result<JsValue, JsValue> {\n        self.__get_text(key, \"arrayBuffer\").await\n    }"));
        assert!(out.contains("pub fn new_deflate_raw() -> Result<Kv, JsValue> {\n        Self::__new(\"deflate-raw\")\n    }"));
    }

    #[test]
    fn null_arm_of_a_literal_cohort_is_dropped_and_passed_as_null() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Picker;
                #[ts_gen_literal(binding(__put_with_null), positions(0), arm(\"a\", put_a_with_null), arm(\"b\", put_b_with_null))]
                #[wasm_bindgen(method, js_name = \"put\")]
                pub fn put_a_with_null<T>(this: &Picker, kind: &str, x: &__TsGenNull, y: f64);
            }");
        assert!(!out.contains("__TsGenNull"), "{out}");
        assert!(out.contains(
            "fn __put_with_null(this: &Picker, kind: &str, x: &::wasm_bindgen::JsValue, y: f64);"
        ), "{out}");
        assert!(out.contains("pub fn put_b_with_null(&self, y: f64) {\n        self.__put_with_null(\"b\", &::wasm_bindgen::JsValue::NULL, y)\n    }"), "{out}");
        // A type parameter only the `null` parameter could name goes.
        assert!(!out.contains("<T>"), "{out}");

        // `null_arms` runs after and leaves the rewritten extern alone.
        let mut file: syn::File = syn::parse_str(&out).unwrap();
        crate::codegen::null_arms::apply(&mut file.items);
        assert_eq!(prettyplease::unparse(&file), out);
    }

    #[test]
    fn literal_arms_convert_union_alias_arguments() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Day;
                #[ts_gen_literal(binding(__step), positions(0), arm(\"next\", step_next), arm(\"previous\", step_previous))]
                #[wasm_bindgen(method, js_name = \"step\")]
                pub fn step_next(this: &Day, direction: &str, other: impl ::core::convert::Into<DayLike>) -> f64;
            }");
        assert!(
            out.contains("fn __step(this: &Day, direction: &str, other: DayLike) -> f64;"),
            "{out}"
        );
        assert!(out.contains("pub fn step_previous(&self, other: impl ::core::convert::Into<DayLike>) -> f64 {\n        self.__step(\"previous\", other.into())"), "{out}");
        // Nothing is left for `null_arms` to wrap again.
        let mut file: syn::File = syn::parse_str(&out).unwrap();
        crate::codegen::null_arms::apply(&mut file.items);
        assert_eq!(prettyplease::unparse(&file), out);
    }

    #[test]
    fn into_target_reads_only_the_core_into_bound() {
        let target = |ty: &str| {
            into_target(&syn::parse_str(ty).unwrap()).map(|t| t.to_token_stream().to_string())
        };
        assert_eq!(
            target("impl ::core::convert::Into<a::DayLike>").as_deref(),
            Some("a :: DayLike")
        );
        assert_eq!(target("impl Into<DayLike>"), None);
        assert_eq!(target("impl ::wasm_bindgen::JsStringLike"), None);
        assert_eq!(target("&DayLike"), None);
    }

    #[test]
    fn static_and_free_forms() {
        let out = run("#[wasm_bindgen] extern \"C\" {
                pub type Foo<T: ::wasm_bindgen::JsGeneric = JsValue>;
                #[ts_gen_literal(binding(__make), positions(0), arm(\"a\", make_a), arm(\"b\", make_b))]
                #[wasm_bindgen(static_method_of = Foo, js_name = \"make\")]
                pub fn make_a<T: ::wasm_bindgen::JsGeneric>(kind: &str) -> Foo<T>;
                #[ts_gen_literal(binding(__label), positions(0), arm(\"a\", label_a), arm(\"b\", label_b))]
                #[wasm_bindgen(static_method_of = Foo, js_name = \"label\")]
                pub fn label_a(kind: &str) -> String;
            }
            #[wasm_bindgen] extern \"C\" {
                #[ts_gen_literal(binding(__pick), positions(0), arm(\"x\", pick_x), arm(\"y\", pick_y))]
                #[wasm_bindgen(js_name = \"pick\")]
                pub fn pick_x(axis: &str) -> f64;
            }");
        assert!(out.contains("impl<T: ::wasm_bindgen::JsGeneric> Foo<T>"));
        assert!(out.contains("pub fn make_b() -> Foo<T> {\n        Self::__make(\"b\")\n    }"));
        // A static not returning its type binds the type's defaults.
        assert!(
            out.contains(
                "impl Foo {\n    pub fn label_a() -> String {\n        Self::__label(\"a\")\n    }"
            ),
            "{out}"
        );
        assert!(
            out.contains("#[wasm_bindgen(js_name = \"pick\")]\n    fn __pick(axis: &str) -> f64;")
        );
        assert!(out.contains("pub fn pick_y() -> f64 {\n    __pick(\"y\")\n}"));
    }
}
