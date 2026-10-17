//! FunctionDecl / VariableDecl → free function and static bindings.

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::signatures::{
    build_signatures, generate_concrete_params_with_mono_bounds, is_void_return, public_rust_name,
    render_generic_bounds, CallableSpec, FunctionSignature, Overload, SignatureKind,
};
use crate::codegen::typemap::{to_return_type, to_syn_type, CodegenContext, TypePosition};
use crate::parse::scope::ScopeId;
use crate::util::naming::{ident_chars, naming_priority, LocalScope, Suffix};
use std::collections::{HashMap, HashSet};

use crate::context::TypeId;
use crate::ir::{FunctionDecl, Member, ModuleContext, TypeDeclaration, TypeKind, VariableDecl};

/// Generate wasm_bindgen extern blocks for a free function.
///
/// Resolves overloads + try_/async/catch via [`build_signatures`], then
/// emits one extern block per resulting [`FunctionSignature`].
pub fn generate_function(
    decl: &FunctionDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    doc: &Option<String>,
    scope: ScopeId,
    value_names: &mut ValueNames,
) -> TokenStream {
    emit_function(decl, ctx, cgctx, doc, &[], scope, value_names)
}

/// Generate wasm_bindgen extern blocks for a free function inside a namespace.
pub fn generate_function_with_js_namespace(
    decl: &FunctionDecl,
    ctx: &ModuleContext,
    js_namespace: &[String],
    cgctx: Option<&CodegenContext<'_>>,
    doc: &Option<String>,
    scope: ScopeId,
    value_names: &mut ValueNames,
) -> TokenStream {
    emit_function(decl, ctx, cgctx, doc, js_namespace, scope, value_names)
}

fn emit_function(
    decl: &FunctionDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    doc: &Option<String>,
    js_namespace: &[String],
    scope: ScopeId,
    value_names: &mut ValueNames,
) -> TokenStream {
    let rust_name = value_names.take(
        &binding_key(ctx, js_namespace, &decl.js_name),
        public_rust_name(&ident_chars(&decl.name)),
    );
    let overloads: Vec<Overload> = std::iter::once(Overload {
        params: &decl.params,
        return_type: &decl.return_type,
        body_scope: decl.body_scope,
    })
    .chain(decl.overloads.iter().map(|o| Overload {
        params: &o.params,
        return_type: &o.return_type,
        body_scope: o.body_scope,
    }))
    .collect();
    let sigs = build_signatures(
        &CallableSpec {
            js_name: &decl.js_name,
            rust_name: Some(&rust_name),
            kind: SignatureKind::Function,
            overloads: &overloads,
            throws: &decl.throws,
            doc,
        },
        &mut value_names.idents,
        cgctx,
    );

    let items: Vec<TokenStream> = sigs
        .iter()
        .map(|sig| generate_expanded_free_function(sig, ctx, cgctx, js_namespace, scope))
        .collect();

    quote! { #(#items)* }
}

/// Generate a single extern block for one resolved free function signature.
fn generate_expanded_free_function(
    sig: &FunctionSignature,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    js_namespace: &[String],
    _enclosing_scope: ScopeId,
) -> TokenStream {
    // Free-function type parameters live in `sig.body_scope`; use it
    // directly so `T` references lower to a bare ident.
    let scope = sig.body_scope;
    let rust_ident = super::typemap::make_ident(&sig.rust_name);
    let params = generate_concrete_params_with_mono_bounds(&sig.params, cgctx, scope, ctx);
    let ret_ty = to_return_type(
        &sig.return_type,
        sig.catch,
        sig.is_async,
        sig.js_string_return,
        sig.error_type.as_ref(),
        cgctx,
        scope,
        ctx,
        super::typemap::ReturnAnchor {
            base: &sig.rust_name,
            kind: super::typemap::ReturnAnchorKind::Callable,
            parent: None,
        },
    );
    let doc = super::doc_tokens(&sig.doc);
    let has_variadic = sig.params.last().is_some_and(|p| p.variadic);

    let mut wb_parts: Vec<TokenStream> = Vec::new();
    if has_variadic {
        wb_parts.push(quote! { variadic });
    }
    if sig.catch {
        wb_parts.push(quote! { catch });
    }
    if super::typemap::needs_js_name(&rust_ident, &sig.js_name) {
        let js_name = &sig.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }
    wb_parts.extend(super::js_namespace_attr(js_namespace));

    let wb_attr = if wb_parts.is_empty() {
        quote! {}
    } else {
        quote! { #[wasm_bindgen(#(#wb_parts),*)] }
    };

    let ret = if is_void_return(&sig.return_type) && !sig.catch {
        quote! {}
    } else {
        quote! { -> #ret_ty }
    };

    let async_kw = if sig.is_async {
        quote! { async }
    } else {
        quote! {}
    };

    let wb_extern_attr = CodegenContext::extern_attr(cgctx, ctx.specifier());

    let generics = render_generic_bounds(&generic_bounds_for_function(sig, cgctx));

    quote! {
        #wb_extern_attr
        extern "C" {
            #doc
            #wb_attr
            pub #async_kw fn #rust_ident #generics (#params) #ret;
        }
    }
}

/// Generic-parameter declaration (`<T: ::wasm_bindgen::JsGeneric, …>`)
/// for a free function, or empty when no type parameters are
/// referenced. Walks the params + return type and consults the
/// signature's `body_scope` to pick out names that bind as
/// [`crate::parse::scope::Binding::TypeParam`].
fn generic_bounds_for_function(
    sig: &FunctionSignature,
    cgctx: Option<&CodegenContext<'_>>,
) -> Vec<TokenStream> {
    let Some(cgctx) = cgctx else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for p in &sig.params {
        super::signatures::collect_type_params(&p.type_ref, cgctx, sig.body_scope, &mut names);
    }
    super::signatures::collect_type_params(&sig.return_type, cgctx, sig.body_scope, &mut names);
    if names.is_empty() {
        return Vec::new();
    }
    names
        .iter()
        .map(|n| CodegenContext::type_param_decl(Some(cgctx), n))
        .collect()
}

/// The names wasm-bindgen's expansion of an import and the dictionary
/// helpers bind as parameters and locals, which a static in scope makes an
/// error at each (E0530). Renaming those can't help, so the static takes a
/// trailing underscore instead.
const BOUND_NAMES: &[&str] = &[
    "abi", "e", "idx", "inner", "js", "obj", "other", "s", "slice", "val", "value",
];

/// A value binding: its module specifier, JS namespace and JS name.
type BindingKey = (Option<String>, Vec<String>, String);

fn binding_key(ctx: &ModuleContext, js_namespace: &[String], js_name: &str) -> BindingKey {
    (
        ctx.specifier().map(str::to_string),
        js_namespace.to_vec(),
        js_name.to_string(),
    )
}

/// The names of the free functions and statics in one Rust module, which
/// share its value namespace.
///
/// `declare var` may be redeclared, including across input files, so a
/// repeated JS binding is emitted once. Distinct bindings that convert to
/// the same ident (`fooBar` / `FOO_BAR`, `ajax` / `$ajax`) are numbered
/// instead, claiming in [`naming_priority`] order so `ajax` keeps its name
/// whichever is declared first.
#[derive(Default)]
pub struct ValueNames {
    idents: LocalScope,
    bindings: HashSet<BindingKey>,
    names: HashMap<BindingKey, String>,
    statics: Vec<String>,
}

impl ValueNames {
    /// Name every function and static among `decls` up front. Each name
    /// stays reserved until its binding is emitted, so a derived `try_`
    /// name declared earlier can't take it.
    pub fn new<'d>(decls: impl IntoIterator<Item = (&'d TypeDeclaration, &'d [String])>) -> Self {
        let mut values: Vec<(BindingKey, String, u8, bool)> = decls
            .into_iter()
            .filter_map(|(decl, js_namespace)| {
                let (js_name, name, is_fn) = match &decl.kind {
                    TypeKind::Function(f) => (&f.js_name, &f.name, true),
                    TypeKind::Variable(v) => (&v.js_name, &v.name, false),
                    _ => return None,
                };
                let ident = ident_chars(name);
                let priority = naming_priority(js_name, &ident);
                let candidate = if is_fn {
                    public_rust_name(&ident)
                } else if BOUND_NAMES.contains(&ident.as_str()) {
                    format!("{ident}_")
                } else {
                    ident
                };
                Some((
                    binding_key(&decl.module_context, js_namespace, js_name),
                    candidate,
                    priority,
                    is_fn,
                ))
            })
            .collect();
        values.sort_by_key(|&(_, _, priority, _)| priority);
        let mut idents = LocalScope::default();
        let mut names = HashMap::new();
        let mut statics = Vec::new();
        for (key, candidate, _, is_fn) in values {
            names.entry(key).or_insert_with(|| {
                let name = idents.declare(&candidate, Suffix::Snake);
                idents.reserve(name.clone());
                if !is_fn {
                    statics.push(name.clone());
                }
                name
            });
        }
        Self {
            idents,
            bindings: HashSet::new(),
            names,
            statics,
        }
    }

    /// The names of the module's statics.
    pub fn statics(&self) -> &[String] {
        &self.statics
    }

    /// The name chosen for `key`, freed for its binding to declare.
    fn take(&mut self, key: &BindingKey, fallback: String) -> String {
        match self.names.get(key) {
            Some(name) => {
                self.idents.unreserve(name);
                name.clone()
            }
            None => fallback,
        }
    }
}

/// Generate a wasm_bindgen extern block for a global constant/variable.
pub fn generate_variable(
    decl: &VariableDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    doc: &Option<String>,
    js_namespace: &[String],
    scope: ScopeId,
    value_names: &mut ValueNames,
) -> TokenStream {
    let binding = binding_key(ctx, js_namespace, &decl.js_name);
    if !value_names.bindings.insert(binding.clone()) {
        return TokenStream::new();
    }
    if let Some(cgctx) = cgctx {
        if let Some(targets) = construct_targets(decl, cgctx) {
            return generate_constructs(decl, &targets, ctx, cgctx, doc, js_namespace);
        }
    }
    let base = value_names.take(&binding, ident_chars(&decl.name));
    let rust_name = value_names.idents.declare(&base, Suffix::Snake);
    let rust_ident = super::typemap::make_ident(&rust_name);
    let ty = to_syn_type(&decl.type_ref, TypePosition::RETURN, cgctx, scope, ctx);
    let doc = super::doc_tokens(doc);

    let mut wb_parts: Vec<TokenStream> = vec![quote! { thread_local_v2 }];
    if super::typemap::needs_js_name(&rust_ident, &decl.js_name) {
        let js_name = &decl.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }
    wb_parts.extend(super::js_namespace_attr(js_namespace));

    let wb_attr = quote! { #[wasm_bindgen(#(#wb_parts),*)] };

    let wb_extern_attr = CodegenContext::extern_attr(cgctx, ctx.specifier());

    // Statics keep the JS binding's casing (`caches`, not `CACHES`), so
    // the lint is allowed per item; wasm-bindgen applies the attribute
    // to the expanded static.
    quote! {
        #wb_extern_attr
        extern "C" {
            #doc
            #[allow(non_upper_case_globals)]
            #wb_attr
            pub static #rust_ident: #ty;
        }
    }
}

/// The types `decl`'s construct signatures construct, claimed, when they
/// bind as constructors of them: each must be a local extern type with no
/// `new` of its own, as a class constructor, a dictionary builder, a member
/// named `new` or another variable's constructor would collide with it.
/// Otherwise the variable binds as a value.
fn construct_targets(decl: &VariableDecl, cgctx: &CodegenContext<'_>) -> Option<Vec<TypeId>> {
    let mut targets = Vec::new();
    for construct in &decl.constructs {
        let crate::ir::TypeRef::Reference { segments, .. } = &construct.return_type else {
            return None;
        };
        let id = cgctx
            .gctx
            .resolve_path(construct.body_scope, &segments.join("."))
            .filter(|id| cgctx.local_type_ids.contains(id))?;
        let members = match &cgctx.gctx.get_type(id).kind {
            TypeKind::Class(c) => &c.members,
            TypeKind::Interface(i)
                if i.classification != crate::ir::InterfaceClassification::Dictionary =>
            {
                &i.members
            }
            _ => return None,
        };
        if members.iter().any(member_is_new) || cgctx.constructed.borrow().contains(&id) {
            return None;
        }
        targets.push(id);
    }
    if targets.is_empty() {
        return None;
    }
    cgctx
        .constructed
        .borrow_mut()
        .extend(targets.iter().copied());
    Some(targets)
}

fn member_is_new(member: &Member) -> bool {
    let js_name = match member {
        Member::Constructor(_) => return true,
        Member::Getter(m) => &m.js_name,
        Member::StaticGetter(m) => &m.js_name,
        Member::Method(m) => &m.js_name,
        Member::StaticMethod(m) => &m.js_name,
        _ => return false,
    };
    crate::util::naming::to_snake_case(js_name) == "new"
}

/// A constructor-shaped variable whose name a type alias takes as the type
/// binds its construct signatures as constructors of the types they return,
/// `new`ing the variable: `RpcStub::new(&value)` is `new RpcStub(value)`
/// returning the `Rpc.Stub<T>` the alias names.
fn generate_constructs(
    decl: &VariableDecl,
    targets: &[TypeId],
    ctx: &ModuleContext,
    cgctx: &CodegenContext<'_>,
    doc: &Option<String>,
    js_namespace: &[String],
) -> TokenStream {
    let js_class = &decl.js_name;
    // Overloads constructing the same type are named together, as
    // `new_with_*`, like a class's constructors, and names are taken per
    // type, whose impl they go on.
    let mut groups: Vec<(&FunctionDecl, TypeId, Vec<Overload>)> = Vec::new();
    for (construct, &target) in decl.constructs.iter().zip(targets) {
        let same = |first: &FunctionDecl| {
            first.return_type == construct.return_type
                && first
                    .type_params
                    .iter()
                    .map(|tp| &tp.name)
                    .eq(construct.type_params.iter().map(|tp| &tp.name))
        };
        let overload = Overload {
            params: &construct.params,
            return_type: &construct.return_type,
            body_scope: construct.body_scope,
        };
        match groups.iter_mut().find(|(first, _, _)| same(first)) {
            Some((_, _, overloads)) => overloads.push(overload),
            None => groups.push((construct, target, vec![overload])),
        }
    }
    let mut names: HashMap<TypeId, LocalScope> = HashMap::new();
    let mut items = Vec::new();
    for (construct, target, overloads) in &groups {
        let sigs = build_signatures(
            &CallableSpec {
                js_name: js_class,
                rust_name: None,
                kind: SignatureKind::Constructor,
                overloads,
                throws: &construct.throws,
                doc,
            },
            names.entry(*target).or_default(),
            Some(cgctx),
        );
        for sig in &sigs {
            let scope = sig.body_scope;
            let rust_ident = super::typemap::make_ident(&sig.rust_name);
            let params =
                generate_concrete_params_with_mono_bounds(&sig.params, Some(cgctx), scope, ctx);
            let ret = to_return_type(
                &sig.return_type,
                sig.catch,
                false,
                sig.js_string_return,
                sig.error_type.as_ref(),
                Some(cgctx),
                scope,
                ctx,
                super::typemap::ReturnAnchor {
                    base: &sig.rust_name,
                    kind: super::typemap::ReturnAnchorKind::Callable,
                    parent: None,
                },
            );
            let mut wb_parts = vec![quote! { constructor }, quote! { js_class = #js_class }];
            wb_parts.extend(super::js_namespace_attr(js_namespace));
            if sig.catch {
                wb_parts.push(quote! { catch });
            }
            if sig.rust_name != "new" {
                wb_parts.push(quote! { js_name = #js_class });
            }
            let doc = super::doc_tokens(&sig.doc);
            let generics = render_generic_bounds(&generic_bounds_for_function(sig, Some(cgctx)));
            let wb_extern_attr = CodegenContext::extern_attr(Some(cgctx), ctx.specifier());
            items.push(quote! {
                #wb_extern_attr
                extern "C" {
                    #doc
                    #[wasm_bindgen(#(#wb_parts),*)]
                    pub fn #rust_ident #generics (#params) -> #ret;
                }
            });
        }
    }
    quote! { #(#items)* }
}
