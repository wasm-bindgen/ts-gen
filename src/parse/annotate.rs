//! `--annotate`: layer `@rust` and `@throws` tags from a side `.d.ts`
//! onto declarations of the main input, matched by declaration and
//! member name. Upstream declaration files carry no such tags, and
//! editing a vendored copy does not survive an upgrade; a side file that
//! re-declares only the annotated members does.

use crate::context::{GlobalContext, TypeId};
use crate::ir::{
    FunctionDecl, Member, Module, Param, RustScalar, Throws, TypeKind, TypeParam, TypeRef,
};

/// Apply every tag found in `annotations` to the matching declarations
/// of `module`. Returns one message per declaration, member or
/// parameter named in the side file that `module` lacks.
pub fn apply(
    gctx: &mut GlobalContext,
    module: &Module,
    annotations: &Module,
    agctx: &GlobalContext,
) -> Vec<String> {
    let mut errors = Vec::new();
    for &aid in &annotations.types {
        let adecl = agctx.get_type(aid);
        let (name, members, type_params) = match &adecl.kind {
            TypeKind::Class(c) => (&c.name, &c.members, &c.type_params),
            TypeKind::Interface(i) => (&i.name, &i.members, &i.type_params),
            TypeKind::Function(f) => {
                apply_function(gctx, module, adecl.doc.as_deref(), f, &mut errors);
                continue;
            }
            // An alias carries no members; only its type-parameter
            // defaults can be overridden (`RequestInfo<…, Cf = …>`).
            TypeKind::TypeAlias(a) if a.type_params.iter().any(|tp| tp.default.is_some()) => {
                apply_type_param_defaults(gctx, module, &a.name, &a.type_params, &mut errors);
                continue;
            }
            // The side file is a curated list: a declaration this pass
            // cannot carry tags for must not be silently skipped.
            other => {
                let name = crate::parse::declaration_name(other).unwrap_or_default();
                errors.push(format!(
                    "`{name}`: only classes, interfaces and functions can be annotated \
                     (namespaces, modules, aliases and variables are not scoped yet)"
                ));
                continue;
            }
        };
        apply_members(gctx, module, name, members, &mut errors);
        apply_type_param_defaults(gctx, module, name, type_params, &mut errors);
    }
    errors
}

/// Every side declaration of a function applies to every input
/// declaration of it, as side method overloads do in `apply_members`.
fn apply_function(
    gctx: &mut GlobalContext,
    module: &Module,
    doc: Option<&str>,
    f: &FunctionDecl,
    errors: &mut Vec<String>,
) {
    let side: Vec<Overlay<'_>> = std::iter::once(Overlay {
        own: scalar_leaf(&f.return_type),
        params: &f.params,
        throws: &f.throws,
        doc,
    })
    .chain(f.overloads.iter().map(|o| Overlay {
        own: scalar_leaf(&o.return_type),
        params: &o.params,
        throws: &o.throws,
        doc: o.doc.as_deref(),
    }))
    .filter(|o| !o.is_empty())
    .collect();
    if side.is_empty() {
        return;
    }
    let id = match find_decl(gctx, module, &f.js_name) {
        Ok(id) => id,
        Err(e) => {
            errors.push(e);
            return;
        }
    };
    let target = gctx.get_type_mut(id);
    let TypeKind::Function(func) = &mut target.kind else {
        errors.push(format!("`{}` is not a function", f.js_name));
        return;
    };
    for (i, mut overlay) in side.into_iter().enumerate() {
        if i > 0 {
            overlay.doc = None;
        }
        let (mut found, mut mismatched) = (Vec::new(), Vec::new());
        let (mut returns_ok, mut return_mismatches) = (false, Vec::new());
        let targets = std::iter::once((
            &mut func.return_type,
            &mut func.params,
            &mut func.throws,
            &mut target.doc,
        ))
        .chain(
            func.overloads
                .iter_mut()
                .map(|o| (&mut o.return_type, &mut o.params, &mut o.throws, &mut o.doc)),
        );
        for (ret, params, throws, doc) in targets {
            match overlay.apply_return(ret) {
                Some(m) => return_mismatches.push(m),
                None => returns_ok = true,
            }
            overlay.apply_params(params, &mut found, &mut mismatched);
            overlay.apply_throws(throws);
            overlay.apply_doc(doc);
        }
        for p in overlay.missing_params(&found) {
            errors.push(format!("`{}` has no parameter `{p}`", f.js_name));
        }
        if overlay.own.is_some() && !returns_ok {
            mismatched.extend(return_mismatches);
        }
        mismatched.dedup();
        for m in mismatched {
            errors.push(format!("`{}`: {m}", f.js_name));
        }
    }
}

/// A side declaration's type-parameter defaults replace the input's, by
/// parameter name. The input's default is a TypeScript choice that may
/// erase (`Request<…, Cf = CfProperties<…>>`, a union of the incoming and
/// outgoing shapes); the side file picks the Rust default for the bare
/// name (`Cf = IncomingRequestCfProperties<CfHostMetadata>`). Only
/// parameters that declare a default in the side file are touched.
fn apply_type_param_defaults(
    gctx: &mut GlobalContext,
    module: &Module,
    name: &str,
    side: &[TypeParam],
    errors: &mut Vec<String>,
) {
    if side.iter().all(|tp| tp.default.is_none()) {
        return;
    }
    let Ok(id) = find_decl(gctx, module, name) else {
        return;
    };
    let targets: &mut Vec<TypeParam> = match &mut gctx.get_type_mut(id).kind {
        TypeKind::Class(c) => &mut c.type_params,
        TypeKind::Interface(i) => &mut i.type_params,
        TypeKind::DiscriminatedUnion(d) => &mut d.type_params,
        TypeKind::TypeAlias(a) => &mut a.type_params,
        _ => return,
    };
    for tp in side.iter().filter(|tp| tp.default.is_some()) {
        match targets.iter_mut().find(|t| t.name == tp.name) {
            Some(target) => target.default = tp.default.clone(),
            None => errors.push(format!("`{name}` has no type parameter `{}`", tp.name)),
        }
    }
}

/// The input declaration a side name targets. A name declared more than
/// once (globally and in a module) cannot be targeted by bare name.
fn find_decl(gctx: &GlobalContext, module: &Module, name: &str) -> Result<TypeId, String> {
    let matches: Vec<TypeId> = module
        .types
        .iter()
        .copied()
        .filter(|&id| {
            let decl = gctx.get_type(id);
            match &decl.kind {
                TypeKind::Class(c) => c.name == name,
                TypeKind::Interface(i) => i.name == name,
                TypeKind::DiscriminatedUnion(d) => d.name == name,
                TypeKind::TypeAlias(a) => a.name == name,
                TypeKind::Function(f) => f.js_name == name,
                _ => false,
            }
        })
        .collect();
    match matches.as_slice() {
        [] => Err(format!("no declaration named `{name}`")),
        [id] => Ok(*id),
        _ => Err(format!(
            "`{name}` names {} declarations; a side file cannot target one by bare name",
            matches.len()
        )),
    }
}

/// The tags one side-file member carries onto its targets.
struct Overlay<'a> {
    /// `@rust <type>`: the property type or callable return.
    own: Option<RustScalar>,
    /// Parameters, of which only `@rust`-tagged ones are applied.
    params: &'a [Param],
    throws: &'a Throws,
    /// Rendered side doc (the `## Errors` section of `@throws`), appended
    /// to the target's doc.
    doc: Option<&'a str>,
}

impl Overlay<'_> {
    fn is_empty(&self) -> bool {
        self.own.is_none()
            && self
                .params
                .iter()
                .all(|p| scalar_leaf(&p.type_ref).is_none())
            && matches!(self.throws, Throws::None)
            && self.doc.is_none_or(|d| d.trim().is_empty())
    }

    /// The mismatch message when the input's leaf cannot carry the
    /// scalar; the side file's own type does not vouch for it.
    fn apply_return(&self, ty: &mut TypeRef) -> Option<String> {
        let scalar = self.own?;
        match ty.with_rust_scalar(scalar) {
            Some(forced) => {
                *ty = forced;
                None
            }
            None => Some(mismatch(scalar, "type", ty)),
        }
    }

    /// Names that matched are pushed to `found`; a tag is only an error
    /// when no overload of the group carries the parameter.
    fn apply_params(
        &self,
        targets: &mut [Param],
        found: &mut Vec<String>,
        mismatched: &mut Vec<String>,
    ) {
        for ap in self.params {
            let Some(scalar) = scalar_leaf(&ap.type_ref) else {
                continue;
            };
            if let Some(tp) = targets.iter_mut().find(|tp| tp.name == ap.name) {
                match tp.type_ref.with_rust_scalar(scalar) {
                    Some(forced) => tp.type_ref = forced,
                    None => mismatched.push(mismatch(
                        scalar,
                        &format!("parameter `{}`", ap.name),
                        &tp.type_ref,
                    )),
                }
                if !found.contains(&ap.name) {
                    found.push(ap.name.clone());
                }
            }
        }
    }

    fn apply_throws(&self, throws: &mut Throws) {
        if !matches!(self.throws, Throws::None) {
            *throws = self.throws.clone();
        }
    }

    fn apply_doc(&self, doc: &mut Option<String>) {
        let Some(extra) = self.doc.map(str::trim).filter(|d| !d.is_empty()) else {
            return;
        };
        *doc = Some(match doc.take().filter(|d| !d.trim().is_empty()) {
            Some(d) => format!("{}\n\n{extra}", d.trim_end()),
            None => extra.to_string(),
        });
    }

    fn missing_params(&self, found: &[String]) -> Vec<String> {
        self.params
            .iter()
            .filter(|p| scalar_leaf(&p.type_ref).is_some() && !found.contains(&p.name))
            .map(|p| p.name.clone())
            .collect()
    }
}

/// Which target members a side-file member addresses.
#[derive(PartialEq)]
enum Key<'a> {
    Property(&'a str),
    StaticProperty(&'a str),
    Method(&'a str),
    StaticMethod(&'a str),
    Constructor,
}

impl Key<'_> {
    fn describe(&self) -> String {
        match self {
            Key::Property(n) => format!("property `{n}`"),
            Key::StaticProperty(n) => format!("static property `{n}`"),
            Key::Method(n) => format!("method `{n}`"),
            Key::StaticMethod(n) => format!("static method `{n}`"),
            Key::Constructor => "constructor".to_string(),
        }
    }
}

/// Key and overlay of a side-file member; setters and index signatures
/// carry nothing (a property's tags sit on its getter).
fn side_member(m: &Member) -> Option<(Key<'_>, Overlay<'_>)> {
    Some(match m {
        // Getters are infallible, so a side getter's `@throws` only
        // contributes its rendered doc, as it would written on the input.
        Member::Getter(g) => (
            Key::Property(&g.js_name),
            Overlay {
                own: scalar_leaf(&g.type_ref),
                params: &[],
                throws: &Throws::None,
                doc: g.doc.as_deref(),
            },
        ),
        Member::StaticGetter(g) => (
            Key::StaticProperty(&g.js_name),
            Overlay {
                own: scalar_leaf(&g.type_ref),
                params: &[],
                throws: &Throws::None,
                doc: g.doc.as_deref(),
            },
        ),
        Member::Method(m) => (
            Key::Method(&m.js_name),
            Overlay {
                own: scalar_leaf(&m.return_type),
                params: &m.params,
                throws: &m.throws,
                doc: m.doc.as_deref(),
            },
        ),
        Member::StaticMethod(m) => (
            Key::StaticMethod(&m.js_name),
            Overlay {
                own: scalar_leaf(&m.return_type),
                params: &m.params,
                throws: &m.throws,
                doc: m.doc.as_deref(),
            },
        ),
        Member::Constructor(c) => (
            Key::Constructor,
            Overlay {
                own: None,
                params: &c.params,
                throws: &c.throws,
                doc: c.doc.as_deref(),
            },
        ),
        Member::Setter(_) | Member::StaticSetter(_) | Member::IndexSignature(_) => return None,
    })
}

fn apply_members(
    gctx: &mut GlobalContext,
    module: &Module,
    name: &str,
    annotated: &[Member],
    errors: &mut Vec<String>,
) {
    let id = match find_decl(gctx, module, name) {
        Ok(id) => id,
        Err(e) => {
            errors.push(e);
            return;
        }
    };
    let members: &mut Vec<Member> = match &mut gctx.get_type_mut(id).kind {
        TypeKind::Class(c) => &mut c.members,
        TypeKind::Interface(i) => &mut i.members,
        TypeKind::DiscriminatedUnion(d) => &mut d.members,
        _ => {
            errors.push(format!("`{name}` is not a class or interface"));
            return;
        }
    };
    // Each side overload applies to every target overload; its doc is
    // appended once per member.
    let mut seen: Vec<Key<'_>> = Vec::new();
    for (key, mut overlay) in annotated.iter().filter_map(side_member) {
        if overlay.is_empty() {
            continue;
        }
        if seen.contains(&key) {
            overlay.doc = None;
        }
        let mut hit = false;
        let (mut found, mut mismatched) = (Vec::new(), Vec::new());
        // Overloads may differ in return type (`delete(key): boolean`,
        // `delete(keys): number`); a return tag applies to those that can
        // carry it and is an error only when none can.
        let (mut returns_ok, mut return_mismatches) = (false, Vec::new());
        let mut on_return = |m: Option<String>| match m {
            Some(m) => return_mismatches.push(m),
            None => returns_ok = true,
        };
        for m in members.iter_mut() {
            match (m, &key) {
                (Member::Getter(t), Key::Property(n)) if t.js_name == *n => {
                    hit = true;
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                    overlay.apply_doc(&mut t.doc);
                }
                (Member::Setter(t), Key::Property(n)) if t.js_name == *n => {
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                }
                (Member::StaticGetter(t), Key::StaticProperty(n)) if t.js_name == *n => {
                    hit = true;
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                    overlay.apply_doc(&mut t.doc);
                }
                (Member::StaticSetter(t), Key::StaticProperty(n)) if t.js_name == *n => {
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                }
                (Member::Method(t), Key::Method(n)) if t.js_name == *n => {
                    hit = true;
                    on_return(overlay.apply_return(&mut t.return_type));
                    overlay.apply_params(&mut t.params, &mut found, &mut mismatched);
                    overlay.apply_throws(&mut t.throws);
                    overlay.apply_doc(&mut t.doc);
                }
                (Member::StaticMethod(t), Key::StaticMethod(n)) if t.js_name == *n => {
                    hit = true;
                    on_return(overlay.apply_return(&mut t.return_type));
                    overlay.apply_params(&mut t.params, &mut found, &mut mismatched);
                    overlay.apply_throws(&mut t.throws);
                    overlay.apply_doc(&mut t.doc);
                }
                (Member::Constructor(t), Key::Constructor) => {
                    hit = true;
                    overlay.apply_params(&mut t.params, &mut found, &mut mismatched);
                    overlay.apply_throws(&mut t.throws);
                    overlay.apply_doc(&mut t.doc);
                }
                _ => {}
            }
        }
        if !hit {
            errors.push(format!("`{name}` has no {}", key.describe()));
        }
        for p in overlay.missing_params(&found) {
            errors.push(format!(
                "`{name}` {} has no parameter `{p}`",
                key.describe()
            ));
        }
        if overlay.own.is_some() && !returns_ok {
            mismatched.extend(return_mismatches);
        }
        mismatched.dedup();
        for m in mismatched {
            errors.push(format!("`{name}` {}: {m}", key.describe()));
        }
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
}

fn mismatch(scalar: RustScalar, what: &str, ty: &TypeRef) -> String {
    format!(
        "{what} `{}` cannot cross as `@rust {}` (u64/i64 need `number` or `bigint`, other \
         scalars `number`)",
        ty.format_ts(),
        scalar.as_str()
    )
}

/// The `@rust` scalar an annotated type carries, if any.
fn scalar_leaf(ty: &TypeRef) -> Option<RustScalar> {
    match ty {
        TypeRef::RustScalar(s) | TypeRef::WideScalar(s) => Some(*s),
        TypeRef::Array(inner) => scalar_leaf(inner),
        TypeRef::Union(u) => u.members().iter().find_map(scalar_leaf),
        TypeRef::Reference { generic_args, .. } => generic_args.iter().find_map(scalar_leaf),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::apply;

    fn errors(input: &str, side: &str) -> Vec<String> {
        let (module, mut gctx) = crate::parse::parse_single_source(input, None).unwrap();
        let (annotations, agctx) = crate::parse::parse_single_source(side, None).unwrap();
        assert!(
            !agctx.diagnostics.has_errors(),
            "side file did not parse cleanly"
        );
        apply(&mut gctx, &module, &annotations, &agctx)
    }

    const INPUT: &str = "
        interface Response { readonly status: number; redirect(url: string, status?: number): Response; }
        declare function fetch(url: string): Promise<Response>;
    ";

    #[test]
    fn clean_side_file_applies() {
        let side = "interface Response { /** @rust u16 */ readonly status: number; }";
        assert!(errors(INPUT, side).is_empty());
    }

    #[test]
    fn missing_declaration() {
        let side = "interface Nope { /** @rust u16 */ readonly status: number; }";
        assert_eq!(errors(INPUT, side), ["no declaration named `Nope`"]);
    }

    #[test]
    fn missing_member() {
        let side = "interface Response { /** @rust u16 */ readonly code: number; }";
        assert_eq!(errors(INPUT, side), ["`Response` has no property `code`"]);
    }

    #[test]
    fn missing_parameter() {
        let side = "interface Response { /** @rust code: u16 */ redirect(url: string, code?: number): Response; }";
        assert_eq!(
            errors(INPUT, side),
            ["`Response` method `redirect` has no parameter `code`"]
        );
    }

    #[test]
    fn scalar_mismatch() {
        // The side file types `url` as a number so its own tag is valid;
        // the input's `string` cannot carry it.
        let side = "/** @rust url: u32 */ declare function fetch(url: number): Promise<Response>;";
        let errs = errors(INPUT, side);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("cannot cross as `@rust u32`"), "{errs:?}");
    }

    #[test]
    fn unsupported_declaration_kind() {
        let side = "declare namespace NS { interface Inner { /** @rust u16 */ a: number; } }";
        let errs = errors(INPUT, side);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("`NS`: only classes"), "{errs:?}");
    }

    #[test]
    fn type_param_default_replaces_input() {
        let input = "
            interface Incoming { readonly colo: string; }
            interface Outgoing { readonly cacheTtl: number; }
            type Cf = Incoming | Outgoing;
            interface Request<Meta = unknown, Props = Cf> { readonly cf?: Props; }
        ";
        let side = "interface Request<Meta = unknown, Props = Incoming> {}";
        let (module, mut gctx) = crate::parse::parse_single_source(input, None).unwrap();
        let (annotations, agctx) = crate::parse::parse_single_source(side, None).unwrap();
        assert!(apply(&mut gctx, &module, &annotations, &agctx).is_empty());
        let id = module.types.iter().copied().find(|&id| {
            matches!(&gctx.get_type(id).kind, crate::ir::TypeKind::Interface(i) if i.name == "Request")
        }).unwrap();
        let crate::ir::TypeKind::Interface(req) = &gctx.get_type(id).kind else {
            unreachable!()
        };
        assert_eq!(
            req.type_params[1].default.as_ref().unwrap().format_ts(),
            "Incoming"
        );
        let bad = "interface Request<Nope = Incoming> {}";
        let (annotations, agctx) = crate::parse::parse_single_source(bad, None).unwrap();
        let errs = apply(&mut gctx, &module, &annotations, &agctx);
        assert_eq!(errs, ["`Request` has no type parameter `Nope`"]);
    }

    #[test]
    fn ambiguous_name() {
        let input = "
            interface Thing { readonly a: number; }
            declare module \"m\" { interface Thing { readonly a: number; } }
        ";
        let side = "interface Thing { /** @rust u16 */ readonly a: number; }";
        let errs = errors(input, side);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("names 2 declarations"), "{errs:?}");
    }
}
