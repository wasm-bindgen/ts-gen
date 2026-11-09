//! Associated types (IR.md §13): a projection of a literal-only type
//! parameter (`PluralizeUnit<Units>`), alone or in a union with string
//! literals (`"auto" | PluralizeUnit<Units>`), is one type per argument.
//! Its rows wait until every use has added its argument to the bound.

use std::collections::HashMap;

use ts_gen_abi::ir as abi;

use super::types::{Named, Tcx};
use super::{names, Lowerer, Place};
use crate::context::{Projection, TypeId};
use crate::ir::{TypeKind, TypeRef};
use crate::parse::scope::Binding;

/// An associated type of `owner`, before its rows.
#[derive(Clone)]
pub(crate) struct Pending {
    name: String,
    /// The parameter's index among those the IR keeps.
    index: usize,
    param: String,
    /// The alias a lone projection goes through, which names the rows.
    alias: Option<String>,
    /// Literals beside the projection.
    statics: Vec<String>,
    literals: Vec<(String, Vec<String>)>,
}

impl Lowerer<'_> {
    /// The associated type `t` reads as, with `null` and `undefined`
    /// beside it left to the caller.
    pub(crate) fn assoc_type(&mut self, t: &TypeRef, cx: &Tcx) -> Option<abi::Type> {
        let members: Vec<&TypeRef> = match t {
            TypeRef::Union(u) => u
                .members()
                .iter()
                .filter(|m| !matches!(m, TypeRef::Null | TypeRef::Undefined))
                .collect(),
            t => vec![t],
        };
        let mut statics = Vec::new();
        let mut projections: Vec<Projection> = Vec::new();
        for m in &members {
            match m {
                TypeRef::StringLiteral(s) => statics.push(s.clone()),
                TypeRef::Reference { .. } => projections.push(match self.projection(m, cx) {
                    Some(p) => p.clone(),
                    None => self.identity(m, cx)?,
                }),
                _ => return None,
            }
        }
        let [p] = projections.as_slice() else {
            return None;
        };
        let p = p.clone();
        // The parameter alone is just the parameter.
        if p.alias.is_empty() && statics.is_empty() {
            return None;
        }
        let info = self.info.get(&p.owner)?.clone();
        let (params, scope) = match &self.gctx.get_type(p.owner).kind {
            TypeKind::Class(c) => (c.type_params.clone(), c.body_scope),
            TypeKind::Interface(i) => (i.type_params.clone(), i.body_scope),
            _ => return None,
        };
        let index = params
            .iter()
            .filter(|tp| !self.dropped(tp, scope))
            .position(|tp| tp.name == p.param)?;
        if !self
            .lit_args
            .contains_key(&(info.place.clone(), info.name.clone(), index))
        {
            return None;
        }
        let spelled = members
            .iter()
            .map(|m| m.format_ts())
            .collect::<Vec<_>>()
            .join(" | ");
        let key = (p.owner, spelled);
        if let Some(name) = self.assoc_names.get(&key) {
            return Some(abi::Type::named([name.as_str()]));
        }
        let lone = statics.is_empty() && !p.alias.is_empty();
        // `PluralizeUnit<Units>` is `PluralizeUnits`.
        let base = if lone {
            let head = p
                .alias
                .char_indices()
                .skip(1)
                .find(|(_, c)| c.is_uppercase())
                .map_or(p.alias.as_str(), |(i, _)| &p.alias[..i]);
            format!("{head}{}", names::pascal(&p.param))
        } else {
            cx.anchor.clone()
        };
        let pending = self.pending_assoc.entry(p.owner).or_default();
        let name = names::unique(&names::escape_type(&base), |n| {
            self.taken.contains(n)
                || params.iter().any(|tp| tp.name == n)
                || pending.iter().any(|a| a.name == n)
        });
        pending.push(Pending {
            name: name.clone(),
            index,
            param: p.param.clone(),
            alias: lone.then(|| p.alias.clone()),
            statics,
            literals: p.literals.clone(),
        });
        self.assoc_names.insert(key, name.clone());
        Some(abi::Type::named([name.as_str()]))
    }

    /// The enclosing declaration's literal-only parameter `t` names, as a
    /// projection mapping each literal of its bound to itself, so that
    /// literals beside it (`"auto" | U`) make an associated type too.
    fn identity(&mut self, t: &TypeRef, cx: &Tcx) -> Option<Projection> {
        let TypeRef::Reference {
            segments,
            generic_args,
        } = t
        else {
            return None;
        };
        let ([param], []) = (segments.as_slice(), generic_args.as_slice()) else {
            return None;
        };
        if cx.subst.contains_key(param)
            || !matches!(
                self.gctx.scopes.resolve_binding(cx.scope, param),
                Some(Binding::TypeParam)
            )
        {
            return None;
        }
        let key = self.lit_param(&abi::Type::named([param.as_str()]), cx)?;
        let owner = self
            .info
            .iter()
            .find(|(_, i)| i.place == key.0 && i.name == key.1)
            .map(|(id, _)| *id)?;
        let bound = self.lit_args.get(&key)?.first()?.clone();
        let literals = self
            .literal_set(&bound, &key.0)?
            .into_iter()
            .map(|l| (l.clone(), vec![l]))
            .collect();
        Some(Projection {
            owner,
            param: param.clone(),
            alias: String::new(),
            literals,
        })
    }

    fn projection(&self, t: &TypeRef, cx: &Tcx) -> Option<&Projection> {
        let key = t.format_ts();
        self.gctx
            .scopes
            .chain(cx.scope)
            .into_iter()
            .find_map(|s| self.gctx.projections.get(&(s, key.clone())))
    }

    /// Give each associated type its rows, one per element of its
    /// parameter's final bound: a literal enum of what the element's
    /// literals map to.
    pub(crate) fn finish_assoc(&mut self) {
        // A parameter passed on instantiates the declaration it's passed to
        // at each of its own arguments.
        let mut changed = true;
        while changed {
            changed = false;
            for (from, to) in self.passed.clone() {
                let args = self.lit_args.get(&from).cloned().unwrap_or_default();
                for t in args {
                    let t = self.rebase(&t, &from.0, &to.0);
                    let bound = self.lit_args.entry(to.clone()).or_default();
                    if !bound.contains(&t) {
                        bound.push(t);
                        changed = true;
                    }
                }
            }
        }
        let pending = std::mem::take(&mut self.pending_assoc);
        let mut owners: Vec<TypeId> = pending.keys().copied().collect();
        owners.sort_by_key(|id| id.index());
        for owner in owners {
            let info = self.info[&owner].clone();
            let scope = match &self.gctx.get_type(owner).kind {
                TypeKind::Class(c) => c.body_scope,
                TypeKind::Interface(i) => i.body_scope,
                _ => continue,
            };
            let cx = Tcx::new(scope, &info.place, &info.name);
            let mut out = Vec::new();
            for a in &pending[&owner] {
                let key = (info.place.clone(), info.name.clone(), a.index);
                let bound = self.lit_args.get(&key).cloned().unwrap_or_default();
                let mut rows = Vec::new();
                for elem in bound {
                    let lits = self.literal_set(&elem, &info.place).unwrap_or_default();
                    // Union order, as an instantiation reads: `T | Map[T]`
                    // is every `T` literal, then every `Map[T]` one.
                    let mut strs = a.statics.clone();
                    let width = a.literals.iter().map(|(_, to)| to.len()).max().unwrap_or(0);
                    for j in 0..width {
                        for (lit, to) in &a.literals {
                            match to.get(j) {
                                Some(s) if lits.contains(lit) && !strs.contains(s) => {
                                    strs.push(s.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    // An element is its declaration, as spelled here
                    // (`Steps.Unit`), not its name.
                    let elem_name = match &elem {
                        abi::Type::Named(r) => r
                            .path
                            .segments
                            .iter()
                            .map(|s| names::unescape_type(s))
                            .collect(),
                        _ => String::new(),
                    };
                    let row = match &a.alias {
                        Some(alias) => {
                            crate::parse::eval::instance_spelling(alias, &[elem_name], None)
                        }
                        None => format!("{}{elem_name}", a.name),
                    };
                    let mut named = Named {
                        name: row,
                        place: info.place.clone(),
                        id: None,
                    };
                    // Another family's row of the same name maps the
                    // element otherwise, so this one is named for its family.
                    if let Some(t) = self.synth_ref(&named.key(), &info.place) {
                        let mut have = self.literal_set(&t, &info.place).unwrap_or_default();
                        let mut want = strs.clone();
                        have.sort();
                        want.sort();
                        if have != want {
                            let base = format!("{}{}", info.name, named.name);
                            named.name = names::unique(&base, |n| self.taken.contains(n));
                        }
                    }
                    let t = self.literal_enum(&strs, &cx, Some(named));
                    rows.push((elem, t));
                }
                out.push(abi::Assoc {
                    meta: abi::Meta::default(),
                    name: a.name.clone(),
                    param: a.param.clone(),
                    rows,
                });
            }
            self.assocs
                .insert((info.place.clone(), info.name.clone()), out);
        }
    }

    /// The associated types of the class `name` at `place`.
    pub(crate) fn take_assoc(&mut self, place: &Place, name: &str) -> Vec<abi::Assoc> {
        self.assocs
            .remove(&(place.clone(), name.to_string()))
            .unwrap_or_default()
    }
}

/// Whether the union `t` holds `null` and `undefined`.
pub(crate) fn absent(t: &TypeRef) -> (bool, bool) {
    let members = match t {
        TypeRef::Union(u) => u.members(),
        t => std::slice::from_ref(t),
    };
    (
        members.contains(&TypeRef::Null),
        members.contains(&TypeRef::Undefined),
    )
}

pub(crate) type Assocs = HashMap<(Place, String), Vec<abi::Assoc>>;
