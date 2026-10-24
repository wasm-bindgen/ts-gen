//! Drop the type arguments given for erased type parameters.
//!
//! A type parameter constrained to a primitive (see
//! [`crate::ir::TypeParam::erasure`]) is removed from its declaration's
//! parameter list while its body is converted. References elsewhere can
//! still pass an argument for it (`ListResult<Metadata, Key>`), and which
//! declaration a reference names is only known once every file is parsed.
//! This pass runs then, removing the argument at each erased position so
//! every reference's arguments line up with the declaration's remaining
//! parameters for everything downstream.

use std::collections::HashMap;

use crate::context::{GlobalContext, TypeId};
use crate::ir::{FunctionDecl, FunctionSig, Member, Param, Throws, TypeKind, TypeParam, TypeRef};
use crate::parse::scope::ScopeId;

pub(crate) fn drop_erased_args(gctx: &mut GlobalContext) {
    let erased: HashMap<TypeId, Vec<usize>> = gctx
        .iter_types()
        .filter_map(|(id, decl)| {
            let body_scope = match &decl.kind {
                TypeKind::Class(c) => c.body_scope,
                TypeKind::Interface(i) => i.body_scope,
                TypeKind::DiscriminatedUnion(d) => d.body_scope,
                TypeKind::TypeAlias(a) => a.body_scope,
                _ => return None,
            };
            let positions = gctx.scopes.erased_positions(body_scope);
            (!positions.is_empty()).then(|| (id, positions.to_vec()))
        })
        .collect();
    if erased.is_empty() {
        return;
    }
    let ids: Vec<TypeId> = gctx.iter_types().map(|(id, _)| id).collect();
    for id in ids {
        let pass = Pass {
            gctx,
            erased: &erased,
        };
        let decl = gctx.get_type(id);
        let kind = pass.kind(&decl.kind, decl.scope_id);
        gctx.get_type_mut(id).kind = kind;
    }
}

struct Pass<'a> {
    gctx: &'a GlobalContext,
    erased: &'a HashMap<TypeId, Vec<usize>>,
}

impl Pass<'_> {
    fn kind(&self, kind: &TypeKind, decl_scope: ScopeId) -> TypeKind {
        let mut kind = kind.clone();
        match &mut kind {
            TypeKind::Class(c) => {
                self.type_params(&mut c.type_params, c.body_scope);
                if let Some(parent) = &mut c.extends {
                    parent.type_ref = self.ty(&parent.type_ref, parent.scope);
                }
                for ty in &mut c.implements {
                    *ty = self.ty(ty, c.body_scope);
                }
                self.members(&mut c.members);
            }
            TypeKind::Interface(i) => {
                self.type_params(&mut i.type_params, i.body_scope);
                for parent in &mut i.extends {
                    parent.type_ref = self.ty(&parent.type_ref, parent.scope);
                }
                self.members(&mut i.members);
            }
            TypeKind::DiscriminatedUnion(d) => {
                self.type_params(&mut d.type_params, d.body_scope);
                self.members(&mut d.members);
                for branch in &mut d.branches {
                    self.members(branch);
                }
            }
            TypeKind::TypeAlias(a) => {
                self.type_params(&mut a.type_params, a.body_scope);
                a.target = self.ty(&a.target, a.body_scope);
            }
            TypeKind::Function(f) => self.function(f),
            TypeKind::Variable(v) => {
                v.type_ref = self.ty(&v.type_ref, decl_scope);
                for f in &mut v.constructs {
                    self.function(f);
                }
            }
            TypeKind::StringEnum(_)
            | TypeKind::NumericEnum(_)
            | TypeKind::TsEnum(_)
            | TypeKind::Namespace(_) => {}
        }
        kind
    }

    fn function(&self, f: &mut FunctionDecl) {
        self.type_params(&mut f.type_params, f.body_scope);
        self.params(&mut f.params, f.body_scope);
        f.return_type = self.ty(&f.return_type, f.body_scope);
        self.throws(&mut f.throws, f.body_scope);
        for o in &mut f.overloads {
            self.params(&mut o.params, o.body_scope);
            o.return_type = self.ty(&o.return_type, o.body_scope);
            self.throws(&mut o.throws, o.body_scope);
        }
    }

    fn type_params(&self, params: &mut [TypeParam], scope: ScopeId) {
        for p in params {
            for ty in p.constraint.iter_mut().chain(&mut p.default) {
                *ty = self.ty(ty, scope);
            }
        }
    }

    fn params(&self, params: &mut [Param], scope: ScopeId) {
        for p in params {
            p.type_ref = self.ty(&p.type_ref, scope);
        }
    }

    fn throws(&self, throws: &mut Throws, scope: ScopeId) {
        if let Throws::Type(ty) = throws {
            *ty = self.ty(ty, scope);
        }
    }

    fn members(&self, members: &mut [Member]) {
        for m in members {
            match m {
                Member::Getter(g) => g.type_ref = self.ty(&g.type_ref, g.scope),
                Member::Setter(s) => s.type_ref = self.ty(&s.type_ref, s.scope),
                Member::StaticGetter(g) => g.type_ref = self.ty(&g.type_ref, g.scope),
                Member::StaticSetter(s) => s.type_ref = self.ty(&s.type_ref, s.scope),
                Member::IndexSignature(i) => {
                    i.key_type = self.ty(&i.key_type, i.scope);
                    i.value_type = self.ty(&i.value_type, i.scope);
                }
                Member::Constructor(c) => {
                    self.params(&mut c.params, c.scope);
                    self.throws(&mut c.throws, c.scope);
                }
                Member::Method(m) => {
                    self.type_params(&mut m.type_params, m.body_scope);
                    self.params(&mut m.params, m.body_scope);
                    m.return_type = self.ty(&m.return_type, m.body_scope);
                    self.throws(&mut m.throws, m.body_scope);
                }
                Member::StaticMethod(m) => {
                    self.type_params(&mut m.type_params, m.body_scope);
                    self.params(&mut m.params, m.body_scope);
                    m.return_type = self.ty(&m.return_type, m.body_scope);
                    self.throws(&mut m.throws, m.body_scope);
                }
            }
        }
    }

    fn ty(&self, ty: &TypeRef, scope: ScopeId) -> TypeRef {
        let sub = |t: &TypeRef| self.ty(t, scope);
        match ty {
            TypeRef::Reference {
                segments,
                generic_args,
            } if !generic_args.is_empty() => {
                let erased = self
                    .gctx
                    .resolve_path(scope, &segments.join("."))
                    .and_then(|id| self.erased.get(&id));
                TypeRef::Reference {
                    segments: segments.clone(),
                    generic_args: generic_args
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| erased.is_none_or(|e| !e.contains(i)))
                        .map(|(_, a)| sub(a))
                        .collect(),
                }
            }
            TypeRef::Array(t) => TypeRef::Array(Box::new(sub(t))),
            TypeRef::Union(u) => TypeRef::union(u.members().iter().map(sub)),
            TypeRef::Intersection(ts) => TypeRef::Intersection(ts.iter().map(sub).collect()),
            TypeRef::Tuple(ts) => TypeRef::Tuple(ts.iter().map(sub).collect()),
            TypeRef::Function(sig) => TypeRef::Function(FunctionSig {
                params: sig
                    .params
                    .iter()
                    .map(|p| Param {
                        type_ref: sub(&p.type_ref),
                        ..p.clone()
                    })
                    .collect(),
                return_type: Box::new(sub(&sig.return_type)),
            }),
            other => other.clone(),
        }
    }
}
