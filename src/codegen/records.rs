//! Named `Record<K, V>` alias generation.
//!
//! `type Name = Record<K, V>` becomes a nominal `Object` wrapper with JS
//! indexing accessors (`record[key]`) plus `new()` / `Default`:
//!
//! * Getters return `Option<V>`: a missing key reads `undefined`, which is the
//!   normal case for a record rather than an error.
//! * Key and value union members that lower to the same Rust type collapse, so
//!   `"a" | "b"` keys stay `&str`. A key that is still heterogeneous erases to
//!   `&JsValue`.
//! * A value with several distinct alternatives gets one `get_<v>` / `set_<v>`
//!   pair per alternative plus an untyped `get(key) -> JsValue`; any other
//!   value uses plain `get` / `set`.

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::codegen::signatures::{
    dedupe_name, flatten_members, generate_dictionary_params, render_generic_bounds, ConcreteParam,
    DictionaryParams,
};
use crate::codegen::typemap::{make_ident, to_syn_type, CodegenContext, TypePosition};
use crate::ir::{ModuleContext, RecordDecl, TypeRef};
use crate::parse::scope::ScopeId;
use crate::util::naming::to_snake_case;

/// Emit a nominal wasm-bindgen wrapper for a named `Record<K, V>` alias.
pub(crate) fn generate_record(
    decl: &RecordDecl,
    module_context: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
) -> TokenStream {
    let scope = decl.body_scope;
    let rust_name = cgctx
        .and_then(|ctx| ctx.renamed_locals.get(&decl.name))
        .cloned()
        .unwrap_or_else(|| decl.name.clone());
    let rust_ident = make_ident(&rust_name);
    let type_param_idents = decl
        .type_params
        .iter()
        .map(|param| make_ident(&param.name))
        .collect::<Vec<_>>();
    let type_args = if type_param_idents.is_empty() {
        quote! {}
    } else {
        quote! { <#(#type_param_idents),*> }
    };
    let type_bounds = decl
        .type_params
        .iter()
        .map(|param| CodegenContext::type_param_decl(cgctx, &param.name))
        .collect::<Vec<_>>();
    let type_generics = render_generic_bounds(&type_bounds);

    let key = ConcreteParam {
        name: "key".to_string(),
        type_ref: match rust_alternatives(&decl.key_type, cgctx, scope, module_context).as_slice() {
            [only] => only.clone(),
            _ => TypeRef::Any,
        },
        variadic: false,
    };
    let values = rust_alternatives(&decl.value_type, cgctx, scope, module_context);
    let suffixed = values.len() > 1;

    let render_params = |params: &[ConcreteParam]| {
        let DictionaryParams { bounds, params, .. } =
            generate_dictionary_params(params, cgctx, scope, module_context);
        let generics = render_generic_bounds(&[type_bounds.clone(), bounds].concat());
        (generics, params)
    };
    let (getter_generics, getter_params) = render_params(std::slice::from_ref(&key));

    let mut used_names = HashSet::new();
    let mut methods = Vec::new();
    if suffixed {
        // No typed family is privileged for a union value, so expose the raw
        // slot as well; it is also the only way to inspect which member is
        // stored.
        let getter_ident = make_ident(&dedupe_name("get", &mut used_names));
        methods.push(quote! {
            #[wasm_bindgen(method, indexing_getter)]
            pub fn #getter_ident #getter_generics(
                this: &#rust_ident #type_args,
                #getter_params
            ) -> JsValue;
        });
    }
    for value in values {
        let suffix = if suffixed {
            format!("_{}", value_name(&value))
        } else {
            String::new()
        };
        let getter_ident = make_ident(&dedupe_name(&format!("get{suffix}"), &mut used_names));
        let setter_ident = make_ident(&dedupe_name(&format!("set{suffix}"), &mut used_names));

        let setter_params = [
            key.clone(),
            ConcreteParam {
                name: "value".to_string(),
                type_ref: value.clone(),
                variadic: false,
            },
        ];
        let slice_to_array = if super::typemap::needs_slice_to_array(&setter_params) {
            quote! { , slice_to_array }
        } else {
            quote! {}
        };
        let (setter_generics, setter_params) = render_params(&setter_params);
        let optional = match &value {
            TypeRef::Nullable(_) => value.clone(),
            _ => TypeRef::Nullable(Box::new(value.clone())),
        };
        let return_type = to_syn_type(
            &optional,
            TypePosition::RETURN,
            cgctx,
            scope,
            module_context,
        );

        methods.push(quote! {
            #[wasm_bindgen(method, indexing_getter)]
            pub fn #getter_ident #getter_generics(
                this: &#rust_ident #type_args,
                #getter_params
            ) -> #return_type;
            #[wasm_bindgen(method, indexing_setter #slice_to_array)]
            pub fn #setter_ident #setter_generics(
                this: &#rust_ident #type_args,
                #setter_params
            );
        });
    }

    let extern_attr = CodegenContext::extern_attr(cgctx, module_context.specifier());
    let public_alias = if rust_name != decl.name {
        let public_ident = make_ident(&decl.name);
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
            #(#methods)*
        }
        #public_alias
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

/// One entry per distinct Rust argument type among `ty`'s union members, in
/// source order. Members that render identically (`"red" | "blue"`, or
/// `string | "auto"`) collapse to one; a non-union yields itself.
fn rust_alternatives(
    ty: &TypeRef,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
    module_context: &ModuleContext,
) -> Vec<TypeRef> {
    let mut seen = HashSet::new();
    flatten_members(ty, cgctx, scope)
        .into_iter()
        .filter(|member| {
            let param = ConcreteParam {
                name: "value".to_string(),
                type_ref: member.clone(),
                variadic: false,
            };
            let DictionaryParams { params, .. } =
                generate_dictionary_params(&[param], cgctx, scope, module_context);
            seen.insert(params.to_string())
        })
        .collect()
}

/// Method-name suffix for one value alternative (`get_<name>`).
fn value_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::String | TypeRef::StringLiteral(_) => "string".to_string(),
        TypeRef::Number | TypeRef::NumberLiteral(_) => "number".to_string(),
        TypeRef::Boolean | TypeRef::BooleanLiteral(_) => "bool".to_string(),
        TypeRef::BigInt => "big_int".to_string(),
        TypeRef::Array(inner) => format!("slice_of_{}", value_name(inner)),
        TypeRef::Nullable(inner) => format!("optional_{}", value_name(inner)),
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
                let args = generic_args
                    .iter()
                    .map(value_name)
                    .collect::<Vec<_>>()
                    .join("_and_");
                format!("{head}_of_{args}")
            }
        }
        other => super::signatures::type_snake_name(other),
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

    fn render(key_type: TypeRef, value_type: TypeRef) -> String {
        generate_record(&record(key_type, value_type), &ModuleContext::Global, None).to_string()
    }

    #[test]
    fn plain_value_uses_unsuffixed_accessors() {
        let tokens = render(TypeRef::String, TypeRef::Number);
        assert!(tokens.contains("fn get (this : & Values , key : & str) -> Option < f64 >"));
        assert!(tokens.contains("fn set (this : & Values , key : & str , value : f64)"));
        assert!(!tokens.contains("try_get"));
        assert!(!tokens.contains("catch"));
        assert!(tokens.contains("impl Default for Values"));
        assert!(tokens.contains("pub fn new () -> Self"));
    }

    #[test]
    fn nullable_value_is_not_double_wrapped() {
        let tokens = render(
            TypeRef::String,
            TypeRef::Nullable(Box::new(TypeRef::Number)),
        );
        assert!(tokens.contains("-> Option < f64 >"));
        assert!(!tokens.contains("Option < Option"));
    }

    #[test]
    fn any_value_reads_as_plain_js_value() {
        let tokens = render(TypeRef::String, TypeRef::Any);
        assert!(tokens.contains("fn get (this : & Values , key : & str) -> JsValue"));
    }

    #[test]
    fn union_value_gets_one_suffixed_family_per_member() {
        let tokens = render(
            TypeRef::String,
            TypeRef::Union(vec![TypeRef::String, TypeRef::Number, TypeRef::Boolean]),
        );
        for (name, ty) in [("string", "String"), ("number", "f64"), ("bool", "bool")] {
            assert!(
                tokens.contains(&format!(
                    "fn get_{name} (this : & Values , key : & str) -> Option < {ty} >"
                )),
                "get_{name}"
            );
            assert!(tokens.contains(&format!("fn set_{name} ")), "set_{name}");
        }
        assert!(tokens.contains("fn get (this : & Values , key : & str) -> JsValue"));
        assert!(!tokens.contains("fn set ("));
    }

    #[test]
    fn value_members_with_the_same_rust_type_collapse() {
        let tokens = render(
            TypeRef::String,
            TypeRef::Union(vec![
                TypeRef::StringLiteral("red".into()),
                TypeRef::StringLiteral("blue".into()),
                TypeRef::String,
            ]),
        );
        assert!(tokens.contains("fn get (this : & Values , key : & str) -> Option < String >"));
        assert!(!tokens.contains("fn get_string"));
    }

    #[test]
    fn heterogeneous_union_key_erases_to_js_value() {
        let tokens = render(
            TypeRef::Union(vec![TypeRef::String, TypeRef::Number]),
            TypeRef::Boolean,
        );
        assert!(tokens.contains("fn get (this : & Values , key : & JsValue) -> Option < bool >"));
        assert!(tokens.contains("fn set (this : & Values , key : & JsValue , value : bool)"));
        assert_eq!(tokens.matches("indexing_getter").count(), 1);
    }

    #[test]
    fn key_members_with_the_same_rust_type_collapse() {
        for key in [
            TypeRef::Union(vec![
                TypeRef::StringLiteral("a".into()),
                TypeRef::StringLiteral("b".into()),
            ]),
            TypeRef::Union(vec![TypeRef::String, TypeRef::StringLiteral("auto".into())]),
        ] {
            let tokens = render(key, TypeRef::String);
            assert!(tokens.contains("key : & str"), "{tokens}");
            assert!(!tokens.contains("JsValue"), "{tokens}");
        }
    }

    #[test]
    fn nested_value_names_describe_the_member() {
        let tokens = render(
            TypeRef::String,
            TypeRef::Union(vec![
                TypeRef::Array(Box::new(TypeRef::String)),
                TypeRef::Array(Box::new(TypeRef::Number)),
            ]),
        );
        assert!(tokens.contains("fn get_slice_of_string"));
        assert!(tokens.contains("fn get_slice_of_number"));
    }
}
