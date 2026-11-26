//! Interface classification: determine if an interface is class-like or a dictionary.

use std::collections::HashSet;

use crate::context::{GlobalContext, TypeId};
use crate::ir::*;

/// Classify an interface based on its members.
///
/// - **Dictionary**: Only has properties (getters/setters), no methods — gets
///   builder pattern and property accessors. Includes interfaces with required
///   properties (the builder validates them at runtime).
/// - **ClassLike**: Has methods, constructors, or static members
/// - **Unclassified**: Empty or unclear
pub fn classify_interface(members: &[Member]) -> InterfaceClassification {
    if members.is_empty() {
        return InterfaceClassification::Unclassified;
    }

    let mut has_methods = false;
    let mut has_properties = false;

    for member in members {
        match member {
            Member::Method(_) | Member::StaticMethod(_) => {
                has_methods = true;
            }
            Member::Constructor(_) => {
                has_methods = true;
            }
            Member::Getter(_) | Member::Setter(_) => {
                has_properties = true;
            }
            Member::StaticGetter(_) | Member::StaticSetter(_) => {
                has_methods = true;
            }
            Member::IndexSignature(_) => {}
        }
    }

    if has_methods {
        InterfaceClassification::ClassLike
    } else if has_properties {
        InterfaceClassification::Dictionary
    } else {
        InterfaceClassification::Unclassified
    }
}

/// Makes optional the required dictionary fields a parameter fixes to one
/// string literal through a type argument (`options: GetOptions<"text">`
/// over `interface GetOptions<Type> { type: Type }`): the call fills the
/// field, so a caller builds the dictionary without it. A type parameter
/// bounded by literals has its enums instead.
pub(crate) fn literal_fields_optional(gctx: &mut GlobalContext) {
    let mut fixed: HashSet<(TypeId, usize)> = HashSet::new();
    let mut params = |gctx: &GlobalContext, params: &[Param], scope| {
        for p in params {
            let ty = p
                .type_ref
                .split_empties()
                .map_or_else(|| p.type_ref.clone(), |(t, _)| t);
            let TypeRef::Reference {
                segments,
                generic_args,
            } = &ty
            else {
                continue;
            };
            if !generic_args
                .iter()
                .any(|a| matches!(a, TypeRef::StringLiteral(_)))
            {
                continue;
            }
            let Some(id) = gctx.resolve_path(scope, &segments.join(".")) else {
                continue;
            };
            for (index, a) in generic_args.iter().enumerate() {
                if matches!(a, TypeRef::StringLiteral(_)) {
                    fixed.insert((id, index));
                }
            }
        }
    };
    for (_, decl) in gctx.iter_types() {
        let (members, construct): (&[Member], &[ConstructorMember]) = match &decl.kind {
            TypeKind::Class(c) => (&c.members, &[]),
            TypeKind::Interface(i) => (&i.members, &i.construct),
            TypeKind::Function(f) => {
                params(gctx, &f.params, f.body_scope);
                for o in &f.overloads {
                    params(gctx, &o.params, o.body_scope);
                }
                continue;
            }
            _ => continue,
        };
        for m in members {
            match m {
                Member::Method(m) => params(gctx, &m.params, m.body_scope),
                Member::StaticMethod(m) => params(gctx, &m.params, m.body_scope),
                Member::Constructor(c) => params(gctx, &c.params, c.scope),
                _ => {}
            }
        }
        for c in construct {
            params(gctx, &c.params, c.scope);
        }
    }
    for (id, index) in fixed {
        let TypeKind::Interface(i) = &mut gctx.get_type_mut(id).kind else {
            continue;
        };
        if !matches!(i.classification, InterfaceClassification::Dictionary) {
            continue;
        }
        let Some(name) = i
            .type_params
            .get(index)
            .filter(|p| p.constraint.is_none())
            .map(|p| p.name.clone())
        else {
            continue;
        };
        let mut filled = Vec::new();
        for m in &mut i.members {
            if let Member::Getter(g) = m {
                if is_type_param(&g.type_ref, &name) && !g.optional {
                    g.optional = true;
                    filled.push(g.js_name.clone());
                }
            }
        }
        gctx.literal_fills
            .extend(filled.into_iter().map(|field| (id, field)));
    }
}

/// Reads an interface's optional methods as optional properties of
/// function type (`element?(e: Element): void` as `element?: (e: Element)
/// => void`), which TypeScript takes one for the other. Only where the
/// interface is then properties alone, so it's an object a caller writes:
/// beside a required method the object is one to call through, and its
/// optional methods stay methods on it.
pub(crate) fn optional_methods_as_properties(gctx: &mut GlobalContext) {
    let ids: Vec<TypeId> = gctx
        .iter_types()
        .filter(|(id, decl)| {
            matches!(&decl.kind, TypeKind::Interface(i)
                if i.members.iter().any(|m| matches!(m, Member::Method(_))))
                && properties_alone(gctx, *id, &mut HashSet::new())
        })
        .map(|(id, _)| id)
        .collect();
    for id in ids {
        let TypeKind::Interface(iface) = &gctx.get_type(id).kind else {
            continue;
        };
        let mut members = Vec::new();
        for m in &iface.members {
            let Member::Method(m) = m else {
                members.push(m.clone());
                continue;
            };
            // The method's own scope binds nothing, so its types resolve
            // in the body it's written in.
            let scope = gctx.scopes.get(m.body_scope).parent.unwrap_or(m.body_scope);
            let type_ref = TypeRef::Function(FunctionSig {
                params: m.params.clone(),
                return_type: Box::new(m.return_type.clone()),
            });
            members.push(Member::Getter(GetterMember {
                scope,
                js_name: m.js_name.clone(),
                type_ref: type_ref.clone(),
                optional: true,
                doc: m.doc.clone(),
                throws: Throws::None,
            }));
            members.push(Member::Setter(SetterMember {
                scope,
                js_name: m.js_name.clone(),
                type_ref,
                doc: None,
                slice_mode: None,
            }));
        }
        if let TypeKind::Interface(iface) = &mut gctx.get_type_mut(id).kind {
            iface.classification = classify_interface(&members);
            iface.members = members;
        }
    }
}

/// Whether an interface is properties alone once its optional methods read
/// as properties: no required, generic or overloaded method, nothing
/// static or constructed, and no parent with one.
fn properties_alone(gctx: &GlobalContext, id: TypeId, seen: &mut HashSet<TypeId>) -> bool {
    if !seen.insert(id) {
        return true;
    }
    let iface = match &gctx.get_type(id).kind {
        TypeKind::Interface(i) => i,
        TypeKind::Class(_) => return false,
        _ => return true,
    };
    let mut names = HashSet::new();
    let own = iface.construct.is_empty()
        && iface.members.iter().all(|m| match m {
            Member::Getter(g) => names.insert(&g.js_name),
            Member::Setter(_) | Member::IndexSignature(_) => true,
            Member::Method(m) => m.optional && m.type_params.is_empty() && names.insert(&m.js_name),
            _ => false,
        });
    own && iface.extends.iter().all(|h| match &h.type_ref {
        TypeRef::Reference { segments, .. } => gctx
            .resolve_path(h.scope, &segments.join("."))
            .is_none_or(|parent| properties_alone(gctx, parent, seen)),
        _ => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn getter(name: &str, optional: bool) -> Member {
        Member::Getter(GetterMember {
            scope: crate::parse::scope::ScopeId::DUMMY,
            js_name: name.to_string(),
            type_ref: TypeRef::Any,
            optional,
            doc: None,
            throws: crate::ir::Throws::None,
        })
    }

    fn static_getter(name: &str) -> Member {
        Member::StaticGetter(StaticGetterMember {
            scope: crate::parse::scope::ScopeId::DUMMY,
            js_name: name.to_string(),
            type_ref: TypeRef::Any,
            optional: false,
            doc: None,
            throws: crate::ir::Throws::None,
        })
    }

    fn method(name: &str) -> Member {
        Member::Method(MethodMember {
            name: name.to_string(),
            js_name: name.to_string(),
            type_params: vec![],
            params: vec![],
            return_type: TypeRef::Void,
            optional: false,
            doc: None,
            throws: crate::ir::Throws::None,
            async_modifier: false,
            body_scope: crate::parse::scope::ScopeId::DUMMY,
        })
    }

    #[test]
    fn empty_is_unclassified() {
        assert_eq!(
            classify_interface(&[]),
            InterfaceClassification::Unclassified
        );
    }

    #[test]
    fn all_optional_getters_is_dictionary() {
        let members = vec![getter("foo", true), getter("bar", true)];
        assert_eq!(
            classify_interface(&members),
            InterfaceClassification::Dictionary
        );
    }

    #[test]
    fn required_getter_is_dictionary() {
        // Properties-only interfaces are dictionaries regardless of optionality.
        // The builder validates required properties at runtime.
        let members = vec![getter("foo", false)];
        assert_eq!(
            classify_interface(&members),
            InterfaceClassification::Dictionary
        );
    }

    #[test]
    fn methods_is_class_like() {
        let members = vec![method("do_thing")];
        assert_eq!(
            classify_interface(&members),
            InterfaceClassification::ClassLike
        );
    }

    #[test]
    fn static_getter_is_class_like() {
        let members = vec![static_getter("instance")];
        assert_eq!(
            classify_interface(&members),
            InterfaceClassification::ClassLike
        );
    }

    #[test]
    fn optional_getters_with_static_is_class_like() {
        // Regression: static properties should prevent Dictionary classification
        let members = vec![getter("foo", true), static_getter("bar")];
        assert_eq!(
            classify_interface(&members),
            InterfaceClassification::ClassLike
        );
    }
}
