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

        Self {
            extern_attr: CodegenContext::extern_attr(cgctx, module),
            rust_name,
            rust_ident,
            type_args,
            type_bounds,
            type_generics,
        }
    }

    fn render(
        &self,
        source_name: &str,
        getters: &[TokenStream],
        setters: &[TokenStream],
        supports_empty_construction: bool,
    ) -> TokenStream {
        let Self {
            extern_attr,
            rust_name,
            rust_ident,
            type_args,
            type_generics,
            ..
        } = self;
        let public_alias = if rust_name != source_name {
            let public_ident = super::typemap::make_ident(source_name);
            quote! { pub use #rust_ident as #public_ident; }
        } else {
            quote! {}
        };
        let construction = if supports_empty_construction {
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
    let alternatives = expand_signatures(&[&source_params], cgctx, decl.body_scope);
    let key_is_union = is_union_type(&decl.key_type, cgctx, decl.body_scope, 0);
    let value_is_union = is_union_type(&decl.value_type, cgctx, decl.body_scope, 0);
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
    }

    let supports_empty_construction =
        !is_finite_literal_key_type(&decl.key_type, cgctx, decl.body_scope, 0);
    wrapper.render(&decl.name, &getters, &setters, supports_empty_construction)
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
    let value_is_union = is_union_type(&decl.value_type, cgctx, decl.body_scope, 0);
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
        }
    }

    // A finite-key Record requires every key to exist. Constructing it from an
    // empty object would create a value that does not satisfy its TS type.
    wrapper.render(&decl.name, &getters, &setters, false)
}

fn string_literal_keys(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    depth: usize,
) -> Option<Vec<String>> {
    if depth > 16 {
        return None;
    }
    match ty {
        TypeRef::StringLiteral(value) => Some(vec![value.clone()]),
        TypeRef::Union(members) => members
            .iter()
            .map(|member| string_literal_keys(member, cgctx, scope, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(|groups| groups.into_iter().flatten().collect()),
        _ => {
            let name = ty.as_ident()?;
            let ctx = cgctx?;
            ctx.resolve_string_literal_set(name, scope).or_else(|| {
                ctx.resolve_alias(name, scope)
                    .and_then(|target| string_literal_keys(target, cgctx, scope, depth + 1))
            })
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

fn is_finite_literal_key_type(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    match ty {
        TypeRef::StringLiteral(_) | TypeRef::NumberLiteral(_) | TypeRef::BooleanLiteral(_) => true,
        TypeRef::Union(members) => {
            !members.is_empty()
                && members
                    .iter()
                    .all(|member| is_finite_literal_key_type(member, cgctx, scope, depth + 1))
        }
        _ => {
            let Some(name) = ty.as_ident() else {
                return false;
            };
            let Some(ctx) = cgctx else {
                return false;
            };
            ctx.resolve_string_literal_set(name, scope).is_some()
                || ctx.resolve_alias(name, scope).is_some_and(|target| {
                    is_finite_literal_key_type(target, cgctx, scope, depth + 1)
                })
        }
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
    fn key_and_value_unions_name_value_before_key() {
        let decl = record(
            TypeRef::Union(vec![TypeRef::String, TypeRef::Number]),
            TypeRef::Union(vec![TypeRef::String, TypeRef::Boolean]),
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_string_with_string"));
        assert!(tokens.contains("fn get_bool_with_number"));
        assert!(tokens.contains("fn set_string_with_number"));
        assert!(!tokens.contains("get_number_as_bool"));
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
        assert!(!tokens.contains("fn new"));
    }

    #[test]
    fn singleton_string_literal_key_uses_fixed_property_accessors() {
        let decl = record(TypeRef::StringLiteral("name".to_string()), TypeRef::String);
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("fn get_name"));
        assert!(tokens.contains("fn set_name"));
        assert!(!tokens.contains("key :"));
    }

    #[test]
    fn empty_constructible_records_implement_default() {
        let decl = record(TypeRef::String, TypeRef::String);
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("impl Default for Values"));
        assert!(tokens.contains("fn new"));
    }

    #[test]
    fn finite_numeric_keys_do_not_allow_empty_construction() {
        let decl = record(
            TypeRef::Union(vec![
                TypeRef::NumberLiteral(1.0),
                TypeRef::NumberLiteral(2.0),
            ]),
            TypeRef::String,
        );
        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(!tokens.contains("fn new"));
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
