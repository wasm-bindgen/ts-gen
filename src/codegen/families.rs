//! Literal-only type parameters under mono.
//!
//! The IR keeps such a parameter generic, bounded `in` the enums its uses
//! pass, and tabulates each type that varies with it as an associated type
//! (IR.md §13). Rust reads an associated type through a trait bounding the
//! parameter, with one impl per argument. Interfaces passing the parameter
//! on to a parent form a family: one that reads no associated type its
//! parent's trait lacks shares that trait, and one that does gets its own,
//! extending the parent's, so a helper bounded by the base trait takes
//! every member of the family.

use std::collections::HashMap;

use ts_gen_abi::ir as abi;
use ts_gen_abi::resolve::{Context, Location, Symbols, TypeDecl};

/// A family member's parameter, and the trait bounding it.
#[derive(Clone, Debug)]
pub struct Bounded {
    pub param: String,
    pub trait_name: String,
    /// The bound's first element, the parameter's own constraint.
    pub default: abi::Type,
}

/// A trait of associated types, emitted after the interface introducing it.
#[derive(Clone, Debug)]
pub struct Trait {
    pub name: String,
    pub parent: Option<String>,
    pub owner: Location,
    /// Each argument, with its row of each associated type the trait adds.
    pub impls: Vec<(abi::Type, Vec<(String, abi::Type)>)>,
    /// The declaration of each argument of `impls`.
    keys: Vec<Option<Location>>,
    pub assoc: Vec<String>,
}

impl Trait {
    /// Whether a root family reading `assoc`, with `impls`, can share this
    /// trait: one of the same associated types, agreeing on each argument
    /// both take.
    fn reads_alike(
        &self,
        assoc: &[&abi::Assoc],
        impls: &[(abi::Type, Vec<(String, abi::Type)>)],
    ) -> bool {
        self.parent.is_none()
            && self.assoc.len() == assoc.len()
            && assoc.iter().all(|a| self.assoc.contains(&a.name))
            && impls.iter().all(|(arg, rows)| {
                self.impls
                    .iter()
                    .all(|(a, theirs)| a != arg || rows.iter().all(|r| theirs.contains(r)))
            })
    }
}

#[derive(Default)]
pub struct Families {
    pub bounded: HashMap<Location, Bounded>,
    pub traits: Vec<Trait>,
    /// Each class whose first parent is a family member, by that
    /// parent and its instantiation, which wasm-bindgen can't build from a
    /// `JsValue` itself.
    pub instances: HashMap<Location, (Location, abi::TypeRef)>,
    /// Each other class whose first parent, or its first parent in turn,
    /// is one of `instances`, by that one's family instantiation.
    pub inherited: HashMap<Location, (Location, abi::TypeRef)>,
    /// The declaration of each argument of a class's instantiation in
    /// `instances` or `inherited`, which another place spells otherwise.
    keys: HashMap<Location, Vec<Option<Location>>>,
}

struct Site<'a> {
    class: &'a abi::Class,
    module: Option<&'a str>,
    namespace: Vec<&'a str>,
}

impl Families {
    pub fn new(ir: &abi::Ir) -> Families {
        let mut sites = HashMap::new();
        collect(&ir.items, None, &mut Vec::new(), &mut sites);
        let symbols = Symbols::new(ir);
        let mut families = Families::default();
        let mut locations: Vec<&Location> = sites.keys().collect();
        locations.sort_by(|a, b| (&a.module, &a.path).cmp(&(&b.module, &b.path)));
        for loc in &locations {
            families.visit(loc, &sites, &symbols, &mut Vec::new());
        }
        let first_parent = |loc: &Location| {
            let site = &sites[loc];
            let first = site.class.extends.first()?;
            let cx = Context {
                module: site.module,
                namespace: site.namespace.clone(),
                ..Default::default()
            };
            let (parent, _) = symbols.resolve_decl(&first.path, &cx)?;
            Some((parent, first.clone()))
        };
        let parents: HashMap<&Location, (Location, abi::TypeRef)> = locations
            .iter()
            .filter_map(|loc| Some((*loc, first_parent(loc)?)))
            .collect();
        for (loc, (parent, first)) in &parents {
            if families.bounded.contains_key(parent) {
                let keys = first
                    .args
                    .iter()
                    .map(|a| key(a, &sites[*loc], &symbols))
                    .collect();
                families.keys.insert((*loc).clone(), keys);
                families
                    .instances
                    .insert((*loc).clone(), (parent.clone(), first.clone()));
            }
        }
        for loc in locations {
            let mut seen = vec![loc];
            let mut at = loc;
            while let Some((parent, _)) = parents.get(at) {
                if seen.contains(&parent) || families.instances.contains_key(loc) {
                    break;
                }
                if let Some(instance) = families.instances.get(parent) {
                    families.inherited.insert(loc.clone(), instance.clone());
                    let keys = families.keys[parent].clone();
                    families.keys.insert(loc.clone(), keys);
                    break;
                }
                seen.push(parent);
                at = parent;
            }
        }
        families
    }

    /// The trait bounding the class at `loc`, after its parents'.
    fn visit(
        &mut self,
        loc: &Location,
        sites: &HashMap<Location, Site<'_>>,
        symbols: &Symbols<'_>,
        stack: &mut Vec<Location>,
    ) -> Option<String> {
        if let Some(b) = self.bounded.get(loc) {
            return Some(b.trait_name.clone());
        }
        if stack.contains(loc) {
            return None;
        }
        let site = sites.get(loc)?;
        let class = site.class;
        let (param, bound) = class.tparams.iter().find_map(|p| match &p.bound {
            Some(abi::Bound::In(b)) => Some((p.name.clone(), b.clone())),
            _ => None,
        })?;
        let default = bound.first()?.clone();
        // The parent the parameter is passed on to, and its trait.
        stack.push(loc.clone());
        let cx = Context {
            module: site.module,
            namespace: site.namespace.clone(),
            ..Default::default()
        };
        let parent = class.extends.iter().find_map(|r| {
            r.args
                .iter()
                .any(|a| matches!(a, abi::Type::Named(n) if n.path.segments == [param.clone()]))
                .then(|| symbols.resolve_decl(&r.path, &cx))
                .flatten()
                .and_then(|(ploc, decl)| matches!(decl, TypeDecl::Class(_)).then_some(ploc))
                .and_then(|ploc| self.visit(&ploc, sites, symbols, stack))
        });
        stack.pop();
        let inherited = self.closure(parent.as_deref());
        let own: Vec<&abi::Assoc> = class
            .assoc
            .iter()
            .filter(|a| a.param == param && !inherited.contains(&a.name))
            .collect();
        let impls: Vec<_> = bound
            .iter()
            .map(|arg| {
                let rows = own
                    .iter()
                    .filter_map(|a| {
                        let (_, row) = a.rows.iter().find(|(k, _)| k == arg)?;
                        Some((a.name.clone(), row.clone()))
                    })
                    .collect();
                (arg.clone(), rows)
            })
            .collect();
        let keys: Vec<Option<Location>> = bound.iter().map(|a| key(a, site, symbols)).collect();
        // Another family's trait of the same associated types, in the same
        // namespace, where its arguments are spelled alike.
        let alike = |t: &Trait| {
            t.owner.module == loc.module
                && t.owner.path[..t.owner.path.len() - 1] == loc.path[..loc.path.len() - 1]
                && t.reads_alike(&own, &impls)
        };
        let trait_name = if own.is_empty() {
            parent?
        } else if let Some(t) = parent
            .is_none()
            .then(|| self.traits.iter_mut().find(|t| alike(t)))
            .flatten()
        {
            // It takes this one's arguments too.
            for ((arg, rows), k) in impls.into_iter().zip(keys) {
                if !t.impls.iter().any(|(a, _)| *a == arg) {
                    t.impls.push((arg, rows));
                    t.keys.push(k);
                }
            }
            t.name.clone()
        } else {
            let name = format!("{}{param}", class.name);
            self.traits.push(Trait {
                name: name.clone(),
                parent,
                owner: loc.clone(),
                impls,
                keys,
                assoc: own.iter().map(|a| a.name.clone()).collect(),
            });
            name
        };
        self.bounded.insert(
            loc.clone(),
            Bounded {
                param,
                trait_name: trait_name.clone(),
                default,
            },
        );
        Some(trait_name)
    }

    /// The associated types of `name` and the traits it extends.
    fn closure(&self, name: Option<&str>) -> Vec<String> {
        let mut out = Vec::new();
        let mut next = name;
        while let Some(n) = next {
            let Some(t) = self.traits.iter().find(|t| t.name == n) else {
                break;
            };
            out.extend(t.assoc.iter().cloned());
            next = t.parent.as_deref();
        }
        out
    }

    /// The row of the associated type `assoc`, of `name` or a trait it
    /// extends, at the first argument of the instantiation of `at`, with
    /// the trait's owner, where it's spelled.
    pub fn row(&self, name: &str, assoc: &str, at: &Location) -> Option<(&abi::Type, &Location)> {
        let arg = self.keys.get(at)?.first()?.as_ref()?;
        let mut next = Some(name);
        while let Some(n) = next {
            let t = self.traits.iter().find(|t| t.name == n)?;
            let found = t.keys.iter().position(|k| k.as_ref() == Some(arg));
            if let Some((_, rows)) = found.map(|i| &t.impls[i]) {
                if let Some((_, row)) = rows.iter().find(|(a, _)| a == assoc) {
                    return Some((row, &t.owner));
                }
            }
            next = t.parent.as_deref();
        }
        None
    }

    /// The traits introduced at `loc`.
    pub fn traits_at<'a>(&'a self, loc: &'a Location) -> impl Iterator<Item = &'a Trait> + 'a {
        self.traits.iter().filter(move |t| &t.owner == loc)
    }
}

/// The declaration the argument `t`, written at `site`, names.
fn key(t: &abi::Type, site: &Site<'_>, symbols: &Symbols<'_>) -> Option<Location> {
    let abi::Type::Named(r) = t else {
        return None;
    };
    let cx = Context {
        module: site.module,
        namespace: site.namespace.clone(),
        ..Default::default()
    };
    symbols.resolve_decl(&r.path, &cx).map(|(loc, _)| loc)
}

fn collect<'a>(
    items: &'a [abi::Decl],
    module: Option<&'a str>,
    namespace: &mut Vec<&'a str>,
    out: &mut HashMap<Location, Site<'a>>,
) {
    for d in items {
        match d {
            abi::Decl::Class(c) => {
                let mut path: Vec<String> = namespace.iter().map(|s| s.to_string()).collect();
                path.push(c.name.clone());
                out.insert(
                    Location {
                        module: module.map(Into::into),
                        path,
                    },
                    Site {
                        class: c,
                        module,
                        namespace: namespace.clone(),
                    },
                );
            }
            abi::Decl::Namespace(n) => {
                namespace.push(&n.name);
                collect(&n.items, module, namespace, out);
                namespace.pop();
            }
            abi::Decl::Module(m) => collect(&m.items, Some(&m.spec), &mut Vec::new(), out),
            _ => {}
        }
    }
}
