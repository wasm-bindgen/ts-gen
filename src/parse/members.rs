//! Convert oxc class/interface member AST nodes to our IR `Member`.

use oxc_ast::ast::*;

use std::collections::HashSet;

use crate::ir::*;
use crate::parse::docs::{DocComments, JsDocInfo};
use crate::parse::first_pass::converters::interface_from_signatures;
use crate::parse::types::{
    binding_pattern_name, convert_formal_params, convert_ts_type, convert_ts_type_scoped,
    convert_type_params,
};
use crate::util::diagnostics::DiagnosticCollector;
use crate::util::naming::{to_pascal_case, to_snake_case};

/// Split the result of [`DocComments::info_for_span`] into a `(doc, info)`
/// pair, defaulting to empty info when no JSDoc is attached.
fn split_info(opt: Option<(String, JsDocInfo)>) -> (Option<String>, JsDocInfo) {
    match opt {
        Some((doc, info)) => (Some(doc), info),
        None => (None, JsDocInfo::default()),
    }
}

/// Like `convert_formal_params`, but additionally hoists any directly-
/// inline `TSTypeLiteral` parameter types into named `InterfaceDecl`s
/// using the existing interface-building pipeline (`interface_from_signatures`).
///
/// The synthesized name is `<Parent><Member>` PascalCased, deduped
/// against `used_type_names` with a numeric suffix on collision. The
/// resulting `InterfaceDecl`s are returned alongside the params for the
/// caller to append to its declarations sink.
///
/// Anonymous types nested inside generics, unions, etc. (rather than
/// flat at the top of the parameter type) are not hoisted — those still
/// erase to `Object` per the existing `convert_ts_type` semantics. Real-
/// world `.d.ts` patterns put the literal at the top level (e.g.
/// `send(builder: { ... })`); deeper hoisting is a follow-up.
pub(crate) fn convert_formal_params_with_synthesis(
    params: &FormalParameters<'_>,
    parent_name: &str,
    member_name: &str,
    used_type_names: &mut HashSet<String>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> (Vec<Param>, Vec<InterfaceDecl>) {
    let mut result_params = Vec::new();
    let mut synthesized = Vec::new();

    for (i, param) in params.items.iter().enumerate() {
        // Capture the original (un-snake_cased) parameter name first so we
        // can use it as the synthesized type's name segment. The Rust
        // param name still goes through snake_case below.
        let raw_param_name = binding_pattern_name(&param.pattern);
        let name = raw_param_name
            .as_deref()
            .map(to_snake_case)
            .unwrap_or_else(|| format!("arg{i}"));

        let type_ref = match param.type_annotation.as_ref() {
            Some(ann) => match try_synthesize_inline_param(
                &ann.type_annotation,
                parent_name,
                raw_param_name.as_deref().unwrap_or(member_name),
                used_type_names,
                &mut synthesized,
                docs,
                diag,
            ) {
                Some(synth) => synth,
                None => convert_ts_type(&ann.type_annotation, diag),
            },
            None => TypeRef::Any,
        };

        let optional = param.optional;
        result_params.push(Param {
            name,
            type_ref,
            optional,
            variadic: false,
        });
    }

    if let Some(rest) = &params.rest {
        let name = binding_pattern_name(&rest.rest.argument).unwrap_or_else(|| "rest".to_string());
        let type_ref = rest
            .type_annotation
            .as_ref()
            .map(|ann| convert_ts_type(&ann.type_annotation, diag))
            .unwrap_or(TypeRef::Array(Box::new(TypeRef::Any)));
        result_params.push(Param {
            name,
            type_ref,
            optional: false,
            variadic: true,
        });
    }

    (result_params, synthesized)
}

/// Compute a unique synthesized type name for an anonymous interface
/// hoisted from `<Parent>.<member>(...)`. Falls back to numeric suffixes
/// on collision (`Foo`, `Foo2`, `Foo3`, …).
fn unique_type_name(parent: &str, member: &str, used: &HashSet<String>) -> String {
    let base = format!("{}{}", parent, to_pascal_case(member));
    if !used.contains(&base) {
        return base;
    }
    for i in 2.. {
        let candidate = format!("{base}{i}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("HashSet exhaustion is impossible in practice");
}

/// Try to synthesize an anonymous-interface hoist for a parameter type.
///
/// Recognised shapes:
///
/// * Bare `{ ... }` — directly hoisted into a single interface whose
///   members are the literal's members.
/// * `{ ... } | { ... } | …` where every union branch is itself a type
///   literal — structurally merged into a single interface (see
///   [`merge_member_branches`]).
///
/// Anything else returns `None` so the caller falls back to the regular
/// type-mapping rules.
#[allow(clippy::too_many_arguments)]
fn try_synthesize_inline_param(
    ts_type: &TSType<'_>,
    parent_name: &str,
    segment: &str,
    used_type_names: &mut HashSet<String>,
    synth: &mut Vec<InterfaceDecl>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Option<TypeRef> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => {
            let synth_name = unique_type_name(parent_name, segment, used_type_names);
            used_type_names.insert(synth_name.clone());
            // Methods inside the hoisted interface may carry their own
            // anonymous parameter types — recurse and let those land in
            // `synth` alongside the parent.
            let iface = interface_from_signatures(
                synth_name.clone(),
                Vec::new(),
                Vec::new(),
                &literal.members,
                used_type_names,
                synth,
                docs,
                diag,
            );
            synth.push(iface);
            Some(TypeRef::Named(synth_name))
        }
        TSType::TSUnionType(union) if all_type_literals(&union.types) => {
            let synth_name = unique_type_name(parent_name, segment, used_type_names);
            used_type_names.insert(synth_name.clone());
            // Convert each branch through the regular signature pipeline
            // first, then structurally merge at the IR level.
            let branches: Vec<Vec<Member>> = union
                .types
                .iter()
                .filter_map(|t| match t {
                    TSType::TSTypeLiteral(lit) => Some(
                        lit.members
                            .iter()
                            .flat_map(|sig| {
                                convert_ts_signature(
                                    sig,
                                    Some(&synth_name),
                                    used_type_names,
                                    synth,
                                    docs,
                                    diag,
                                )
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .collect();
            let merged = crate::parse::literal_union::merge_member_branches(&branches);
            let classification = crate::parse::classify::classify_interface(&merged);
            synth.push(InterfaceDecl {
                name: synth_name.clone(),
                js_name: synth_name.clone(),
                type_params: Vec::new(),
                extends: Vec::new(),
                members: merged,
                classification,
            });
            Some(TypeRef::Named(synth_name))
        }
        _ => None,
    }
}

/// Return true when every entry in a union is a type literal — the
/// signal for "this is an anonymous-interface union we can merge."
fn all_type_literals(types: &[TSType<'_>]) -> bool {
    !types.is_empty() && types.iter().all(|t| matches!(t, TSType::TSTypeLiteral(_)))
}

/// Convert a `TSSignature` (interface body member) to our IR `Member`(s).
///
/// `parent` is the surrounding type's Rust name when one is available
/// — passed down so that anonymous parameter types inside method
/// signatures can be hoisted into named interfaces (see
/// [`convert_formal_params_with_synthesis`]). `synth` is the sink those
/// hoisted interfaces are appended to. Pass `None` / a throwaway sink
/// when you don't have parent context (no synthesis happens).
pub fn convert_ts_signature(
    sig: &TSSignature<'_>,
    parent: Option<&str>,
    used_type_names: &mut HashSet<String>,
    synth: &mut Vec<InterfaceDecl>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<Member> {
    match sig {
        TSSignature::TSPropertySignature(prop) => convert_property_signature(prop, docs, diag),
        TSSignature::TSMethodSignature(method) => {
            convert_method_signature(method, parent, used_type_names, synth, docs, diag)
        }
        TSSignature::TSIndexSignature(idx) => {
            convert_index_signature(idx, diag).into_iter().collect()
        }
        TSSignature::TSConstructSignatureDeclaration(ctor) => {
            convert_construct_signature(ctor, parent, used_type_names, synth, docs, diag)
                .into_iter()
                .collect()
        }
        TSSignature::TSCallSignatureDeclaration(_) => {
            diag.warn("Call signatures on interfaces are not supported, skipping");
            vec![]
        }
    }
}

/// Convert a `ClassElement` (class body member) to our IR `Member`(s).
///
/// See [`convert_ts_signature`] for the meaning of `parent` / `synth`.
pub fn convert_class_element(
    elem: &ClassElement<'_>,
    parent: Option<&str>,
    used_type_names: &mut HashSet<String>,
    synth: &mut Vec<InterfaceDecl>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<Member> {
    match elem {
        ClassElement::MethodDefinition(method) => {
            convert_class_method(method, parent, used_type_names, synth, docs, diag)
        }
        ClassElement::PropertyDefinition(prop) => convert_class_property(prop, docs, diag),
        ClassElement::AccessorProperty(acc) => convert_accessor_property(acc, docs, diag),
        ClassElement::TSIndexSignature(idx) => {
            convert_index_signature(idx, diag).into_iter().collect()
        }
        ClassElement::StaticBlock(_) => vec![],
    }
}

// ─── Interface member conversions ────────────────────────────────────

fn convert_property_signature(
    prop: &TSPropertySignature<'_>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<Member> {
    let js_name = match property_key_name(&prop.key) {
        Some(n) => n,
        None => return vec![],
    };
    let doc = docs.for_span(prop.span.start);

    let type_ref = prop
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type(&ann.type_annotation, diag))
        .unwrap_or(TypeRef::Any);

    let mut members = vec![Member::Getter(GetterMember {
        js_name: js_name.clone(),
        type_ref: type_ref.clone(),
        optional: prop.optional,
        doc,
    })];

    if !prop.readonly {
        members.push(Member::Setter(SetterMember {
            js_name,
            type_ref,
            doc: None,
        }));
    }

    members
}

fn convert_method_signature(
    method: &TSMethodSignature<'_>,
    parent: Option<&str>,
    used_type_names: &mut HashSet<String>,
    synth: &mut Vec<InterfaceDecl>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<Member> {
    let js_name = match property_key_name(&method.key) {
        Some(n) => n,
        None => return vec![],
    };
    let (doc, info) = split_info(docs.info_for_span(method.span.start));

    let type_params = convert_type_params(method.type_parameters.as_ref(), diag);

    // Build scope from method type parameters so references like `T` in
    // `json<T>(): Promise<T>` get erased to Any instead of Named("T")
    let scope: HashSet<&str> = method
        .type_parameters
        .as_ref()
        .map(|tp| tp.params.iter().map(|p| p.name.name.as_str()).collect())
        .unwrap_or_default();

    // Hoist anonymous `{ ... }` parameter types into named interfaces when
    // we know the surrounding parent name. Without `parent` we can't
    // generate a sensible name, so fall back to the regular path that
    // erases inline objects to `Object`.
    let params = match parent {
        Some(p) => {
            let (params, more_synth) = convert_formal_params_with_synthesis(
                &method.params,
                p,
                &js_name,
                used_type_names,
                docs,
                diag,
            );
            synth.extend(more_synth);
            params
        }
        None => convert_formal_params(&method.params, diag),
    };
    let return_type = method
        .return_type
        .as_ref()
        .map(|rt| convert_ts_type_scoped(&rt.type_annotation, &scope, diag))
        .unwrap_or(TypeRef::Void);

    match method.kind {
        TSMethodSignatureKind::Get => vec![Member::Getter(GetterMember {
            js_name,
            type_ref: return_type,
            optional: method.optional,
            doc,
        })],
        TSMethodSignatureKind::Set => {
            let type_ref = params
                .into_iter()
                .next()
                .map(|p| p.type_ref)
                .unwrap_or(TypeRef::Any);
            vec![Member::Setter(SetterMember {
                js_name,
                type_ref,
                doc,
            })]
        }
        TSMethodSignatureKind::Method => vec![Member::Method(MethodMember {
            name: crate::util::naming::to_snake_case(&js_name),
            js_name,
            type_params,
            params,
            return_type,
            optional: method.optional,
            doc,
            throws: info.throws_typeref(),
        })],
    }
}

fn convert_index_signature(
    idx: &TSIndexSignature<'_>,
    diag: &mut DiagnosticCollector,
) -> Option<Member> {
    let key_type = idx
        .parameters
        .first()
        .map(|p| convert_ts_type(&p.type_annotation.type_annotation, diag))
        .unwrap_or(TypeRef::String);

    // type_annotation is Box<TSTypeAnnotation> (not Option) in oxc 0.118
    let value_type = convert_ts_type(&idx.type_annotation.type_annotation, diag);

    Some(Member::IndexSignature(IndexSigMember {
        key_type,
        value_type,
        readonly: idx.readonly,
    }))
}

fn convert_construct_signature(
    ctor: &TSConstructSignatureDeclaration<'_>,
    parent: Option<&str>,
    used_type_names: &mut HashSet<String>,
    synth: &mut Vec<InterfaceDecl>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Option<Member> {
    // Constructors hoist anonymous parameter types under `<Parent>Constructor`
    // when a parent is known — matches the convention of using a method-like
    // name segment for the synthesized type.
    let params = match parent {
        Some(p) => {
            let (params, more_synth) = convert_formal_params_with_synthesis(
                &ctor.params,
                p,
                "Constructor",
                used_type_names,
                docs,
                diag,
            );
            synth.extend(more_synth);
            params
        }
        None => convert_formal_params(&ctor.params, diag),
    };
    let (doc, info) = split_info(docs.info_for_span(ctor.span.start));
    Some(Member::Constructor(ConstructorMember {
        params,
        doc,
        throws: info.throws_typeref(),
    }))
}

// ─── Class member conversions ────────────────────────────────────────

fn convert_class_method(
    method: &MethodDefinition<'_>,
    parent: Option<&str>,
    used_type_names: &mut HashSet<String>,
    synth: &mut Vec<InterfaceDecl>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<Member> {
    let js_name = match property_key_name(&method.key) {
        Some(n) => n,
        None => return vec![],
    };
    let (doc, info) = split_info(docs.info_for_span(method.span.start));

    let func = &method.value;
    let type_params = convert_type_params(func.type_parameters.as_ref(), diag);

    // Build scope from method type parameters
    let scope: HashSet<&str> = func
        .type_parameters
        .as_ref()
        .map(|tp| tp.params.iter().map(|p| p.name.name.as_str()).collect())
        .unwrap_or_default();

    let params = match parent {
        Some(p) => {
            // Constructors use a special "Constructor" segment so the
            // synthesized type reads `<Parent>Constructor*` rather than
            // `<Parent>` alone (which would clash with the parent itself).
            let member_name = match method.kind {
                MethodDefinitionKind::Constructor => "Constructor".to_string(),
                _ => js_name.clone(),
            };
            let (params, more_synth) = convert_formal_params_with_synthesis(
                &func.params,
                p,
                &member_name,
                used_type_names,
                docs,
                diag,
            );
            synth.extend(more_synth);
            params
        }
        None => convert_formal_params(&func.params, diag),
    };
    let return_type = func
        .return_type
        .as_ref()
        .map(|rt| convert_ts_type_scoped(&rt.type_annotation, &scope, diag))
        .unwrap_or(TypeRef::Void);

    let is_static = method.r#static;

    match method.kind {
        MethodDefinitionKind::Constructor => {
            vec![Member::Constructor(ConstructorMember {
                params,
                doc,
                throws: info.throws_typeref(),
            })]
        }
        MethodDefinitionKind::Get => {
            if is_static {
                vec![Member::StaticGetter(StaticGetterMember {
                    js_name,
                    type_ref: return_type,
                    doc,
                })]
            } else {
                vec![Member::Getter(GetterMember {
                    js_name,
                    type_ref: return_type,
                    optional: method.optional,
                    doc,
                })]
            }
        }
        MethodDefinitionKind::Set => {
            let type_ref = params
                .into_iter()
                .next()
                .map(|p| p.type_ref)
                .unwrap_or(TypeRef::Any);
            if is_static {
                vec![Member::StaticSetter(StaticSetterMember {
                    js_name,
                    type_ref,
                    doc,
                })]
            } else {
                vec![Member::Setter(SetterMember {
                    js_name,
                    type_ref,
                    doc,
                })]
            }
        }
        MethodDefinitionKind::Method => {
            if is_static {
                vec![Member::StaticMethod(StaticMethodMember {
                    name: crate::util::naming::to_snake_case(&js_name),
                    js_name,
                    type_params,
                    params,
                    return_type,
                    doc,
                    throws: info.throws_typeref(),
                })]
            } else {
                vec![Member::Method(MethodMember {
                    name: crate::util::naming::to_snake_case(&js_name),
                    js_name,
                    type_params,
                    params,
                    return_type,
                    optional: method.optional,
                    doc,
                    throws: info.throws_typeref(),
                })]
            }
        }
    }
}

fn convert_class_property(
    prop: &PropertyDefinition<'_>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<Member> {
    let js_name = match property_key_name(&prop.key) {
        Some(n) => n,
        None => return vec![],
    };
    let doc = docs.for_span(prop.span.start);

    let type_ref = prop
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type(&ann.type_annotation, diag))
        .unwrap_or(TypeRef::Any);

    if prop.r#static {
        let mut members = vec![Member::StaticGetter(StaticGetterMember {
            js_name: js_name.clone(),
            type_ref: type_ref.clone(),
            doc,
        })];
        if !prop.readonly {
            members.push(Member::StaticSetter(StaticSetterMember {
                js_name,
                type_ref,
                doc: None,
            }));
        }
        members
    } else {
        let mut members = vec![Member::Getter(GetterMember {
            js_name: js_name.clone(),
            type_ref: type_ref.clone(),
            optional: prop.optional,
            doc,
        })];
        if !prop.readonly {
            members.push(Member::Setter(SetterMember {
                js_name,
                type_ref,
                doc: None,
            }));
        }
        members
    }
}

fn convert_accessor_property(
    acc: &AccessorProperty<'_>,
    docs: &DocComments<'_>,
    diag: &mut DiagnosticCollector,
) -> Vec<Member> {
    let js_name = match property_key_name(&acc.key) {
        Some(n) => n,
        None => return vec![],
    };
    let doc = docs.for_span(acc.span.start);

    let type_ref = acc
        .type_annotation
        .as_ref()
        .map(|ann| convert_ts_type(&ann.type_annotation, diag))
        .unwrap_or(TypeRef::Any);

    if acc.r#static {
        vec![
            Member::StaticGetter(StaticGetterMember {
                js_name: js_name.clone(),
                type_ref: type_ref.clone(),
                doc,
            }),
            Member::StaticSetter(StaticSetterMember {
                js_name,
                type_ref,
                doc: None,
            }),
        ]
    } else {
        vec![
            Member::Getter(GetterMember {
                js_name: js_name.clone(),
                type_ref: type_ref.clone(),
                optional: false,
                doc,
            }),
            Member::Setter(SetterMember {
                js_name,
                type_ref,
                doc: None,
            }),
        ]
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Extract a string name from a `PropertyKey`.
pub fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(ident) => Some(ident.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.value.to_string()),
        _ => None,
    }
}
