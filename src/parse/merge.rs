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
    match declarator.type_annotation.as_deref() {
        Some(TSTypeAnnotation {
            type_annotation: TSType::TSTypeLiteral(lit),
            ..
        }) => has_constructor_shape(&lit.members),
        _ => false,
    }
}

/// Whether an object type body has a `prototype` property and/or a
/// construct signature, the shape TS libs give a class's static side.
pub fn has_constructor_shape(members: &[TSSignature<'_>]) -> bool {
    members.iter().any(|m| match m {
        TSSignature::TSConstructSignatureDeclaration(_) => true,
        TSSignature::TSPropertySignature(prop) => {
            property_key_name(&prop.key).is_some_and(|n| n == "prototype")
        }
        _ => false,
    })
}

/// The bare interface name a `declare var X: XConstructor` annotation
/// refers to. Whether that interface actually has constructor shape is
/// only known once the whole file has been collected.
pub fn named_constructor_ref(declarator: &VariableDeclarator<'_>) -> Option<String> {
    match declarator.type_annotation.as_deref() {
        Some(TSTypeAnnotation {
            type_annotation: TSType::TSTypeReference(r),
            ..
        }) if r.type_arguments.is_none() => match &r.type_name {
            TSTypeName::IdentifierReference(id) => Some(id.name.to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// Extract the name from a variable declarator.
/// In oxc 0.118, BindingPattern is an enum directly.
pub fn var_declarator_name(declarator: &VariableDeclarator<'_>) -> Option<String> {
    match &declarator.id {
        BindingPattern::BindingIdentifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

/// Extract constructor, static methods, and static properties from the
/// static-side object type of a class: an inline `{ new(...): T; ... }`
/// literal or a `*Constructor` interface body.
/// A construct signature (`new (…): T`) as the constructor of the class it
/// constructs.
pub fn convert_construct_signature(
    ctor: &oxc_ast::ast::TSConstructSignatureDeclaration<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> ConstructorMember {
    let mut type_params =
        crate::parse::types::convert_type_params(ctor.type_parameters.as_ref(), ctx.diag);
    let scope = crate::parse::members::create_body_scope(&mut type_params, parent_scope, ctx);
    let mut params = convert_formal_params_scoped(&ctor.params, scope, ctx);
    let (doc, info) = match ctx.docs.info_for_span(ctor.span.start) {
        Some((d, i)) => (Some(d), i),
        None => (None, crate::parse::docs::JsDocInfo::default()),
    };
    crate::parse::members::apply_rust_tags(&info, None, &mut params, "constructor", ctx);
    ConstructorMember {
        scope,
        type_params,
        params,
        doc,
        throws: info.throws(),
    }
}

pub fn extract_var_members(
    members: &[TSSignature<'_>],
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> (Vec<ConstructorMember>, Vec<Member>) {
    let mut constructors = Vec::new();
    let mut static_members = Vec::new();

    for member in members {
        match member {
            TSSignature::TSConstructSignatureDeclaration(ctor) => {
                constructors.push(convert_construct_signature(ctor, parent_scope, ctx));
            }
            TSSignature::TSPropertySignature(prop) => {
                let js_name = match property_key_name(&prop.key) {
                    Some(name) => name,
                    None => continue,
                };
                if js_name == "prototype" {
                    continue;
                }
                let (doc, info) = match ctx.docs.info_for_span(prop.span.start) {
                    Some((d, i)) => (Some(d), i),
                    None => (None, crate::parse::docs::JsDocInfo::default()),
                };
                let type_ref = prop
                    .type_annotation
                    .as_ref()
                    .map(|ann| convert_ts_type_scoped(&ann.type_annotation, parent_scope, ctx))
                    .unwrap_or(TypeRef::Any);
                static_members.push(Member::StaticGetter(StaticGetterMember {
                    scope: parent_scope,
                    js_name: js_name.clone(),
                    type_ref: type_ref.clone(),
                    optional: prop.optional,
                    doc,
                    throws: info.throws(),
                    string_mode: info.string_mode.as_deref().and_then(StringModeHint::parse),
                }));
                if !prop.readonly {
                    static_members.push(Member::StaticSetter(StaticSetterMember {
                        scope: parent_scope,
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
                let mut type_params =
                    convert_type_params(method.type_parameters.as_ref(), ctx.diag);
                let body_scope = create_body_scope(&mut type_params, parent_scope, ctx);
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
                    async_modifier: false,
                    body_scope,
                    string_mode: info.string_mode.as_deref().and_then(StringModeHint::parse),
                }));
            }
            TSSignature::TSCallSignatureDeclaration(_) => {
                ctx.diag
                    .warn("Call signatures on a class static side are not supported");
            }
            TSSignature::TSIndexSignature(_) => {
                ctx.diag
                    .warn("Index signatures on a class static side are not supported");
            }
        }
    }

    (constructors, static_members)
}
