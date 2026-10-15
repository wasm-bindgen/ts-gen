//! `--provides`: what the module provides to JS, the reverse of the import
//! bindings (wasm-bindgen's export direction). The spec is either a `.d.ts`
//! whose exports *are* the module's exports, or `Name[.member,...]`, an
//! ambient interface the default export implements. Each resolved function
//! sits at a JS path such as `default.fetch` or `api.math.add`, receives its
//! parameters by value and returns an owned result.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use oxc_allocator::Allocator;
use oxc_ast::ast;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::context::GlobalContext;
use crate::ir::{InterfaceDecl, Member, Module, Nullability, Param, TypeKind, TypeRef};
use crate::parse::docs::DocComments;
use crate::parse::scope::ScopeId;
use crate::parse::types::{convert_formal_params_with_arena, convert_ts_type_with_arena};

/// One function the module provides.
#[derive(Clone, Debug)]
pub struct ProvidedFn {
    /// JS path from the module namespace: `["default", "fetch"]`,
    /// `["fetch"]`, `["api", "math", "add"]`.
    pub path: Vec<String>,
    pub params: Vec<Param>,
    /// The result as the module produces it. `T | Promise<T>` is recorded
    /// as `Promise<T>`: the provider may always return the promise.
    pub return_type: TypeRef,
    pub doc: Option<String>,
    /// Scope the types resolve in.
    pub scope: ScopeId,
    /// The Rust name, assigned by [`resolve`]: the path in snake case
    /// (`api_math_add`), with a leading `default` dropped when no other
    /// provider takes that name (`fetch` for `default.fetch`, but
    /// `default_add` beside a top-level `add`).
    pub rust_name: String,
}

impl ProvidedFn {
    pub fn js_path(&self) -> String {
        self.path.join(".")
    }

    /// The last path segment: the function's own name.
    pub fn leaf(&self) -> &str {
        self.path.last().map_or("default", String::as_str)
    }

    fn snake_path(&self, skip_default: bool) -> String {
        let segments: &[String] = match self.path.as_slice() {
            [first, rest @ ..] if skip_default && first == "default" && !rest.is_empty() => rest,
            all => all,
        };
        segments
            .iter()
            .map(|s| crate::util::naming::to_snake_case(s))
            .collect::<Vec<_>>()
            .join("_")
    }

    /// For a `Promise<T>` result, `T`.
    pub fn promise_inner(&self) -> Option<&TypeRef> {
        match self.return_type.as_generic_head("Promise") {
            Some([inner]) => Some(inner),
            _ => None,
        }
    }
}

/// Resolve a provides spec against the parsed `-i` declarations.
pub fn resolve(spec: &str, module: &Module, gctx: &mut GlobalContext) -> Result<Vec<ProvidedFn>> {
    let path = Path::new(spec);
    let mut fns = if path.is_file() {
        from_file(path, module, gctx)?
    } else {
        from_shorthand(spec, module, gctx)?
    };
    if fns.is_empty() {
        bail!("--provides {spec:?}: nothing to provide");
    }
    let mut seen = HashMap::new();
    for f in &fns {
        if let Some(prev) = seen.insert(f.js_path(), f.js_path()) {
            bail!("provides {prev:?} twice");
        }
        if f.params.iter().any(|p| p.variadic) {
            bail!(
                "provides {}: variadic parameters are not supported",
                f.js_path()
            );
        }
    }
    assign_rust_names(&mut fns)?;
    Ok(fns)
}

/// `default.<name>` is spelled `name` unless some other provider already
/// has that Rust name, in which case it keeps `default_`. Any clash left
/// after that (`foo.bar` beside `foo_bar`) is an error naming both paths.
fn assign_rust_names(fns: &mut [ProvidedFn]) -> Result<()> {
    let full: Vec<String> = fns.iter().map(|f| f.snake_path(false)).collect();
    let mut taken: HashMap<String, String> = HashMap::new();
    for (f, name) in fns.iter().zip(&full) {
        if !f.path.first().is_some_and(|s| s == "default") {
            taken.insert(name.clone(), f.js_path());
        }
    }
    for f in fns.iter_mut() {
        let short = f.snake_path(true);
        let name = if short != f.snake_path(false) && taken.contains_key(&short) {
            f.snake_path(false)
        } else {
            short
        };
        f.rust_name = name;
    }
    let mut by_name: HashMap<&str, &ProvidedFn> = HashMap::new();
    for f in fns.iter() {
        if let Some(prev) = by_name.insert(&f.rust_name, f) {
            bail!(
                "provides {:?} and {:?} would both be `{}` in Rust",
                prev.js_path(),
                f.js_path(),
                f.rust_name
            );
        }
    }
    Ok(())
}

/// `Name[.member,...]`: the default export is an object implementing the
/// ambient interface `Name`. Required members are always provided;
/// optional ones only when named.
fn from_shorthand(
    spec: &str,
    module: &Module,
    gctx: &mut GlobalContext,
) -> Result<Vec<ProvidedFn>> {
    let (name, selected) = match spec.split_once('.') {
        Some((n, rest)) => (n, rest.split(',').map(str::trim).collect::<Vec<_>>()),
        None => (spec, Vec::new()),
    };
    let scope = types_scope(module);
    let Some(type_id) = gctx.scopes.resolve(scope, name) else {
        bail!(
            "--provides {spec:?}: {name:?} is not a file and no interface of that name is declared in the inputs"
        );
    };
    let TypeKind::Interface(iface) = &gctx.get_type(type_id).kind else {
        bail!("--provides {spec:?}: {name:?} is not an interface");
    };
    let iface = iface.clone();
    let mut out = Vec::new();
    Members {
        gctx,
        selected: &selected,
        type_args: &[],
    }
    .provide(&["default".to_string()], &iface, &mut out)?;
    Ok(out)
}

/// Where the inputs' types resolve from: ambient declarations are hoisted
/// to the global scope, a module-mode input (`--lib-name`) keeps its own
/// file scope, whose parent is the global one.
fn types_scope(module: &Module) -> ScopeId {
    module
        .file_scopes
        .first()
        .copied()
        .unwrap_or(module.global_scope)
}

fn member_name(m: &Member) -> Option<&str> {
    match m {
        Member::Method(m) => Some(&m.js_name),
        Member::Getter(g) => Some(&g.js_name),
        _ => None,
    }
}

/// Walks an interface's members into provided functions.
struct Members<'a> {
    gctx: &'a mut GlobalContext,
    /// Optional members to include; the rest are skipped.
    selected: &'a [&'a str],
    /// Type arguments for the interface's type parameters, in order.
    type_args: &'a [TypeRef],
}

impl Members<'_> {
    /// The function members of `iface`, as providers under `path`. Each
    /// selected member must be providable, otherwise it is an error.
    fn provide(
        &mut self,
        path: &[String],
        iface: &InterfaceDecl,
        out: &mut Vec<ProvidedFn>,
    ) -> Result<()> {
        for sel in self.selected {
            if !iface.members.iter().any(|m| member_name(m) == Some(*sel)) {
                bail!(
                    "--provides: {}.{sel} is not a member of {}",
                    iface.js_name,
                    iface.js_name
                );
            }
        }
        // The interface's own type parameters: the given arguments, else
        // the declared default, else `unknown`.
        let subst: HashMap<&str, TypeRef> = iface
            .type_params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let ty = self
                    .type_args
                    .get(i)
                    .cloned()
                    .or_else(|| p.default.clone())
                    .unwrap_or(TypeRef::Unknown);
                (p.name.as_str(), ty)
            })
            .collect();
        for m in &iface.members {
            let (js_name, optional, doc, ty, params, ret, scope) = match m {
                Member::Method(m) => {
                    if !m.type_params.is_empty() {
                        self.gctx.warn(format!(
                            "provides {}.{}: generic methods are not supported, skipping",
                            path.join("."),
                            m.js_name
                        ));
                        continue;
                    }
                    (
                        &m.js_name,
                        m.optional,
                        &m.doc,
                        None,
                        Some(&m.params),
                        Some(&m.return_type),
                        m.body_scope,
                    )
                }
                Member::Getter(g) => (
                    &g.js_name,
                    g.optional,
                    &g.doc,
                    Some(&g.type_ref),
                    None,
                    None,
                    g.scope,
                ),
                Member::Setter(_) => continue,
                other => {
                    self.gctx.warn(format!(
                        "provides {}: {} members cannot be provided, skipping",
                        path.join("."),
                        member_kind(other)
                    ));
                    continue;
                }
            };
            let selected = self.selected.contains(&js_name.as_str());
            if optional && !selected {
                self.gctx.warn(format!(
                    "provides {}: optional member {}.{} not selected, skipping",
                    path.join("."),
                    iface.js_name,
                    js_name
                ));
                continue;
            }
            let child = child(path, js_name);
            let (params, ret) = match (params, ret, ty) {
                (Some(params), Some(ret), _) => (params.clone(), ret.clone()),
                // A property: optional-ness was folded into its type, and
                // the type may be an alias to a function type.
                (_, _, Some(ty)) => {
                    let ty = match ty.split_empties() {
                        Some((inner, _)) if optional => inner,
                        _ => ty.clone(),
                    };
                    match self.function_type(&ty, scope) {
                        Some(sig) => sig,
                        None if selected => bail!(
                            "--provides: {} is not a function (`{}`)",
                            child.join("."),
                            ty.format_ts()
                        ),
                        None => {
                            self.gctx.warn(format!(
                                "provides {}: not a function, skipping property",
                                child.join(".")
                            ));
                            continue;
                        }
                    }
                }
                _ => unreachable!(),
            };
            let params = params
                .iter()
                .map(|p| Param {
                    type_ref: p.type_ref.substitute(&subst),
                    ..p.clone()
                })
                .collect();
            out.push(ProvidedFn {
                path: child,
                params,
                return_type: normalise_result(ret.substitute(&subst)),
                doc: doc.clone(),
                scope,
                rust_name: String::new(),
            });
        }
        Ok(())
    }

    /// The signature a type denotes, through aliases including generic
    /// instantiations (`Handler<Env>` for `type Handler<E> = (..) => ..`).
    fn function_type(&self, ty: &TypeRef, scope: ScopeId) -> Option<(Vec<Param>, TypeRef)> {
        match ty {
            TypeRef::Function(sig) => Some((sig.params.clone(), (*sig.return_type).clone())),
            TypeRef::Reference {
                segments,
                generic_args,
            } if segments.len() == 1 => {
                let type_id = self.gctx.scopes.resolve(scope, &segments[0])?;
                let TypeKind::TypeAlias(alias) = &self.gctx.get_type(type_id).kind else {
                    return None;
                };
                let subst: HashMap<&str, TypeRef> = alias
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let ty = generic_args
                            .get(i)
                            .cloned()
                            .or_else(|| p.default.clone())
                            .unwrap_or(TypeRef::Unknown);
                        (p.name.as_str(), ty)
                    })
                    .collect();
                let target = alias.target.substitute(&subst);
                self.function_type(&target, alias.body_scope)
            }
            _ => None,
        }
    }
}

/// `T | Promise<T>` is provided as `Promise<T>`; `Promise<T>` and plain
/// types pass through. `void | Promise<void>` arrives as the promise
/// with an `undefined` arm, since `void` coalesces like `undefined`.
fn normalise_result(ret: TypeRef) -> TypeRef {
    if let Some((inner, Nullability::Undefined)) = ret.split_empties() {
        if inner.as_generic_head("Promise") == Some(&[TypeRef::Void][..]) {
            return inner;
        }
    }
    let TypeRef::Union(union) = &ret else {
        return ret;
    };
    let members = union.members();
    let promise = members
        .iter()
        .find_map(|m| m.as_generic_head("Promise").map(|a| a.to_vec()));
    match promise {
        Some(args)
            if members
                .iter()
                .all(|m| m.as_generic_head("Promise").is_some() || Some(m) == args.first()) =>
        {
            TypeRef::generic("Promise", args)
        }
        _ => ret,
    }
}

fn member_kind(m: &Member) -> &'static str {
    match m {
        Member::Method(_) => "method",
        Member::Getter(_) | Member::Setter(_) => "property",
        Member::Constructor(_) => "constructor",
        Member::StaticMethod(_) | Member::StaticGetter(_) | Member::StaticSetter(_) => "static",
        Member::IndexSignature(_) => "index signature",
    }
}

fn child(path: &[String], name: &str) -> Vec<String> {
    let mut p = path.to_vec();
    p.push(name.to_string());
    p
}

/// A `.d.ts` whose export surface is the module's. Types resolve against
/// the inputs through a scope under theirs. A default export of an
/// interface type may select optional members with a JSDoc
/// `@provides a, b` on the `export default`.
fn from_file(path: &Path, module: &Module, gctx: &mut GlobalContext) -> Result<Vec<ProvidedFn>> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading --provides {}", path.display()))?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &source, SourceType::d_ts()).parse();
    if let Some(e) = parsed.diagnostics.first() {
        bail!("--provides {}: {e}", path.display());
    }
    let program = &parsed.program;
    let docs = DocComments::new(&program.comments, &source);
    let scope = gctx.create_child_scope(types_scope(module));

    // `export default x` names a `declare const x: T` in the same file.
    let mut consts: Vec<(&str, &ast::TSType<'_>, Option<String>)> = Vec::new();
    for stmt in &program.body {
        let ast::Statement::VariableDeclaration(var) = stmt else {
            continue;
        };
        for d in &var.declarations {
            if let (Some(name), Some(ann)) = (d.id.get_identifier_name(), &d.type_annotation) {
                consts.push((
                    name.as_str(),
                    &ann.type_annotation,
                    docs.for_span(var.span.start),
                ));
            }
        }
    }

    let mut w = Walker {
        gctx,
        scope,
        docs: &docs,
        out: Vec::new(),
    };
    for stmt in &program.body {
        match stmt {
            ast::Statement::ExportDeclaration(export) => match &export.declaration {
                ast::Declaration::FunctionDeclaration(f) => {
                    let Some(id) = &f.id else { continue };
                    w.function(
                        vec![id.name.to_string()],
                        &f.params,
                        f.return_type.as_deref().map(|r| &r.type_annotation),
                        docs.for_span(export.span.start),
                        !f.type_parameters
                            .as_ref()
                            .is_none_or(|t| t.params.is_empty()),
                    );
                }
                ast::Declaration::VariableDeclaration(var) => {
                    for d in &var.declarations {
                        let Some(name) = d.id.get_identifier_name() else {
                            continue;
                        };
                        let doc = docs.for_span(export.span.start);
                        match &d.type_annotation {
                            Some(ann) => w.value(
                                vec![name.to_string()],
                                &ann.type_annotation,
                                doc.as_deref(),
                            )?,
                            None => {
                                bail!("--provides: `export const {name}` needs a type annotation")
                            }
                        }
                    }
                }
                other => bail!(
                    "--provides: only functions and typed consts can be exported, found {}",
                    declaration_kind(other)
                ),
            },
            ast::Statement::ExportNamedDeclaration(_) => {
                bail!("--provides: `export {{ ... }}` lists are not supported; export the declarations directly");
            }
            ast::Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ast::ExportDefaultDeclarationKind::FunctionDeclaration(f) => w.function(
                    vec!["default".into()],
                    &f.params,
                    f.return_type.as_deref().map(|r| &r.type_annotation),
                    docs.for_span(export.span.start),
                    !f.type_parameters
                        .as_ref()
                        .is_none_or(|t| t.params.is_empty()),
                ),
                ast::ExportDefaultDeclarationKind::Identifier(id) => {
                    let Some((_, ty, const_doc)) =
                        consts.iter().find(|(n, _, _)| *n == id.name.as_str())
                    else {
                        bail!(
                            "--provides: `export default {}` must name a `declare const {}: T` in the same file",
                            id.name,
                            id.name
                        );
                    };
                    let doc = docs
                        .for_span(export.span.start)
                        .or_else(|| const_doc.clone());
                    w.value(vec!["default".into()], ty, doc.as_deref())?;
                }
                other => bail!(
                    "--provides: unsupported `export default` form ({})",
                    crate::parse::first_pass::converters::export_default_kind_name(other)
                ),
            },
            _ => {}
        }
    }
    Ok(w.out)
}

fn declaration_kind(d: &ast::Declaration<'_>) -> &'static str {
    match d {
        ast::Declaration::ClassDeclaration(_) => "class",
        ast::Declaration::TSInterfaceDeclaration(_) => "interface",
        ast::Declaration::TSTypeAliasDeclaration(_) => "type alias",
        ast::Declaration::TSEnumDeclaration(_) => "enum",
        ast::Declaration::TSNamespaceDeclaration(_)
        | ast::Declaration::TSExternalModuleDeclaration(_)
        | ast::Declaration::TSGlobalDeclaration(_) => "namespace",
        _ => "declaration",
    }
}

/// `@provides a, b` in a JSDoc block: the optional members to include.
fn selected_from_doc(doc: Option<&str>) -> Vec<String> {
    doc.into_iter()
        .flat_map(str::lines)
        .filter_map(|l| l.trim().strip_prefix("@provides"))
        .flat_map(|rest| rest.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

struct Walker<'a, 'd> {
    gctx: &'a mut GlobalContext,
    scope: ScopeId,
    docs: &'a DocComments<'d>,
    out: Vec<ProvidedFn>,
}

impl Walker<'_, '_> {
    fn function(
        &mut self,
        path: Vec<String>,
        params: &ast::FormalParameters<'_>,
        ret: Option<&ast::TSType<'_>>,
        doc: Option<String>,
        generic: bool,
    ) {
        if generic {
            self.gctx.warn(format!(
                "provides {}: generic functions are not supported, skipping",
                path.join(".")
            ));
            return;
        }
        let mut diag = std::mem::take(&mut self.gctx.diagnostics);
        let params =
            convert_formal_params_with_arena(params, self.scope, &mut self.gctx.scopes, &mut diag);
        let return_type = ret.map_or(TypeRef::Void, |r| {
            convert_ts_type_with_arena(r, self.scope, &mut self.gctx.scopes, &mut diag)
        });
        self.gctx.diagnostics = diag;
        self.out.push(ProvidedFn {
            path,
            params,
            return_type: normalise_result(return_type),
            doc: strip_provides_tag(doc),
            scope: self.scope,
            rust_name: String::new(),
        });
    }

    /// A typed value export: a function type, an object literal (nested
    /// objects allowed), or a reference to an interface, possibly with
    /// type arguments.
    fn value(&mut self, path: Vec<String>, ty: &ast::TSType<'_>, doc: Option<&str>) -> Result<()> {
        match ty {
            ast::TSType::TSFunctionType(f) => {
                self.function(
                    path,
                    &f.params,
                    Some(&f.return_type.type_annotation),
                    doc.map(str::to_string),
                    false,
                );
                Ok(())
            }
            ast::TSType::TSTypeLiteral(lit) => {
                for sig in &lit.members {
                    match sig {
                        ast::TSSignature::TSMethodSignature(m) => {
                            let Some(name) = crate::parse::members::property_key_name(&m.key)
                            else {
                                continue;
                            };
                            self.function(
                                child(&path, &name),
                                &m.params,
                                m.return_type.as_deref().map(|r| &r.type_annotation),
                                self.docs.for_span(m.span.start),
                                !m.type_parameters
                                    .as_ref()
                                    .is_none_or(|t| t.params.is_empty()),
                            );
                        }
                        ast::TSSignature::TSPropertySignature(p) => {
                            let Some(name) = crate::parse::members::property_key_name(&p.key)
                            else {
                                continue;
                            };
                            let doc = self.docs.for_span(p.span.start);
                            match &p.type_annotation {
                                Some(ann) => self.value(
                                    child(&path, &name),
                                    &ann.type_annotation,
                                    doc.as_deref(),
                                )?,
                                None => bail!(
                                    "--provides: `{}` needs a type annotation",
                                    child(&path, &name).join(".")
                                ),
                            }
                        }
                        _ => bail!(
                            "--provides: `{}` may only contain methods and properties",
                            path.join(".")
                        ),
                    }
                }
                Ok(())
            }
            ast::TSType::TSTypeReference(r) => {
                let ast::TSTypeName::IdentifierReference(id) = &r.type_name else {
                    bail!(
                        "--provides: `{}` must reference an interface by name",
                        path.join(".")
                    );
                };
                let Some(type_id) = self.gctx.scopes.resolve(self.scope, &id.name) else {
                    bail!("--provides: `{}`: unknown type {}", path.join("."), id.name);
                };
                let TypeKind::Interface(iface) = &self.gctx.get_type(type_id).kind else {
                    bail!(
                        "--provides: `{}`: {} is not an interface",
                        path.join("."),
                        id.name
                    );
                };
                let iface = iface.clone();
                let mut diag = std::mem::take(&mut self.gctx.diagnostics);
                let type_args: Vec<TypeRef> = r
                    .type_arguments
                    .as_ref()
                    .map(|args| {
                        args.params
                            .iter()
                            .map(|t| {
                                convert_ts_type_with_arena(
                                    t,
                                    self.scope,
                                    &mut self.gctx.scopes,
                                    &mut diag,
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.gctx.diagnostics = diag;
                let selected = selected_from_doc(doc);
                let selected: Vec<&str> = selected.iter().map(String::as_str).collect();
                Members {
                    gctx: self.gctx,
                    selected: &selected,
                    type_args: &type_args,
                }
                .provide(&path, &iface, &mut self.out)
            }
            ast::TSType::TSUnionType(u)
                if u.types.iter().any(|t| {
                    matches!(t, ast::TSType::TSFunctionType(_))
                        || matches!(t, ast::TSType::TSParenthesizedType(p) if matches!(p.type_annotation, ast::TSType::TSFunctionType(_)))
                }) =>
            {
                self.gctx.warn(format!(
                    "provides {}: a nullable function type cannot be provided (the function is always present), skipping",
                    path.join(".")
                ));
                Ok(())
            }
            _ => {
                self.gctx.warn(format!(
                    "provides {}: only functions and objects can be provided, skipping",
                    path.join(".")
                ));
                Ok(())
            }
        }
    }
}

/// Drop `@provides` lines from a doc comment kept for the scaffold.
fn strip_provides_tag(doc: Option<String>) -> Option<String> {
    let doc = doc?;
    let kept: Vec<&str> = doc
        .lines()
        .filter(|l| !l.trim().starts_with("@provides"))
        .collect();
    let joined = kept.join("\n").trim().to_string();
    (!joined.is_empty()).then_some(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Nullability;

    const WORKER: &str = r#"
        declare class Request<Cf = unknown> { constructor(url: string); readonly url: string; }
        declare class Response { constructor(body: string); readonly status: number; }
        declare class ExecutionContext<Props = unknown> { waitUntil(p: Promise<any>): void; }
        interface Env { [key: string]: any; }
        interface MyEnv { readonly KV: string; }
        type FetchHandler<E = unknown, Cf = unknown> = (request: Request<Cf>, env: E, ctx: ExecutionContext) => Response | Promise<Response>;
        type TailHandler<E = unknown> = (events: TraceItem[], env: E) => void | Promise<void>;
        interface ExportedHandler<E = unknown, Cf = unknown> {
            /** Handles a request. */
            fetch?: FetchHandler<E, Cf>;
            tail?: TailHandler<E>;
            scheduled?(controller: ScheduledController, env: E): Promise<void>;
            /** Always there. */
            health(): string;
            readonly name: string;
        }
        interface ScheduledController { readonly cron: string; }
        interface TraceItem { readonly outcome: string; }
    "#;

    fn with_spec(spec: &str) -> Result<(Vec<ProvidedFn>, GlobalContext)> {
        let (module, mut gctx) = crate::parse_source(WORKER, None).unwrap();
        let fns = resolve(spec, &module, &mut gctx)?;
        Ok((fns, gctx))
    }

    fn with_file(src: &str) -> Result<(Vec<ProvidedFn>, GlobalContext)> {
        let dir = std::env::temp_dir().join(format!("ts-gen-provides-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{:x}.d.ts", {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            src.hash(&mut h);
            h.finish()
        }));
        std::fs::write(&path, src).unwrap();
        with_spec(path.to_str().unwrap())
    }

    fn paths(fns: &[ProvidedFn]) -> Vec<String> {
        fns.iter().map(ProvidedFn::js_path).collect()
    }

    #[test]
    fn shorthand_selects_optional_members_through_generic_aliases() {
        let (fns, gctx) = with_spec("ExportedHandler.fetch,tail").unwrap();
        assert_eq!(
            paths(&fns),
            ["default.fetch", "default.tail", "default.health"]
        );
        let fetch = &fns[0];
        assert_eq!(fetch.rust_name, "fetch");
        // `Response | Promise<Response>` is provided as the promise.
        assert_eq!(
            fetch.promise_inner().map(TypeRef::format_ts).as_deref(),
            Some("Response")
        );
        // E and Cf default to `unknown`; Request<Cf> keeps its instantiation.
        assert_eq!(fetch.params[0].type_ref.format_ts(), "Request<unknown>");
        assert_eq!(fetch.params[1].type_ref, TypeRef::Unknown);
        assert_eq!(fetch.doc.as_deref(), Some("Handles a request."));
        assert_eq!(
            fns[1].promise_inner().map(TypeRef::format_ts).as_deref(),
            Some("void")
        );
        assert_eq!(fns[2].return_type, TypeRef::String);
        let warnings = format!("{:?}", gctx.diagnostics);
        assert!(warnings.contains("ExportedHandler.scheduled"), "{warnings}");
        assert!(
            warnings.contains("default.name: not a function"),
            "{warnings}"
        );
    }

    #[test]
    fn shorthand_errors() {
        let err = with_spec("Nope").unwrap_err().to_string();
        assert!(err.contains("no interface of that name"), "{err}");
        let err = with_spec("ExportedHandler.nope").unwrap_err().to_string();
        assert!(err.contains("not a member"), "{err}");
        let err = with_spec("Request").unwrap_err().to_string();
        assert!(err.contains("not an interface"), "{err}");
        // A selected member that is not a function is an error, not a skip.
        let err = with_spec("ExportedHandler.name").unwrap_err().to_string();
        assert!(err.contains("default.name is not a function"), "{err}");
    }

    #[test]
    fn file_named_default_nested_and_selected() {
        let (fns, _) = with_file(
            r#"
            /** Plain named export. */
            export function fetch(request: Request, flag?: boolean): Response;
            export const api: { math: { add(a: number, b: number): number }; str: { add(s: string | null): string } };
            /** @provides fetch */
            declare const handler: ExportedHandler<MyEnv>;
            export default handler;
            "#,
        )
        .unwrap();
        assert_eq!(
            paths(&fns),
            [
                "fetch",
                "api.math.add",
                "api.str.add",
                "default.fetch",
                "default.health"
            ]
        );
        let names: Vec<&str> = fns.iter().map(|f| f.rust_name.as_str()).collect();
        // `default.fetch` keeps `default_` beside the top-level `fetch`.
        assert_eq!(
            names,
            [
                "fetch",
                "api_math_add",
                "api_str_add",
                "default_fetch",
                "health"
            ]
        );
        assert!(fns[0].params[1].optional);
        assert_eq!(fns[0].doc.as_deref(), Some("Plain named export."));
        assert_eq!(
            fns[2].params[0].type_ref,
            TypeRef::String.with_empties(Nullability::Null)
        );
        // Type argument applied: env is MyEnv, not unknown.
        assert_eq!(fns[3].params[1].type_ref.format_ts(), "MyEnv");
        assert!(
            fns[3].doc.as_deref() == Some("Handles a request."),
            "{:?}",
            fns[3].doc
        );
    }

    #[test]
    fn file_rejections() {
        let err = with_file("export default 42;").unwrap_err().to_string();
        assert!(err.contains("export default"), "{err}");
        let err = with_file("export default handler;")
            .unwrap_err()
            .to_string();
        assert!(err.contains("declare const handler"), "{err}");
        let err = with_file("export class Foo {}").unwrap_err().to_string();
        assert!(err.contains("found class"), "{err}");
        let err = with_file("export const foo: { [k: string]: number };")
            .unwrap_err()
            .to_string();
        assert!(err.contains("methods and properties"), "{err}");
        let err = with_file("export function f(...rest: string[]): void;")
            .unwrap_err()
            .to_string();
        assert!(err.contains("variadic"), "{err}");
        let err = with_file("export function f(): void; export function f(): void;")
            .unwrap_err()
            .to_string();
        assert!(err.contains("twice"), "{err}");
        let err = with_file("export const n: number;")
            .unwrap_err()
            .to_string();
        assert!(err.contains("nothing to provide"), "{err}");
    }

    #[test]
    fn rust_name_clash_is_an_error() {
        let err = with_file(
            r#"
            export const foo: { bar(): void };
            export function foo_bar(): void;
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("\"foo.bar\"") && err.contains("\"foo_bar\""),
            "{err}"
        );
        let (fns, _) = with_file(
            r#"
            export function fetch(): void;
            export const other: { fetch(): void };
            declare const h: { fetch(): void };
            export default h;
            "#,
        )
        .unwrap();
        let names: Vec<&str> = fns.iter().map(|f| f.rust_name.as_str()).collect();
        assert_eq!(names, ["fetch", "other_fetch", "default_fetch"]);
    }

    #[test]
    fn file_warns_and_skips() {
        let (fns, gctx) = with_file(
            r#"
            export function g<T>(x: T): T;
            export const n: number;
            export const maybe: ((x: number) => Promise<number>) | undefined;
            export const fetch: (r: Request) => Promise<Response>;
            "#,
        )
        .unwrap();
        assert_eq!(paths(&fns), ["fetch"]);
        let warnings = format!("{:?}", gctx.diagnostics);
        assert!(warnings.contains("generic functions"), "{warnings}");
        assert!(warnings.contains("provides n"), "{warnings}");
        assert!(warnings.contains("nullable function type"), "{warnings}");
    }
}
