//! ClassDecl / ClassLike InterfaceDecl → wasm_bindgen `extern "C"` block generation.
//!
//! Generates the standard pattern seen in worker-sys:
//!
//! ```rust,ignore
//! #[wasm_bindgen]
//! extern "C" {
//!     #[wasm_bindgen(extends = js_sys::Object, js_name = "MyClass")]
//!     #[derive(Debug, Clone, PartialEq, Eq)]
//!     pub type MyClass;
//!
//!     #[wasm_bindgen(constructor, catch)]
//!     pub fn new(arg: &str) -> Result<MyClass, JsValue>;
//!
//!     #[wasm_bindgen(method, getter)]
//!     pub fn name(this: &MyClass) -> String;
//!
//!     #[wasm_bindgen(method, js_name = "doThing")]
//!     pub fn do_thing(this: &MyClass, x: f64);
//! }
//! ```

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use std::collections::HashMap;

use crate::codegen::signatures::{
    build_signatures, dedupe_name, generate_concrete_params, is_void_return, CallableSpec,
    ConcreteParam, FunctionSignature, SignatureKind,
};
use crate::codegen::typemap::{to_return_type, to_syn_type, CodegenContext, TypePosition};
use crate::ir::{
    ClassDecl, GetterMember, InterfaceClassification, InterfaceDecl, Member, ModuleContext,
    SetterMember, StaticGetterMember, StaticSetterMember, TypeRef,
};
use crate::parse::scope::ScopeId;
use crate::util::naming::to_snake_case;

/// Configuration for generating a class-like extern block.
struct ClassConfig<'a> {
    /// Rust type name.
    rust_name: String,
    /// JS class name (for `js_name` / `js_class` attributes).
    js_name: String,
    /// The `extends` parents (wasm_bindgen supports chained `extends = ...`).
    extends: Vec<TokenStream>,
    /// Module specifier for `#[wasm_bindgen(module = "...")]`.
    module: Option<std::rc::Rc<str>>,
    /// JS namespace (e.g., `"WebAssembly"`) for types inside a namespace.
    js_namespace: Option<String>,
    /// Whether this is an abstract class (skip constructor).
    is_abstract: bool,
    /// Members to generate.
    members: Vec<Member>,
    /// Codegen context for type resolution.
    cgctx: Option<&'a CodegenContext<'a>>,
    /// Scope for type reference resolution.
    scope: ScopeId,
}

impl<'a> ClassConfig<'a> {
    fn from_class(
        decl: &ClassDecl,
        ctx: &ModuleContext,
        cgctx: Option<&'a CodegenContext>,
        scope: ScopeId,
    ) -> Self {
        let extends = match &decl.extends {
            Some(e) => vec![extends_tokens(e, cgctx, scope, ctx)],
            None => vec![quote! { Object }],
        };
        let module = match ctx {
            ModuleContext::Module(m) => Some(m.clone()),
            ModuleContext::Global => None,
        };

        ClassConfig {
            rust_name: decl.name.clone(),
            js_name: decl.js_name.clone(),
            extends,
            module,
            js_namespace: None,
            is_abstract: decl.is_abstract,
            members: decl.members.clone(),
            cgctx,
            scope,
        }
    }

    fn from_interface(
        decl: &InterfaceDecl,
        ctx: &ModuleContext,
        cgctx: Option<&'a CodegenContext>,
        scope: ScopeId,
    ) -> Self {
        let extends = if decl.extends.is_empty() {
            vec![quote! { Object }]
        } else {
            decl.extends
                .iter()
                .map(|e| extends_tokens(e, cgctx, scope, ctx))
                .collect()
        };
        let module = match ctx {
            ModuleContext::Module(m) => Some(m.clone()),
            ModuleContext::Global => None,
        };

        ClassConfig {
            rust_name: decl.name.clone(),
            js_name: decl.js_name.clone(),
            extends,
            module,
            js_namespace: None,
            is_abstract: false,
            members: decl.members.clone(),
            cgctx,
            scope,
        }
    }

    /// Rust name to use everywhere the class identifier appears in generated
    /// code (`pub type X`, `this: &X`, `static_method_of = X`, …).
    ///
    /// When the declared name collides with a `js_sys` reserved identifier,
    /// `CodegenContext::resolve_collisions` chose a suffixed alternative
    /// (`Global` → `Global_`); this method threads that through. Falls back
    /// to the original `rust_name` when no rename was registered.
    fn effective_rust_name(&self) -> String {
        self.cgctx
            .and_then(|ctx| ctx.renamed_locals.get(&self.rust_name).cloned())
            .unwrap_or_else(|| self.rust_name.clone())
    }

    /// The `ModuleContext` this extern block lives in. Used as the
    /// `from_module` argument to type-emit helpers so cross-module
    /// references get the correct path qualifier.
    #[allow(clippy::wrong_self_convention)]
    fn from_module(&self) -> ModuleContext {
        match &self.module {
            Some(m) => ModuleContext::Module(m.clone()),
            None => ModuleContext::Global,
        }
    }
}

/// Generate a complete `extern "C"` block for a class-like declaration.
pub fn generate_class(
    decl: &ClassDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TokenStream {
    let config = ClassConfig::from_class(decl, ctx, cgctx, scope);
    generate_extern_block(&config)
}

/// Generate a complete `extern "C"` block for a class-like interface.
pub fn generate_class_like_interface(
    decl: &InterfaceDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    js_namespace: Option<&str>,
    scope: ScopeId,
) -> TokenStream {
    debug_assert!(
        matches!(
            decl.classification,
            InterfaceClassification::ClassLike | InterfaceClassification::Unclassified
        ),
        "expected ClassLike or Unclassified, got {:?}",
        decl.classification
    );
    let mut config = ClassConfig::from_interface(decl, ctx, cgctx, scope);
    config.js_namespace = js_namespace.map(|s| s.to_string());
    generate_extern_block(&config)
}

/// Generate a complete `extern "C"` block for a class inside a namespace, with `js_namespace`.
pub fn generate_class_with_js_namespace(
    decl: &ClassDecl,
    ctx: &ModuleContext,
    js_namespace: &str,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TokenStream {
    let mut config = ClassConfig::from_class(decl, ctx, cgctx, scope);
    config.js_namespace = Some(js_namespace.to_string());
    generate_extern_block(&config)
}

/// Generate a simple extern "C" block for a dictionary interface.
/// Temporary until M5 implements proper dictionary builders.
pub fn generate_dictionary_extern(
    decl: &InterfaceDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    js_namespace: Option<&str>,
    scope: ScopeId,
) -> TokenStream {
    let mut config = ClassConfig::from_interface(decl, ctx, cgctx, scope);
    config.js_namespace = js_namespace.map(|s| s.to_string());

    let extern_block = generate_extern_block(&config);
    let factory = generate_dictionary_factory(&config);

    quote! {
        #extern_block
        #factory
    }
}

/// Generate a Rust `impl` block with factory constructors for a dictionary interface.
///
/// Produces `new()` plus expanded variants like `new_with_status(status: f64)`,
/// `new_with_status_and_status_text(status: f64, status_text: &str)`, etc.
/// Each factory creates a bare `Object`, sets the provided properties via their
/// setters, and returns it cast to the dictionary type.
/// Generate a Rust `impl` block with `new()` and `builder()` for a dictionary interface.
///
/// Produces:
/// ```ignore
/// impl ResponseInit {
///     pub fn new() -> Self { ... }
///     pub fn builder() -> ResponseInitBuilder { ... }
/// }
///
/// pub struct ResponseInitBuilder { inner: ResponseInit }
/// impl ResponseInitBuilder {
///     pub fn status(self, val: f64) -> Self { ... }
///     pub fn headers(self, val: &Headers) -> Self { ... }
///     pub fn build(self) -> ResponseInit { ... }
/// }
/// ```
fn generate_dictionary_factory(config: &ClassConfig) -> TokenStream {
    let rust_type = super::typemap::make_ident(&config.effective_rust_name());
    let builder_name = super::typemap::make_ident(&format!("{}Builder", config.rust_name));

    // If any getter lacks a corresponding setter the type has readonly
    // properties, which means it is not constructible via setters — skip
    // the builder entirely and only emit a bare `new()`.
    let setter_names: std::collections::HashSet<&str> = config
        .members
        .iter()
        .filter_map(|m| {
            if let Member::Setter(s) = m {
                Some(s.js_name.as_str())
            } else {
                None
            }
        })
        .collect();
    let has_readonly = config.members.iter().any(|m| {
        if let Member::Getter(g) = m {
            !setter_names.contains(g.js_name.as_str())
        } else {
            false
        }
    });
    if has_readonly {
        return quote! {
            impl #rust_type {
                #[allow(clippy::new_without_default)]
                pub fn new() -> Self {
                    #[allow(unused_unsafe)]
                    unsafe { JsValue::from(js_sys::Object::new()).unchecked_into() }
                }
            }
        };
    }

    // Collect getter properties for builder methods
    let getters: Vec<&crate::ir::GetterMember> = config
        .members
        .iter()
        .filter_map(|m| {
            if let Member::Getter(g) = m {
                Some(g)
            } else {
                None
            }
        })
        .collect();

    // Partition getters: required ones become positional `builder(...)`
    // arguments, optional ones become fluent setter methods.
    let mut required_getters: Vec<&crate::ir::GetterMember> = Vec::new();
    let mut optional_getters: Vec<&crate::ir::GetterMember> = Vec::new();
    for g in &getters {
        if g.optional {
            optional_getters.push(g);
        } else {
            required_getters.push(g);
        }
    }

    // Resolve each getter through the setter pipeline. Multi-overload
    // setters (e.g. `set_from(&str)` + `set_from_with_email_address(&EmailAddress)`
    // for `from: string | EmailAddress`) produce multiple sigs — we
    // cartesian-product across required fields below to expose every
    // combination as its own `new` / `builder` variant.
    let resolve_setter_sigs = |g: &crate::ir::GetterMember| -> Vec<FunctionSignature> {
        let setter_param = crate::ir::Param {
            name: "val".to_string(),
            type_ref: g.type_ref.clone(),
            optional: false,
            variadic: false,
        };
        let mut setter_used = HashSet::new();
        let setter_overloads = [&[setter_param][..]];
        let void = crate::ir::TypeRef::Void;
        build_signatures(
            &CallableSpec {
                js_name: &g.js_name,
                kind: SignatureKind::Setter,
                overloads: &setter_overloads,
                return_type: &void,
                error_type: None,
                doc: &None,
            },
            &mut setter_used,
            config.cgctx,
            config.scope,
        )
    };

    // Per required field, decompose the type ref into a list of
    // *options*. Each option becomes one cell in the cartesian product
    // across fields; literal-typed members are special-cased so the
    // user picks the discriminant via the function name (`new_inline`)
    // rather than passing the literal as a `&str` argument.
    enum FieldOption {
        /// String/number/boolean literal baked into the body. No param
        /// contributed; the suffix part is the literal value.
        Literal {
            /// JS field name (`disposition`) — used for the doc bullet.
            field_js_name: String,
            /// JSDoc on the original getter (if any).
            field_doc: Option<String>,
            /// TS source form of the literal — `"inline"`, `42`, `true` —
            /// rendered into the bullet as
            /// `` `disposition: "inline"` ``.
            literal_display: String,
            suffix: String,
            setter_ident: syn::Ident,
            literal_expr: TokenStream,
        },
        /// Free-form value: contributes a normal positional parameter.
        Value {
            field_doc: Option<String>,
            param: ConcreteParam,
            setter_ident: syn::Ident,
        },
    }
    struct FieldDim {
        options: Vec<FieldOption>,
    }

    // Decompose a getter's `type_ref` into its union members (or the
    // single member if it isn't a union), with literal types peeled
    // out for the discriminant collapse.
    fn split_union(ty: &crate::ir::TypeRef) -> Vec<&crate::ir::TypeRef> {
        match ty {
            crate::ir::TypeRef::Union(members) => members.iter().collect(),
            other => vec![other],
        }
    }

    let mut field_dims: Vec<FieldDim> = Vec::new();
    for g in &required_getters {
        let param_name = to_snake_case(&g.js_name);
        let setters = resolve_setter_sigs(g);
        if setters.is_empty() {
            continue;
        }

        // Each member of the union (or the single non-union type) maps
        // to one option. Literals find their setter by matching the
        // setter's param type_ref structurally; non-literals get their
        // own option with an `expand_signatures`-compatible param.
        let mut options: Vec<FieldOption> = Vec::new();
        let members = split_union(&g.type_ref);
        let any_literal = members.iter().any(|m| {
            matches!(
                m,
                crate::ir::TypeRef::StringLiteral(_)
                    | crate::ir::TypeRef::NumberLiteral(_)
                    | crate::ir::TypeRef::BooleanLiteral(_)
            )
        });
        let value_members: Vec<&crate::ir::TypeRef> = members
            .iter()
            .copied()
            .filter(|m| {
                !matches!(
                    m,
                    crate::ir::TypeRef::StringLiteral(_)
                        | crate::ir::TypeRef::NumberLiteral(_)
                        | crate::ir::TypeRef::BooleanLiteral(_)
                )
            })
            .collect();

        let pick_setter = |target: &crate::ir::TypeRef| -> Option<syn::Ident> {
            setters
                .iter()
                .find(|sig| sig.params.first().is_some_and(|p| &p.type_ref == target))
                .map(|sig| super::typemap::make_ident(&sig.rust_name))
        };
        // Fallback for fields without per-member setter granularity: the
        // first setter handles everything (typical when the IR only
        // synthesised one setter for the whole union).
        let default_setter = super::typemap::make_ident(&setters[0].rust_name);

        for m in &members {
            match m {
                crate::ir::TypeRef::StringLiteral(s) => {
                    let setter_ident = pick_setter(m).unwrap_or_else(|| default_setter.clone());
                    options.push(FieldOption::Literal {
                        field_js_name: g.js_name.clone(),
                        field_doc: g.doc.clone(),
                        literal_display: format!("\"{s}\""),
                        suffix: format!("_{}", to_snake_case(s)),
                        setter_ident,
                        literal_expr: quote! { #s },
                    });
                }
                crate::ir::TypeRef::NumberLiteral(n) => {
                    let setter_ident = pick_setter(m).unwrap_or_else(|| default_setter.clone());
                    let lit = syn::LitFloat::new(&n.to_string(), proc_macro2::Span::call_site());
                    options.push(FieldOption::Literal {
                        field_js_name: g.js_name.clone(),
                        field_doc: g.doc.clone(),
                        literal_display: n.to_string(),
                        suffix: format!("_{}", n.to_string().replace(['.', '-'], "_")),
                        setter_ident,
                        literal_expr: quote! { #lit },
                    });
                }
                crate::ir::TypeRef::BooleanLiteral(b) => {
                    let setter_ident = pick_setter(m).unwrap_or_else(|| default_setter.clone());
                    options.push(FieldOption::Literal {
                        field_js_name: g.js_name.clone(),
                        field_doc: g.doc.clone(),
                        literal_display: b.to_string(),
                        suffix: format!("_{}", b),
                        setter_ident,
                        literal_expr: quote! { #b },
                    });
                }
                _ => {}
            }
        }
        // Non-literal members: if the union mixed literals with concrete
        // types, those concrete types still need their own value options
        // (e.g. `disposition: "inline" | string` → `new_inline()` plus a
        // catch-all `new(disposition: &str)`). For purely non-literal
        // unions we emit one value option per member.
        let value_targets: Vec<&crate::ir::TypeRef> = if any_literal {
            value_members
        } else {
            members.clone()
        };
        for m in value_targets {
            let setter_ident = pick_setter(m).unwrap_or_else(|| default_setter.clone());
            let param = ConcreteParam {
                name: param_name.clone(),
                type_ref: m.clone(),
                variadic: false,
            };
            options.push(FieldOption::Value {
                field_doc: g.doc.clone(),
                param,
                setter_ident,
            });
        }
        if options.is_empty() {
            continue;
        }
        field_dims.push(FieldDim { options });
    }

    // Cartesian product across field dimensions → list of combos
    // (each is a Vec<&FieldOption>).
    let combos: Vec<Vec<&FieldOption>> = if field_dims.is_empty() {
        vec![vec![]]
    } else {
        let mut acc: Vec<Vec<&FieldOption>> = vec![vec![]];
        for dim in &field_dims {
            acc = acc
                .into_iter()
                .flat_map(|prefix| {
                    dim.options.iter().map(move |opt| {
                        let mut next = prefix.clone();
                        next.push(opt);
                        next
                    })
                })
                .collect();
        }
        acc
    };

    // For each combo: collect the value params (literals contribute
    // only to the suffix, not to the param list) plus the markdown
    // bullets that document this variant. Literal bullets render the
    // baked-in value; value bullets read off the original getter's
    // JSDoc.
    struct ComboPlan {
        literal_suffix: String,
        value_params: Vec<ConcreteParam>,
        init_calls: Vec<TokenStream>,
        arg_idents: Vec<syn::Ident>,
        /// Lines like `` `disposition: "inline"`: <getter doc> `` that
        /// describe the literal discriminants baked into this variant.
        literal_doc_bullets: Vec<String>,
        /// Lines like `` `from`: <getter doc> `` describing the
        /// caller-provided fields.
        provided_doc_bullets: Vec<String>,
    }
    let mut plans: Vec<ComboPlan> = Vec::with_capacity(combos.len());
    for combo in &combos {
        let mut literal_suffix = String::new();
        let mut value_params: Vec<ConcreteParam> = Vec::new();
        let mut init_calls: Vec<TokenStream> = Vec::new();
        let mut arg_idents: Vec<syn::Ident> = Vec::new();
        let mut literal_doc_bullets: Vec<String> = Vec::new();
        let mut provided_doc_bullets: Vec<String> = Vec::new();
        for opt in combo {
            match opt {
                FieldOption::Literal {
                    field_js_name,
                    field_doc,
                    literal_display,
                    suffix,
                    setter_ident,
                    literal_expr,
                } => {
                    literal_suffix.push_str(suffix);
                    init_calls.push(quote! { inner.#setter_ident(#literal_expr); });
                    let bullet_head = format!("`{field_js_name}: {literal_display}`");
                    literal_doc_bullets.push(match field_doc {
                        Some(doc) => format!("* {bullet_head}: {}", doc.trim()),
                        None => format!("* {bullet_head}"),
                    });
                }
                FieldOption::Value {
                    field_doc,
                    param,
                    setter_ident,
                } => {
                    let arg_ident = super::typemap::make_ident(&param.name);
                    arg_idents.push(arg_ident.clone());
                    init_calls.push(quote! { inner.#setter_ident(#arg_ident); });
                    value_params.push(param.clone());
                    let bullet_head = format!("`{}`", param.name);
                    provided_doc_bullets.push(match field_doc {
                        Some(doc) => format!("* {bullet_head}: {}", doc.trim()),
                        None => format!("* {bullet_head}"),
                    });
                }
            }
        }
        plans.push(ComboPlan {
            literal_suffix,
            value_params,
            init_calls,
            arg_idents,
            literal_doc_bullets,
            provided_doc_bullets,
        });
    }

    // Compute `_with_X[_and_Y]` suffixes ONLY across combos sharing the
    // same literal prefix. Combos with distinct literal prefixes are
    // already disambiguated by the prefix, so they shouldn't influence
    // each other's `_with_*` decisions.
    let mut value_suffixes: Vec<String> = vec![String::new(); plans.len()];
    let mut by_prefix: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in plans.iter().enumerate() {
        by_prefix
            .entry(p.literal_suffix.clone())
            .or_default()
            .push(i);
    }
    for indices in by_prefix.values() {
        let group_params: Vec<Vec<ConcreteParam>> = indices
            .iter()
            .map(|&i| plans[i].value_params.clone())
            .collect();
        let group_suffixes = crate::codegen::signatures::compute_suffixes_pub(&group_params);
        for (suffix, &i) in group_suffixes.iter().zip(indices) {
            value_suffixes[i] = suffix.clone();
        }
    }

    let mut builder_variants: Vec<TokenStream> = Vec::new();
    let mut new_variants: Vec<TokenStream> = Vec::new();
    let mut emitted_names: HashSet<String> = HashSet::new();
    for (plan, value_suffix) in plans.iter().zip(&value_suffixes) {
        let full_suffix = format!("{}{}", plan.literal_suffix, value_suffix);
        // Dedup combos that produce the same final name (e.g. when a
        // discriminator collapse plus a value collapse converge).
        if !emitted_names.insert(full_suffix.clone()) {
            continue;
        }
        let builder_ident = super::typemap::make_ident(&format!("builder{full_suffix}"));
        let new_ident = super::typemap::make_ident(&format!("new{full_suffix}"));
        let params_tokens = generate_concrete_params(
            &plan.value_params,
            config.cgctx,
            config.scope,
            &config.from_module(),
        );
        let init_calls = &plan.init_calls;
        let arg_idents = &plan.arg_idents;

        // Compose the doc block. Literal bullets come first (they
        // describe the discriminants baked into this variant), then a
        // `# Provided fields` heading and the bullets for the
        // caller-supplied fields. Both sections are skipped when their
        // bullet list is empty.
        let mut doc_text = String::new();
        if !plan.literal_doc_bullets.is_empty() {
            doc_text.push_str(&plan.literal_doc_bullets.join("\n"));
        }
        if !plan.provided_doc_bullets.is_empty() {
            if !doc_text.is_empty() {
                doc_text.push_str("\n\n");
            }
            doc_text.push_str("# Provided fields\n\n");
            doc_text.push_str(&plan.provided_doc_bullets.join("\n"));
        }
        let doc_attr = if doc_text.is_empty() {
            quote! {}
        } else {
            super::doc_tokens(&Some(doc_text))
        };

        // No-required case has zero `init_calls`, so we inline the
        // factory directly into the struct literal where the field type
        // pins inference. The required-args case keeps a `let inner:
        // Self` so setter calls type-resolve before construction.
        let body = if init_calls.is_empty() {
            quote! {
                #builder_name {
                    inner: JsCast::unchecked_into(js_sys::Object::new()),
                }
            }
        } else {
            quote! {
                let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
                #(#init_calls)*
                #builder_name { inner }
            }
        };
        builder_variants.push(quote! {
            #doc_attr
            pub fn #builder_ident(#params_tokens) -> #builder_name {
                #body
            }
        });
        new_variants.push(quote! {
            #doc_attr
            pub fn #new_ident(#params_tokens) -> #rust_type {
                Self::#builder_ident(#(#arg_idents),*).build()
            }
        });
    }

    // Fluent methods on the wrapper for optional fields. Required fields
    // are intentionally absent — they're only settable through `builder`.
    let mut builder_methods: Vec<TokenStream> = Vec::new();
    for g in &optional_getters {
        for sig in resolve_setter_sigs(g) {
            let builder_method_name = sig.rust_name.strip_prefix("set_").unwrap_or(&sig.rust_name);
            let method_ident = super::typemap::make_ident(builder_method_name);
            let setter_ident = super::typemap::make_ident(&sig.rust_name);
            let params = generate_concrete_params(
                &sig.params,
                config.cgctx,
                config.scope,
                &config.from_module(),
            );
            let param_idents: Vec<_> = sig
                .params
                .iter()
                .map(|p| super::typemap::make_ident(&p.name))
                .collect();
            builder_methods.push(quote! {
                pub fn #method_ident(self, #params) -> Self {
                    self.inner.#setter_ident(#(#param_idents),*);
                    self
                }
            });
        }
    }

    quote! {
        impl #rust_type {
            #(#new_variants)*
            #(#builder_variants)*
        }

        pub struct #builder_name {
            inner: #rust_type,
        }

        impl #builder_name {
            #(#builder_methods)*

            pub fn build(self) -> #rust_type {
                self.inner
            }
        }
    }
}

/// Build the full `#[wasm_bindgen] extern "C" { ... }` block.
///
/// All naming happens through a single `used_names` set that spans the entire
/// extern block. Members are processed in declaration order. Methods with the
/// same `js_name` (TypeScript overloads) are grouped and expanded together as
/// one unit — overloads feed into the same expansion, producing disambiguated
/// `_with_`/`_and_` suffixes across all overloads rather than opaque `_1` suffixes.
///
/// Each name — including `try_` variants — is assigned via `dedupe_name`, which
/// guarantees uniqueness by appending numeric suffixes on collision.
fn generate_extern_block(config: &ClassConfig) -> TokenStream {
    use crate::ir::{ConstructorMember, MethodMember, Param, StaticMethodMember};
    use std::collections::HashMap;

    let mut items = Vec::new();
    let mut used_names: HashSet<String> = HashSet::new();

    // Pre-group methods/statics/constructors by js_name.
    // We iterate config.members in declaration order, so the first occurrence of
    // each js_name determines where its expanded signatures appear in the output.
    let mut method_groups: HashMap<String, Vec<&MethodMember>> = HashMap::new();
    let mut static_method_groups: HashMap<String, Vec<&StaticMethodMember>> = HashMap::new();
    let mut constructor_overloads: Vec<&ConstructorMember> = Vec::new();

    for member in &config.members {
        match member {
            Member::Constructor(ctor) if !config.is_abstract => {
                constructor_overloads.push(ctor);
            }
            Member::Method(m) => {
                method_groups.entry(m.js_name.clone()).or_default().push(m);
            }
            Member::StaticMethod(m) => {
                static_method_groups
                    .entry(m.js_name.clone())
                    .or_default()
                    .push(m);
            }
            _ => {}
        }
    }

    // Track which method groups have been expanded (by js_name).
    let mut expanded_methods: HashSet<String> = HashSet::new();
    let mut expanded_static_methods: HashSet<String> = HashSet::new();
    let mut expanded_constructors = false;

    // 1. Type declaration with attributes
    items.push(generate_type_decl(config));

    // 2. Process all members in declaration order through the single naming pass.
    //    When we encounter the first member of a method group, expand all overloads
    //    of that group together. Skip subsequent members of the same group.
    for member in &config.members {
        match member {
            Member::Constructor(_) if !config.is_abstract => {
                if expanded_constructors {
                    continue;
                }
                expanded_constructors = true;

                let overloads: Vec<&[Param]> = constructor_overloads
                    .iter()
                    .map(|c| c.params.as_slice())
                    .collect();
                let doc = constructor_overloads.first().and_then(|c| c.doc.clone());
                // Use the first overload's `@throws` (if any) as the error
                // type for the constructor — TS overloads conventionally
                // share semantics, and the first one carries the doc.
                let throws = constructor_overloads
                    .first()
                    .and_then(|c| c.throws.as_ref());
                let return_type = TypeRef::Named(config.rust_name.clone());
                let sigs = build_signatures(
                    &CallableSpec {
                        js_name: &config.js_name,
                        kind: SignatureKind::Constructor,
                        overloads: &overloads,
                        return_type: &return_type,
                        error_type: throws,
                        doc: &doc,
                    },
                    &mut used_names,
                    config.cgctx,
                    config.scope,
                );
                for sig in &sigs {
                    items.push(generate_expanded_constructor(config, sig));
                }
            }
            Member::Method(m) => {
                if expanded_methods.contains(&m.js_name) {
                    continue;
                }
                expanded_methods.insert(m.js_name.clone());

                let group = &method_groups[&m.js_name];
                let overloads: Vec<&[Param]> = group.iter().map(|m| m.params.as_slice()).collect();
                let doc = group.first().and_then(|m| m.doc.clone());
                let return_type = &group[0].return_type;
                let throws = group.first().and_then(|m| m.throws.as_ref());
                let sigs = build_signatures(
                    &CallableSpec {
                        js_name: &m.js_name,
                        kind: SignatureKind::Method,
                        overloads: &overloads,
                        return_type,
                        error_type: throws,
                        doc: &doc,
                    },
                    &mut used_names,
                    config.cgctx,
                    config.scope,
                );
                for sig in &sigs {
                    items.push(generate_expanded_method(config, sig));
                }
            }
            Member::StaticMethod(m) => {
                if expanded_static_methods.contains(&m.js_name) {
                    continue;
                }
                expanded_static_methods.insert(m.js_name.clone());

                let group = &static_method_groups[&m.js_name];
                let overloads: Vec<&[Param]> = group.iter().map(|m| m.params.as_slice()).collect();
                let doc = group.first().and_then(|m| m.doc.clone());
                let return_type = &group[0].return_type;
                let throws = group.first().and_then(|m| m.throws.as_ref());
                let sigs = build_signatures(
                    &CallableSpec {
                        js_name: &m.js_name,
                        kind: SignatureKind::StaticMethod,
                        overloads: &overloads,
                        return_type,
                        error_type: throws,
                        doc: &doc,
                    },
                    &mut used_names,
                    config.cgctx,
                    config.scope,
                );
                for sig in &sigs {
                    items.push(generate_expanded_static_method(config, sig));
                }
            }
            Member::Getter(g) => {
                items.push(generate_getter(config, g, &mut used_names));
            }
            Member::Setter(s) => {
                items.extend(generate_setter(config, s, &mut used_names));
            }
            Member::StaticGetter(g) => {
                items.push(generate_static_getter(config, g, &mut used_names));
            }
            Member::StaticSetter(s) => {
                items.extend(generate_static_setter(config, s, &mut used_names));
            }
            Member::IndexSignature(_) | Member::Constructor(_) => {
                // IndexSignature: not yet supported in codegen
                // Constructor on abstract class: skip
            }
        }
    }

    // Build the extern block with optional module attribute
    let wb_extern_attr = match &config.module {
        Some(m) => quote! { #[wasm_bindgen(module = #m)] },
        None => quote! { #[wasm_bindgen] },
    };

    // Re-export the type under its original (un-suffixed) name when collision
    // resolution renamed it. The suffixed name (`Foo_`) is purely an internal
    // disambiguator against the `use js_sys::*` glob inside the surrounding
    // module — it must never leak into the public Rust path consumers see.
    let public_alias = if config.effective_rust_name() != config.rust_name {
        let public = super::typemap::make_ident(&config.rust_name);
        let internal = super::typemap::make_ident(&config.effective_rust_name());
        quote! { pub use #internal as #public; }
    } else {
        quote! {}
    };

    quote! {
        #wb_extern_attr
        extern "C" {
            #(#items)*
        }
        #public_alias
    }
}

/// Generate the type declaration:
///
/// ```rust,ignore
/// #[wasm_bindgen(extends = ..., js_name = "FooBar")]
/// #[derive(Debug, Clone, PartialEq, Eq)]
/// pub type FooBar;
/// ```
fn generate_type_decl(config: &ClassConfig) -> TokenStream {
    // If the bare class name collided with a `js_sys` reserved name (e.g.
    // `Global`, `Function`), `CodegenContext::resolve_collisions` chose a
    // suffixed Rust name (`Global_`). The type decl needs to use that
    // renamed identifier so references through `to_syn_type` line up.
    // The original JS-side name still goes through `js_name = "..."` below.
    let rust_name = config
        .cgctx
        .and_then(|ctx| ctx.renamed_locals.get(&config.rust_name).cloned())
        .unwrap_or_else(|| config.rust_name.clone());
    let rust_ident = super::typemap::make_ident(&rust_name);
    let js_name = &config.js_name;

    // Build wasm_bindgen attribute parts
    let mut wb_parts: Vec<TokenStream> = Vec::new();

    let mut has_object = false;
    for extends in &config.extends {
        let extends_str = extends.to_string();
        // Skip extends that resolve to JsValue (implicit, causes conflicting impls)
        if extends_str == "JsValue" {
            continue;
        }
        if let Some(cgctx) = config.cgctx {
            let uses = cgctx.external_uses.borrow();
            if uses.get(&extends_str).is_some_and(|v| v == "JsValue") {
                continue;
            }
        }
        if extends_str == "Object" {
            has_object = true;
        }
        wb_parts.push(quote! { extends = #extends });
    }
    // Every type extends Object at minimum
    if !has_object {
        wb_parts.push(quote! { extends = Object });
    }

    // Emit `js_name` whenever the JS-side class name differs from the Rust
    // ident we'll use — that's true after a collision rename even if the
    // `config.rust_name` (pre-rename) matched `js_name`.
    if rust_name != *js_name {
        wb_parts.push(quote! { js_name = #js_name });
    }

    // Namespace for types inside a JS namespace (e.g., WebAssembly.Module)
    if let Some(ns) = &config.js_namespace {
        wb_parts.push(quote! { js_namespace = #ns });
    }

    let wb_attr = if wb_parts.is_empty() {
        quote! {}
    } else {
        quote! { #[wasm_bindgen(#(#wb_parts),*)] }
    };

    quote! {
        #wb_attr
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub type #rust_ident;
    }
}

/// Generate a constructor binding from a resolved signature.
fn generate_expanded_constructor(config: &ClassConfig, sig: &FunctionSignature) -> TokenStream {
    let rust_ident = super::typemap::make_ident(&sig.rust_name);
    let params = generate_concrete_params(
        &sig.params,
        config.cgctx,
        config.scope,
        &config.from_module(),
    );
    let doc = super::doc_tokens(&sig.doc);

    // Constructors always return the constructed type, wrapped in Result
    // when `catch` (which is always-true here per `SignatureKind::Constructor`).
    let ret = to_return_type(
        &sig.return_type,
        sig.catch,
        sig.error_type.as_ref(),
        config.cgctx,
        config.scope,
        &config.from_module(),
    );

    let mut wb_parts = vec![quote! { constructor }];
    if sig.catch {
        wb_parts.push(quote! { catch });
    }
    // For non-"new" overloads, we need js_name so wasm_bindgen maps them
    // to the same JS constructor.
    if sig.rust_name != "new" {
        let js_name = &config.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub fn #rust_ident(#params) -> #ret;
    }
}

/// Generate an instance method binding from an expanded signature.
fn generate_expanded_method(config: &ClassConfig, sig: &FunctionSignature) -> TokenStream {
    let rust_ident = super::typemap::make_ident(&sig.rust_name);
    let this_type = super::typemap::make_ident(&config.effective_rust_name());
    let params = generate_concrete_params(
        &sig.params,
        config.cgctx,
        config.scope,
        &config.from_module(),
    );
    let doc = super::doc_tokens(&sig.doc);
    let has_variadic = sig.params.last().is_some_and(|p| p.variadic);

    let mut wb_parts: Vec<TokenStream> = vec![quote! { method }];
    if has_variadic {
        wb_parts.push(quote! { variadic });
    }
    if sig.catch {
        wb_parts.push(quote! { catch });
    }
    // Emit js_name when the JS name differs from the Rust name.
    if sig.rust_name != sig.js_name {
        let js_name = &sig.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    let ret_ty = to_return_type(
        &sig.return_type,
        sig.catch,
        sig.error_type.as_ref(),
        config.cgctx,
        config.scope,
        &config.from_module(),
    );
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

    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub #async_kw fn #rust_ident(this: &#this_type, #params) #ret;
    }
}

/// Generate a static method binding from an expanded signature.
fn generate_expanded_static_method(config: &ClassConfig, sig: &FunctionSignature) -> TokenStream {
    let rust_ident = super::typemap::make_ident(&sig.rust_name);
    let class_ident = super::typemap::make_ident(&config.effective_rust_name());
    let params = generate_concrete_params(
        &sig.params,
        config.cgctx,
        config.scope,
        &config.from_module(),
    );
    let doc = super::doc_tokens(&sig.doc);
    let has_variadic = sig.params.last().is_some_and(|p| p.variadic);

    let mut wb_parts: Vec<TokenStream> = vec![quote! { static_method_of = #class_ident }];
    if has_variadic {
        wb_parts.push(quote! { variadic });
    }
    if sig.catch {
        wb_parts.push(quote! { catch });
    }
    if sig.rust_name != sig.js_name {
        let js_name = &sig.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    let ret_ty = to_return_type(
        &sig.return_type,
        sig.catch,
        sig.error_type.as_ref(),
        config.cgctx,
        config.scope,
        &config.from_module(),
    );
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

    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub #async_kw fn #rust_ident(#params) #ret;
    }
}

/// Generate an instance getter binding.
fn generate_getter(
    config: &ClassConfig,
    getter: &GetterMember,
    used_names: &mut HashSet<String>,
) -> TokenStream {
    let this_type = super::typemap::make_ident(&config.effective_rust_name());
    let doc = super::doc_tokens(&getter.doc);

    let candidate = to_snake_case(&getter.js_name);
    let rust_name = dedupe_name(&candidate, used_names);
    let rust_ident = super::typemap::make_ident(&rust_name);

    let getter_type = if getter.optional {
        // Unwrap Nullable to avoid Option<Option<T>> — the optionality from `?`
        // already provides the outer Option.
        let unwrapped = match &getter.type_ref {
            TypeRef::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        let inner = to_syn_type(
            unwrapped,
            TypePosition::RETURN,
            config.cgctx,
            config.scope,
            &config.from_module(),
        );
        quote! { Option<#inner> }
    } else {
        to_syn_type(
            &getter.type_ref,
            TypePosition::RETURN,
            config.cgctx,
            config.scope,
            &config.from_module(),
        )
    };

    let mut wb_parts: Vec<TokenStream> = vec![quote! { method }, quote! { getter }];
    if rust_name != getter.js_name {
        let js_name = &getter.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub fn #rust_ident(this: &#this_type) -> #getter_type;
    }
}

/// Generate instance setter bindings, expanding union types into separate overloads.
fn generate_setter(
    config: &ClassConfig,
    setter: &SetterMember,
    used_names: &mut HashSet<String>,
) -> Vec<TokenStream> {
    let this_type = super::typemap::make_ident(&config.effective_rust_name());
    let doc = setter.doc.clone();

    // Treat the setter as a single-param method and expand through signatures
    let param = crate::ir::Param {
        name: "val".to_string(),
        type_ref: setter.type_ref.clone(),
        optional: false,
        variadic: false,
    };

    let overloads = [&[param][..]];
    let void = crate::ir::TypeRef::Void;
    let sigs = build_signatures(
        &CallableSpec {
            js_name: &setter.js_name,
            kind: SignatureKind::Setter,
            overloads: &overloads,
            return_type: &void,
            error_type: None,
            doc: &doc,
        },
        used_names,
        config.cgctx,
        config.scope,
    );

    sigs.iter()
        .map(|sig| {
            let rust_ident = super::typemap::make_ident(&sig.rust_name);
            let params = generate_concrete_params(
                &sig.params,
                config.cgctx,
                config.scope,
                &config.from_module(),
            );

            let mut wb_parts: Vec<TokenStream> = vec![quote! { method }, quote! { setter }];
            if sig.rust_name != format!("set_{}", setter.js_name) {
                let js_name = &setter.js_name;
                wb_parts.push(quote! { js_name = #js_name });
            }

            let doc = super::doc_tokens(&sig.doc);
            quote! {
                #doc
                #[wasm_bindgen(#(#wb_parts),*)]
                pub fn #rust_ident(this: &#this_type, #params);
            }
        })
        .collect()
}

/// Generate a static getter binding.
fn generate_static_getter(
    config: &ClassConfig,
    getter: &StaticGetterMember,
    used_names: &mut HashSet<String>,
) -> TokenStream {
    let class_ident = super::typemap::make_ident(&config.effective_rust_name());
    let doc = super::doc_tokens(&getter.doc);

    let candidate = to_snake_case(&getter.js_name);
    let rust_name = dedupe_name(&candidate, used_names);
    let rust_ident = super::typemap::make_ident(&rust_name);

    let getter_type = to_syn_type(
        &getter.type_ref,
        TypePosition::RETURN,
        config.cgctx,
        config.scope,
        &config.from_module(),
    );

    let mut wb_parts: Vec<TokenStream> = vec![
        quote! { static_method_of = #class_ident },
        quote! { getter },
    ];
    if rust_name != getter.js_name {
        let js_name = &getter.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub fn #rust_ident() -> #getter_type;
    }
}

/// Generate static setter bindings, expanding union types into separate overloads.
fn generate_static_setter(
    config: &ClassConfig,
    setter: &StaticSetterMember,
    used_names: &mut HashSet<String>,
) -> Vec<TokenStream> {
    let class_ident = super::typemap::make_ident(&config.effective_rust_name());
    let doc = setter.doc.clone();

    let param = crate::ir::Param {
        name: "val".to_string(),
        type_ref: setter.type_ref.clone(),
        optional: false,
        variadic: false,
    };

    let overloads = [&[param][..]];
    let void = crate::ir::TypeRef::Void;
    let sigs = build_signatures(
        &CallableSpec {
            js_name: &setter.js_name,
            kind: SignatureKind::StaticSetter,
            overloads: &overloads,
            return_type: &void,
            error_type: None,
            doc: &doc,
        },
        used_names,
        config.cgctx,
        config.scope,
    );

    sigs.iter()
        .map(|sig| {
            let rust_ident = super::typemap::make_ident(&sig.rust_name);
            let params = generate_concrete_params(
                &sig.params,
                config.cgctx,
                config.scope,
                &config.from_module(),
            );

            let mut wb_parts: Vec<TokenStream> = vec![
                quote! { static_method_of = #class_ident },
                quote! { setter },
            ];
            if sig.rust_name != format!("set_{}", setter.js_name) {
                let js_name = &setter.js_name;
                wb_parts.push(quote! { js_name = #js_name });
            }

            let doc = super::doc_tokens(&sig.doc);
            quote! {
                #doc
                #[wasm_bindgen(#(#wb_parts),*)]
                pub fn #rust_ident(#params);
            }
        })
        .collect()
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Convert concrete params to `fn` parameter token stream.
/// Convert a `TypeRef` representing an extends target into tokens for
/// the `extends = ...` attribute.
///
/// Falls back to `Object` for unresolved types — `extends = JsValue` is
/// never useful (it's implicit and causes conflicting trait impls).
fn extends_tokens(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    from_module: &ModuleContext,
) -> TokenStream {
    let tokens = match ty {
        TypeRef::Named(_) | TypeRef::GenericInstantiation(_, _) => super::typemap::to_syn_type(
            ty,
            TypePosition::ARGUMENT.to_inner(),
            cgctx,
            scope,
            from_module,
        ),
        _ => {
            if let Some(ctx) = cgctx {
                ctx.warn(format!(
                    "unsupported extends type `{ty:?}`, falling back to Object"
                ));
            }
            quote! { Object }
        }
    };
    // JsValue is the root of all wasm_bindgen types — extending it is
    // implicit, so fall back to Object (which is always safe).
    if tokens.to_string() == "JsValue" {
        quote! { Object }
    } else {
        tokens
    }
}
