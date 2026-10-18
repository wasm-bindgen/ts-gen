//! Code generation: IR → Rust source code.
//!
//! The main entry point is [`generate`], which takes a parsed IR [`Module`]
//! and produces formatted Rust source code as a string.

pub mod classes;
pub mod discriminated_unions;
pub mod enums;
pub mod error_enum;
pub mod functions;
pub(crate) mod inline_wrappers;
mod keyof;
pub(crate) mod primitive_unions;
pub mod provides;
pub mod signatures;
pub mod subtyping;
pub mod surface;
mod type_params;
pub mod typemap;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::{InterfaceClassification, Module, TypeDeclaration, TypeKind, TypeRef};
use crate::parse::scope::ScopeId;

use typemap::CodegenContext;

/// Convert an optional doc string into `/// ...` doc-comment attributes.
///
/// Returns an empty `TokenStream` if `doc` is `None`.
pub(crate) fn doc_tokens(doc: &Option<String>) -> TokenStream {
    match doc {
        Some(text) => {
            let lines: Vec<TokenStream> = text
                .lines()
                .map(|line| {
                    // Non-empty lines get a leading space so the rendered
                    // doc reads `/// foo` rather than `///foo`. Empty
                    // lines stay bare so blank-line separators don't
                    // emit as a stray `#[doc = " "]`.
                    let line = if line.is_empty() {
                        String::new()
                    } else {
                        format!(" {line}")
                    };
                    quote! { #[doc = #line] }
                })
                .collect();
            quote! { #(#lines)* }
        }
        None => quote! {},
    }
}

/// Options for controlling code generation output.
#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    /// Rust path of the default error type for fallible bindings
    /// (`@throws`-less callables that catch JS exceptions); `None`
    /// means wasm-bindgen's `JsValue`. Bindings whose `@throws`
    /// JSDoc names a specific type still use that type.
    ///
    /// wasm-bindgen's `catch` lowers to `take_last_exception()?`, so
    /// any type implementing `From<JsValue>` works here: a project's
    /// own error enum, or `::js_sys::Error` for the typed `Error`
    /// API (`message`, `name`, `cause`, …). The trade-off with a
    /// `js_sys` class is that JS code throwing a non-Error value
    /// (`throw "oops"`) is an unchecked cast at the FFI boundary.
    error_type: Option<String>,

    /// Emit sync methods, free functions and constructors as a single
    /// `Result`-returning binding under the primary name, with no
    /// panicking variant and no `try_` prefix. Getters and setters stay
    /// infallible unless annotated `@throws {X}`; `@throws {never}`
    /// makes a callable plain.
    ///
    /// Off by default: the panicking-primary + `try_` shape is what
    /// existing users have. A project whose code is written as
    /// `handler() -> Result<_, Error>` with `?` on every call turns
    /// this on so there is exactly one binding per JS member and it
    /// never aborts on a JS throw.
    fallible: bool,

    /// Name of a generated error enum collecting every type named in a
    /// `@throws` across the output, with `From<JsValue>` dispatching by
    /// `instanceof` (most derived class first) and an `Other(JsValue)`
    /// fallback. When set, every `@throws`-typed binding returns
    /// `Result<T, ThisEnum>` — a union keeps its members instead of
    /// collapsing to the LUB, and a single type routes through the same
    /// enum so callers have one `?`-able type. Untyped bindings keep the
    /// `error_type` default. `None` keeps the LUB lowering.
    error_enum: Option<String>,

    /// Opt generic imports into wasm-bindgen's experimental
    /// per-monomorphization codegen. Type parameters are emitted without
    /// `JsGeneric` bounds and cross the ABI at their concrete Rust types.
    experimental_generic_mono: bool,

    /// The output surface — the authoritative list of inputs (files /
    /// modules) whose declarations are emitted. The `--input` set
    /// still defines the *type universe* for resolution; `exports`
    /// only selects what gets emitted as public Rust code.
    ///
    /// * Empty → implicit default: emit every input file's global
    ///   declarations. No module-context declarations are emitted.
    /// * Non-empty → authoritative: emit exactly the listed files'
    ///   globals plus the listed modules' exports, and the types their
    ///   signatures reach. Each module is
    ///   emitted in a `pub mod` named by its specifier (`node:fs` →
    ///   `node::fs`), except the `--lib-name` module, which is emitted
    ///   at the top level.
    ///
    /// A module that is fully covered by `--external` is always
    /// suppressed from emission regardless of its presence here.
    exports: HashSet<ExportSpec>,
}

impl GenerateOptions {
    /// Create code-generation options with the default behavior.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the default error type for fallible bindings to a Rust path
    /// such as `crate::Error`. The type must implement `From<JsValue>`.
    pub fn error_type(mut self, path: impl Into<String>) -> Self {
        self.error_type = Some(path.into());
        self
    }

    /// Shorthand for [`error_type`](Self::error_type) with
    /// `::js_sys::Error`; `false` restores the `JsValue` default.
    pub fn errors_as_error(mut self, enabled: bool) -> Self {
        self.error_type = enabled.then(|| "::js_sys::Error".to_string());
        self
    }

    /// Emit every sync callable as one `Result`-returning binding
    /// instead of a panicking primary plus `try_` companion.
    pub fn fallible(mut self, enabled: bool) -> Self {
        self.fallible = enabled;
        self
    }

    /// Generate an enum of this name over every `@throws` type and
    /// return it from every `@throws`-typed binding.
    pub fn error_enum(mut self, name: impl Into<String>) -> Self {
        self.error_enum = Some(name.into());
        self
    }

    /// Configure wasm-bindgen's experimental per-monomorphization codegen.
    pub fn experimental_generic_mono(mut self, enabled: bool) -> Self {
        self.experimental_generic_mono = enabled;
        self
    }

    /// Select the input files and modules emitted as public Rust code.
    pub fn exports(mut self, exports: HashSet<ExportSpec>) -> Self {
        self.exports = exports;
        self
    }
}

/// A single entry in [`GenerateOptions::exports`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExportSpec {
    /// Canonical path of an input file whose globals are emitted.
    File(PathBuf),
    /// JS module specifier whose declarations are emitted in its
    /// `pub mod`.
    Module(String),
    /// Public name of an export, optionally dotted through namespaces.
    /// Emits that export of every file's globals and every module, as if
    /// its root were listed with only that export in it.
    Decl(String),
}

/// Generate Rust source code from a parsed IR module + global context.
///
/// Returns the formatted source as a string, ready to be written to a file.
pub fn generate(module: &Module, gctx: &crate::context::GlobalContext) -> anyhow::Result<String> {
    generate_with_options(module, gctx, &GenerateOptions::default())
}

/// Generate Rust source code with explicit options.
pub fn generate_with_options(
    module: &Module,
    gctx: &crate::context::GlobalContext,
    options: &GenerateOptions,
) -> anyhow::Result<String> {
    let tokens = generate_tokens(module, gctx, options)?;
    // Validate that the tokens are valid Rust syntax.
    let mut file = syn::parse2::<syn::File>(tokens.clone()).map_err(|e| {
        anyhow::anyhow!("generated tokens are not valid syn:\n{e}\n\nTokens:\n{tokens}")
    })?;
    inline_wrappers::apply(&mut file);
    split_unshared_views(&mut file.items);
    let formatted = rustfmt(&quote::ToTokens::to_token_stream(&file).to_string())?;
    // Prepend a header comment. Line comments don't survive `quote!`
    // token generation, so they have to be emitted as plain text
    // after formatting.
    Ok(format!(
        "// Generated by ts-gen. Do not edit.\n\n{formatted}"
    ))
}

/// Placeholder `slice` value for [`typemap::SliceMode::UnsharedView`], which
/// [`split_unshared_views`] replaces before the output is rendered.
pub(crate) const UNSHARED_VIEW: &str = "__ts_gen_unshared_view";

/// Split every extern fn with a [`typemap::SliceMode::UnsharedView`]
/// argument into a `view` without `target_feature = "atomics"` and a `copy`
/// with it. An atomics build's memory is shared, so a view there would be a
/// `SharedArrayBuffer` view the callee rejects. Only a whole-fn `#[cfg]`
/// reaches wasm-bindgen: it reads a parameter's `cfg_attr` unevaluated.
fn split_unshared_views(items: &mut [syn::Item]) {
    for item in items {
        match item {
            syn::Item::Mod(syn::ItemMod {
                content: Some((_, items)),
                ..
            }) => split_unshared_views(items),
            syn::Item::ForeignMod(foreign) => {
                foreign.items = std::mem::take(&mut foreign.items)
                    .into_iter()
                    .flat_map(|item| match item {
                        syn::ForeignItem::Fn(f) if has_unshared_view(&f) => vec![
                            syn::ForeignItem::Fn(with_unshared_views(
                                &f,
                                syn::parse_quote!(#[cfg(not(target_feature = "atomics"))]),
                                "view",
                            )),
                            syn::ForeignItem::Fn(with_unshared_views(
                                &f,
                                syn::parse_quote!(#[cfg(target_feature = "atomics")]),
                                "copy",
                            )),
                        ],
                        item => vec![item],
                    })
                    .collect();
            }
            _ => {}
        }
    }
}

fn unshared_view_attr() -> syn::Attribute {
    syn::parse_quote!(#[wasm_bindgen(slice = #UNSHARED_VIEW)])
}

fn has_unshared_view(f: &syn::ForeignItemFn) -> bool {
    let marker = unshared_view_attr();
    f.sig.inputs.iter().any(|arg| match arg {
        syn::FnArg::Typed(pat) => pat.attrs.contains(&marker),
        syn::FnArg::Receiver(_) => false,
    })
}

fn with_unshared_views(
    f: &syn::ForeignItemFn,
    cfg: syn::Attribute,
    mode: &str,
) -> syn::ForeignItemFn {
    let marker = unshared_view_attr();
    let mut f = f.clone();
    f.attrs.insert(0, cfg);
    for arg in &mut f.sig.inputs {
        if let syn::FnArg::Typed(pat) = arg {
            for attr in &mut pat.attrs {
                if *attr == marker {
                    *attr = syn::parse_quote!(#[wasm_bindgen(slice = #mode)]);
                }
            }
        }
    }
    f
}

/// The `--provides` wrapper and app scaffold, as `(provides.rs, app.rs)`
/// sources.
pub fn generate_provides(
    module: &Module,
    gctx: &crate::context::GlobalContext,
    options: &GenerateOptions,
    fns: &[crate::provides::ProvidedFn],
    paths: &provides::ProvidesPaths,
) -> anyhow::Result<(String, String)> {
    let surface = surface::Surface::build(module, gctx, options);
    let cgctx = CodegenContext::from_module_full(module, gctx, options, Some(&surface))?;
    let wrapper = provides::generate_wrapper(fns, &cgctx, paths)?;
    let app = provides::generate_app(fns, &cgctx, paths)?;
    cgctx.diagnostics.borrow().emit();
    Ok((wrapper, app))
}

/// Validate as a `syn::File` and format with prettyplease, which unlike
/// rustfmt also normalises attribute arguments it cannot parse as meta
/// (`js_namespace = ["a", "b"]`).
pub(crate) fn rustfmt_tokens(tokens: &proc_macro2::TokenStream) -> anyhow::Result<String> {
    let file = syn::parse2::<syn::File>(tokens.clone()).map_err(|e| {
        anyhow::anyhow!("generated tokens are not valid syn:\n{e}\n\nTokens:\n{tokens}")
    })?;
    Ok(prettyplease::unparse(&file))
}

/// Format Rust source via `rustfmt`, falling back to `prettyplease` if
/// `rustfmt` is not available.
fn rustfmt(code: &str) -> anyhow::Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = match Command::new("rustfmt")
        .arg("--edition=2021")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            let file = syn::parse_str::<syn::File>(code)?;
            return Ok(prettyplease::unparse(&file));
        }
    };

    child.stdin.take().unwrap().write_all(code.as_bytes())?;

    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?)
    } else {
        let file = syn::parse_str::<syn::File>(code)?;
        Ok(prettyplease::unparse(&file))
    }
}

/// Generate the token stream for a full module.
///
/// `Promise<T>` awaitability used to require an emitted `PromiseExt` trait
/// (orphan rule prevented an `IntoFuture` impl from generated code), but
/// upstream `js_sys` now implements `IntoFuture` for `Promise<T>` directly,
/// so callers get `.await` for free without anything generated here.
fn generate_tokens(
    module: &Module,
    gctx: &crate::context::GlobalContext,
    options: &GenerateOptions,
) -> anyhow::Result<TokenStream> {
    let surface = surface::Surface::build(module, gctx, options);
    let cgctx = CodegenContext::from_module_full(module, gctx, options, Some(&surface))?;

    let preamble = quote! {
        #[allow(unused_imports)]
        use wasm_bindgen::prelude::*;
        #[allow(unused_imports)]
        use js_sys::*;
    };

    // Partial-coverage diagnostic: when a module is explicitly
    // emitted but only *some* of its types resolve through the
    // external map, the user has a mismatch — call it out so they
    // can either expand the external map to cover the rest or drop
    // the partial mapping.
    let mut module_groups: std::collections::BTreeMap<
        std::rc::Rc<str>,
        Vec<crate::context::TypeId>,
    > = std::collections::BTreeMap::new();
    for &type_id in &module.types {
        let decl = gctx.get_type(type_id);
        if let crate::ir::ModuleContext::Module(m) = &decl.module_context {
            module_groups.entry(m.clone()).or_default().push(type_id);
        }
    }
    for (spec, ids) in &module_groups {
        if cgctx.externalised_modules.contains(spec.as_ref()) {
            continue;
        }
        if !options
            .exports
            .contains(&ExportSpec::Module(spec.to_string()))
        {
            continue;
        }
        let unresolved = cgctx.external_module_unresolved(spec, ids);
        if !unresolved.is_empty() && unresolved.len() < ids.len() {
            cgctx.warn(format!(
                "module \"{spec}\" partially externalised; emitting bindings for \
                 unresolved types: {}",
                unresolved.join(", ")
            ));
        }
    }

    let mut root = ModTree::default();
    for &type_id in &module.types {
        for decl in surface.get(type_id) {
            root.at(&cgctx.module_path(&decl.module_context)).insert(
                type_id,
                decl,
                Vec::new(),
                &surface,
            );
        }
    }
    let global_items = root.generate(&mut Vec::new(), &cgctx);
    cgctx.emit_path.replace(Vec::new());

    for name in &surface.missing {
        cgctx.error(format!(
            "`--export {name}`: no declaration named `{name}` in the input files"
        ));
    }

    let error_enum = options.error_enum.as_deref().map(|name| {
        let names = error_enum::collect_throws_names(&module.types, &surface);
        error_enum::generate_error_enum(name, &names, &cgctx)
    });

    let external_uses = cgctx.external_use_tokens();

    // Dynamic-union enums synthesised during the per-declaration
    // emit pass.
    let dynamic_unions = generate_dynamic_unions(&cgctx);

    cgctx.take_diagnostics().emit();

    Ok(quote! {
        #preamble
        #external_uses
        #(#global_items)*
        #dynamic_unions
        #error_enum
    })
}

/// Emit the synthesised dynamic-union enums collected on `cgctx`
/// during the per-declaration pass. Each enum lowers each member
/// type via the regular return-position path, resolved from the
/// scope and module of the union's first use site.
fn generate_dynamic_unions(cgctx: &CodegenContext) -> TokenStream {
    use crate::ir::TypeRef as T;
    let registry = cgctx.dynamic_unions.borrow();
    if registry.by_key.is_empty() {
        return quote! {};
    }
    let enums: Vec<TokenStream> = registry
        .by_key
        .values()
        .map(|info| {
            let enum_ident = typemap::make_ident(&info.rust_name);
            let variants: Vec<TokenStream> = info
                .members
                .iter()
                .zip(dynamic_union_variant_names(&info.members))
                .map(|(m, variant_name)| {
                    let variant_ident = typemap::make_ident(&variant_name);
                    match m {
                        // Literal members become discriminant variants.
                        // `typemap::dynamic_union_members` only keeps
                        // number literals when every member is a 32-bit
                        // integer literal (a C-style enum), and widens
                        // boolean literals to `boolean`.
                        T::StringLiteral(s) => quote! { #variant_ident = #s },
                        T::NumberLiteral(n) if n.fract() == 0.0 => {
                            let lit = proc_macro2::Literal::i64_unsuffixed(n.abs() as i64);
                            let sign = (*n < 0.0).then(|| quote! { - });
                            quote! { #variant_ident = #sign #lit }
                        }
                        _ => {
                            let payload = typemap::to_syn_type(
                                m,
                                typemap::TypePosition::RETURN,
                                Some(cgctx),
                                info.scope,
                                &info.from_module,
                            );
                            quote! { #variant_ident(#payload) }
                        }
                    }
                })
                .collect();
            quote! {
                #[wasm_bindgen(private)]
                pub enum #enum_ident {
                    #(#variants),*
                }
            }
        })
        .collect();
    quote! { #(#enums)* }
}

/// Variant names for the members of a synthesised dynamic union, in member
/// order. A name already taken in the enum (a `string` arm next to a
/// `"String"` literal) is numbered, first-seen wins.
pub(crate) fn dynamic_union_variant_names(members: &[crate::ir::TypeRef]) -> Vec<String> {
    use crate::ir::TypeRef as T;
    let mut used = std::collections::HashSet::new();
    members
        .iter()
        .map(|m| {
            let base = match m {
                T::StringLiteral(s) => crate::util::naming::to_pascal_case(s),
                T::NumberLiteral(n) if n.fract() == 0.0 => {
                    crate::util::naming::number_literal_pascal(*n)
                }
                _ => dynamic_union_variant_name(m),
            };
            if used.insert(base.clone()) {
                return base;
            }
            (2..)
                .map(|i| format!("{base}{i}"))
                .find(|candidate| used.insert(candidate.clone()))
                .unwrap()
        })
        .collect()
}

/// Pick a Rust variant name for a `TypeRef` member of a synthesised
/// dynamic union.
///
/// Variants are named after the **Rust payload type** rather than
/// the TypeScript spelling, so the rendered enum reads consistently:
///
/// ```rust,ignore
/// pub enum ContentKind {
///     String(String),
///     F64(f64),
///     Bool(bool),
///     ArrayBuffer(ArrayBuffer),
///     Uint8Array(Uint8Array),    // not `ArrayBufferView` — that's the TS spelling
/// }
/// ```
///
/// The mapping mirrors `to_syn_type` at return position:
///
/// | TypeScript        | Rust (return)   | Variant name    |
/// | ----------------- | --------------- | --------------- |
/// | `string`          | `String`        | `String`        |
/// | `number`          | `f64`           | `F64`           |
/// | `bigint`          | `BigInt`        | `BigInt`        |
/// | `boolean`         | `bool`          | `Bool`          |
/// | `symbol`          | `Symbol`        | `Symbol`        |
/// | `ArrayBufferView` | `Uint8Array`    | `Uint8Array`    |
/// | `Foo` (named)     | `Foo`           | `Foo`           |
fn dynamic_union_variant_name(ty: &TypeRef) -> String {
    use crate::ir::TypeRef as T;
    use crate::util::naming::to_pascal_case;
    match ty {
        T::Boolean | T::BooleanLiteral(_) => "Bool".to_string(),
        T::Number | T::NumberLiteral(_) => "F64".to_string(),
        T::RustScalar(s) | T::WideScalar(s) => s.as_str().to_uppercase(),
        T::String | T::StringLiteral(_) => "String".to_string(),
        T::BigInt => "BigInt".to_string(),
        T::Void | T::Undefined => "Undefined".to_string(),
        T::Null => "Null".to_string(),
        T::Any | T::Unknown => "JsValue".to_string(),
        T::Object => "Object".to_string(),
        T::Symbol => "Symbol".to_string(),
        // Return-position lowering specialises `ArrayBufferView` to
        // `Uint8Array` (see `to_syn_type`), so the variant name
        // matches the payload.
        T::ArrayBufferView => "Uint8Array".to_string(),
        T::TypedSlice(e, _) => e.typed_array().to_string(),
        // `Array<T>` / `T[]` lowers to `Vec<T>` in return position.
        T::Array(inner) => format!("VecOf{}", dynamic_union_variant_name(inner)),
        T::Nullable(inner, _) => format!("Optional{}", dynamic_union_variant_name(inner)),
        T::Reference { segments, .. } => {
            // Use the last segment for qualified paths.
            to_pascal_case(segments.last().map(|s| s.as_str()).unwrap_or("Unknown"))
        }
        T::Tuple(_) => "Tuple".to_string(),
        T::Union(_) | T::PropertyKey => "JsValue".to_string(),
        T::Intersection(_) => "JsValue".to_string(),
        T::Function(_) => "Function".to_string(),
        T::Unresolved(_) | T::IndexedAccess(..) => "JsValue".to_string(),
    }
}

/// Generate tokens for a single declaration.
fn generate_declaration(
    decl: &TypeDeclaration,
    cgctx: &CodegenContext,
    value_names: &mut functions::ValueNames,
) -> Option<TokenStream> {
    match &decl.kind {
        TypeKind::Class(c) => Some(classes::generate_class(
            c,
            &decl.module_context,
            Some(cgctx),
            decl.scope_id,
        )),
        TypeKind::Interface(i) => match classes::effective_classification(i, Some(cgctx)) {
            InterfaceClassification::ClassLike | InterfaceClassification::Unclassified => {
                Some(classes::generate_class_like_interface(
                    i,
                    &decl.module_context,
                    Some(cgctx),
                    &[],
                    decl.scope_id,
                ))
            }
            InterfaceClassification::Dictionary => Some(classes::generate_dictionary_extern(
                i,
                &decl.module_context,
                Some(cgctx),
                &[],
                decl.scope_id,
            )),
        },
        TypeKind::DiscriminatedUnion(d) => {
            Some(discriminated_unions::generate_discriminated_union(
                d,
                &decl.module_context,
                Some(cgctx),
                decl.scope_id,
            ))
        }
        TypeKind::StringEnum(e) => Some(enums::generate_string_enum(e, &[])),
        TypeKind::NumericEnum(e) => Some(enums::generate_numeric_enum(e, &[])),
        // Evaluated into one of the above once parsing ends.
        TypeKind::TsEnum(_) => None,
        TypeKind::Function(f) => Some(functions::generate_function(
            f,
            &decl.module_context,
            Some(cgctx),
            &decl.doc,
            decl.scope_id,
            value_names,
        )),
        TypeKind::Variable(v) => Some(functions::generate_variable(
            v,
            &decl.module_context,
            Some(cgctx),
            &decl.doc,
            &[],
            decl.scope_id,
            value_names,
        )),
        TypeKind::TypeAlias(alias) => Some(generate_type_alias(
            alias,
            cgctx,
            decl.scope_id,
            &decl.module_context,
        )),
        // Namespaces are `ModTree` children.
        TypeKind::Namespace(_) => None,
    }
}

/// Generate output for a TypeAlias declaration.
///
/// - Local alias: `pub type WritableStream = Writable;`
/// - External re-export: `pub use external_crate::Foo;`
fn generate_type_alias(
    alias: &crate::ir::TypeAliasDecl,
    cgctx: &CodegenContext,
    scope: ScopeId,
    from_module: &crate::ir::ModuleContext,
) -> TokenStream {
    if let Some(ref module) = alias.from_module {
        // External re-export: resolve through external map. The
        // target name is the bare ident from the alias target if
        // available, otherwise the alias name itself.
        let type_name = alias.target.as_ident().unwrap_or(alias.name.as_str());
        if let Some(rust_path) = cgctx.resolve_external(type_name, module) {
            let path: syn::Path = syn::parse_str(&rust_path.path).unwrap_or_else(|_| {
                syn::Path::from(syn::Ident::new("JsValue", proc_macro2::Span::call_site()))
            });
            let name = typemap::make_ident(&alias.name);
            if alias.name == type_name {
                return quote! { pub use #path; };
            } else {
                return quote! { pub use #path as #name; };
            }
        }
        cgctx.warn(format!(
            "No external mapping for `{}` from \"{module}\" — emitting JsValue alias",
            type_name,
        ));
        let name = typemap::make_ident(&alias.name);
        return quote! {
            #[allow(dead_code)]
            pub type #name = JsValue;
        };
    }

    // Local alias — only emit if the target resolves to a known type.
    // We only inspect bare references here; generic instantiations
    // and qualified paths flow through to `to_syn_type` unchanged.
    if let Some(target_name) = alias.target.as_ident() {
        let is_type_param = matches!(
            cgctx
                .gctx
                .scopes
                .resolve_binding(alias.body_scope, target_name),
            Some(crate::parse::scope::Binding::TypeParam)
        );
        let resolves_local = cgctx
            .gctx
            .scopes
            .resolve(alias.body_scope, target_name)
            .is_some_and(|id| cgctx.local_type_ids.contains(&id));
        if !is_type_param
            && !resolves_local
            && !cgctx.local_types.contains_key(target_name)
            && !crate::codegen::typemap::JS_SYS_RESERVED.contains(&target_name)
        {
            cgctx.warn(format!(
                "Type alias `{}` targets unknown type `{target_name}`, skipping",
                alias.name
            ));
            return quote! {};
        }
    }

    let target = cgctx.alias_target(alias, from_module);
    let renamed = cgctx.renamed_locals.get(&alias.name);
    let name = typemap::make_ident(renamed.map_or(alias.name.as_str(), String::as_str));

    // Identity — skip.
    if target.to_string() == alias.name {
        return quote! {};
    }

    let public_alias = if renamed.is_some() && !typemap::is_prelude_reserved(&alias.name) {
        let public = typemap::make_ident(&alias.name);
        quote! { pub use #name as #public; }
    } else {
        quote! {}
    };

    // Type-parameter declaration so aliases that mention generics
    // survive codegen: `pub type Handler<Env, Props> = …;` rather
    // than the bare `Handler = …` that would leave `Props`
    // undeclared. Type aliases use plain `<T, U>` (no `JsGeneric`
    // bound) — they're erased at use-site monomorphisation. Parameters
    // the target doesn't mention are dropped, matching every use site.
    // `None` for an alias without type parameters.
    let generics = match cgctx.alias_params(alias, from_module) {
        Some(params) => {
            let idents = alias
                .type_params
                .iter()
                .enumerate()
                .filter(|&(i, _)| params.kept[i])
                .map(|(i, tp)| {
                    let ident = typemap::make_ident(&tp.name);
                    match &tp.default {
                        Some(default) if params.defaulted[i] => {
                            let default = cgctx.alias_param_default(default, alias, from_module);
                            quote! { #ident = #default }
                        }
                        _ => quote! { #ident },
                    }
                })
                .collect::<Vec<_>>();
            if idents.is_empty() {
                quote! {}
            } else {
                quote! { <#(#idents),*> }
            }
        }
        None => quote! {},
    };

    let _ = scope;
    quote! {
        #[allow(dead_code)]
        pub type #name #generics = #target;
        #public_alias
    }
}

/// `js_namespace = "Outer"` for one segment, `js_namespace = ["Outer",
/// "Inner"]` for a nested namespace, so a member binds to its full JS path.
pub(crate) fn js_namespace_attr(js_namespace: &[String]) -> Option<TokenStream> {
    match js_namespace {
        [] => None,
        [ns] => Some(quote! { js_namespace = #ns }),
        segments => Some(quote! { js_namespace = [#(#segments),*] }),
    }
}

/// The top level of the output, or one `pub mod` in it. A declared module
/// (`node:fs` → `node::fs`) and a namespace each get one, and share it when
/// their paths meet, as `cloudflare:email` and a global
/// `namespace Cloudflare` do under `cloudflare`.
#[derive(Default)]
struct ModTree<'d> {
    entries: Vec<ModEntry<'d>>,
    children: HashMap<String, ModTree<'d>>,
    /// The other public names of each child's namespace, which alias it.
    aliases: HashMap<String, Vec<String>>,
}

enum ModEntry<'d> {
    /// A declaration and the JS path of the namespaces enclosing it.
    Item(&'d TypeDeclaration, Vec<String>),
    /// A child `pub mod`, at the position of its first declaration.
    Mod(String),
}

impl<'d> ModTree<'d> {
    fn child(&mut self, name: &str) -> &mut ModTree<'d> {
        if !self.children.contains_key(name) {
            self.entries.push(ModEntry::Mod(name.to_string()));
        }
        self.children.entry(name.to_string()).or_default()
    }

    fn at(&mut self, path: &[String]) -> &mut ModTree<'d> {
        path.iter().fold(self, |node, name| node.child(name))
    }

    /// Add `decl`, whose enclosing namespaces have the JS path
    /// `js_namespace`. A namespace becomes a child holding those of its
    /// members that are in the surface, under the name it binds by in JS.
    fn insert(
        &mut self,
        id: crate::context::TypeId,
        decl: &'d TypeDeclaration,
        mut js_namespace: Vec<String>,
        surface: &'d surface::Surface<'d>,
    ) {
        match &decl.kind {
            TypeKind::Namespace(ns) => {
                let name = surface.mod_name(id, &ns.name);
                let aliases = self.aliases.entry(name.clone()).or_default();
                for public in surface.names(id).iter().filter(|&p| p != "default") {
                    let alias = crate::util::naming::to_snake_case(public);
                    if alias != name && !aliases.contains(&alias) {
                        aliases.push(alias);
                    }
                }
                let child = self.child(&name);
                js_namespace.push(surface.js_name(id, &ns.name).to_string());
                for &member in &ns.declarations {
                    for decl in surface.get(member) {
                        child.insert(member, decl, js_namespace.clone(), surface);
                    }
                }
            }
            _ => self.entries.push(ModEntry::Item(decl, js_namespace)),
        }
    }

    /// Generate this module's items. `path` is its `pub mod` path.
    fn generate(&self, path: &mut Vec<String>, cgctx: &CodegenContext) -> Vec<TokenStream> {
        let mut value_names =
            functions::ValueNames::new(self.entries.iter().filter_map(|entry| match entry {
                ModEntry::Item(decl, js_namespace) => Some((*decl, js_namespace.as_slice())),
                ModEntry::Mod(_) => None,
            }));
        // A child module's `use super::*` brings its parent's statics into
        // scope too.
        let outer_statics = cgctx.statics_in_scope.borrow().clone();
        cgctx
            .statics_in_scope
            .borrow_mut()
            .extend(value_names.statics().iter().cloned());
        let mut items = Vec::new();
        for entry in &self.entries {
            match entry {
                ModEntry::Item(decl, js_namespace) => {
                    cgctx.emit_path.replace(path.clone());
                    items.extend(if js_namespace.is_empty() {
                        generate_declaration(decl, cgctx, &mut value_names)
                    } else {
                        generate_ns_declaration(decl, js_namespace, cgctx, &mut value_names)
                    });
                }
                ModEntry::Mod(name) => {
                    path.push(name.clone());
                    let members = self.children[name].generate(path, cgctx);
                    path.pop();
                    let ident = typemap::make_ident(name);
                    // Members name `js_sys` types, globals and the file's
                    // external aliases bare, exactly like top-level items.
                    items.push(quote! {
                        pub mod #ident {
                            #[allow(unused_imports)]
                            use super::*;
                            #(#members)*
                        }
                    });
                    // A module alias rather than `pub use`, which would also
                    // import a same-named function under the alias.
                    let aliases = self.aliases.get(name).into_iter().flatten();
                    for alias in aliases.filter(|a| !self.children.contains_key(*a)) {
                        let alias = typemap::make_ident(alias);
                        items.push(quote! {
                            pub mod #alias {
                                pub use super::#ident::*;
                            }
                        });
                    }
                }
            }
        }
        cgctx.statics_in_scope.replace(outer_statics);
        items
    }
}

/// Generate tokens for a declaration inside a namespace.
/// Adds `js_namespace` attribute to extern blocks so wasm_bindgen emits
/// the correct JS access (e.g., `WebAssembly.Module`).
fn generate_ns_declaration(
    decl: &TypeDeclaration,
    js_namespace: &[String],
    cgctx: &CodegenContext,
    value_names: &mut functions::ValueNames,
) -> Option<TokenStream> {
    match &decl.kind {
        TypeKind::Class(c) => Some(classes::generate_class_with_js_namespace(
            c,
            &decl.module_context,
            js_namespace,
            Some(cgctx),
            decl.scope_id,
        )),
        TypeKind::Interface(i) => match classes::effective_classification(i, Some(cgctx)) {
            InterfaceClassification::ClassLike | InterfaceClassification::Unclassified => {
                Some(classes::generate_class_like_interface(
                    i,
                    &decl.module_context,
                    Some(cgctx),
                    js_namespace,
                    decl.scope_id,
                ))
            }
            InterfaceClassification::Dictionary => Some(classes::generate_dictionary_extern(
                i,
                &decl.module_context,
                Some(cgctx),
                js_namespace,
                decl.scope_id,
            )),
        },
        TypeKind::DiscriminatedUnion(d) => {
            // Discriminated unions don't currently support `js_namespace`
            // (no observed cases in the wild). Emit without the namespace
            // attribute — if a real case shows up we can thread it
            // through `ClassConfig::js_namespace` like the others.
            Some(discriminated_unions::generate_discriminated_union(
                d,
                &decl.module_context,
                Some(cgctx),
                decl.scope_id,
            ))
        }
        TypeKind::Function(f) => Some(functions::generate_function_with_js_namespace(
            f,
            &decl.module_context,
            js_namespace,
            Some(cgctx),
            &decl.doc,
            decl.scope_id,
            value_names,
        )),
        TypeKind::StringEnum(e) => Some(enums::generate_string_enum(e, js_namespace)),
        TypeKind::NumericEnum(e) => Some(enums::generate_numeric_enum(e, js_namespace)),
        TypeKind::TsEnum(_) => None,
        TypeKind::Variable(v) => Some(functions::generate_variable(
            v,
            &decl.module_context,
            Some(cgctx),
            &decl.doc,
            js_namespace,
            decl.scope_id,
            value_names,
        )),
        TypeKind::TypeAlias(alias) => Some(generate_type_alias(
            alias,
            cgctx,
            decl.scope_id,
            &decl.module_context,
        )),
        // Namespaces are `ModTree` children.
        TypeKind::Namespace(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{js_namespace_attr, split_unshared_views, UNSHARED_VIEW};
    use quote::{quote, ToTokens};

    #[test]
    fn unshared_views_split_on_target_feature_atomics() {
        let mut file: syn::File = syn::parse2(quote! {
            pub mod inner {
                extern "C" {
                    #[wasm_bindgen(js_name = "keep")]
                    pub fn keep(#[wasm_bindgen(slice = #UNSHARED_VIEW)] data: &[u8], n: f64);
                    pub fn other(#[wasm_bindgen(slice = "view")] data: &[u8]);
                }
            }
        })
        .unwrap();
        split_unshared_views(&mut file.items);
        let expected: syn::File = syn::parse2(quote! {
            pub mod inner {
                extern "C" {
                    #[cfg(not(target_feature = "atomics"))]
                    #[wasm_bindgen(js_name = "keep")]
                    pub fn keep(#[wasm_bindgen(slice = "view")] data: &[u8], n: f64);
                    #[cfg(target_feature = "atomics")]
                    #[wasm_bindgen(js_name = "keep")]
                    pub fn keep(#[wasm_bindgen(slice = "copy")] data: &[u8], n: f64);
                    pub fn other(#[wasm_bindgen(slice = "view")] data: &[u8]);
                }
            }
        })
        .unwrap();
        assert_eq!(
            file.to_token_stream().to_string(),
            expected.to_token_stream().to_string()
        );
    }

    #[test]
    fn js_namespace_attr_spells_one_segment_bare_and_nested_as_an_array() {
        let attr = |path: &[&str]| {
            let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
            js_namespace_attr(&path).map(|t| t.to_string())
        };
        assert_eq!(attr(&[]), None);
        assert_eq!(
            attr(&["Outer"]).as_deref(),
            Some("js_namespace = \"Outer\"")
        );
        assert_eq!(
            attr(&["Outer", "Inner"]).as_deref(),
            Some("js_namespace = [\"Outer\" , \"Inner\"]")
        );
    }
}
