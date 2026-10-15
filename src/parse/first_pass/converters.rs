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
use crate::util::diagnostics::DiagnosticCollector;
use crate::util::naming::to_snake_case;

pub fn convert_class_decl(
    class: &ast::Class<'_>,
    module_ctx: &ir::ModuleContext,
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Option<ir::ClassDecl> {
    let name = class.id.as_ref()?.name.to_string();
    let js_name = name.clone();

    let type_params = convert_type_params(class.type_parameters.as_ref(), ctx.diag);

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
    let body_scope = create_body_scope(&type_params, parent_scope, ctx);

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
    type_params: Vec<ir::TypeParam>,
    extends: Vec<ir::TypeRef>,
    signatures: &[ast::TSSignature<'_>],
    parent_scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> ir::InterfaceDecl {
    let js_name = name.clone();
    let parent = name.clone();
    let body_scope = create_body_scope(&type_params, parent_scope, ctx);
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

    let type_params = convert_type_params(func.type_parameters.as_ref(), ctx.diag);
    let body_scope = create_body_scope(&type_params, parent_scope, ctx);

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

/// Classify a TS enum as string or numeric based on its member initializers.
pub fn classify_ts_enum_kind(enum_decl: &ast::TSEnumDeclaration<'_>) -> ir::RegisteredKind {
    let mut has_string = false;
    let mut has_numeric = false;

    for member in &enum_decl.body.members {
        match &member.initializer {
            Some(ast::Expression::StringLiteral(_)) => has_string = true,
            Some(ast::Expression::NumericLiteral(_)) => has_numeric = true,
            Some(ast::Expression::UnaryExpression(_)) => has_numeric = true,
            None => {
                has_numeric = true;
            }
            _ => {}
        }
    }

    if has_string && !has_numeric {
        ir::RegisteredKind::StringEnum
    } else if has_numeric {
        ir::RegisteredKind::NumericEnum
    } else {
        ir::RegisteredKind::StringEnum
    }
}

/// The value a TS enum member evaluates to.
#[derive(Clone, Debug, PartialEq)]
enum EnumValue {
    Num(f64),
    Str(String),
}

/// Evaluate an enum member initialiser the way TypeScript does: literals,
/// unary and binary constant expressions with JS numeric semantics
/// (bitwise operators on int32, `>>>` on uint32), string concatenation,
/// and references to earlier members, bare or qualified by the enum's own
/// name. `None` for anything else.
fn eval_enum_expr(
    expr: &ast::Expression<'_>,
    enum_name: &str,
    members: &[(String, EnumValue)],
) -> Option<EnumValue> {
    use ast::Expression as E;
    let lookup = |name: &str| {
        members
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    match expr {
        E::NumericLiteral(n) => Some(EnumValue::Num(n.value)),
        E::StringLiteral(s) => Some(EnumValue::Str(s.value.to_string())),
        E::TemplateLiteral(t) if t.expressions.is_empty() => Some(EnumValue::Str(
            t.quasis
                .iter()
                .filter_map(|q| q.value.cooked.as_ref())
                .map(|c| c.as_str())
                .collect(),
        )),
        E::ParenthesizedExpression(p) => eval_enum_expr(&p.expression, enum_name, members),
        E::Identifier(id) => lookup(&id.name),
        E::StaticMemberExpression(m) => match &m.object {
            E::Identifier(obj) if obj.name == enum_name => lookup(&m.property.name),
            _ => None,
        },
        E::UnaryExpression(u) => {
            let EnumValue::Num(v) = eval_enum_expr(&u.argument, enum_name, members)? else {
                return None;
            };
            Some(EnumValue::Num(match u.operator.as_str() {
                "-" => -v,
                "+" => v,
                "~" => f64::from(!to_int32(v)),
                _ => return None,
            }))
        }
        E::BinaryExpression(b) => {
            let l = eval_enum_expr(&b.left, enum_name, members)?;
            let r = eval_enum_expr(&b.right, enum_name, members)?;
            let op = b.operator.as_str();
            let (l, r) = match (l, r) {
                (EnumValue::Str(l), EnumValue::Str(r)) if op == "+" => {
                    return Some(EnumValue::Str(l + &r));
                }
                (EnumValue::Num(l), EnumValue::Num(r)) => (l, r),
                _ => return None,
            };
            let (li, ri) = (to_int32(l), to_int32(r));
            Some(EnumValue::Num(match op {
                "+" => l + r,
                "-" => l - r,
                "*" => l * r,
                "/" => l / r,
                "%" => l % r,
                "**" => l.powf(r),
                "&" => f64::from(li & ri),
                "|" => f64::from(li | ri),
                "^" => f64::from(li ^ ri),
                "<<" => f64::from(li.wrapping_shl(ri as u32 & 31)),
                ">>" => f64::from(li.wrapping_shr(ri as u32 & 31)),
                ">>>" => f64::from((li as u32).wrapping_shr(ri as u32 & 31)),
                _ => return None,
            }))
        }
        _ => None,
    }
}

/// ECMAScript `ToInt32`: modular wrap, not saturation.
fn to_int32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    (v.trunc().rem_euclid(4_294_967_296.0) as u32) as i32
}

fn enum_member_name(member: &ast::TSEnumMember<'_>) -> Option<String> {
    match &member.id {
        ast::TSEnumMemberName::Identifier(id) => Some(id.name.to_string()),
        ast::TSEnumMemberName::String(s) => Some(s.value.to_string()),
        _ => None,
    }
}

/// Convert a TS enum with string values to our StringEnumDecl IR.
pub fn convert_string_ts_enum(
    enum_decl: &ast::TSEnumDeclaration<'_>,
    diag: &mut DiagnosticCollector,
) -> ir::StringEnumDecl {
    let name = enum_decl.id.name.to_string();
    let mut values: Vec<(String, EnumValue)> = Vec::new();

    let variants: Vec<_> = enum_decl
        .body
        .members
        .iter()
        .filter_map(|member| {
            let member_name = enum_member_name(member)?;
            let js_value = match member
                .initializer
                .as_ref()
                .and_then(|init| eval_enum_expr(init, &name, &values))
            {
                Some(EnumValue::Str(s)) => s,
                _ => {
                    diag.warn(format!(
                        "Enum `{name}::{member_name}` has no string value, using its name"
                    ));
                    member_name.clone()
                }
            };
            values.push((member_name.clone(), EnumValue::Str(js_value.clone())));
            let rust_name = crate::util::naming::to_enum_variant(&member_name);
            Some(ir::StringEnumVariant {
                rust_name,
                js_value,
            })
        })
        .collect();

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

    ir::StringEnumDecl { name, variants }
}

/// Convert a TS enum with numeric values to our NumericEnumDecl IR.
pub fn convert_numeric_enum(
    enum_decl: &ast::TSEnumDeclaration<'_>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> ir::NumericEnumDecl {
    let name = enum_decl.id.name.to_string();
    let mut next_value: i64 = 0;
    let mut values: Vec<(String, EnumValue)> = Vec::new();

    let variants: Vec<_> = enum_decl
        .body
        .members
        .iter()
        .filter_map(|member| {
            let member_name = enum_member_name(member)?;

            let value = match &member.initializer {
                None => next_value,
                Some(init) => match eval_enum_expr(init, &name, &values) {
                    Some(EnumValue::Num(v)) => f64_to_i64(v, &member_name, &name, diag),
                    _ => {
                        diag.warn(format!(
                            "Enum `{name}::{member_name}` has an initializer that is not a \
                             numeric constant expression, using {next_value}"
                        ));
                        next_value
                    }
                },
            };
            next_value = value + 1;
            values.push((member_name.clone(), EnumValue::Num(value as f64)));

            let doc = docs.for_span(member.span.start);
            let rust_name = crate::util::naming::to_enum_variant(&member_name);

            Some(ir::NumericEnumVariant {
                rust_name,
                js_name: member_name,
                value,
                doc,
            })
        })
        .collect();

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

    ir::NumericEnumDecl { name, variants }
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

/// Convert f64 (JS number) to i64 with a diagnostic if the value is not
/// an exact integer or is out of i64 range.
fn f64_to_i64(
    value: f64,
    member_name: &str,
    enum_name: &str,
    diag: &mut DiagnosticCollector,
) -> i64 {
    if value.fract() != 0.0 {
        diag.warn(format!(
            "Enum `{enum_name}::{member_name}` has non-integer value {value}, truncating to {}",
            value as i64
        ));
    } else if value > i64::MAX as f64 || value < i64::MIN as f64 {
        diag.warn(format!(
            "Enum `{enum_name}::{member_name}` value {value} is out of i64 range, truncating"
        ));
    }
    let result = value as i64;
    // Warn if value won't fit in the codegen repr (i32 for signed, u32 for unsigned)
    if i32::try_from(result).is_err() && u32::try_from(result).is_err() {
        diag.warn(format!(
            "Enum `{enum_name}::{member_name}` value {result} exceeds i32/u32 range, \
             will be truncated in generated code"
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn numeric_values(src: &str) -> Vec<(String, i64)> {
        let allocator = Allocator::default();
        let program = Parser::new(&allocator, src, SourceType::d_ts()).parse();
        let docs = DocComments::new(&[], src);
        let mut diag = DiagnosticCollector::new();
        let ast::Statement::TSEnumDeclaration(decl) = &program.program.body[0] else {
            panic!("expected an enum");
        };
        convert_numeric_enum(decl, &docs, &mut diag)
            .variants
            .into_iter()
            .map(|v| (v.js_name, v.value))
            .collect()
    }

    fn string_values(src: &str) -> Vec<(String, String)> {
        let allocator = Allocator::default();
        let program = Parser::new(&allocator, src, SourceType::d_ts()).parse();
        let mut diag = DiagnosticCollector::new();
        let ast::Statement::TSEnumDeclaration(decl) = &program.program.body[0] else {
            panic!("expected an enum");
        };
        convert_string_ts_enum(decl, &mut diag)
            .variants
            .into_iter()
            .map(|v| (v.rust_name, v.js_value))
            .collect()
    }

    fn pairs<V: Clone>(items: &[(&str, V)]) -> Vec<(String, V)> {
        items
            .iter()
            .map(|(n, v)| (n.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn member_references_take_the_referenced_value() {
        let values = numeric_values("declare enum E { A = 1, B = 2, C = B, D = E.A, F }");
        assert_eq!(
            values,
            pairs(&[("A", 1), ("B", 2), ("C", 2), ("D", 1), ("F", 2)])
        );
    }

    #[test]
    fn constant_expressions_use_js_numeric_semantics() {
        let values = numeric_values(
            "declare enum F { R = 1 << 0, W = 1 << 1, RW = R | W, All = RW | 4, Mask = ~0, \
             Neg = -R, Big = 1 << 31, Shr = -8 >> 1, Ushr = -1 >>> 28, Pow = 2 ** 3 }",
        );
        assert_eq!(
            values,
            pairs(&[
                ("R", 1),
                ("W", 2),
                ("RW", 3),
                ("All", 7),
                ("Mask", -1),
                ("Neg", -1),
                ("Big", -2147483648),
                ("Shr", -4),
                ("Ushr", 15),
                ("Pow", 8),
            ])
        );
    }

    #[test]
    fn unknown_initializer_falls_back_to_auto_increment() {
        let values = numeric_values("declare enum E { A = 5, B = foo(), C }");
        assert_eq!(values, pairs(&[("A", 5), ("B", 6), ("C", 7)]));
    }

    #[test]
    fn string_members_reference_and_concatenate() {
        let values =
            string_values("declare enum K { A = \"a\", B = A, C = A + \"c\", D = `d`, E = K.B }");
        assert_eq!(
            values,
            pairs(&[
                ("A", "a".to_string()),
                ("B", "a".into()),
                ("C", "ac".into()),
                ("D", "d".into()),
                ("E", "a".into())
            ])
        );
    }
}
