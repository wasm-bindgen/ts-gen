//! Detection and execution of the `declare var` + `interface` merge pattern.

use oxc_ast::ast::*;

use crate::ir::*;
use crate::parse::ctx::ParseCtx;
use crate::parse::members::{create_body_scope, property_key_name};
use crate::parse::scope::ScopeId;
use crate::parse::types::{
    convert_formal_params_scoped, convert_ts_type_scoped, convert_type_params,
};
use crate::util::naming::to_snake_case;

/// Check if a variable declarator looks like a class constructor pattern.
/// Returns true if the type annotation is a type literal containing `prototype` and/or `new`.
pub fn is_class_constructor_var(declarator: &VariableDeclarator<'_>) -> bool {
    // In oxc 0.118, type_annotation is directly on VariableDeclarator
    if let Some(type_ann) = &declarator.type_annotation {
        if let TSType::TSTypeLiteral(lit) = &type_ann.type_annotation {
            let has_prototype = lit.members.iter().any(|m| {
                if let TSSignature::TSPropertySignature(prop) = m {
                    property_key_name(&prop.key)
                        .map(|n| n == "prototype")
                        .unwrap_or(false)
                } else {
                    false
                }
            });
            let has_new = lit
                .members
                .iter()
                .any(|m| matches!(m, TSSignature::TSConstructSignatureDeclaration(_)));
            return has_prototype || has_new;
        }
    }
    false
}

/// Extract the name from a variable declarator.
/// In oxc 0.118, BindingPattern is an enum directly.
pub fn var_declarator_name(declarator: &VariableDeclarator<'_>) -> Option<String> {
    match &declarator.id {
        BindingPattern::BindingIdentifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

/// Extract constructor, static methods, and static properties from a type literal.
pub fn extract_var_members(
    type_literal: &TSTypeLiteral<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> (Option<ConstructorMember>, Vec<Member>) {
    let mut constructor = None;
    let mut static_members = Vec::new();

    for member in &type_literal.members {
        match member {
            TSSignature::TSConstructSignatureDeclaration(ctor) => {
                let params = convert_formal_params_scoped(&ctor.params, parent_scope, ctx);
                let (doc, info) = match ctx.docs.info_for_span(ctor.span.start) {
                    Some((d, i)) => (Some(d), i),
                    None => (None, crate::parse::docs::JsDocInfo::default()),
                };
                constructor = Some(ConstructorMember {
                    params,
                    doc,
                    throws: info.throws(),
                });
            }
            TSSignature::TSPropertySignature(prop) => {
                let js_name = match property_key_name(&prop.key) {
                    Some(name) => name,
                    None => continue,
                };
                if js_name == "prototype" {
                    continue;
                }
                let doc = ctx.docs.for_span(prop.span.start);
                let type_ref = prop
                    .type_annotation
                    .as_ref()
                    .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
                    .unwrap_or(TypeRef::Any);
                static_members.push(Member::StaticGetter(StaticGetterMember {
                    js_name: js_name.clone(),
                    type_ref: type_ref.clone(),
                    doc,
                }));
                if !prop.readonly {
                    static_members.push(Member::StaticSetter(StaticSetterMember {
                        js_name,
                        type_ref,
                        doc: None,
                    }));
                }
            }
            TSSignature::TSMethodSignature(method) => {
                let js_name = match property_key_name(&method.key) {
                    Some(name) => name,
                    None => continue,
                };
                let (doc, info) = match ctx.docs.info_for_span(method.span.start) {
                    Some((d, i)) => (Some(d), i),
                    None => (None, crate::parse::docs::JsDocInfo::default()),
                };
                let name = to_snake_case(&js_name);
                let type_params = convert_type_params(method.type_parameters.as_ref(), ctx.diag);
                let body_scope = create_body_scope(&type_params, parent_scope, ctx);
                let params = convert_formal_params_scoped(&method.params, body_scope, ctx);
                let return_type = method
                    .return_type
                    .as_ref()
                    .map(|rt| convert_ts_type_scoped(&rt.type_annotation, body_scope, ctx))
                    .unwrap_or(TypeRef::Void);
                static_members.push(Member::StaticMethod(StaticMethodMember {
                    name,
                    js_name,
                    type_params,
                    params,
                    return_type,
                    doc,
                    throws: info.throws(),
                    body_scope,
                }));
            }
            TSSignature::TSCallSignatureDeclaration(_) => {
                ctx.diag
                    .warn("Call signatures in declare var type literal are not supported");
            }
            TSSignature::TSIndexSignature(_) => {
                ctx.diag
                    .warn("Index signatures in declare var type literal are not supported");
            }
        }
    }

    (constructor, static_members)
}
