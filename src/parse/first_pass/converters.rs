//! Individual declaration converters: AST → IR.
//!
//! Pure functions that convert oxc AST nodes into IR declarations.
//! Used by both Phase 1 (for classification) and Phase 2 (for full population).

use oxc_ast::ast;

use crate::ir;
use crate::parse::classify::classify_interface;
use crate::parse::ctx::ParseCtx;
use crate::parse::docs::DocComments;
use crate::parse::members::{convert_class_element, convert_ts_signature, create_body_scope};
use crate::parse::scope::ScopeId;
use crate::parse::types::{
    convert_formal_params_scoped, convert_ts_type_scoped, convert_type_params,
};
use crate::util::naming::to_snake_case;

pub fn convert_class_decl(
    class: &ast::Class<'_>,
    module_ctx: &ir::ModuleContext,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<ir::ClassDecl> {
    let name = class.id.as_ref()?.name.to_string();
    let js_name = name.clone();

    let mut type_params = convert_type_params(class.type_parameters.as_ref(), ctx.diag);

    let extends =
        class
            .heritage
            .as_ref()
            .and_then(|h| match expression_to_dotted_name(&h.expression) {
                Some(name) => Some(ir::TypeRef::ident(name)),
                None => {
                    ctx.diag
                        .warn("Complex super class expression is not supported");
                    None
                }
            });

    let implements: Vec<ir::TypeRef> = class
        .implements
        .iter()
        .map(|i| convert_ts_type_name_to_ref(&i.expression))
        .collect();

    let is_abstract = class.r#abstract;

    // Class body scope holds the class's own `<T, ...>` parameters.
    let body_scope = create_body_scope(&mut type_params, parent_scope, ctx);

    let members: Vec<ir::Member> = class
        .body
        .body
        .iter()
        .flat_map(|elem| convert_class_element(elem, Some(&name), body_scope, ctx))
        .collect();

    Some(ir::ClassDecl {
        name,
        js_name,
        type_params,
        extends,
        implements,
        is_abstract,
        members,
        type_module_context: module_ctx.clone(),
        body_scope,
    })
}

pub fn convert_interface_decl(
    iface: &ast::TSInterfaceDeclaration<'_>,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> ir::InterfaceDecl {
    let name = iface.id.name.to_string();
    let type_params = convert_type_params(iface.type_parameters.as_ref(), ctx.diag);
    let extends: Vec<ir::TypeRef> = iface
        .extends
        .iter()
        .map(|ext| convert_ts_type_name_to_ref(&ext.type_name))
        .collect();
    interface_from_signatures(
        name,
        type_params,
        extends,
        &iface.body.body,
        parent_scope,
        ctx,
    )
}

/// Build an `InterfaceDecl` from raw inputs — the same pipeline that
/// `convert_interface_decl` runs on a real `TSInterfaceDeclaration`,
/// factored so that synthesized anonymous interfaces (lifted from
/// `{ ... }` parameter types) take the identical path.
///
/// This is the single point of truth for "given a name and a list of
/// signature members, produce an interface IR node." Member conversion
/// goes through `convert_ts_signature` (the same function real
/// declarations use), classification goes through `classify_interface`,
/// nothing diverges.
pub fn interface_from_signatures(
    name: String,
    mut type_params: Vec<ir::TypeParam>,
    extends: Vec<ir::TypeRef>,
    signatures: &[ast::TSSignature<'_>],
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> ir::InterfaceDecl {
    let js_name = name.clone();
    let parent = name.clone();
    let body_scope = create_body_scope(&mut type_params, parent_scope, ctx);
    let members: Vec<ir::Member> = signatures
        .iter()
        .flat_map(|sig| convert_ts_signature(sig, Some(&parent), body_scope, ctx))
        .collect();
    let classification = classify_interface(&members);
    ir::InterfaceDecl {
        name,
        js_name,
        type_params,
        extends,
        members,
        classification,
        body_scope,
    }
}

pub fn convert_function_decl(
    func: &ast::Function<'_>,
    throws: ir::Throws,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<ir::FunctionDecl> {
    let name = func.id.as_ref()?.name.to_string();
    let js_name = name.clone();
    let rust_name = to_snake_case(&name);

    let mut type_params = convert_type_params(func.type_parameters.as_ref(), ctx.diag);
    let body_scope = create_body_scope(&mut type_params, parent_scope, ctx);

    let params = convert_formal_params_scoped(&func.params, body_scope, ctx);
    let return_type = func
        .return_type
        .as_ref()
        .map(|rt| convert_ts_type_scoped(&rt.type_annotation, body_scope, ctx))
        .unwrap_or(ir::TypeRef::Void);

    Some(ir::FunctionDecl {
        name: rust_name,
        js_name,
        type_params,
        params,
        return_type,
        overloads: vec![],
        throws,
        body_scope,
    })
}

pub fn convert_string_enum(name: &str, ts_type: &ast::TSType<'_>) -> Option<ir::StringEnumDecl> {
    let variants = match ts_type {
        ast::TSType::TSUnionType(union) => union
            .types
            .iter()
            .filter_map(|t| {
                if let ast::TSType::TSLiteralType(lit) = t {
                    if let ast::TSLiteral::StringLiteral(s) = &lit.literal {
                        let js_value = s.value.to_string();
                        let rust_name = crate::util::naming::to_enum_variant(&js_value);
                        return Some(ir::StringEnumVariant {
                            rust_name,
                            js_value,
                        });
                    }
                }
                None
            })
            .collect(),
        ast::TSType::TSLiteralType(lit) => {
            if let ast::TSLiteral::StringLiteral(s) = &lit.literal {
                let js_value = s.value.to_string();
                let rust_name = crate::util::naming::to_enum_variant(&js_value);
                vec![ir::StringEnumVariant {
                    rust_name,
                    js_value,
                }]
            } else {
                return None;
            }
        }
        _ => return None,
    };

    // Deduplicate variant names (e.g., "text-plain" and "textPlain" both → "TextPlain")
    let mut rust_names: Vec<String> = variants.iter().map(|v| v.rust_name.clone()).collect();
    crate::util::naming::dedup_names(&mut rust_names);
    let variants: Vec<_> = variants
        .into_iter()
        .zip(rust_names)
        .map(|(mut v, name)| {
            v.rust_name = name;
            v
        })
        .collect();

    Some(ir::StringEnumDecl {
        name: name.to_string(),
        variants,
    })
}

/// Convert a TS enum declaration, leaving its initialisers to be evaluated
/// once every enum they can reference is parsed.
pub fn convert_ts_enum(
    enum_decl: &ast::TSEnumDeclaration<'_>,
    docs: &DocComments<'_>,
) -> ir::TsEnumDecl {
    let members = enum_decl
        .body
        .members
        .iter()
        .filter_map(|member| {
            Some(ir::TsEnumMember {
                name: enum_member_name(member)?,
                init: member.initializer.as_ref().map(convert_enum_init),
                doc: docs.for_span(member.span.start),
            })
        })
        .collect();
    ir::TsEnumDecl {
        name: enum_decl.id.name.to_string(),
        members,
        global: false,
    }
}

/// An enum member initialiser in the forms TS evaluates at compile time:
/// literals, unary and binary operators, and member references, bare or
/// through an enum's path, including `E["A"]`.
fn convert_enum_init(expr: &ast::Expression<'_>) -> ir::EnumInit {
    use ast::Expression as E;
    match expr {
        E::NumericLiteral(n) => ir::EnumInit::Num(n.value),
        E::StringLiteral(s) => ir::EnumInit::Str(s.value.to_string()),
        E::TemplateLiteral(t) if t.expressions.is_empty() => ir::EnumInit::Str(
            t.quasis
                .iter()
                .filter_map(|q| q.value.cooked.as_ref())
                .map(|c| c.as_str())
                .collect(),
        ),
        E::ParenthesizedExpression(p) => convert_enum_init(&p.expression),
        E::Identifier(_) | E::StaticMemberExpression(_) => match expression_to_path(expr) {
            Some((head, segments)) => {
                ir::EnumInit::Ref(std::iter::once(head).chain(segments).collect())
            }
            None => ir::EnumInit::Unknown,
        },
        E::ComputedMemberExpression(m) => match (expression_to_path(&m.object), &m.expression) {
            (Some((head, mut segments)), E::StringLiteral(key)) => {
                segments.push(key.value.to_string());
                ir::EnumInit::Ref(std::iter::once(head).chain(segments).collect())
            }
            _ => ir::EnumInit::Unknown,
        },
        E::UnaryExpression(u) => ir::EnumInit::Unary(
            u.operator.as_str().to_string(),
            Box::new(convert_enum_init(&u.argument)),
        ),
        E::BinaryExpression(b) => ir::EnumInit::Binary(
            b.operator.as_str().to_string(),
            Box::new(convert_enum_init(&b.left)),
            Box::new(convert_enum_init(&b.right)),
        ),
        _ => ir::EnumInit::Unknown,
    }
}

fn enum_member_name(member: &ast::TSEnumMember<'_>) -> Option<String> {
    match &member.id {
        ast::TSEnumMemberName::Identifier(id) => Some(id.name.to_string()),
        ast::TSEnumMemberName::String(s) => Some(s.value.to_string()),
        _ => None,
    }
}

/// Extract a path from an expression: `Foo` → `("Foo", [])`,
/// `Foo.Bar.Baz` → `("Foo", ["Bar", "Baz"])`. Returns `None` for
/// expressions that don't form a static dotted name.
pub fn expression_to_path(expr: &ast::Expression<'_>) -> Option<(String, Vec<String>)> {
    match expr {
        ast::Expression::Identifier(ident) => Some((ident.name.to_string(), Vec::new())),
        ast::Expression::StaticMemberExpression(member) => {
            let (head, mut segments) = expression_to_path(&member.object)?;
            segments.push(member.property.name.to_string());
            Some((head, segments))
        }
        _ => None,
    }
}

/// Backwards-compatible helper that flattens a path expression to
/// dotted form. Prefer [`expression_to_path`] for new code.
pub fn expression_to_dotted_name(expr: &ast::Expression<'_>) -> Option<String> {
    let (head, segments) = expression_to_path(expr)?;
    if segments.is_empty() {
        Some(head)
    } else {
        Some(format!("{}.{}", head, segments.join(".")))
    }
}

fn convert_ts_type_name_to_ref(type_name: &ast::TSTypeName<'_>) -> ir::TypeRef {
    match type_name {
        ast::TSTypeName::IdentifierReference(ident) => ir::TypeRef::ident(ident.name.to_string()),
        ast::TSTypeName::QualifiedName(qualified) => {
            let (head, mut segments) = collect_qualified_path(qualified);
            segments.insert(0, head);
            ir::TypeRef::Reference {
                segments,
                generic_args: Vec::new(),
            }
        }
        ast::TSTypeName::ThisExpression(_) => ir::TypeRef::Unresolved("this".to_string()),
    }
}

fn collect_qualified_path(q: &ast::TSQualifiedName<'_>) -> (String, Vec<String>) {
    fn walk(name: &ast::TSTypeName<'_>) -> (String, Vec<String>) {
        match name {
            ast::TSTypeName::IdentifierReference(ident) => (ident.name.to_string(), Vec::new()),
            ast::TSTypeName::QualifiedName(qual) => {
                let (head, mut segments) = walk(&qual.left);
                segments.push(qual.right.name.to_string());
                (head, segments)
            }
            ast::TSTypeName::ThisExpression(_) => ("this".to_string(), Vec::new()),
        }
    }
    let (head, mut segments) = walk(&q.left);
    segments.push(q.right.name.to_string());
    (head, segments)
}

/// Debug name for an `ExportDefaultDeclarationKind` variant.
pub fn export_default_kind_name(kind: &ast::ExportDefaultDeclarationKind<'_>) -> &'static str {
    match kind {
        ast::ExportDefaultDeclarationKind::ClassDeclaration(_) => "ClassDeclaration",
        ast::ExportDefaultDeclarationKind::FunctionDeclaration(_) => "FunctionDeclaration",
        ast::ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => "TSInterfaceDeclaration",
        _ => "Expression",
    }
}
