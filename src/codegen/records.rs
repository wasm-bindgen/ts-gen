//! Named `Record<K, V>` alias generation.

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::signatures::{
    expand_signatures, generate_dictionary_params, render_generic_bounds, ConcreteParam,
};
use crate::codegen::typemap::CodegenContext;
use crate::ir::{ModuleContext, Param, RecordDecl, TypeRef};
use crate::util::naming::to_snake_case;

/// Emit a nominal wasm-bindgen wrapper for a named `Record<K, V>` alias.
///
/// Each concrete value alternative becomes an indexing setter. The indexing
/// operation is the wasm-bindgen representation of `this[key] = value`, so the
/// alias remains a plain JavaScript object rather than requiring a runtime
/// constructor with the alias's name.
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

    let source_param = Param {
        name: "value".to_string(),
        type_ref: decl.value_type.clone(),
        optional: false,
        variadic: false,
    };
    let overloads = [&[source_param][..]];
    let alternatives = expand_signatures(&overloads, cgctx, decl.body_scope);
    let multiple = alternatives.len() > 1;
    let mut used_names = HashSet::new();
    let mut setters = Vec::new();

    for alternative in alternatives {
        let Some(value) = alternative.params.first() else {
            continue;
        };
        let base_name = if multiple {
            format!("set_{}", record_value_name(&value.type_ref))
        } else {
            "set".to_string()
        };
        let method_name = super::signatures::dedupe_name(&base_name, &mut used_names);
        let method_ident = super::typemap::make_ident(&method_name);
        let params = vec![
            ConcreteParam {
                name: "key".to_string(),
                type_ref: TypeRef::String,
                variadic: false,
            },
            value.clone(),
        ];
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
            pub fn #method_ident #method_generics(
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

fn record_value_name(ty: &TypeRef) -> String {
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
        TypeRef::Nullable(inner) => record_value_name(inner),
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
    fn primitive_union_generates_named_indexing_setters() {
        let decl = RecordDecl {
            name: "EvaluationContext".to_string(),
            type_params: Vec::<TypeParam>::new(),
            key_type: TypeRef::String,
            value_type: TypeRef::Union(vec![TypeRef::String, TypeRef::Number, TypeRef::Boolean]),
            body_scope: ScopeId::DUMMY,
        };

        let tokens = generate_record(&decl, &ModuleContext::Global, None).to_string();

        assert!(tokens.contains("pub type EvaluationContext"));
        assert!(tokens.contains("indexing_setter"));
        assert!(tokens.contains("fn set_string"));
        assert!(tokens.contains("fn set_number"));
        assert!(tokens.contains("fn set_bool"));
        assert!(tokens.contains("impl EvaluationContext"));
    }
}
