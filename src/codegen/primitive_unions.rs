//! TypeScript primitive unions → `js_sys` primitive-union marker traits.
//!
//! `js-sys` exposes one experimental marker trait for every union of two or
//! more of the value primitives `bigint`, `boolean`, `number`, `string`, and
//! `symbol` (`JsNumberOrStringLike` for `number | string`, …). Under
//! `experimental_generic_mono` an import parameter bounded by one of them
//! accepts every Rust representation of every member while keeping each
//! instantiation's native ABI, so a primitive union parameter becomes one
//! `impl Trait` binding instead of one binding per member.
//!
//! The traits are only meaningful on per-monomorphization imports; the
//! type-erased path keeps its per-member fan-out.

use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::TypeRef;

/// One TypeScript value-primitive category. Declaration order is the
/// alphabetical order `js-sys` uses to spell trait names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PrimitiveCategory {
    BigInt,
    Boolean,
    Number,
    String,
    Symbol,
}

impl PrimitiveCategory {
    /// Category of a primitive keyword or literal. `PropertyKey` spans
    /// several categories and is handled by [`leaf_categories`].
    fn of(ty: &TypeRef) -> Option<Self> {
        match ty {
            TypeRef::BigInt => Some(Self::BigInt),
            TypeRef::Boolean | TypeRef::BooleanLiteral(_) => Some(Self::Boolean),
            TypeRef::Number | TypeRef::NumberLiteral(_) => Some(Self::Number),
            TypeRef::String | TypeRef::StringLiteral(_) => Some(Self::String),
            TypeRef::Symbol => Some(Self::Symbol),
            _ => None,
        }
    }

    fn trait_segment(self) -> &'static str {
        match self {
            Self::BigInt => "BigInt",
            Self::Boolean => "Boolean",
            Self::Number => "Number",
            Self::String => "String",
            Self::Symbol => "Symbol",
        }
    }

    fn suffix_segment(self) -> &'static str {
        match self {
            Self::BigInt => "big_int",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::String => "string",
            Self::Symbol => "symbol",
        }
    }
}

const PROPERTY_KEY_CATEGORIES: [PrimitiveCategory; 3] = [
    PrimitiveCategory::Number,
    PrimitiveCategory::String,
    PrimitiveCategory::Symbol,
];

fn leaf_categories(ty: &TypeRef) -> Option<&'static [PrimitiveCategory]> {
    const BIGINT: &[PrimitiveCategory] = &[PrimitiveCategory::BigInt];
    const BOOLEAN: &[PrimitiveCategory] = &[PrimitiveCategory::Boolean];
    const NUMBER: &[PrimitiveCategory] = &[PrimitiveCategory::Number];
    const STRING: &[PrimitiveCategory] = &[PrimitiveCategory::String];
    const SYMBOL: &[PrimitiveCategory] = &[PrimitiveCategory::Symbol];
    if matches!(ty, TypeRef::PropertyKey) {
        return Some(&PROPERTY_KEY_CATEGORIES);
    }
    Some(match PrimitiveCategory::of(ty)? {
        PrimitiveCategory::BigInt => BIGINT,
        PrimitiveCategory::Boolean => BOOLEAN,
        PrimitiveCategory::Number => NUMBER,
        PrimitiveCategory::String => STRING,
        PrimitiveCategory::Symbol => SYMBOL,
    })
}

fn is_literal(ty: &TypeRef) -> bool {
    matches!(
        ty,
        TypeRef::StringLiteral(_) | TypeRef::NumberLiteral(_) | TypeRef::BooleanLiteral(_)
    )
}

/// A union of at least two primitive categories, lowered to one marker trait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrimitiveUnion {
    categories: BTreeSet<PrimitiveCategory>,
    /// The source spelled the whole union as TypeScript's `PropertyKey`,
    /// which maps to the `js_sys::PropertyKey` alias rather than its
    /// canonical `JsNumberOrStringOrSymbolLike` name.
    property_key: bool,
}

impl PrimitiveUnion {
    /// Classify a grouped alternative produced by [`group_alternatives`]:
    /// `PropertyKey`, or a `Union` whose members are all primitive leaves
    /// spanning at least two categories. Aliases must already be resolved.
    pub(crate) fn classify(ty: &TypeRef) -> Option<Self> {
        match ty {
            TypeRef::PropertyKey => Some(Self {
                categories: PROPERTY_KEY_CATEGORIES.into_iter().collect(),
                property_key: true,
            }),
            TypeRef::Union(members) => {
                let mut categories = BTreeSet::new();
                for member in members {
                    categories.extend(leaf_categories(member)?.iter().copied());
                }
                (categories.len() >= 2).then_some(Self {
                    categories,
                    property_key: false,
                })
            }
            _ => None,
        }
    }

    /// `::js_sys::JsNumberOrStringLike`, or `::js_sys::PropertyKey` when
    /// the source used that spelling.
    pub(crate) fn trait_path(&self) -> TokenStream {
        if self.property_key {
            return quote! { ::js_sys::PropertyKey };
        }
        let name = self
            .categories
            .iter()
            .map(|c| c.trait_segment())
            .collect::<Vec<_>>()
            .join("Or");
        let ident = syn::Ident::new(&format!("Js{name}Like"), proc_macro2::Span::call_site());
        quote! { ::js_sys::#ident }
    }

    /// `_with_<suffix>` disambiguator: the TypeScript member names joined
    /// by `_or_`, mirroring the trait name.
    pub(crate) fn suffix(&self) -> String {
        if self.property_key {
            return "property_key".to_string();
        }
        self.categories
            .iter()
            .map(|c| c.suffix_segment())
            .collect::<Vec<_>>()
            .join("_or_")
    }

    /// Whether a value of type `ty` (a primitive leaf or another grouped
    /// union) is accepted by this union's trait.
    pub(crate) fn accepts(&self, ty: &TypeRef) -> bool {
        let categories: Vec<PrimitiveCategory> = match Self::classify(ty) {
            Some(other) => other.categories.into_iter().collect(),
            None => match leaf_categories(ty) {
                Some(leaf) => leaf.to_vec(),
                None => return false,
            },
        };
        categories.iter().all(|c| self.categories.contains(c))
    }
}

/// Merge the primitive leaves of a flattened alternative list into one
/// grouped alternative when they span at least two categories.
///
/// The grouped alternative takes the position of the first primitive leaf;
/// every non-primitive alternative keeps its own slot, so `string | number |
/// Foo` becomes `[string | number, Foo]`. With `keep_literals`, literal
/// leaves stay separate alternatives (dictionary factories turn them into
/// `new_<literal>` constructors) and only the non-literal leaves group.
pub(crate) fn group_alternatives(alts: Vec<TypeRef>, keep_literals: bool) -> Vec<TypeRef> {
    let is_candidate =
        |ty: &TypeRef| leaf_categories(ty).is_some() && !(keep_literals && is_literal(ty));
    let categories: BTreeSet<PrimitiveCategory> = alts
        .iter()
        .filter(|ty| is_candidate(ty))
        .flat_map(|ty| leaf_categories(ty).unwrap().iter().copied())
        .collect();
    if categories.len() < 2 {
        return alts;
    }

    let candidates: Vec<&TypeRef> = alts.iter().filter(|ty| is_candidate(ty)).collect();
    let grouped = match candidates.as_slice() {
        [single] => (*single).clone(),
        many => TypeRef::Union(many.iter().map(|ty| (*ty).clone()).collect()),
    };

    let mut out = Vec::with_capacity(alts.len());
    let mut grouped = Some(grouped);
    for ty in alts {
        if is_candidate(&ty) {
            if let Some(g) = grouped.take() {
                out.push(g);
            }
        } else {
            out.push(ty);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn union(members: &[TypeRef]) -> TypeRef {
        TypeRef::Union(members.to_vec())
    }

    fn trait_name(ty: &TypeRef) -> String {
        PrimitiveUnion::classify(ty)
            .unwrap()
            .trait_path()
            .to_string()
    }

    #[test]
    fn trait_names_list_categories_alphabetically() {
        assert_eq!(
            trait_name(&union(&[TypeRef::String, TypeRef::Number])),
            ":: js_sys :: JsNumberOrStringLike"
        );
        assert_eq!(
            trait_name(&union(&[
                TypeRef::Symbol,
                TypeRef::String,
                TypeRef::Boolean,
                TypeRef::BigInt,
                TypeRef::Number,
            ])),
            ":: js_sys :: JsBigIntOrBooleanOrNumberOrStringOrSymbolLike"
        );
    }

    #[test]
    fn literals_count_toward_their_category() {
        let ty = union(&[
            TypeRef::NumberLiteral(1.0),
            TypeRef::NumberLiteral(2.0),
            TypeRef::StringLiteral("auto".into()),
        ]);
        assert_eq!(trait_name(&ty), ":: js_sys :: JsNumberOrStringLike");
    }

    #[test]
    fn single_category_is_not_a_primitive_union() {
        let ty = union(&[TypeRef::StringLiteral("a".into()), TypeRef::String]);
        assert!(PrimitiveUnion::classify(&ty).is_none());
    }

    #[test]
    fn non_primitive_members_are_not_a_primitive_union() {
        let ty = union(&[TypeRef::String, TypeRef::ident("Foo")]);
        assert!(PrimitiveUnion::classify(&ty).is_none());
    }

    #[test]
    fn property_key_keeps_its_spelling() {
        assert_eq!(
            trait_name(&TypeRef::PropertyKey),
            ":: js_sys :: PropertyKey"
        );
        assert_eq!(
            trait_name(&union(&[TypeRef::Number, TypeRef::String, TypeRef::Symbol])),
            ":: js_sys :: JsNumberOrStringOrSymbolLike"
        );
        assert_eq!(
            trait_name(&union(&[TypeRef::PropertyKey, TypeRef::Boolean])),
            ":: js_sys :: JsBooleanOrNumberOrStringOrSymbolLike"
        );
    }

    #[test]
    fn suffixes_mirror_trait_names() {
        let ty = union(&[TypeRef::String, TypeRef::Number]);
        assert_eq!(
            PrimitiveUnion::classify(&ty).unwrap().suffix(),
            "number_or_string"
        );
        assert_eq!(
            PrimitiveUnion::classify(&TypeRef::PropertyKey)
                .unwrap()
                .suffix(),
            "property_key"
        );
    }

    #[test]
    fn grouping_keeps_non_primitive_alternatives_in_place() {
        let foo = TypeRef::ident("Foo");
        let out = group_alternatives(vec![foo.clone(), TypeRef::String, TypeRef::Number], false);
        assert_eq!(out, vec![foo, union(&[TypeRef::String, TypeRef::Number])]);
    }

    #[test]
    fn grouping_leaves_single_category_alone() {
        let alts = vec![TypeRef::StringLiteral("a".into()), TypeRef::String];
        assert_eq!(group_alternatives(alts.clone(), false), alts);
    }

    #[test]
    fn grouping_can_keep_literals_separate() {
        let alts = vec![
            TypeRef::StringLiteral("auto".into()),
            TypeRef::String,
            TypeRef::Number,
        ];
        assert_eq!(
            group_alternatives(alts, true),
            vec![
                TypeRef::StringLiteral("auto".into()),
                union(&[TypeRef::String, TypeRef::Number]),
            ]
        );

        // Only literals span categories: nothing to group.
        let literals = vec![
            TypeRef::NumberLiteral(1.0),
            TypeRef::StringLiteral("auto".into()),
        ];
        assert_eq!(group_alternatives(literals.clone(), true), literals);
    }

    #[test]
    fn lone_property_key_groups_to_itself() {
        assert_eq!(
            group_alternatives(vec![TypeRef::PropertyKey], false),
            vec![TypeRef::PropertyKey]
        );
    }

    #[test]
    fn accepts_checks_category_containment() {
        let wide = PrimitiveUnion::classify(&union(&[
            TypeRef::String,
            TypeRef::Number,
            TypeRef::Boolean,
        ]))
        .unwrap();
        assert!(wide.accepts(&TypeRef::StringLiteral("x".into())));
        assert!(wide.accepts(&union(&[TypeRef::String, TypeRef::Number])));
        assert!(!wide.accepts(&TypeRef::Symbol));
        assert!(!wide.accepts(&TypeRef::ident("Foo")));
    }
}
