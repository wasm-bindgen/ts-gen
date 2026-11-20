//! Every type reference and enum in lowered declarations, for the passes
//! that rewrite them once lowering is done.

use ts_gen_abi::ir as abi;

pub(super) fn enums(items: &mut [abi::Decl], f: &mut impl FnMut(&mut abi::Enum)) {
    for d in items {
        match d {
            abi::Decl::Namespace(n) => enums(&mut n.items, f),
            abi::Decl::Module(m) => enums(&mut m.items, f),
            abi::Decl::Enum(e) => f(e),
            abi::Decl::Class(_) | abi::Decl::Member(_) | abi::Decl::Alias(_) => {}
        }
    }
}

/// Each member, in classes and at any depth of containers.
pub(super) fn members(items: &mut [abi::Decl], f: &mut impl FnMut(&mut abi::Member)) {
    for d in items {
        match d {
            abi::Decl::Namespace(n) => members(&mut n.items, f),
            abi::Decl::Module(m) => members(&mut m.items, f),
            abi::Decl::Class(c) => c.members.iter_mut().for_each(&mut *f),
            abi::Decl::Enum(e) => e.members.iter_mut().for_each(&mut *f),
            abi::Decl::Member(m) => f(m),
            abi::Decl::Alias(_) => {}
        }
    }
}

pub(super) fn retain_members(items: &mut Vec<abi::Decl>, f: &mut impl FnMut(&abi::Member) -> bool) {
    items.retain(|d| !matches!(d, abi::Decl::Member(m) if !f(m)));
    for d in items {
        match d {
            abi::Decl::Namespace(n) => retain_members(&mut n.items, f),
            abi::Decl::Module(m) => retain_members(&mut m.items, f),
            abi::Decl::Class(c) => c.members.retain(|m| f(m)),
            abi::Decl::Enum(e) => e.members.retain(|m| f(m)),
            abi::Decl::Member(_) | abi::Decl::Alias(_) => {}
        }
    }
}

/// Each reference, innermost arguments first.
pub(super) fn decls(items: &mut [abi::Decl], f: &mut impl FnMut(&mut abi::TypeRef)) {
    for d in items {
        match d {
            abi::Decl::Namespace(n) => decls(&mut n.items, f),
            abi::Decl::Module(m) => decls(&mut m.items, f),
            abi::Decl::Class(c) => {
                tparams(&mut c.tparams, f);
                for r in &mut c.extends {
                    type_ref(r, f);
                }
                for m in &mut c.members {
                    member(m, f);
                }
                for (e, t) in c.assoc.iter_mut().flat_map(|a| &mut a.rows) {
                    ty(e, f);
                    ty(t, f);
                }
            }
            abi::Decl::Enum(e) => {
                for t in e.arms.iter_mut().flat_map(|a| &mut a.payload) {
                    ty(t, f);
                }
            }
            abi::Decl::Member(m) => member(m, f),
            abi::Decl::Alias(a) => {
                tparams(&mut a.tparams, f);
                ty(&mut a.ty, f);
            }
        }
    }
}

fn member(m: &mut abi::Member, f: &mut impl FnMut(&mut abi::TypeRef)) {
    tparams(&mut m.tparams, f);
    for p in &mut m.params {
        ty(&mut p.ty, f);
    }
    ty(&mut m.result, f);
}

fn tparams(ps: &mut [abi::TypeParam], f: &mut impl FnMut(&mut abi::TypeRef)) {
    for p in ps {
        match &mut p.bound {
            Some(abi::Bound::Subtype(t)) => ty(t, f),
            Some(abi::Bound::In(ts)) => ts.iter_mut().for_each(|t| ty(t, f)),
            None => {}
        }
    }
}

fn type_ref(r: &mut abi::TypeRef, f: &mut impl FnMut(&mut abi::TypeRef)) {
    r.args.iter_mut().for_each(|t| ty(t, f));
    f(r);
}

fn ty(t: &mut abi::Type, f: &mut impl FnMut(&mut abi::TypeRef)) {
    use abi::Type as T;
    match t {
        T::Vec(e)
        | T::VecBox(e)
        | T::MutVec(e)
        | T::Array(e)
        | T::ArrBox(e)
        | T::Nullable(e, _)
        | T::Future(e, _)
        | T::View { elem: e, .. } => ty(e, f),
        T::Tuple(ts) | T::TupleBox(ts) => ts.iter_mut().for_each(|t| ty(t, f)),
        T::Fn(fun) => {
            fun.params.iter_mut().for_each(|t| ty(t, f));
            ty(&mut fun.result, f);
        }
        T::Named(r) => type_ref(r, f),
        T::Unit
        | T::Never
        | T::Scalar(_)
        | T::Str
        | T::Any
        | T::ScalarBox(_)
        | T::StrBox
        | T::BigInt
        | T::BigIntBox
        | T::UnitBox => {}
    }
}
