//! `--annotate`: layer `@rust`, `@throws`, `{@string}` and `@retype`
//! tags from a side `.d.ts` onto declarations of the main input, matched
//! by declaration and member name. Upstream declaration files carry no
//! such tags, and editing a vendored copy does not survive an upgrade; a
//! side file that re-declares only the annotated members does.

use crate::context::{GlobalContext, TypeId};
use crate::ir::{
    FunctionDecl, Member, Module, Param, RustScalar, StringModeHint, Throws, TypeKind, TypeParam,
    TypeRef,
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
    apply_in(gctx, &module.types, &annotations.types, agctx, &mut errors);
    crate::parse::refresh_partials(gctx);
    errors
}

/// The side declarations `side` onto the input declarations `targets`,
/// the top level or one namespace's members.
fn apply_in(
    gctx: &mut GlobalContext,
    targets: &[TypeId],
    side: &[TypeId],
    agctx: &GlobalContext,
    errors: &mut Vec<String>,
) {
    for &aid in side {
        let adecl = agctx.get_type(aid);
        let (name, members, type_params, string_mode) = match &adecl.kind {
            TypeKind::Class(c) => (&c.name, &c.members, &c.type_params, c.string_mode),
            TypeKind::Interface(i) => (&i.name, &i.members, &i.type_params, i.string_mode),
            TypeKind::Function(f) => {
                apply_function(gctx, targets, adecl.doc.as_deref(), f, errors);
                continue;
            }
            // An alias carries no members; only its type-parameter
            // defaults can be overridden (`RequestInfo<…, Cf = …>`).
            TypeKind::TypeAlias(a) if a.type_params.iter().any(|tp| tp.default.is_some()) => {
                apply_type_param_defaults(gctx, targets, &a.name, &a.type_params, errors);
                continue;
            }
            TypeKind::Namespace(ns) => {
                match find_decl(gctx, targets, &ns.name) {
                    Ok(id) => match &gctx.get_type(id).kind {
                        TypeKind::Namespace(target) => {
                            let inner = target.declarations.clone();
                            apply_in(gctx, &inner, &ns.declarations, agctx, errors);
                        }
                        _ => errors.push(format!("`{}` is not a namespace", ns.name)),
                    },
                    Err(e) => errors.push(e),
                }
                continue;
            }
            // The side file is a curated list: a declaration this pass
            // cannot carry tags for must not be silently skipped.
            other => {
                let name = crate::parse::declaration_name(other).unwrap_or_default();
                errors.push(format!(
                    "`{name}`: only classes, interfaces, functions and namespaces can be \
                     annotated (modules, aliases and variables are not scoped yet)"
                ));
                continue;
            }
        };
        apply_members(gctx, targets, name, members, errors);
        apply_type_param_defaults(gctx, targets, name, type_params, errors);
        apply_string_default(gctx, targets, name, string_mode, errors);
    }
}

/// Every side declaration of a function applies to every input
/// declaration of it, as side method overloads do in `apply_members`.
fn apply_function(
    gctx: &mut GlobalContext,
    targets: &[TypeId],
    doc: Option<&str>,
    f: &FunctionDecl,
    errors: &mut Vec<String>,
) {
    let side: Vec<Overlay<'_>> = std::iter::once(Overlay::new(
        &f.return_type,
        &f.params,
        &f.throws,
        f.string_mode,
        doc,
    ))
    .chain(f.overloads.iter().map(|o| {
        Overlay::new(
            &o.return_type,
            &o.params,
            &o.throws,
            o.string_mode,
            o.doc.as_deref(),
        )
    }))
    .filter(|o| !o.is_empty())
    .collect();
    if side.is_empty() {
        return;
    }
    let id = match find_decl(gctx, targets, &f.js_name) {
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
            &mut func.string_mode,
        ))
        .chain(func.overloads.iter_mut().map(|o| {
            (
                &mut o.return_type,
                &mut o.params,
                &mut o.throws,
                &mut o.doc,
                &mut o.string_mode,
            )
        }));
        for (ret, params, throws, doc, string_mode) in targets {
            overlay.apply_retype(ret);
            overlay.apply_string_mode(string_mode);
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
    targets: &[TypeId],
    name: &str,
    side: &[TypeParam],
    errors: &mut Vec<String>,
) {
    if side.iter().all(|tp| tp.default.is_none()) {
        return;
    }
    let Ok(id) = find_decl(gctx, targets, name) else {
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

/// A side declaration's `{@string mode}` is the default of every input
/// member of it that no tag (input or side) has set.
fn apply_string_default(
    gctx: &mut GlobalContext,
    targets: &[TypeId],
    name: &str,
    mode: Option<StringModeHint>,
    errors: &mut Vec<String>,
) {
    if mode.is_none() {
        return;
    }
    let id = match find_decl(gctx, targets, name) {
        Ok(id) => id,
        Err(e) => {
            errors.push(e);
            return;
        }
    };
    match &mut gctx.get_type_mut(id).kind {
        TypeKind::Class(c) => crate::parse::members::apply_string_default(&mut c.members, mode),
        TypeKind::Interface(i) => crate::parse::members::apply_string_default(&mut i.members, mode),
        TypeKind::DiscriminatedUnion(d) => {
            crate::parse::members::apply_string_default(&mut d.members, mode)
        }
        _ => errors.push(format!("`{name}` is not a class or interface")),
    }
}

/// The input declaration a side name targets. A name declared more than
/// once (globally and in a module) cannot be targeted by bare name.
fn find_decl(gctx: &GlobalContext, targets: &[TypeId], name: &str) -> Result<TypeId, String> {
    let matches: Vec<TypeId> = targets
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
                TypeKind::Namespace(n) => n.name == name,
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
    /// `@retype`: the side member's own type replaces the property type or
    /// callable return, for an input type that erases what the API
    /// produces (`get body(): ReadableStream | null` of bytes).
    retype: Option<&'a TypeRef>,
    /// Parameters, of which only `@rust`- or `{@slice}`-tagged ones are
    /// applied.
    params: &'a [Param],
    throws: &'a Throws,
    /// `{@string mode}` for the property type or callable return.
    string_mode: Option<StringModeHint>,
    /// Rendered side doc (the `## Errors` section of `@throws`), appended
    /// to the target's doc.
    doc: Option<std::borrow::Cow<'a, str>>,
}

const RETYPE_TAG: &str = "@retype";

/// The side doc without a `@retype` line (an unknown tag the renderer
/// passes through), and whether it had one.
fn take_retype(doc: Option<&str>) -> (Option<std::borrow::Cow<'_, str>>, bool) {
    let Some(doc) = doc else {
        return (None, false);
    };
    if !doc.lines().any(|l| l.trim() == RETYPE_TAG) {
        return (Some(doc.into()), false);
    }
    let rest: Vec<&str> = doc.lines().filter(|l| l.trim() != RETYPE_TAG).collect();
    (Some(rest.join("\n").into()), true)
}

impl<'a> Overlay<'a> {
    fn new(
        own_type: &'a TypeRef,
        params: &'a [Param],
        throws: &'a Throws,
        string_mode: Option<StringModeHint>,
        doc: Option<&'a str>,
    ) -> Self {
        let (doc, retype) = take_retype(doc);
        Overlay {
            own: scalar_leaf(own_type),
            retype: retype.then_some(own_type),
            params,
            throws,
            string_mode,
            doc,
        }
    }
}

impl Overlay<'_> {
    fn apply_retype(&self, ty: &mut TypeRef) {
        if let Some(side) = self.retype {
            *ty = side.clone();
        }
    }

    fn is_empty(&self) -> bool {
        self.own.is_none()
            && self.retype.is_none()
            && !self.params.iter().any(is_tagged)
            && matches!(self.throws, Throws::None)
            && self.string_mode.is_none()
            && self.doc.as_deref().is_none_or(|d| d.trim().is_empty())
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
        for ap in self.params.iter().filter(|p| is_tagged(p)) {
            if let Some(tp) = targets.iter_mut().find(|tp| tp.name == ap.name) {
                if let Some(scalar) = scalar_leaf(&ap.type_ref) {
                    match tp.type_ref.with_rust_scalar(scalar) {
                        Some(forced) => tp.type_ref = forced,
                        None => mismatched.push(mismatch(
                            scalar,
                            &format!("parameter `{}`", ap.name),
                            &tp.type_ref,
                        )),
                    }
                }
                if let Some(hint) = ap.slice_mode {
                    tp.slice_mode = Some(hint);
                    if let Some(widened) = hint.widen_bare_typed_array(&tp.type_ref) {
                        tp.type_ref = widened;
                    }
                }
                if !found.contains(&ap.name) {
                    found.push(ap.name.clone());
                }
            }
        }
    }

    fn apply_string_mode(&self, mode: &mut Option<StringModeHint>) {
        if self.string_mode.is_some() {
            *mode = self.string_mode;
        }
    }

    fn apply_throws(&self, throws: &mut Throws) {
        if !matches!(self.throws, Throws::None) {
            *throws = self.throws.clone();
        }
    }

    fn apply_doc(&self, doc: &mut Option<String>) {
        let Some(extra) = self.doc.as_deref().map(str::trim).filter(|d| !d.is_empty()) else {
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
            .filter(|p| is_tagged(p) && !found.contains(&p.name))
            .map(|p| p.name.clone())
            .collect()
    }
}

fn is_tagged(p: &Param) -> bool {
    scalar_leaf(&p.type_ref).is_some() || p.slice_mode.is_some()
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
        Member::Getter(g) => (
            Key::Property(&g.js_name),
            Overlay::new(&g.type_ref, &[], &g.throws, g.string_mode, g.doc.as_deref()),
        ),
        Member::StaticGetter(g) => (
            Key::StaticProperty(&g.js_name),
            Overlay::new(&g.type_ref, &[], &g.throws, g.string_mode, g.doc.as_deref()),
        ),
        Member::Method(m) => (
            Key::Method(&m.js_name),
            Overlay::new(
                &m.return_type,
                &m.params,
                &m.throws,
                m.string_mode,
                m.doc.as_deref(),
            ),
        ),
        Member::StaticMethod(m) => (
            Key::StaticMethod(&m.js_name),
            Overlay::new(
                &m.return_type,
                &m.params,
                &m.throws,
                m.string_mode,
                m.doc.as_deref(),
            ),
        ),
        Member::Constructor(c) => (
            Key::Constructor,
            Overlay {
                own: None,
                retype: None,
                params: &c.params,
                throws: &c.throws,
                string_mode: None,
                doc: c.doc.as_deref().map(Into::into),
            },
        ),
        Member::Setter(_) | Member::StaticSetter(_) | Member::IndexSignature(_) => return None,
    })
}

fn apply_members(
    gctx: &mut GlobalContext,
    targets: &[TypeId],
    name: &str,
    annotated: &[Member],
    errors: &mut Vec<String>,
) {
    let id = match find_decl(gctx, targets, name) {
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
                    overlay.apply_retype(&mut t.type_ref);
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                    overlay.apply_string_mode(&mut t.string_mode);
                    overlay.apply_throws(&mut t.throws);
                    overlay.apply_doc(&mut t.doc);
                }
                (Member::Setter(t), Key::Property(n)) if t.js_name == *n => {
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                }
                (Member::StaticGetter(t), Key::StaticProperty(n)) if t.js_name == *n => {
                    hit = true;
                    overlay.apply_retype(&mut t.type_ref);
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                    overlay.apply_string_mode(&mut t.string_mode);
                    overlay.apply_throws(&mut t.throws);
                    overlay.apply_doc(&mut t.doc);
                }
                (Member::StaticSetter(t), Key::StaticProperty(n)) if t.js_name == *n => {
                    mismatched.extend(overlay.apply_return(&mut t.type_ref));
                }
                (Member::Method(t), Key::Method(n)) if t.js_name == *n => {
                    hit = true;
                    overlay.apply_retype(&mut t.return_type);
                    on_return(overlay.apply_return(&mut t.return_type));
                    overlay.apply_string_mode(&mut t.string_mode);
                    overlay.apply_params(&mut t.params, &mut found, &mut mismatched);
                    overlay.apply_throws(&mut t.throws);
                    overlay.apply_doc(&mut t.doc);
                }
                (Member::StaticMethod(t), Key::StaticMethod(n)) if t.js_name == *n => {
                    hit = true;
                    overlay.apply_retype(&mut t.return_type);
                    on_return(overlay.apply_return(&mut t.return_type));
                    overlay.apply_string_mode(&mut t.string_mode);
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
        TypeRef::Nullable(inner, _) | TypeRef::Array(inner) => scalar_leaf(inner),
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
    fn slice_tag_counts_as_a_tag() {
        let side = "interface Response { /** @param code {@slice view} */ redirect(url: string, code?: number): Response; }";
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
        let side = "type Alias = string;";
        let errs = errors(INPUT, side);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("`Alias`: only classes"), "{errs:?}");
    }

    #[test]
    fn namespace_members_apply() {
        let input = "declare namespace NS { interface Inner { readonly a: number; } }";
        let side =
            "declare namespace NS { interface Inner { /** @rust u16 */ readonly a: number; } }";
        assert!(errors(input, side).is_empty());
        let side =
            "declare namespace NS { interface Nope { /** @rust u16 */ readonly a: number; } }";
        assert_eq!(errors(input, side), ["no declaration named `Nope`"]);
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
