//! The `_typed` twins of keyed members (CONVENTIONS.md, "Keyed members"):
//! a `K extends keyof M` that types a parameter binds a second time with
//! the key a marker type, which names the property typing the rest.
//!
//! A twin wraps the plain binding, the erased reading: it passes the
//! marker's key and its arguments as the types the plain binding takes.

use std::cell::RefCell;
use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use ts_gen_abi::ir as abi;
use ts_gen_abi::keyed::{self, Map};
use ts_gen_abi::resolve::{Context, Symbols, Target, TypeDecl};

use super::typemap::{to_syn_type, CodegenContext, TypePosition};
use ts_gen::context::TypeId;
use ts_gen::ir::{ModuleContext, TypeRef};
use ts_gen::parse::scope::ScopeId;

/// The IR with its keys kept, and the map modules its twins name.
pub struct Keyed {
    ir: abi::Ir,
    libs: Vec<&'static abi::Ir>,
    /// The declarations of the input the output emits.
    emitted: HashSet<TypeId>,
    /// The key modules emitted, by map.
    modules: RefCell<Vec<(String, TokenStream)>>,
    /// The twins of a class over its map parameter, again for each class
    /// that extends it at a declared map: its parent in Rust is the class
    /// at its default.
    forwards: RefCell<Vec<(String, TokenStream, Vec<TokenStream>)>>,
}

/// How a binding is called.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Receiver {
    Method,
    Static,
    Function,
}

/// Where a twin's types are rendered.
#[derive(Clone, Copy)]
pub(crate) struct At<'a> {
    pub cgctx: &'a CodegenContext<'a>,
    pub scope: ScopeId,
    pub module: &'a ModuleContext,
}

impl Keyed {
    pub fn new(ir: abi::Ir, libs: Vec<&'static abi::Ir>, emitted: HashSet<TypeId>) -> Keyed {
        Keyed {
            ir,
            libs,
            emitted,
            modules: RefCell::default(),
            forwards: RefCell::default(),
        }
    }

    /// The key trait and the modules of every map a twin names.
    pub fn support(&self) -> TokenStream {
        let modules = self.modules.borrow();
        if modules.is_empty() {
            return quote! {};
        }
        let modules = modules.iter().map(|m| &m.1);
        let forwards = self.forwards.borrow();
        let forwards = forwards.iter().map(|(_, this, twins)| {
            quote! {
                impl #this {
                    #(#twins)*
                }
            }
        });
        quote! {
            /// A key of the map `M`: the property it names, and the type
            /// of that property's value.
            pub trait MapKey<M: ?Sized> {
                type Value: ::wasm_bindgen::JsGeneric;
                const KEY: &'static str;
            }
            #(#modules)*
            #(#forwards)*
        }
    }

    /// The twin of `binding`, a plain binding of the callable `js_name` of
    /// the class `owner`, or of the top level: a function for an `impl` of
    /// the class, or a free one. `own` names the class's type parameters.
    pub(crate) fn twin(
        &self,
        owner: Option<&str>,
        js_name: &str,
        receiver: Receiver,
        binding: TokenStream,
        own: &dyn Fn(&syn::Ident) -> bool,
        at: At<'_>,
    ) -> Option<TokenStream> {
        let f = match syn::parse2::<syn::ForeignItemFn>(binding.clone()) {
            Ok(f) => f,
            // A free function is alone in its extern block.
            Err(_) => match syn::parse2::<syn::ItemForeignMod>(binding)
                .ok()?
                .items
                .pop()?
            {
                syn::ForeignItem::Fn(f) => f,
                _ => return None,
            },
        };
        let mut inputs = Vec::new();
        for arg in &f.sig.inputs {
            let syn::FnArg::Typed(p) = arg else {
                return None;
            };
            let syn::Pat::Ident(name) = &*p.pat else {
                return None;
            };
            inputs.push((name.ident.clone(), (*p.ty).clone()));
        }
        if receiver == Receiver::Method {
            inputs.remove(0);
        }
        let sym = Symbols::with_libs(&self.ir, &self.libs);
        let class = owner.and_then(|o| {
            self.ir.items.iter().find_map(|d| match d {
                abi::Decl::Class(c) if c.name == o => Some(c),
                _ => None,
            })
        });
        if owner.is_some() && class.is_none() {
            return None;
        }
        let members: Vec<&abi::Member> = match class {
            Some(c) => c.members.iter().collect(),
            None => self
                .ir
                .items
                .iter()
                .filter_map(|d| match d {
                    abi::Decl::Member(m) => Some(m),
                    _ => None,
                })
                .collect(),
        };
        let ccx = match class {
            Some(c) => Context::default().with_class(c),
            None => Context::default(),
        };
        let mut found = Vec::new();
        for m in members {
            let named = m.meta.attrs.js.iter().find_map(|e| match e {
                abi::JsEntry::Name(n) => Some(n.as_str()),
                _ => None,
            });
            let abi::MemberKind::Method(name) = &m.kind else {
                continue;
            };
            if named.unwrap_or(name) != js_name
                || m.is_static != (receiver == Receiver::Static)
                || m.params.len() != inputs.len()
            {
                continue;
            }
            let cx = ccx.with_tparams(&m.tparams);
            if let Some(twin) = self.retyped(&sym, m, &cx, &inputs, at) {
                found.push(twin);
            }
        }
        // The arms of a union are overloads the plain bindings name apart.
        let [(key, map, params)] = &found[..] else {
            return None;
        };
        self.modules_of(&sym, class, map, at);
        let map = self.map_tokens(&sym, map, &ccx, at)?;

        let plain = &f.sig.ident;
        let name = plain.to_string();
        let name = match name.strip_suffix("_promise") {
            Some(base) => format_ident!("{base}_typed_promise"),
            None => format_ident!("{name}_typed"),
        };
        let docs = f.attrs.iter().filter(|a| a.path().is_ident("doc"));
        let mut generics: Vec<TokenStream> = f
            .sig
            .generics
            .params
            .iter()
            .filter(|p| !matches!(p, syn::GenericParam::Type(t) if own(&t.ident)))
            .map(|p| quote! { #p })
            .collect();
        generics.push(quote! { K: MapKey<#map> });
        let where_clause = &f.sig.generics.where_clause;
        let declared = params
            .iter()
            .enumerate()
            .filter(|(i, _)| i != key)
            .map(|(_, (name, ty, _))| quote! { #name: #ty });
        let args = params.iter().enumerate().map(|(i, (name, _, cast))| {
            if i == *key {
                return quote! { <K as MapKey<#map>>::KEY };
            }
            match cast {
                Cast::None => quote! { #name },
                Cast::Ref => quote! { ::wasm_bindgen::JsCast::unchecked_ref(#name) },
                Cast::OptionRef => {
                    quote! { #name.map(::wasm_bindgen::JsCast::unchecked_ref) }
                }
            }
        });
        let (this, call) = match receiver {
            Receiver::Method => (quote! { &self, }, quote! { self.#plain }),
            Receiver::Static => (quote! {}, quote! { Self::#plain }),
            Receiver::Function => (quote! {}, quote! { #plain }),
        };
        let asyncness = &f.sig.asyncness;
        let awaited = asyncness.map(|_| quote! { .await });
        let output = &f.sig.output;
        let twin = quote! {
            #(#docs)*
            pub #asyncness fn #name <#(#generics),*> (#this #(#declared),*) #output #where_clause {
                #call(#(#args),*) #awaited
            }
        };
        if let (Receiver::Method, Some(class)) = (receiver, class) {
            self.forward(&sym, class, found[0].1, &twin, at);
        }
        Some(twin)
    }

    /// `twin`, of `class` over its parameter `map`, for each class that
    /// extends `class` at a declared map.
    fn forward(
        &self,
        sym: &Symbols<'_>,
        class: &abi::Class,
        map: &abi::Type,
        twin: &TokenStream,
        at: At<'_>,
    ) {
        let abi::Type::Named(r) = map else { return };
        let [param] = &r.path.segments[..] else {
            return;
        };
        let Some(i) = class.tparams.iter().position(|p| p.name == *param) else {
            return;
        };
        let cx = Context::default();
        for d in &self.ir.items {
            let abi::Decl::Class(sub) = d else { continue };
            let parent = sub
                .extends
                .iter()
                .find(|e| e.path.segments == [class.name.as_str()]);
            let Some(abi::Type::Named(arg)) = parent.and_then(|e| e.args.get(i)) else {
                continue;
            };
            let Some(Target::Decl(loc, TypeDecl::Class(_))) = sym.resolve(&arg.path, &cx) else {
                continue;
            };
            if !sub.tparams.is_empty() || loc.module.is_some() || !arg.args.is_empty() {
                continue;
            }
            if !self.emitted(std::slice::from_ref(&sub.name), at) || !self.emitted(&loc.path, at) {
                continue;
            }
            let this = self.named(std::slice::from_ref(&sub.name), at);
            let body = replace_ident(twin.clone(), param, &self.named(&loc.path, at));
            let mut forwards = self.forwards.borrow_mut();
            match forwards.iter_mut().find(|f| f.0 == sub.name) {
                Some(f) => f.2.push(body),
                None => forwards.push((sub.name.clone(), this, vec![body])),
            }
        }
    }

    /// The parameters of the plain binding `inputs` as the keyed member
    /// `m` types them: its key parameter's position, its map, and each
    /// parameter with how the plain binding takes it. `None` when the
    /// binding isn't `m`'s, or a twin can't state `m`.
    #[allow(clippy::type_complexity)]
    fn retyped<'m>(
        &self,
        sym: &Symbols<'_>,
        m: &'m abi::Member,
        cx: &Context<'_>,
        inputs: &[(syn::Ident, syn::Type)],
        at: At<'_>,
    ) -> Option<(usize, &'m abi::Type, Vec<(syn::Ident, syn::Type, Cast)>)> {
        let mut keys = m.tparams.iter().filter_map(|p| match &p.bound {
            Some(abi::Bound::KeyOf(map)) => Some((p.name.as_str(), map)),
            _ => None,
        });
        let (k, map) = keys.next()?;
        if keys.next().is_some() || keyed::is_keyed(sym, &m.result, cx) {
            return None;
        }
        let is_key = |t: &abi::Type| matches!(t, abi::Type::Named(r) if r.args.is_empty() && r.path.segments == [k]);
        let mut key = m.params.iter().enumerate().filter(|(_, p)| is_key(&p.ty));
        let (key, _) = key.next()?;
        // The key is passed as a `&'static str`.
        let (_, key_ty) = &inputs[key];
        let key_ty = quote!(#key_ty).to_string();
        if key_ty != "& str" && !key_ty.ends_with("JsStringLike") {
            return None;
        }
        let map_tokens = self.map_tokens(sym, map, cx, at)?;
        let value: syn::Type = syn::parse_quote! { <K as MapKey<#map_tokens>>::Value };
        let mut typed = false;
        let mut params = Vec::new();
        for (i, (p, (name, ty))) in m.params.iter().zip(inputs).enumerate() {
            let mut ty = ty.clone();
            let mut cast = Cast::None;
            if i != key && keyed::is_keyed(sym, &p.ty, cx) {
                cast = match &ty {
                    syn::Type::Reference(r) if r.mutability.is_none() => Cast::Ref,
                    t if option_ref(t) => Cast::OptionRef,
                    _ => return None,
                };
                if !retype(sym, &p.ty, cx, &mut ty, &value, k) {
                    return None;
                }
                typed = true;
            }
            params.push((name.clone(), ty, cast));
        }
        typed.then_some((key, map, params))
    }

    /// `map` in Rust: a type parameter by name, a declaration as a
    /// reference to it reads.
    fn map_tokens(
        &self,
        sym: &Symbols<'_>,
        map: &abi::Type,
        cx: &Context<'_>,
        at: At<'_>,
    ) -> Option<TokenStream> {
        let abi::Type::Named(r) = map else {
            return None;
        };
        match sym.resolve(&r.path, cx)? {
            Target::TypeParam => {
                let name = super::typemap::make_ident(&r.path.segments[0]);
                Some(quote! { #name })
            }
            Target::Decl(loc, TypeDecl::Class(_))
                if loc.module.is_none() && r.args.is_empty() && self.emitted(&loc.path, at) =>
            {
                Some(self.named(&loc.path, at))
            }
            _ => None,
        }
    }

    /// Whether `path` names something the output has: anything but a
    /// declaration of the input that `--export` leaves out.
    fn emitted(&self, path: &[String], at: At<'_>) -> bool {
        let gctx = at.cgctx.gctx;
        gctx.resolve_path(at.scope, &path.join("."))
            .is_none_or(|id| self.emitted.contains(&id) || gctx.builtin_type_ids.contains(&id))
    }

    fn named(&self, path: &[String], at: At<'_>) -> TokenStream {
        let t = TypeRef::Reference {
            segments: path.to_vec(),
            generic_args: Vec::new(),
        };
        to_syn_type(
            &t,
            TypePosition::RETURN.to_inner(),
            Some(at.cgctx),
            at.scope,
            at.module,
        )
    }

    /// Emit the key modules of `map`: its own when it is declared, and
    /// those of every map a reference to `class` gives its parameter.
    fn modules_of(
        &self,
        sym: &Symbols<'_>,
        class: Option<&abi::Class>,
        map: &abi::Type,
        at: At<'_>,
    ) {
        let abi::Type::Named(r) = map else { return };
        let mut maps: Vec<abi::Type> = Vec::new();
        let position = class.and_then(|c| {
            let [name] = &r.path.segments[..] else {
                return None;
            };
            let i = c.tparams.iter().position(|p| p.name == *name)?;
            Some((c.name.as_str(), i))
        });
        match position {
            Some((class, i)) => {
                let mut visit = |t: &abi::Type| {
                    t.walk(&mut |t| {
                        let abi::Type::Named(r) = t else { return };
                        if r.path.segments == [class] {
                            if let Some(arg) = r.args.get(i) {
                                if !maps.contains(arg) {
                                    maps.push(arg.clone());
                                }
                            }
                        }
                    })
                };
                for d in &self.ir.items {
                    match d {
                        abi::Decl::Class(c) => {
                            for e in &c.extends {
                                visit(&abi::Type::Named(e.clone()));
                            }
                            for m in &c.members {
                                m.params.iter().for_each(|p| visit(&p.ty));
                                visit(&m.result);
                            }
                        }
                        abi::Decl::Member(m) => {
                            m.params.iter().for_each(|p| visit(&p.ty));
                            visit(&m.result);
                        }
                        _ => {}
                    }
                }
            }
            None => maps.push(map.clone()),
        }
        let cx = Context::default();
        for map in maps {
            let abi::Type::Named(r) = &map else { continue };
            let Some(Target::Decl(loc, TypeDecl::Class(_))) = sym.resolve(&r.path, &cx) else {
                continue;
            };
            let ([name], None, true) = (&loc.path[..], &loc.module, r.args.is_empty()) else {
                continue;
            };
            if !self.emitted(&loc.path, at) || self.modules.borrow().iter().any(|m| m.0 == *name) {
                continue;
            }
            let Some(Map::Closed(props)) = keyed::map(sym, &map, &cx) else {
                continue;
            };
            let map_tokens = self.named(&loc.path, at);
            let mut taken: Vec<String> = Vec::new();
            let mut keys = Vec::new();
            for (key, ty) in props {
                // A key's value is a class or an interface, as its handle.
                let abi::Type::Named(v) = &ty else { continue };
                let Some(Target::Decl(vloc, TypeDecl::Class(_))) = sym.resolve(&v.path, &cx) else {
                    continue;
                };
                if vloc.module.is_some() || !v.args.is_empty() || !self.emitted(&vloc.path, at) {
                    continue;
                }
                let mut marker = ts_gen_abi::naming::pascal(&key);
                if marker.is_empty() || marker.starts_with(|c: char| c.is_ascii_digit()) {
                    marker.insert(0, '_');
                }
                while taken.contains(&marker) {
                    marker.push('_');
                }
                taken.push(marker.clone());
                let marker = super::typemap::make_ident(&marker);
                let value = self.named(&vloc.path, at);
                let doc = format!("`{key}` of [`{name}`].");
                keys.push(quote! {
                    #[doc = #doc]
                    pub struct #marker;
                    impl MapKey<#map_tokens> for #marker {
                        type Value = #value;
                        const KEY: &'static str = #key;
                    }
                });
            }
            let module = super::typemap::make_ident(&ts_gen::util::naming::to_snake_case(name));
            let doc = format!("The keys of [`{name}`].");
            let tokens = quote! {
                #[doc = #doc]
                pub mod #module {
                    #[allow(unused_imports)]
                    use super::*;
                    #(#keys)*
                }
            };
            self.modules.borrow_mut().push((name.clone(), tokens));
        }
    }
}

/// How a twin's argument becomes the plain binding's.
#[derive(Clone, Copy)]
enum Cast {
    None,
    Ref,
    OptionRef,
}

fn last_segment(t: &mut syn::Type) -> Option<&mut syn::PathSegment> {
    match t {
        syn::Type::Path(p) if p.qself.is_none() => p.path.segments.last_mut(),
        _ => None,
    }
}

fn type_args(seg: &mut syn::PathSegment) -> Vec<&mut syn::Type> {
    match &mut seg.arguments {
        syn::PathArguments::AngleBracketed(a) => a
            .args
            .iter_mut()
            .filter_map(|a| match a {
                syn::GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn option_ref(t: &syn::Type) -> bool {
    let mut t = t.clone();
    let Some(seg) = last_segment(&mut t) else {
        return false;
    };
    seg.ident == "Option"
        && matches!(&type_args(seg)[..], [syn::Type::Reference(r)] if r.mutability.is_none())
}

fn indexed(t: &abi::Type) -> bool {
    let mut found = false;
    t.walk(&mut |t| found |= matches!(t, abi::Type::Index(..) | abi::Type::Lit(_)));
    found
}

/// Put `value` in `s`, the Rust type of the erased reading of `t`, where
/// `t` indexes by the key parameter `k`. `false` when the two don't have
/// the same shape there, or the index isn't one a marker's value states.
fn retype(
    sym: &Symbols<'_>,
    t: &abi::Type,
    cx: &Context<'_>,
    s: &mut syn::Type,
    value: &syn::Type,
    k: &str,
) -> bool {
    if !indexed(t) {
        return true;
    }
    let by_key = |t: &abi::Type| {
        matches!(t, abi::Type::Index(m, key)
            if !matches!(**m, abi::Type::Index(..))
                && matches!(&**key, abi::Type::Named(r) if r.args.is_empty() && r.path.segments == [k]))
    };
    match s {
        syn::Type::Reference(r) => return retype(sym, t, cx, &mut r.elem, value, k),
        syn::Type::Paren(p) => return retype(sym, t, cx, &mut p.elem, value, k),
        syn::Type::Group(g) => return retype(sym, t, cx, &mut g.elem, value, k),
        _ => {}
    }
    match t {
        abi::Type::Index(..) if by_key(t) => {
            *s = value.clone();
            true
        }
        abi::Type::Nullable(inner, _) => {
            let Some(seg) = last_segment(s) else {
                return false;
            };
            match (seg.ident == "Option", &mut type_args(seg)[..]) {
                (true, [arg]) => retype(sym, inner, cx, arg, value, k),
                _ => false,
            }
        }
        abi::Type::Fn(f) => {
            let Some(seg) = last_segment(s) else {
                return false;
            };
            if seg.ident != "Function" {
                return false;
            }
            let mut args = type_args(seg);
            let [syn::Type::BareFn(bare)] = &mut args[..] else {
                return false;
            };
            if f.rest || bare.inputs.len() != f.params.len() {
                return false;
            }
            for (p, arg) in f.params.iter().zip(bare.inputs.iter_mut()) {
                if !retype(sym, p, cx, &mut arg.ty, value, k) {
                    return false;
                }
            }
            match &mut bare.output {
                syn::ReturnType::Type(_, out) => retype(sym, &f.result, cx, out, value, k),
                syn::ReturnType::Default => !indexed(&f.result),
            }
        }
        abi::Type::Named(r) => {
            let Some(seg) = last_segment(s) else {
                return false;
            };
            if r.path.segments.last().is_some_and(|n| seg.ident == n) {
                let mut args = type_args(seg);
                if args.len() == r.args.len() {
                    return r
                        .args
                        .iter()
                        .zip(args.iter_mut())
                        .all(|(t, s)| retype(sym, t, cx, s, value, k));
                }
                // The plain binding leaves out arguments at their defaults.
                if args.is_empty() && r.args.iter().all(by_key) {
                    let values = r.args.iter().map(|_| value);
                    seg.arguments =
                        syn::PathArguments::AngleBracketed(syn::parse_quote! { <#(#values),*> });
                    return true;
                }
                return false;
            }
            match sym.unfold(r, cx) {
                Some((_, u)) => retype(sym, &u, cx, s, value, k),
                None => false,
            }
        }
        _ => false,
    }
}

/// `tokens` with each identifier `name` replaced by `with`.
fn replace_ident(tokens: TokenStream, name: &str, with: &TokenStream) -> TokenStream {
    use proc_macro2::{Group, TokenTree};
    tokens
        .into_iter()
        .flat_map(|t| match t {
            TokenTree::Ident(i) if i == name => with.clone(),
            TokenTree::Group(g) => {
                let mut group = Group::new(g.delimiter(), replace_ident(g.stream(), name, with));
                group.set_span(g.span());
                TokenTree::Group(group).into()
            }
            t => t.into(),
        })
        .collect()
}
