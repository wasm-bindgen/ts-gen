//! Named `Record<K, V>` alias generation.

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::signatures::{
    expand_signatures, generate_dictionary_params, render_generic_bounds, type_snake_name,
};
use crate::codegen::typemap::{CodegenContext, TypePosition};
use crate::ir::{ModuleContext, Param, RecordDecl, TypeRef};
use crate::parse::scope::ScopeId;
use crate::util::naming::to_snake_case;

struct RecordWrapper {
    extern_attr: TokenStream,
    rust_name: String,
    rust_ident: syn::Ident,
    type_args: TokenStream,
    type_bounds: Vec<TokenStream>,
    type_generics: TokenStream,
    constructor_impl_generics: TokenStream,
}

#[derive(Clone)]
enum FiniteKey {
    String(String),
    Number(f64),
    Boolean(bool),
}

struct SetterBinding {
    key_type: TypeRef,
    value_type: TypeRef,
    method_ident: syn::Ident,
}

impl RecordWrapper {
    fn new(
        decl: &RecordDecl,
        module_context: &ModuleContext,
        cgctx: Option<&CodegenContext<'_>>,
    ) -> Self {
        let module = match module_context {
            ModuleContext::Global => None,
            ModuleContext::Module(module) => Some(module.as_ref()),
        };
        let rust_name = cgctx
            .and_then(|ctx| ctx.renamed_locals.get(&decl.name))
            .cloned()
            .unwrap_or_else(|| decl.name.clone());
        let rust_ident = super::typemap::make_ident(&rust_name);
        let type_param_idents = decl
            .type_params
            .iter()
            .map(|param| super::typemap::make_ident(&param.name))
            .collect::<Vec<_>>();
        let type_args = if type_param_idents.is_empty() {
            quote! {}
        } else {
            quote! { <#(#type_param_idents),*> }
        };
        let per_mono = cgctx.is_some_and(|ctx| ctx.experimental_generic_mono);
        let type_bounds = type_param_idents
            .iter()
            .map(|ident| {
                if per_mono {
                    quote! { #ident }
                } else {
                    quote! { #ident: ::wasm_bindgen::JsGeneric }
                }
            })
            .collect::<Vec<_>>();
        let type_generics = render_generic_bounds(&type_bounds);
        let constructor_impl_generics = if per_mono {
            render_generic_bounds(
                &type_param_idents
                    .iter()
                    .map(|ident| quote! { #ident: ::wasm_bindgen::convert::IntoWasmAbi })
                    .collect::<Vec<_>>(),
            )
        } else {
            type_generics.clone()
        };

        Self {
            extern_attr: CodegenContext::extern_attr(cgctx, module),
            rust_name,
            rust_ident,
            type_args,
            type_bounds,
            type_generics,
            constructor_impl_generics,
        }
    }

    fn render(
        &self,
        source_name: &str,
        getters: &[TokenStream],
        setters: &[TokenStream],
        construction: &TokenStream,
    ) -> TokenStream {
        let Self {
            extern_attr,
            rust_name,
            rust_ident,
            type_generics,
            ..
        } = self;
        let public_alias = if rust_name != source_name {
            let public_ident = super::typemap::make_ident(source_name);
            quote! { pub use #rust_ident as #public_ident; }
        } else {
            quote! {}
        };
        quote! {
            #extern_attr
            extern "C" {
                #[wasm_bindgen(extends = Object)]
                #[derive(Debug, Clone, PartialEq, Eq)]
                pub type #rust_ident #type_generics;
                #(#getters)*
                #(#setters)*
            }
            #public_alias
            #construction
        }
    }

    fn empty_construction(&self) -> TokenStream {
        let rust_ident = &self.rust_ident;
        let type_args = &self.type_args;
        let type_generics = &self.type_generics;
        quote! {
            impl #type_generics Default for #rust_ident #type_args {
                fn default() -> Self {
                    JsCast::unchecked_into(js_sys::Object::new())
                }
            }

            impl #type_generics #rust_ident #type_args {
                pub fn new() -> Self {
                    Self::default()
                }
            }
        }
    }
}

/// Emit a nominal wasm-bindgen wrapper for a named `Record<K, V>` alias.
pub(crate) fn generate_record(
    decl: &RecordDecl,
    module_context: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
) -> TokenStream {
    let wrapper = RecordWrapper::new(decl, module_context, cgctx);
    if let Some(literal_keys) = string_literal_keys(&decl.key_type, cgctx, decl.body_scope, 0) {
        return generate_string_literal_record(decl, module_context, cgctx, &wrapper, literal_keys);
    }

    generate_indexed_record(decl, module_context, cgctx, &wrapper)
}

fn generate_indexed_record(
    decl: &RecordDecl,
    module_context: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    wrapper: &RecordWrapper,
) -> TokenStream {
    let finite_keys = finite_literal_keys(&decl.key_type, cgctx, decl.body_scope, 0);
    let key_is_open_union =
        is_union_type(&decl.key_type, cgctx, decl.body_scope, 0) && finite_keys.is_none();
    let source_params = [
        Param {
            name: "key".to_string(),
            type_ref: if key_is_open_union {
                TypeRef::Any
            } else {
                decl.key_type.clone()
            },
            optional: false,
            variadic: false,
        },
        Param {
            name: "value".to_string(),
            type_ref: decl.value_type.clone(),
            optional: false,
            variadic: false,
        },
    ];
    let alternatives = expand_signatures(&[&source_params], cgctx, decl.body_scope);
    let mut distinct_value_types = Vec::new();
    for alternative in &alternatives {
        if let Some(value) = alternative.params.get(1) {
            if !distinct_value_types.contains(&value.type_ref) {
                distinct_value_types.push(value.type_ref.clone());
            }
        }
    }
    let value_alternative_count = distinct_value_types.len();
    let key_is_union =
        !key_is_open_union && is_union_type(&decl.key_type, cgctx, decl.body_scope, 0);
    let value_is_union = is_union_type(&decl.value_type, cgctx, decl.body_scope, 0);
    let mut used_names = HashSet::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();
    let mut setter_bindings = Vec::new();

    for alternative in alternatives {
        let (Some(key), Some(value)) = (alternative.params.first(), alternative.params.get(1))
        else {
            continue;
        };
        let key_name = record_type_name(&key.type_ref);
        let value_name =
            record_value_name(&value.type_ref, value_is_union, value_alternative_count);
        let getter_base =
            record_method_name("get", &key_name, &value_name, key_is_union, value_is_union);
        let getter_name = super::signatures::dedupe_name(&getter_base, &mut used_names);
        let try_getter_name =
            super::signatures::dedupe_name(&format!("try_{getter_name}"), &mut used_names);
        let getter_ident = super::typemap::make_ident(&getter_name);
        let try_getter_ident = super::typemap::make_ident(&try_getter_name);
        let getter_params = vec![key.clone()];
        let (key_bounds, _, rendered_key) =
            generate_dictionary_params(&getter_params, cgctx, decl.body_scope, module_context);
        let getter_bounds = wrapper
            .type_bounds
            .iter()
            .cloned()
            .chain(key_bounds)
            .collect::<Vec<_>>();
        let getter_generics = render_generic_bounds(&getter_bounds);
        let return_type = super::typemap::to_syn_type(
            &value.type_ref,
            TypePosition::RETURN,
            cgctx,
            decl.body_scope,
            module_context,
        );
        let rust_ident = &wrapper.rust_ident;
        let type_args = &wrapper.type_args;
        getters.push(quote! {
            #[wasm_bindgen(method, indexing_getter)]
            pub fn #getter_ident #getter_generics(
                this: &#rust_ident #type_args,
                #rendered_key,
            ) -> #return_type;
            #[wasm_bindgen(catch, method, indexing_getter)]
            pub fn #try_getter_ident #getter_generics(
                this: &#rust_ident #type_args,
                #rendered_key,
            ) -> Result<#return_type, JsValue>;
        });

        let setter_base =
            record_method_name("set", &key_name, &value_name, key_is_union, value_is_union);
        let setter_name = super::signatures::dedupe_name(&setter_base, &mut used_names);
        let setter_ident = super::typemap::make_ident(&setter_name);
        let params = vec![key.clone(), value.clone()];
        let (value_bounds, _, rendered_params) =
            generate_dictionary_params(&params, cgctx, decl.body_scope, module_context);
        let method_bounds = wrapper
            .type_bounds
            .iter()
            .cloned()
            .chain(value_bounds)
            .collect::<Vec<_>>();
        let method_generics = render_generic_bounds(&method_bounds);
        let slice_to_array = if super::typemap::needs_slice_to_array(&params) {
            quote! { , slice_to_array }
        } else {
            quote! {}
        };
        setters.push(quote! {
            #[wasm_bindgen(method, indexing_setter #slice_to_array)]
            pub fn #setter_ident #method_generics(
                this: &#rust_ident #type_args,
                #rendered_params,
            );
        });
        setter_bindings.push(SetterBinding {
            key_type: key.type_ref.clone(),
            value_type: value.type_ref.clone(),
            method_ident: setter_ident,
        });
    }

    let construction = match finite_keys {
        Some(keys) => generate_finite_constructors(
            decl,
            module_context,
            cgctx,
            wrapper,
            &keys,
            &setter_bindings,
        ),
        None => wrapper.empty_construction(),
    };
    wrapper.render(&decl.name, &getters, &setters, &construction)
}

fn generate_string_literal_record(
    decl: &RecordDecl,
    module_context: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    wrapper: &RecordWrapper,
    literal_keys: Vec<String>,
) -> TokenStream {
    let source_value = Param {
        name: "value".to_string(),
        type_ref: decl.value_type.clone(),
        optional: false,
        variadic: false,
    };
    let alternatives = expand_signatures(&[&[source_value]], cgctx, decl.body_scope);
    let value_alternative_count = alternatives.len();
    let value_is_union = is_union_type(&decl.value_type, cgctx, decl.body_scope, 0);
    let mut used_names = HashSet::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();
    let mut setter_bindings = Vec::new();

    for literal in &literal_keys {
        let literal_name = string_literal_method_name(literal);
        for alternative in &alternatives {
            let Some(value) = alternative.params.first() else {
                continue;
            };
            let value_name =
                record_value_name(&value.type_ref, value_is_union, value_alternative_count);
            let getter_base = if value_is_union {
                format!("get_{value_name}_with_{literal_name}")
            } else {
                format!("get_{literal_name}")
            };
            let getter_name = super::signatures::dedupe_name(&getter_base, &mut used_names);
            let try_getter_name =
                super::signatures::dedupe_name(&format!("try_{getter_name}"), &mut used_names);
            let getter_ident = super::typemap::make_ident(&getter_name);
            let try_getter_ident = super::typemap::make_ident(&try_getter_name);
            let return_type = super::typemap::to_syn_type(
                &value.type_ref,
                TypePosition::RETURN,
                cgctx,
                decl.body_scope,
                module_context,
            );
            let rust_ident = &wrapper.rust_ident;
            let type_args = &wrapper.type_args;
            let type_generics = &wrapper.type_generics;
            getters.push(quote! {
                #[wasm_bindgen(method, getter, js_name = #literal)]
                pub fn #getter_ident #type_generics(
                    this: &#rust_ident #type_args,
                ) -> #return_type;
                #[wasm_bindgen(catch, method, getter, js_name = #literal)]
                pub fn #try_getter_ident #type_generics(
                    this: &#rust_ident #type_args,
                ) -> Result<#return_type, JsValue>;
            });

            let setter_base = if value_is_union {
                format!("set_{value_name}_with_{literal_name}")
            } else {
                format!("set_{literal_name}")
            };
            let setter_name = super::signatures::dedupe_name(&setter_base, &mut used_names);
            let setter_ident = super::typemap::make_ident(&setter_name);
            let params = vec![value.clone()];
            let (value_bounds, _, rendered_params) =
                generate_dictionary_params(&params, cgctx, decl.body_scope, module_context);
            let method_bounds = wrapper
                .type_bounds
                .iter()
                .cloned()
                .chain(value_bounds)
                .collect::<Vec<_>>();
            let method_generics = render_generic_bounds(&method_bounds);
            let slice_to_array = if super::typemap::needs_slice_to_array(&params) {
                quote! { , slice_to_array }
            } else {
                quote! {}
            };
            setters.push(quote! {
                #[wasm_bindgen(method, setter, js_name = #literal #slice_to_array)]
                pub fn #setter_ident #method_generics(
                    this: &#rust_ident #type_args,
                    #rendered_params,
                );
            });
            setter_bindings.push(SetterBinding {
                key_type: TypeRef::StringLiteral(literal.clone()),
                value_type: value.type_ref.clone(),
                method_ident: setter_ident,
            });
        }
    }

    let keys = literal_keys
        .into_iter()
        .map(FiniteKey::String)
        .collect::<Vec<_>>();
    let construction = generate_finite_constructors(
        decl,
        module_context,
        cgctx,
        wrapper,
        &keys,
        &setter_bindings,
    );
    wrapper.render(&decl.name, &getters, &setters, &construction)
}

fn generate_finite_constructors(
    decl: &RecordDecl,
    module_context: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    wrapper: &RecordWrapper,
    keys: &[FiniteKey],
    setter_bindings: &[SetterBinding],
) -> TokenStream {
    let value_probe = [Param {
        name: "value".to_string(),
        type_ref: decl.value_type.clone(),
        optional: false,
        variadic: false,
    }];
    let value_alternatives = expand_signatures(&[&value_probe], cgctx, decl.body_scope);
    let constructor_value_type = if value_alternatives.len() > 1 {
        TypeRef::Any
    } else {
        value_alternatives
            .first()
            .and_then(|alternative| alternative.params.first())
            .map(|param| param.type_ref.clone())
            .unwrap_or_else(|| decl.value_type.clone())
    };
    let mut used_param_names = HashSet::new();
    let source_params = keys
        .iter()
        .map(|key| Param {
            name: super::signatures::dedupe_name(&key.parameter_name(), &mut used_param_names),
            type_ref: constructor_value_type.clone(),
            optional: false,
            variadic: false,
        })
        .collect::<Vec<_>>();
    let alternatives = expand_signatures(&[&source_params], cgctx, decl.body_scope);
    let mut used_constructor_names = HashSet::new();
    let mut constructors = Vec::new();

    for alternative in alternatives {
        let constructor_name = super::signatures::dedupe_name(
            &format!("new{}", alternative.name_suffix),
            &mut used_constructor_names,
        );
        let constructor_ident = super::typemap::make_ident(&constructor_name);
        let (bounds, helper_where_clause, rendered_params) =
            generate_dictionary_params(&alternative.params, cgctx, decl.body_scope, module_context);
        let generics = render_generic_bounds(&bounds);
        let mut initialization = Vec::new();

        for ((key, param), source_param) in keys.iter().zip(&alternative.params).zip(&source_params)
        {
            let param_ident = super::typemap::make_ident(&source_param.name);
            let binding = setter_bindings.iter().find(|binding| {
                key.matches_binding_type(&binding.key_type) && binding.value_type == param.type_ref
            });
            if let Some(binding) = binding {
                let setter_ident = &binding.method_ident;
                let key_arg = key.setter_argument();
                if key_arg.is_empty() {
                    initialization.push(quote! { inner.#setter_ident(#param_ident); });
                } else {
                    initialization.push(quote! { inner.#setter_ident(#key_arg, #param_ident); });
                }
            } else {
                let key_value = key.js_value();
                initialization.push(quote! {
                    js_sys::Reflect::set(inner.as_ref(), &#key_value, #param_ident.as_ref())
                        .expect("setting a property on a fresh object should not fail");
                });
            }
        }

        constructors.push(quote! {
            /// Creates a record with every required key initialized.
            pub fn #constructor_ident #generics(
                #rendered_params,
            ) -> Self
            #helper_where_clause
            {
                let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
                #(#initialization)*
                inner
            }
        });
    }

    let rust_ident = &wrapper.rust_ident;
    let type_args = &wrapper.type_args;
    let constructor_impl_generics = &wrapper.constructor_impl_generics;
    quote! {
        impl #constructor_impl_generics #rust_ident #type_args {
            #(#constructors)*
        }
    }
}

impl FiniteKey {
    fn parameter_name(&self) -> String {
        match self {
            Self::String(value) => string_literal_method_name(value),
            Self::Number(value) => format!("number_{}", normalized_number_name(*value)),
            Self::Boolean(value) => format!("bool_{value}"),
        }
    }

    fn matches_binding_type(&self, ty: &TypeRef) -> bool {
        match (self, ty) {
            (Self::String(key), TypeRef::StringLiteral(binding)) => key == binding,
            (Self::Number(_), TypeRef::NumberLiteral(_))
            | (Self::Boolean(_), TypeRef::BooleanLiteral(_)) => true,
            _ => false,
        }
    }

    fn setter_argument(&self) -> TokenStream {
        match self {
            // String-literal records use fixed-property setters, which do not
            // take the key as a runtime argument.
            Self::String(_) => quote! {},
            Self::Number(value) => quote! { #value },
            Self::Boolean(value) => quote! { #value },
        }
    }

    fn js_value(&self) -> TokenStream {
        match self {
            Self::String(value) => quote! { JsValue::from_str(#value) },
            Self::Number(value) => quote! { JsValue::from_f64(#value) },
            Self::Boolean(value) => quote! { JsValue::from_bool(#value) },
        }
    }
}

fn string_literal_keys(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    depth: usize,
) -> Option<Vec<String>> {
    finite_literal_keys(ty, cgctx, scope, depth)?
        .into_iter()
        .map(|key| match key {
            FiniteKey::String(value) => Some(value),
            FiniteKey::Number(_) | FiniteKey::Boolean(_) => None,
        })
        .collect()
}

fn finite_literal_keys(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    depth: usize,
) -> Option<Vec<FiniteKey>> {
    if depth > 16 {
        return None;
    }
    match ty {
        TypeRef::StringLiteral(value) => Some(vec![FiniteKey::String(value.clone())]),
        TypeRef::NumberLiteral(value) => Some(vec![FiniteKey::Number(*value)]),
        TypeRef::BooleanLiteral(value) => Some(vec![FiniteKey::Boolean(*value)]),
        TypeRef::Union(members) if !members.is_empty() => members
            .iter()
            .map(|member| finite_literal_keys(member, cgctx, scope, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(|groups| groups.into_iter().flatten().collect()),
        _ => {
            let name = ty.as_ident()?;
            let ctx = cgctx?;
            if let Some(values) = ctx.resolve_string_literal_set(name, scope) {
                return Some(values.into_iter().map(FiniteKey::String).collect());
            }
            ctx.resolve_alias(name, scope)
                .and_then(|target| finite_literal_keys(target, cgctx, scope, depth + 1))
        }
    }
}

fn is_union_type(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    match ty {
        TypeRef::Union(_) => true,
        _ => ty
            .as_ident()
            .and_then(|name| cgctx?.resolve_alias(name, scope))
            .is_some_and(|target| is_union_type(target, cgctx, scope, depth + 1)),
    }
}

fn string_literal_method_name(literal: &str) -> String {
    let name = normalized_literal_name(literal);
    if matches!(
        name.as_str(),
        "string"
            | "number"
            | "bool"
            | "big_int"
            | "undefined"
            | "null"
            | "js_value"
            | "object"
            | "typed_array"
            | "slice"
            | "function"
    ) {
        format!("string_{name}")
    } else {
        name
    }
}

fn normalized_literal_name(literal: &str) -> String {
    let name = to_snake_case(literal);
    if name.is_empty() {
        "empty".to_string()
    } else {
        name
    }
}

fn normalized_number_name(value: f64) -> String {
    value
        .to_string()
        .replace('-', "negative_")
        .replace('.', "_")
}

fn record_method_name(
    operation: &str,
    key: &str,
    value: &str,
    key_is_union: bool,
    value_is_union: bool,
) -> String {
    match (key_is_union, value_is_union) {
        (false, false) => operation.to_string(),
        (true, false) => format!("{operation}_{key}"),
        (false, true) => format!("{operation}_{value}"),
        (true, true) => format!("{operation}_{value}_with_{key}"),
    }
}

fn record_value_name(ty: &TypeRef, source_is_union: bool, alternative_count: usize) -> String {
    if source_is_union && alternative_count == 1 {
        match ty {
            TypeRef::StringLiteral(_) => return "string".to_string(),
            TypeRef::NumberLiteral(_) => return "number".to_string(),
            TypeRef::BooleanLiteral(_) => return "bool".to_string(),
            _ => {}
        }
    }
    record_type_name(ty)
}

fn record_type_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::StringLiteral(value) => format!("string_{}", normalized_literal_name(value)),
        TypeRef::NumberLiteral(_) => "number".to_string(),
        TypeRef::BooleanLiteral(value) => format!("bool_{value}"),
        TypeRef::Array(inner) => format!("slice_of_{}", record_type_name(inner)),
        TypeRef::Nullable(inner) => format!("optional_{}", record_type_name(inner)),
        TypeRef::Reference {
            segments,
            generic_args,
        } => {
            let head = segments
                .last()
                .map(|name| to_snake_case(name))
                .unwrap_or_else(|| "js_value".to_string());
            if generic_args.is_empty() {
                head
            } else {
                format!(
                    "{head}_of_{}",
                    generic_args
                        .iter()
                        .map(record_type_name)
                        .collect::<Vec<_>>()
                        .join("_and_")
                )
            }
        }
        TypeRef::Tuple(members) => format!(
            "tuple_of_{}",
            members
                .iter()
                .map(record_type_name)
                .collect::<Vec<_>>()
                .join("_and_")
        ),
        other => match type_snake_name(other).as_str() {
            "str" => "string".to_string(),
            "f64" => "number".to_string(),
            name => name.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::TypeParam;

    fn record(key_type: TypeRef, value_type: TypeRef) -> RecordDecl {
        RecordDecl {
            name: "Values".to_string(),
            type_params: Vec::<TypeParam>::new(),
            key_type,
            value_type,
            body_scope: ScopeId::DUMMY,
        }
    }

    #[test]
    fn primitive_value_union_generates_value_named_accessors() {
        let decl = record(
            TypeRef::String,
            TypeRef::Union(vec![TypeRef::String, TypeRef::Number, TypeRef::Boolean]),
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_string"));
        assert!(tokens.contains("fn try_get_string"));
        assert!(tokens.contains("fn set_number"));
        assert!(!tokens.contains("catch , method , indexing_setter"));
    }

    #[test]
    fn open_key_union_erases_the_key_without_a_name_suffix() {
        let decl = record(
            TypeRef::Union(vec![TypeRef::String, TypeRef::Number]),
            TypeRef::Union(vec![TypeRef::String, TypeRef::Boolean]),
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_string"));
        assert!(tokens.contains("fn get_bool"));
        assert!(tokens.contains("key : & JsValue"));
        assert!(!tokens.contains("with_js_value"));
        assert!(!tokens.contains("with_string"));
    }

    #[test]
    fn string_literal_keys_generate_fixed_property_accessors() {
        let decl = record(
            TypeRef::Union(vec![
                TypeRef::StringLiteral("displayName".to_string()),
                TypeRef::StringLiteral("enabled".to_string()),
            ]),
            TypeRef::Union(vec![TypeRef::String, TypeRef::Boolean]),
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_string_with_display_name"));
        assert!(tokens.contains("fn try_get_bool_with_enabled"));
        assert!(tokens.contains("fn set_bool_with_display_name"));
        assert!(tokens.contains("getter , js_name = \"displayName\""));
        assert!(!tokens.contains("indexing_getter"));
        assert!(tokens.contains("fn new"));
        assert!(tokens.contains("display_name"));
        assert!(tokens.contains("enabled"));
        assert!(tokens.contains("display_name : & JsValue"));
        assert!(tokens.contains("enabled : & JsValue"));
        assert!(!tokens.contains("new_with_"));
    }

    #[test]
    fn homogeneous_value_union_keeps_one_precise_constructor() {
        let decl = record(
            TypeRef::Union(vec![
                TypeRef::StringLiteral("primary".to_string()),
                TypeRef::StringLiteral("secondary".to_string()),
            ]),
            TypeRef::Union(vec![
                TypeRef::StringLiteral("red".to_string()),
                TypeRef::StringLiteral("blue".to_string()),
            ]),
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("primary : & str"));
        assert!(tokens.contains("secondary : & str"));
        assert!(tokens.contains("get_string_with_primary"));
        assert!(!tokens.contains("get_string_red"));
        assert!(!tokens.contains("primary : & JsValue"));
        assert!(!tokens.contains("new_with_"));
    }

    #[test]
    fn singleton_string_literal_key_uses_fixed_property_accessors() {
        let decl = record(TypeRef::StringLiteral("name".to_string()), TypeRef::String);
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_name"));
        assert!(tokens.contains("fn set_name"));
        assert!(!tokens.contains("key :"));
        assert!(tokens.contains("fn new"));
        assert!(tokens.contains("name : & str"));
    }

    #[test]
    fn empty_constructible_records_implement_default() {
        let decl = record(TypeRef::String, TypeRef::String);
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("impl Default for Values"));
        assert!(tokens.contains("fn new"));
    }

    #[test]
    fn finite_numeric_keys_require_every_value_during_construction() {
        let decl = record(
            TypeRef::Union(vec![
                TypeRef::NumberLiteral(1.0),
                TypeRef::NumberLiteral(2.0),
            ]),
            TypeRef::String,
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn new"));
        assert!(tokens.contains("number_1 : & str"));
        assert!(tokens.contains("number_2 : & str"));
        assert!(!tokens.contains("impl Default"));
    }

    #[test]
    fn nested_value_flavors_do_not_fall_back_to_numeric_suffixes() {
        let decl = record(
            TypeRef::String,
            TypeRef::Union(vec![
                TypeRef::Array(Box::new(TypeRef::String)),
                TypeRef::Array(Box::new(TypeRef::Number)),
            ]),
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_slice_of_string"));
        assert!(tokens.contains("fn get_slice_of_number"));
        assert!(!tokens.contains("get_slice_2"));
    }
}
