//! Code generation: IR → Rust source code.
//!
//! The main entry point is [`generate`], which takes a parsed IR [`Module`]
//! and produces formatted Rust source code as a string.

pub mod classes;
pub mod discriminated_unions;
pub mod enums;
pub mod functions;
pub(crate) mod primitive_unions;
pub mod signatures;
pub mod subtyping;
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
    /// When `true`, the default error type for fallible bindings
    /// (`@throws`-less callables that catch JS exceptions) is
    /// `js_sys::Error` rather than `JsValue`. Bindings whose
    /// `@throws` JSDoc names a specific type still use that type.
    ///
    /// Useful in environments that prefer the typed `Error` API
    /// (`message`, `name`, `cause`, …) over raw `JsValue`. The
    /// trade-off is that JS code returning a non-Error throwable
    /// (e.g. `throw "oops"`) now produces a runtime conversion
    /// error rather than the silent `JsValue` pass-through.
    errors_as_error: bool,

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
    ///   globals plus the listed modules' declarations. Modules in
    ///   this set are lifted directly to global scope — there is no
    ///   enclosing `mod` block; the per-decl
    ///   `#[wasm_bindgen(module = "...")]` attribute carries the
    ///   module association.
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

    /// Configure whether untyped caught exceptions use `js_sys::Error`.
    pub fn errors_as_error(mut self, enabled: bool) -> Self {
        self.errors_as_error = enabled;
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
    /// Canonical path of an input file whose global-scope
    /// declarations should be emitted.
    File(PathBuf),
    /// JS module specifier whose declarations are lifted to global
    /// scope in the output.
    Module(String),
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
    let tokens = generate_tokens(module, gctx, options);
    // Validate that the tokens are valid Rust syntax.
    syn::parse2::<syn::File>(tokens.clone()).map_err(|e| {
        anyhow::anyhow!("generated tokens are not valid syn:\n{e}\n\nTokens:\n{tokens}")
    })?;
    let formatted = rustfmt(&tokens.to_string())?;
    // Prepend a header comment. Line comments don't survive `quote!`
    // token generation, so they have to be emitted as plain text
    // after formatting.
    Ok(format!(
        "// Generated by ts-gen. Do not edit.\n\n{formatted}"
    ))
}

/// Format Rust source via `rustfmt`.
///
/// Falls back to `prettyplease` if `rustfmt` is not available.
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
) -> TokenStream {
    let exported_modules: HashSet<String> = options
        .exports
        .iter()
        .filter_map(|e| match e {
            ExportSpec::Module(m) => Some(m.clone()),
            ExportSpec::File(_) => None,
        })
        .collect();
    let cgctx = CodegenContext::from_module_full(
        module,
        gctx,
        options.errors_as_error,
        options.experimental_generic_mono,
        exported_modules,
    );

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

    // Decide whether a declaration is in the output surface. The type
    // universe is the full `module.types` (so resolution sees
    // everything); this only filters emission.
    //
    // * `Global` decls: when `exports` is empty, every input file's
    //   globals are emitted (implicit default). Otherwise the
    //   decl's owning file must be listed as `ExportSpec::File`.
    // * `Module(m)` decls: only emitted when `m` is explicitly listed
    //   as `ExportSpec::Module`. A module that's externalised wins
    //   regardless — emission is suppressed and references route
    //   through the external map.
    let exports_empty = options.exports.is_empty();
    let should_emit = |decl: &crate::ir::TypeDeclaration| -> bool {
        match &decl.module_context {
            crate::ir::ModuleContext::Global => {
                if exports_empty {
                    return true;
                }
                if let Some(path) = module.file_scope_paths.get(&decl.scope_id) {
                    options.exports.contains(&ExportSpec::File(path.clone()))
                } else {
                    // Decls without a tracked file (synthetic, builtin)
                    // are emitted alongside any explicit selection.
                    true
                }
            }
            crate::ir::ModuleContext::Module(m) => {
                let m_str: &str = m.as_ref();
                if cgctx.externalised_modules.contains(m_str) {
                    return false;
                }
                options
                    .exports
                    .contains(&ExportSpec::Module(m_str.to_string()))
            }
        }
    };

    let emitted: Vec<(&TypeDeclaration, Vec<String>)> = module
        .types
        .iter()
        .map(|&type_id| gctx.get_type(type_id))
        .filter(|decl| should_emit(decl))
        .map(|decl| (decl, Vec::new()))
        .collect();
    let mut static_names = functions::StaticNames::default();
    let global_items = generate_members(&emitted, &cgctx, |decl, _| {
        generate_declaration(decl, &cgctx, &mut static_names)
    });

    let external_uses = cgctx.external_use_tokens();

    // Dynamic-union enums synthesised during the per-declaration
    // emit pass.
    let dynamic_unions = generate_dynamic_unions(&cgctx);

    cgctx.take_diagnostics().emit();

    quote! {
        #preamble
        #external_uses
        #(#global_items)*
        #dynamic_unions
    }
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
        // `Array<T>` / `T[]` lowers to `Vec<T>` in return position.
        T::Array(inner) => format!("VecOf{}", dynamic_union_variant_name(inner)),
        T::Nullable(inner) => format!("Optional{}", dynamic_union_variant_name(inner)),
        T::Reference { segments, .. } => {
            // Use the last segment for qualified paths.
            to_pascal_case(segments.last().map(|s| s.as_str()).unwrap_or("Unknown"))
        }
        T::Tuple(_) => "Tuple".to_string(),
        T::Union(_) | T::PropertyKey => "JsValue".to_string(),
        T::Intersection(_) => "JsValue".to_string(),
        T::Function(_) => "Function".to_string(),
        T::Unresolved(_) => "JsValue".to_string(),
    }
}

/// Generate tokens for a single declaration.
fn generate_declaration(
    decl: &TypeDeclaration,
    cgctx: &CodegenContext,
    static_names: &mut functions::StaticNames,
) -> Option<TokenStream> {
    match &decl.kind {
        TypeKind::Class(c) => Some(classes::generate_class(
            c,
            &decl.module_context,
            Some(cgctx),
            decl.scope_id,
        )),
        TypeKind::Interface(i) => match i.classification {
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
        TypeKind::Function(f) => Some(functions::generate_function(
            f,
            &decl.module_context,
            Some(cgctx),
            &decl.doc,
            decl.scope_id,
        )),
        TypeKind::Variable(v) => Some(functions::generate_variable(
            v,
            &decl.module_context,
            Some(cgctx),
            &decl.doc,
            &[],
            decl.scope_id,
            static_names,
        )),
        TypeKind::TypeAlias(alias) => Some(generate_type_alias(
            alias,
            cgctx,
            decl.scope_id,
            &decl.module_context,
        )),
        // Grouped into `pub mod`s by `generate_members`.
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
        if !cgctx.local_types.contains_key(target_name)
            && !crate::codegen::typemap::JS_SYS_RESERVED.contains(&target_name)
        {
            cgctx.warn(format!(
                "Type alias `{}` targets unknown type `{target_name}`, skipping",
                alias.name
            ));
            return quote! {};
        }
    }

    // Resolve the target inside the alias's body scope so its own
    // type parameters bind locally (the alias is a scope-introducing
    // construct in TS). An enum target has no inner form, so it is
    // named directly.
    let pos = if cgctx.enum_kind(&alias.target, alias.body_scope).is_some() {
        typemap::TypePosition::RETURN
    } else {
        typemap::TypePosition::ARGUMENT.to_inner()
    };
    let target = typemap::to_syn_type(
        &alias.target,
        pos,
        Some(cgctx),
        alias.body_scope,
        from_module,
    );
    let name = typemap::make_ident(&alias.name);

    // Identity — skip.
    if target.to_string() == alias.name {
        return quote! {};
    }

    // Type-parameter declaration so aliases that mention generics
    // survive codegen: `pub type Handler<Env, Props> = …;` rather
    // than the bare `Handler = …` that would leave `Props`
    // undeclared. Type aliases use plain `<T, U>` (no `JsGeneric`
    // bound) — they're erased at use-site monomorphisation.
    let generics = if alias.type_params.is_empty() {
        quote! {}
    } else {
        let idents = alias
            .type_params
            .iter()
            .map(|tp| typemap::make_ident(&tp.name))
            .collect::<Vec<_>>();
        quote! { <#(#idents),*> }
    };

    let _ = scope;
    quote! {
        #[allow(dead_code)]
        pub type #name #generics = #target;
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

/// A namespace together with the JS path of the namespaces enclosing it.
type NamespaceSource<'d> = (&'d crate::ir::NamespaceDecl, Vec<String>);

/// Emit `members`, each paired with the JS path of its enclosing namespaces,
/// through `generate`. Namespaces with the same name share one `pub mod`:
/// the namespaces of several modules, or of a module and the global scope,
/// are separate declarations that `namespace_paths` maps to the same path.
fn generate_members<'d>(
    members: &[(&'d TypeDeclaration, Vec<String>)],
    cgctx: &CodegenContext,
    mut generate: impl FnMut(&TypeDeclaration, &[String]) -> Option<TokenStream>,
) -> Vec<TokenStream> {
    let mut groups: HashMap<String, Vec<NamespaceSource<'d>>> = HashMap::new();
    for (decl, js_namespace) in members {
        if let TypeKind::Namespace(ns) = &decl.kind {
            groups
                .entry(crate::util::naming::to_snake_case(&ns.name))
                .or_default()
                .push((ns, js_namespace.clone()));
        }
    }
    let mut items = Vec::new();
    for (decl, js_namespace) in members {
        match &decl.kind {
            TypeKind::Namespace(ns) => {
                if let Some(group) = groups.remove(&crate::util::naming::to_snake_case(&ns.name)) {
                    items.push(generate_namespace(&group, cgctx));
                }
            }
            _ => items.extend(generate(decl, js_namespace)),
        }
    }
    items
}

/// Generate the Rust `mod` block for one or more same-named namespaces, with
/// all their nested declarations.
fn generate_namespace(sources: &[NamespaceSource<'_>], cgctx: &CodegenContext) -> TokenStream {
    let first = sources[0].0;
    let mod_name = typemap::make_ident(&crate::util::naming::to_snake_case(&first.name));
    let members: Vec<(&TypeDeclaration, Vec<String>)> = sources
        .iter()
        .flat_map(|(ns, parent_js_namespace)| {
            let mut js_namespace = parent_js_namespace.clone();
            js_namespace.push(ns.name.clone());
            ns.declarations
                .iter()
                .map(move |&id| (cgctx.gctx.get_type(id), js_namespace.clone()))
        })
        .collect();

    let mut static_names = functions::StaticNames::default();
    let path = cgctx.namespace_paths[&first.child_scope].clone();
    let outer = cgctx.emit_path.replace(path);
    let items = generate_members(&members, cgctx, |decl, js_namespace| {
        generate_ns_declaration(decl, js_namespace, cgctx, &mut static_names)
    });
    cgctx.emit_path.replace(outer);

    // Members name `js_sys` types, globals and the file's external aliases
    // bare, exactly like top-level items.
    quote! {
        pub mod #mod_name {
            #[allow(unused_imports)]
            use super::*;
            #(#items)*
        }
    }
}

/// Generate tokens for a declaration inside a namespace.
/// Adds `js_namespace` attribute to extern blocks so wasm_bindgen emits
/// the correct JS access (e.g., `WebAssembly.Module`).
fn generate_ns_declaration(
    decl: &TypeDeclaration,
    js_namespace: &[String],
    cgctx: &CodegenContext,
    static_names: &mut functions::StaticNames,
) -> Option<TokenStream> {
    match &decl.kind {
        TypeKind::Class(c) => Some(classes::generate_class_with_js_namespace(
            c,
            &decl.module_context,
            js_namespace,
            Some(cgctx),
            decl.scope_id,
        )),
        TypeKind::Interface(i) => match i.classification {
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
        )),
        TypeKind::StringEnum(e) => Some(enums::generate_string_enum(e, js_namespace)),
        TypeKind::NumericEnum(e) => Some(enums::generate_numeric_enum(e, js_namespace)),
        TypeKind::Variable(v) => Some(functions::generate_variable(
            v,
            &decl.module_context,
            Some(cgctx),
            &decl.doc,
            js_namespace,
            decl.scope_id,
            static_names,
        )),
        TypeKind::TypeAlias(_) => None,
        // Grouped into `pub mod`s by `generate_members`.
        TypeKind::Namespace(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::js_namespace_attr;

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
