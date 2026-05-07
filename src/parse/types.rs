//! Convert oxc TypeScript type AST nodes to our IR `TypeRef`.
//!
//! Type conversion is purely syntactic: it walks the AST and produces
//! a `TypeRef`. Name resolution — figuring out whether `T` refers to a
//! declared type or to an in-scope generic parameter — is deferred to
//! codegen, which consults the scope chain at the relevant use site.
//!
//! However, the converter still needs to understand *some* lexical
//! scope, for one reason: function type literals (`(x: T) => U`)
//! introduce their own type-parameter scope. Each such literal
//! allocates a fresh child scope on the arena so its `T` shadows any
//! outer `T` for the duration of the conversion. Child scopes are
//! appended to the arena and their IDs stay stable.

use oxc_ast::ast::*;

use crate::ir::TypeRef;
use crate::parse::ctx::ParseCtx;
use crate::parse::scope::{ScopeArena, ScopeId};
use crate::util::diagnostics::DiagnosticCollector;
use crate::util::naming::to_snake_case;

/// Convert an oxc `TSType` to our IR `TypeRef` with no enclosing
/// type-parameter scope. Use [`convert_ts_type_scoped`] when the
/// surrounding context (a class body, method, type alias, ...)
/// introduces type parameters that should be visible to nested types.
///
/// Internally allocates a throwaway scope arena. Suitable for sites
/// that genuinely have no surrounding scope (top-level type-param
/// declarations' constraints / defaults, isolated helpers in tests).
/// Production parse paths should prefer [`convert_ts_type_scoped`]
/// with a real `ParseCtx`.
pub fn convert_ts_type(ts_type: &TSType<'_>, diag: &mut DiagnosticCollector) -> TypeRef {
    let mut scratch = ScopeArena::new();
    let root = scratch.create_root();
    convert_ts_type_with_arena(ts_type, root, &mut scratch, diag)
}

/// Convert an oxc `TSType` to our IR `TypeRef`, with a lexical scope
/// rooted at `scope`.
///
/// Every named reference becomes a [`TypeRef::Reference`]; codegen
/// disambiguates declared types from in-scope generic parameters via
/// `scopes.resolve_binding`. The scope is consulted only to manage
/// nested function-type literals that declare their own type
/// parameters — those allocate child scopes so the inner parameter
/// shadows any outer one (TypeScript's standard rules).
pub fn convert_ts_type_scoped(
    ts_type: &TSType<'_>,
    scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> TypeRef {
    convert_ts_type_with_arena(ts_type, scope, ctx.scopes, ctx.diag)
}

/// Internal: walk a `TSType` against a raw scope-arena handle.
///
/// Public converters route through this so they can be called either
/// with a full `ParseCtx` (production paths) or with a throwaway
/// arena (the bare `convert_ts_type` helper).
fn convert_ts_type_with_arena(
    ts_type: &TSType<'_>,
    scope: ScopeId,
    scopes: &mut ScopeArena,
    diag: &mut DiagnosticCollector,
) -> TypeRef {
    let ts_type = ts_type.without_parenthesized();

    match ts_type {
        // === Keyword types ===
        TSType::TSAnyKeyword(_) => TypeRef::Any,
        TSType::TSBooleanKeyword(_) => TypeRef::Boolean,
        TSType::TSBigIntKeyword(_) => TypeRef::BigInt,
        TSType::TSNeverKeyword(_) => TypeRef::Any, // erase `never` to `any`
        TSType::TSNullKeyword(_) => TypeRef::Null,
        TSType::TSNumberKeyword(_) => TypeRef::Number,
        TSType::TSObjectKeyword(_) => TypeRef::Object,
        TSType::TSStringKeyword(_) => TypeRef::String,
        TSType::TSSymbolKeyword(_) => TypeRef::Symbol,
        TSType::TSUndefinedKeyword(_) => TypeRef::Undefined,
        TSType::TSUnknownKeyword(_) => TypeRef::Unknown,
        TSType::TSVoidKeyword(_) => TypeRef::Void,
        TSType::TSIntrinsicKeyword(_) => TypeRef::Any,

        // === Type reference (named type, generic instantiation) ===
        TSType::TSTypeReference(type_ref) => convert_type_reference(type_ref, scope, scopes, diag),

        // === Array types ===
        TSType::TSArrayType(arr) => {
            let inner = convert_ts_type_with_arena(&arr.element_type, scope, scopes, diag);
            TypeRef::Array(Box::new(inner))
        }

        // === Union types ===
        TSType::TSUnionType(union_type) => {
            let types: Vec<TypeRef> = union_type
                .types
                .iter()
                .map(|t| convert_ts_type_with_arena(t, scope, scopes, diag))
                .collect();
            simplify_union(types)
        }

        // === Intersection types ===
        TSType::TSIntersectionType(inter) => {
            let types: Vec<TypeRef> = inter
                .types
                .iter()
                .map(|t| convert_ts_type_with_arena(t, scope, scopes, diag))
                .collect();
            TypeRef::Intersection(types)
        }

        // === Tuple types ===
        TSType::TSTupleType(tuple) => {
            let types: Vec<TypeRef> = tuple
                .element_types
                .iter()
                .map(|elem| convert_tuple_element(elem, scope, scopes, diag))
                .collect();
            TypeRef::Tuple(types)
        }

        // === Function types ===
        TSType::TSFunctionType(func) => {
            let sig = convert_function_type(func, scope, scopes, diag);
            TypeRef::Function(sig)
        }

        // === Literal types ===
        TSType::TSLiteralType(lit) => convert_literal_type(lit, diag),

        // === Type literal (object type) ===
        TSType::TSTypeLiteral(_) => TypeRef::Object,

        // === Constructor type ===
        TSType::TSConstructorType(_) => {
            diag.warn("Constructor types are not supported, erasing to JsValue");
            TypeRef::Any
        }

        // === Conditional types, mapped types, template literals, etc. ===
        TSType::TSConditionalType(_) => {
            diag.warn("Conditional types are not supported, erasing to JsValue");
            TypeRef::Unresolved("conditional type".to_string())
        }
        TSType::TSMappedType(_) => {
            diag.warn("Mapped types are not supported, erasing to JsValue");
            TypeRef::Unresolved("mapped type".to_string())
        }
        TSType::TSTemplateLiteralType(_) => TypeRef::String,
        TSType::TSIndexedAccessType(_) => {
            diag.warn("Indexed access types are not supported, erasing to JsValue");
            TypeRef::Unresolved("indexed access type".to_string())
        }
        TSType::TSInferType(_) => {
            diag.warn("Infer types are not supported, erasing to JsValue");
            TypeRef::Unresolved("infer type".to_string())
        }
        TSType::TSTypeOperatorType(op) => match op.operator {
            TSTypeOperatorOperator::Readonly => {
                convert_ts_type_with_arena(&op.type_annotation, scope, scopes, diag)
            }
            _ => {
                diag.warn_with_source(
                    "Type operator not supported, erasing to JsValue",
                    format!("{:?}", op.operator),
                );
                TypeRef::Unresolved("type operator".to_string())
            }
        },
        TSType::TSTypePredicate(_) => TypeRef::Boolean,
        TSType::TSTypeQuery(_) => {
            diag.warn("typeof type queries are not supported, erasing to JsValue");
            TypeRef::Unresolved("typeof query".to_string())
        }
        TSType::TSImportType(_) => {
            diag.warn("Import types are not supported, erasing to JsValue");
            TypeRef::Unresolved("import type".to_string())
        }
        TSType::TSThisType(_) => TypeRef::Unresolved("this".to_string()),

        // TSNamedTupleMember is also a direct variant of TSType via inherit_variants
        TSType::TSNamedTupleMember(member) => {
            convert_tuple_element(&member.element_type, scope, scopes, diag)
        }

        TSType::TSParenthesizedType(paren) => {
            convert_ts_type_with_arena(&paren.type_annotation, scope, scopes, diag)
        }

        // JSDoc types
        TSType::JSDocNullableType(nullable) => {
            let inner = convert_ts_type_with_arena(&nullable.type_annotation, scope, scopes, diag);
            TypeRef::Nullable(Box::new(inner))
        }
        TSType::JSDocNonNullableType(non_nullable) => {
            convert_ts_type_with_arena(&non_nullable.type_annotation, scope, scopes, diag)
        }
        TSType::JSDocUnknownType(_) => TypeRef::Any,
    }
}

/// Convert a type reference.
///
/// Most named type references — built-in JS classes, user-declared
/// types, in-scope generic parameters, `Promise<T>`, qualified paths
/// — flow through `TypeRef::Reference`. Resolution happens at codegen
/// time against the scope chain (mirroring TypeScript's name lookup):
/// user declarations shadow built-ins, and in-scope generic
/// parameters (recorded as `Binding::TypeParam` in the scope chain)
/// lower to bare Rust idents while declared types lower through the
/// type arena.
///
/// A few constructs *are* desugared at parse time because they're
/// purely TS utility-type sugar with no JS runtime presence:
///
/// * `Boolean` / `Number` / `String` / `Object` / `Symbol` (capital-
///   letter "wrapper" keyword aliases) → primitive variants.
/// * `Partial` / `Required` / ... `Awaited` (utility types that don't
///   produce new runtime types) → identity on their first type
///   argument.
/// * `Function` (the legacy global `Function` type alias) → an
///   `(args) => any` function type.
fn convert_type_reference(
    type_ref: &TSTypeReference<'_>,
    scope: ScopeId,
    scopes: &mut ScopeArena,
    diag: &mut DiagnosticCollector,
) -> TypeRef {
    let (head, segments) = collect_type_name_path(&type_ref.type_name);

    let type_args: Vec<TypeRef> = type_ref
        .type_arguments
        .as_ref()
        .map(|args| {
            args.params
                .iter()
                .map(|t| convert_ts_type_with_arena(t, scope, scopes, diag))
                .collect()
        })
        .unwrap_or_default();

    if segments.is_empty() {
        match head.as_str() {
            "ArrayBufferView" => return TypeRef::ArrayBufferView,
            "Boolean" => return TypeRef::Boolean,
            "Number" => return TypeRef::Number,
            "String" => return TypeRef::String,
            "Object" => return TypeRef::Object,
            "Symbol" => return TypeRef::Symbol,

            "Function" => {
                return TypeRef::Function(crate::ir::FunctionSig {
                    params: vec![],
                    return_type: Box::new(TypeRef::Any),
                });
            }

            "Partial"
            | "Required"
            | "Pick"
            | "Omit"
            | "Exclude"
            | "Extract"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "ConstructorParameters"
            | "InstanceType"
            | "ThisParameterType"
            | "OmitThisParameter"
            | "ThisType"
            | "Awaited" => {
                return type_args.into_iter().next().unwrap_or(TypeRef::Object);
            }
            _ => {}
        }
    }

    // The lexical scope is not consulted here — codegen disambiguates
    // declared types from in-scope generic parameters at emit time.
    let _ = scope;
    let mut path = vec![head];
    path.extend(segments);
    TypeRef::Reference {
        segments: path,
        generic_args: type_args,
    }
}

/// Convert a `TSTypeName` into its head identifier and any qualified
/// path segments (`A.B.C` → `("A", ["B", "C"])`).
fn collect_type_name_path(type_name: &TSTypeName<'_>) -> (String, Vec<String>) {
    match type_name {
        TSTypeName::IdentifierReference(ident) => (ident.name.to_string(), Vec::new()),
        TSTypeName::QualifiedName(qualified) => {
            let (head, mut segments) = collect_type_name_path(&qualified.left);
            segments.push(qualified.right.name.to_string());
            (head, segments)
        }
        TSTypeName::ThisExpression(_) => ("this".to_string(), Vec::new()),
    }
}

/// Convert a tuple element to a `TypeRef`.
fn convert_tuple_element(
    elem: &TSTupleElement<'_>,
    scope: ScopeId,
    scopes: &mut ScopeArena,
    diag: &mut DiagnosticCollector,
) -> TypeRef {
    match elem {
        TSTupleElement::TSNamedTupleMember(member) => {
            convert_tuple_element(&member.element_type, scope, scopes, diag)
        }
        TSTupleElement::TSRestType(rest) => {
            convert_ts_type_with_arena(&rest.type_annotation, scope, scopes, diag)
        }
        TSTupleElement::TSOptionalType(opt) => {
            let inner = convert_ts_type_with_arena(&opt.type_annotation, scope, scopes, diag);
            TypeRef::Nullable(Box::new(inner))
        }
        // All remaining variants are TSType variants flattened by inherit_variants!
        other => match other.as_ts_type() {
            Some(ts_type) => convert_ts_type_with_arena(ts_type, scope, scopes, diag),
            None => {
                diag.warn("Unsupported tuple element type");
                TypeRef::Any
            }
        },
    }
}

/// Convert a `TSFunctionType` to our IR `FunctionSig`.
///
/// Function-type literals may declare their own type parameters
/// (`<T>(x: T) => T`); those go into a fresh child scope so the
/// inner `T` shadows any outer one for the duration of the
/// conversion. The child scope is purely transient — function-type
/// literals have no IR node to attach a `body_scope` to.
fn convert_function_type(
    func: &TSFunctionType<'_>,
    scope: ScopeId,
    scopes: &mut ScopeArena,
    diag: &mut DiagnosticCollector,
) -> crate::ir::FunctionSig {
    let inner_scope = if let Some(tp) = &func.type_parameters {
        let child = scopes.create_child(scope);
        for p in &tp.params {
            scopes.insert_type_param(child, p.name.name.to_string());
        }
        child
    } else {
        scope
    };

    let params = convert_formal_params_with_arena(&func.params, inner_scope, scopes, diag);
    let return_type =
        convert_ts_type_with_arena(&func.return_type.type_annotation, inner_scope, scopes, diag);

    crate::ir::FunctionSig {
        params,
        return_type: Box::new(return_type),
    }
}

/// Convert oxc `FormalParameters` to our IR `Param` list, with no
/// enclosing scope. Use [`convert_formal_params_scoped`] for parsing
/// paths that already have a `ParseCtx` to thread.
pub fn convert_formal_params(
    params: &FormalParameters<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<crate::ir::Param> {
    let mut scratch = ScopeArena::new();
    let root = scratch.create_root();
    convert_formal_params_with_arena(params, root, &mut scratch, diag)
}

/// Convert oxc `FormalParameters` to our IR `Param` list, scoped at
/// the given `ScopeId` for nested type resolution.
pub fn convert_formal_params_scoped(
    params: &FormalParameters<'_>,
    scope: ScopeId,
    ctx: &mut ParseCtx<'_, '_>,
) -> Vec<crate::ir::Param> {
    convert_formal_params_with_arena(params, scope, ctx.scopes, ctx.diag)
}

fn convert_formal_params_with_arena(
    params: &FormalParameters<'_>,
    scope: ScopeId,
    scopes: &mut ScopeArena,
    diag: &mut DiagnosticCollector,
) -> Vec<crate::ir::Param> {
    let mut result = Vec::new();
    for (i, param) in params.items.iter().enumerate() {
        let name = binding_pattern_name(&param.pattern)
            .map(|n| to_snake_case(&n))
            .unwrap_or_else(|| format!("arg{i}"));

        let type_ref = param
            .type_annotation
            .as_ref()
            .map(|ann| convert_ts_type_with_arena(&ann.type_annotation, scope, scopes, diag))
            .unwrap_or(TypeRef::Any);

        result.push(crate::ir::Param {
            name,
            type_ref,
            optional: param.optional,
            variadic: false,
        });
    }

    if let Some(rest) = &params.rest {
        let name = binding_pattern_name(&rest.rest.argument).unwrap_or_else(|| "rest".to_string());

        let type_ref = rest
            .type_annotation
            .as_ref()
            .map(|ann| convert_ts_type_with_arena(&ann.type_annotation, scope, scopes, diag))
            .unwrap_or(TypeRef::Array(Box::new(TypeRef::Any)));

        result.push(crate::ir::Param {
            name,
            type_ref,
            optional: false,
            variadic: true,
        });
    }

    result
}

/// Extract a name from a binding pattern (only handles simple identifier patterns).
pub(crate) fn binding_pattern_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

/// Convert a `TSLiteralType` to our IR `TypeRef`.
fn convert_literal_type(lit: &TSLiteralType<'_>, _diag: &mut DiagnosticCollector) -> TypeRef {
    match &lit.literal {
        TSLiteral::BooleanLiteral(b) => TypeRef::BooleanLiteral(b.value),
        TSLiteral::NumericLiteral(n) => TypeRef::NumberLiteral(n.value),
        TSLiteral::StringLiteral(s) => TypeRef::StringLiteral(s.value.to_string()),
        TSLiteral::BigIntLiteral(_) => TypeRef::BigInt,
        TSLiteral::TemplateLiteral(_) => TypeRef::String,
        TSLiteral::UnaryExpression(_) => TypeRef::Number,
    }
}

/// Simplify a union type at parse time.
///
/// Coalesces `null` / `undefined` arms into the standard
/// `Nullable<T>` wrapper that the rest of the pipeline understands —
/// `string | null` and `string | undefined` and
/// `string | null | undefined` all become `Nullable<String>`.
pub(crate) fn simplify_union(types: Vec<TypeRef>) -> TypeRef {
    let mut non_null_types = Vec::new();
    let mut has_null = false;
    let mut has_undefined = false;

    for ty in types {
        match ty {
            TypeRef::Null => has_null = true,
            TypeRef::Undefined => has_undefined = true,
            TypeRef::Void => has_undefined = true,
            other => non_null_types.push(other),
        }
    }

    let core_type = if non_null_types.len() == 1 {
        non_null_types.pop().unwrap()
    } else if non_null_types.is_empty() {
        return TypeRef::Null;
    } else {
        TypeRef::Union(non_null_types)
    };

    if has_null || has_undefined {
        TypeRef::Nullable(Box::new(core_type))
    } else {
        core_type
    }
}

/// Convert `TSTypeParameterDeclaration` to our IR `TypeParam` list.
pub fn convert_type_params(
    type_params: Option<&oxc_allocator::Box<'_, TSTypeParameterDeclaration<'_>>>,
    diag: &mut DiagnosticCollector,
) -> Vec<crate::ir::TypeParam> {
    type_params
        .map(|tp| {
            tp.params
                .iter()
                .map(|p| crate::ir::TypeParam {
                    name: p.name.to_string(),
                    constraint: p.constraint.as_ref().map(|c| convert_ts_type(c, diag)),
                    default: p.default.as_ref().map(|d| convert_ts_type(d, diag)),
                })
                .collect()
        })
        .unwrap_or_default()
}
