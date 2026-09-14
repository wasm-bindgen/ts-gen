//! Named `Record<K, V>` alias generation.

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::signatures::{
    expand_signatures, generate_dictionary_params, render_generic_bounds,
};
use crate::codegen::typemap::{CodegenContext, TypePosition};
use crate::ir::{ModuleContext, Param, RecordDecl, TypeRef};
use crate::util::naming::to_snake_case;

/// Emit a nominal wasm-bindgen wrapper for a named `Record<K, V>` alias.
///
/// Each concrete key/value pair becomes an indexing getter and setter. These
/// operations model `this[key]` and `this[key] = value`, so the alias remains a
/// plain JavaScript object rather than requiring a runtime constructor with
/// the alias's name.
pub(crate) fn generate_record(
    decl: &RecordDecl,
    module_context: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
) -> TokenStream {
    let module = match module_context {
        ModuleContext::Global => None,
        ModuleContext::Module(module) => Some(module.as_ref()),
    };
    let extern_attr = CodegenContext::extern_attr(cgctx, module);
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

    if let Some(literal_keys) = string_literal_key_union(&decl.key_type) {
        return generate_string_literal_record(
            decl,
            module_context,
            cgctx,
            literal_keys,
            &extern_attr,
            &rust_name,
            &rust_ident,
            &type_args,
            &type_bounds,
            &type_generics,
        );
    }

    let source_params = [
        Param {
            name: "key".to_string(),
            type_ref: decl.key_type.clone(),
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
    let overloads = [&source_params[..]];
    let alternatives = expand_signatures(&overloads, cgctx, decl.body_scope);
    let key_names = alternatives
        .iter()
        .filter_map(|alternative| alternative.params.first())
        .map(|key| record_type_name(&key.type_ref))
        .collect::<HashSet<_>>();
    let value_names = alternatives
        .iter()
        .filter_map(|alternative| alternative.params.get(1))
        .map(|value| record_type_name(&value.type_ref))
        .collect::<HashSet<_>>();
    let multiple_keys = matches!(decl.key_type, TypeRef::Union(_)) && key_names.len() > 1;
    let multiple_values = value_names.len() > 1;
    let mut used_names = HashSet::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();

    for alternative in alternatives {
        let (Some(key), Some(value)) = (alternative.params.first(), alternative.params.get(1))
        else {
            continue;
        };
        let key_name = record_type_name(&key.type_ref);
        let value_name = record_type_name(&value.type_ref);

        let getter_base = record_method_name(
            "get",
            &key_name,
            &value_name,
            multiple_keys,
            multiple_values,
            "as",
        );
        let getter_name = super::signatures::dedupe_name(&getter_base, &mut used_names);
        let getter_ident = super::typemap::make_ident(&getter_name);
        let getter_params = vec![key.clone()];
        let (key_bounds, _, rendered_key) =
            generate_dictionary_params(&getter_params, cgctx, decl.body_scope, module_context);
        let getter_bounds = type_bounds
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
        getters.push(quote! {
            #[wasm_bindgen(catch, method, indexing_getter)]
            pub fn #getter_ident #getter_generics(
                this: &#rust_ident #type_args,
                #rendered_key,
            ) -> Result<#return_type, JsValue>;
        });

        let setter_base = record_method_name(
            "set",
            &key_name,
            &value_name,
            multiple_keys,
            multiple_values,
            "with",
        );
        let setter_name = super::signatures::dedupe_name(&setter_base, &mut used_names);
        let setter_ident = super::typemap::make_ident(&setter_name);
        let params = vec![key.clone(), value.clone()];
        let (value_bounds, _, rendered_params) =
            generate_dictionary_params(&params, cgctx, decl.body_scope, module_context);
        let method_bounds = type_bounds
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
            #[wasm_bindgen(catch, method, indexing_setter #slice_to_array)]
            pub fn #setter_ident #method_generics(
                this: &#rust_ident #type_args,
                #rendered_params,
            ) -> Result<(), JsValue>;
        });
    }

    let public_alias = if rust_name != decl.name {
        let public_ident = super::typemap::make_ident(&decl.name);
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
        impl #type_generics #rust_ident #type_args {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                JsCast::unchecked_into(js_sys::Object::new())
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_string_literal_record(
    decl: &RecordDecl,
    module_context: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    literal_keys: Vec<String>,
    extern_attr: &TokenStream,
    rust_name: &str,
    rust_ident: &syn::Ident,
    type_args: &TokenStream,
    type_bounds: &[TokenStream],
    type_generics: &TokenStream,
) -> TokenStream {
    let source_value = Param {
        name: "value".to_string(),
        type_ref: decl.value_type.clone(),
        optional: false,
        variadic: false,
    };
    let overloads = [&[source_value][..]];
    let alternatives = expand_signatures(&overloads, cgctx, decl.body_scope);
    let multiple_values = alternatives.len() > 1;
    let mut used_names = HashSet::new();
    let mut getters = Vec::new();
    let mut setters = Vec::new();

    for literal in literal_keys {
        let literal_name = string_literal_method_name(&literal);
        for alternative in &alternatives {
            let Some(value) = alternative.params.first() else {
                continue;
            };
            let value_name = record_type_name(&value.type_ref);
            let getter_base = if multiple_values {
                format!("get_{literal_name}_as_{value_name}")
            } else {
                format!("get_{literal_name}")
            };
            let getter_name = super::signatures::dedupe_name(&getter_base, &mut used_names);
            let getter_ident = super::typemap::make_ident(&getter_name);
            let return_type = super::typemap::to_syn_type(
                &value.type_ref,
                TypePosition::RETURN,
                cgctx,
                decl.body_scope,
                module_context,
            );
            getters.push(quote! {
                #[wasm_bindgen(catch, method, getter, js_name = #literal)]
                pub fn #getter_ident #type_generics(
                    this: &#rust_ident #type_args,
                ) -> Result<#return_type, JsValue>;
            });

            let setter_base = if multiple_values {
                format!("set_{literal_name}_with_{value_name}")
            } else {
                format!("set_{literal_name}")
            };
            let setter_name = super::signatures::dedupe_name(&setter_base, &mut used_names);
            let setter_ident = super::typemap::make_ident(&setter_name);
            let params = vec![value.clone()];
            let (value_bounds, _, rendered_params) =
                generate_dictionary_params(&params, cgctx, decl.body_scope, module_context);
            let method_bounds = type_bounds
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
                #[wasm_bindgen(catch, method, setter, js_name = #literal #slice_to_array)]
                pub fn #setter_ident #method_generics(
                    this: &#rust_ident #type_args,
                    #rendered_params,
                ) -> Result<(), JsValue>;
            });
        }
    }

    let public_alias = if rust_name != decl.name {
        let public_ident = super::typemap::make_ident(&decl.name);
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
        impl #type_generics #rust_ident #type_args {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                JsCast::unchecked_into(js_sys::Object::new())
            }
        }
    }
}

fn string_literal_key_union(ty: &TypeRef) -> Option<Vec<String>> {
    let TypeRef::Union(members) = ty else {
        return None;
    };
    members
        .iter()
        .map(|member| match member {
            TypeRef::StringLiteral(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn string_literal_method_name(literal: &str) -> String {
    let name = to_snake_case(literal);
    if name.is_empty() {
        "empty".to_string()
    } else {
        name
    }
}

fn record_method_name(
    operation: &str,
    key: &str,
    value: &str,
    multiple_keys: bool,
    multiple_values: bool,
    separator: &str,
) -> String {
    match (multiple_keys, multiple_values) {
        (false, false) => operation.to_string(),
        (true, false) => format!("{operation}_{key}"),
        (false, true) => format!("{operation}_{value}"),
        (true, true) => format!("{operation}_{key}_{separator}_{value}"),
    }
}

fn record_type_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::String | TypeRef::StringLiteral(_) => "string".to_string(),
        TypeRef::Number | TypeRef::NumberLiteral(_) => "number".to_string(),
        TypeRef::Boolean | TypeRef::BooleanLiteral(_) => "bool".to_string(),
        TypeRef::BigInt => "big_int".to_string(),
        TypeRef::Void | TypeRef::Undefined => "undefined".to_string(),
        TypeRef::Null => "null".to_string(),
        TypeRef::Any | TypeRef::Unknown | TypeRef::Symbol | TypeRef::Unresolved(_) => {
            "js_value".to_string()
        }
        TypeRef::Object => "object".to_string(),
        TypeRef::ArrayBufferView => "typed_array".to_string(),
        TypeRef::Array(_) => "slice".to_string(),
        TypeRef::Reference { segments, .. } => segments
            .first()
            .map(|name| to_snake_case(name))
            .unwrap_or_else(|| "js_value".to_string()),
        TypeRef::Nullable(inner) => record_type_name(inner),
        TypeRef::Function(_) => "function".to_string(),
        TypeRef::Tuple(_) | TypeRef::Union(_) | TypeRef::Intersection(_) => "js_value".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::TypeParam;
    use crate::parse::scope::ScopeId;

    #[test]
    fn primitive_union_generates_named_indexing_accessors() {
        let decl = RecordDecl {
            name: "EvaluationContext".to_string(),
            type_params: Vec::<TypeParam>::new(),
            key_type: TypeRef::String,
            value_type: TypeRef::Union(vec![TypeRef::String, TypeRef::Number, TypeRef::Boolean]),
            body_scope: ScopeId::DUMMY,
        };

        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("pub type EvaluationContext"));
        assert!(tokens.contains("indexing_getter"));
        assert!(tokens.contains("indexing_setter"));
        assert!(tokens.contains("fn get_string"));
        assert!(tokens.contains("fn get_number"));
        assert!(tokens.contains("fn get_bool"));
        assert!(tokens.contains("fn set_string"));
        assert!(tokens.contains("fn set_number"));
        assert!(tokens.contains("fn set_bool"));
        assert!(tokens.contains("impl EvaluationContext"));
    }

    #[test]
    fn key_and_value_unions_generate_a_cartesian_api() {
        let decl = RecordDecl {
            name: "FlexibleRecord".to_string(),
            type_params: Vec::<TypeParam>::new(),
            key_type: TypeRef::Union(vec![TypeRef::String, TypeRef::Number]),
            value_type: TypeRef::Union(vec![TypeRef::String, TypeRef::Boolean]),
            body_scope: ScopeId::DUMMY,
        };

        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_string_as_string"));
        assert!(tokens.contains("fn get_string_as_bool"));
        assert!(tokens.contains("fn get_number_as_string"));
        assert!(tokens.contains("fn get_number_as_bool"));
        assert!(tokens.contains("fn set_string_with_string"));
        assert!(tokens.contains("fn set_number_with_bool"));
    }

    #[test]
    fn string_literal_key_union_generates_fixed_property_accessors() {
        let decl = RecordDecl {
            name: "KnownFields".to_string(),
            type_params: Vec::<TypeParam>::new(),
            key_type: TypeRef::Union(vec![
                TypeRef::StringLiteral("displayName".to_string()),
                TypeRef::StringLiteral("enabled".to_string()),
            ]),
            value_type: TypeRef::Union(vec![TypeRef::String, TypeRef::Boolean]),
            body_scope: ScopeId::DUMMY,
        };

        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_display_name_as_string"));
        assert!(tokens.contains("fn get_enabled_as_bool"));
        assert!(tokens.contains("fn set_display_name_with_bool"));
        assert!(tokens.contains("fn set_enabled_with_string"));
        assert!(tokens.contains("getter , js_name = \"displayName\""));
        assert!(!tokens.contains("indexing_getter"));
    }
}
