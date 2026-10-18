//! Bounds per-monomorphization output needs on type parameters that reach a
//! `js_sys::Function<fn(..) -> R>`.
//!
//! A per-monomorphization parameter is unbounded, but `Function`'s
//! signature type is not monomorphized: `fn(A) -> R: JsFunction` needs every
//! `A` and `R` to be a JS handle. A parameter named anywhere inside a
//! `Function<…>` (a callback property of `ExportedHandler<Env>`, say) keeps
//! `JsGeneric` on the extern fn that mentions it, and on the inherent impls
//! of its type and builder, whose helpers call those fns. This pass runs on
//! the finished `syn::File`, after the shapes are final.

use std::collections::{BTreeSet, HashMap};

use quote::ToTokens;
use syn::visit_mut::{self, VisitMut};
use syn::{ForeignItem, Generics, ImplItem, Item, Type, TypeParamBound};

pub(crate) fn apply(file: &mut syn::File) {
    let mut needs: HashMap<String, BTreeSet<String>> = HashMap::new();
    collect_impl_needs(&file.items, &mut needs);
    process(&mut file.items, &needs);
}

fn is_mono(block: &syn::ItemForeignMod) -> bool {
    block.attrs.iter().any(|a| {
        a.path().is_ident("wasm_bindgen")
            && a.to_token_stream()
                .to_string()
                .contains("experimental_generic_mono")
    })
}

/// Idents named inside a `Function<…>` argument list.
#[derive(Default)]
struct InFunction {
    depth: usize,
    out: BTreeSet<String>,
}

impl VisitMut for InFunction {
    fn visit_path_segment_mut(&mut self, seg: &mut syn::PathSegment) {
        if self.depth > 0 {
            self.out.insert(seg.ident.to_string());
        }
        let is_fn = seg.ident == "Function";
        self.depth += usize::from(is_fn);
        visit_mut::visit_path_segment_mut(self, seg);
        self.depth -= usize::from(is_fn);
    }
}

fn in_function(sig: &syn::Signature) -> BTreeSet<String> {
    let mut v = InFunction::default();
    v.visit_signature_mut(&mut sig.clone());
    v.out
}

/// A type and its builder share one entry: the type's factories go through
/// the builder.
fn impl_key(ty: &Type) -> Option<String> {
    let Type::Path(p) = ty else { return None };
    let name = p.path.segments.last()?.ident.to_string();
    Some(name.strip_suffix("Builder").unwrap_or(&name).to_string())
}

fn collect_impl_needs(items: &[Item], needs: &mut HashMap<String, BTreeSet<String>>) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect_impl_needs(inner, needs);
                }
            }
            Item::Impl(i) if i.trait_.is_none() => {
                let Some(key) = impl_key(&i.self_ty) else {
                    continue;
                };
                let params: BTreeSet<String> = type_params(&i.generics);
                let entry = needs.entry(key).or_default();
                for ii in &i.items {
                    if let ImplItem::Fn(f) = ii {
                        entry.extend(in_function(&f.sig).intersection(&params).cloned());
                    }
                }
            }
            _ => {}
        }
    }
}

fn type_params(g: &Generics) -> BTreeSet<String> {
    g.type_params().map(|t| t.ident.to_string()).collect()
}

fn add_js_generic(g: &mut Generics, wanted: &BTreeSet<String>) {
    for tp in g.type_params_mut() {
        if !wanted.contains(&tp.ident.to_string()) {
            continue;
        }
        let has = tp.bounds.iter().any(|b| match b {
            TypeParamBound::Trait(t) => t
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == "JsGeneric"),
            _ => false,
        });
        if !has {
            tp.bounds.push(syn::parse_quote!(::wasm_bindgen::JsGeneric));
        }
    }
}

fn process(items: &mut [Item], needs: &HashMap<String, BTreeSet<String>>) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, inner)) = &mut m.content {
                    process(inner, needs);
                }
            }
            Item::ForeignMod(block) if is_mono(block) => {
                for fi in &mut block.items {
                    if let ForeignItem::Fn(f) = fi {
                        let wanted = in_function(&f.sig);
                        add_js_generic(&mut f.sig.generics, &wanted);
                    }
                }
            }
            Item::Impl(i) if i.trait_.is_none() => {
                if let Some(wanted) = impl_key(&i.self_ty).and_then(|k| needs.get(&k)) {
                    add_js_generic(&mut i.generics, wanted);
                }
            }
            _ => {}
        }
    }
}
