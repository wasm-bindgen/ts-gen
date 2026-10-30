//! The Wasm-level ABI derived from a `.d.ts`: one function per member, all
//! JS values as `externref`, scalars as Wasm numeric types.
//!
//! The model and its file format live in the `ts-gen-abi` crate; this
//! module lowers a `.d.ts` to it. Two consumers are in ts-gen:
//!
//! * [`wat`] prints the ABI as the annotated import template `ABI.md`
//!   specifies (`--target wat`), which bindings generators read.
//! * [`embed`] implements the ABI for a given language module: a
//!   `bindings.wasm` built on a fixed set of JS primitives, a
//!   `wrapper.wasm` that presents the module's handlers as a Worker, and
//!   the `builtins.js` those two import.

pub mod embed;
pub mod wat;

use std::collections::{HashMap, HashSet};

use crate::codegen::signatures::{
    base_rust_name, expand_signatures, param_names, ConcreteParam, ExpandedSignature, Overload,
    SignatureKind,
};
use crate::codegen::surface::Surface;
use crate::codegen::GenerateOptions;
use crate::context::GlobalContext;
use crate::ir::{
    ClassDecl, InterfaceClassification, InterfaceDecl, Member, Module, ModuleContext, Nullability,
    Param, Throws, TypeDeclaration, TypeKind, TypeRef,
};
use crate::parse::scope::{Binding, ScopeId};
use crate::provides::ProvidedFn;
use crate::util::naming::{LocalScope, Suffix};

pub use ts_gen_abi::{
    format, Abi, AbiFunction, AbiHandler, AbiOp, AbiOwner, AbiParam, Head, OwnerKind, WasmType,
    BINDINGS_SPECIFIER, BUILTINS_SPECIFIER, STRING_CONSTANTS, UNDEFINED_GLOBAL,
};

/// Lowers TS types to ABI types with the scope needed to see through type
/// aliases: `type Foo = string | null` is nullable, and `embed` would
/// otherwise let a module take it as `(ref extern)` and trap.
struct Lower<'a> {
    gctx: &'a GlobalContext,
    /// Dotted JS path of every declared type (`WebAssembly.Module`), for
    /// heads that match the owner names whatever a reference spells.
    paths: HashMap<crate::context::TypeId, String>,
    /// Aliases declared inside namespaces: the arena entry a namespace
    /// member resolves to is a placeholder, the body lives in the
    /// namespace's declaration list.
    aliases: HashMap<crate::context::TypeId, &'a crate::ir::TypeAliasDecl>,
    /// The declarations the Rust bindings emit, in the forms they are
    /// emitted in: the ABI binds exactly those.
    surface: Surface<'a>,
    /// Type arguments substituted into alias bodies, each with the scope
    /// it came from, keyed by the placeholder name standing in for it.
    bound: std::cell::RefCell<Vec<(TypeRef, ScopeId)>>,
}

/// Placeholder prefix for substituted type arguments; `#` cannot start a
/// TS identifier, so it never clashes with a real name.
const BOUND_PREFIX: &str = "#bound:";

impl Lower<'_> {
    fn of(&self, ty: &TypeRef, scope: ScopeId) -> WasmType {
        let (ty, scope) = self.target(ty, scope);
        match ty {
            TypeRef::Boolean | TypeRef::BooleanLiteral(_) => WasmType::I32,
            TypeRef::Number | TypeRef::NumberLiteral(_) => WasmType::F64,
            TypeRef::BigInt => WasmType::I64,
            _ => {
                let n = self.admits_empty(&ty, scope);
                WasmType::ExternRef {
                    null: n.is_some_and(Nullability::admits_null),
                    undefined: n.is_some_and(Nullability::admits_undefined),
                }
            }
        }
    }

    /// What `ty` finally names through aliases and substituted type
    /// arguments, with the scope it reads in. Type parameters stay as
    /// they are. The Rust bindings lower a plain alias reference to its
    /// target, so `type Millis = number` crosses as `f64`.
    fn target(&self, ty: &TypeRef, scope: ScopeId) -> (TypeRef, ScopeId) {
        let (mut ty, mut scope) = (ty.clone(), scope);
        // Bounded: a cyclic alias is a TS error, not a reason to hang.
        for _ in 0..64 {
            let TypeRef::Reference {
                segments,
                generic_args,
            } = &ty
            else {
                break;
            };
            match self.resolve(segments, generic_args, scope) {
                Some(Resolved::Alias(t, s) | Resolved::Bound(t, s)) => (ty, scope) = (t, s),
                _ => break,
            }
        }
        (ty, scope)
    }

    /// What a reference position names, through aliases and ignoring
    /// empty arms: `Headers | null`, `type H = Headers` and
    /// `type Gen<T> = T | null; Gen<Headers>` all give `Headers`. Unions
    /// of different heads and other types give `None`.
    fn head(&self, ty: &TypeRef, scope: ScopeId) -> Option<Head> {
        match ty {
            TypeRef::String | TypeRef::StringLiteral(_) => Some(Head::String),
            TypeRef::Number | TypeRef::NumberLiteral(_) => Some(Head::Number),
            TypeRef::Boolean | TypeRef::BooleanLiteral(_) => Some(Head::Boolean),
            TypeRef::BigInt => Some(Head::BigInt),
            TypeRef::Union(union) => {
                let mut heads = union.values().iter().map(|m| self.head(m, scope));
                let first = heads.next()??;
                heads.all(|h| h.as_ref() == Some(&first)).then_some(first)
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => match self.resolve(segments, generic_args, scope)? {
                Resolved::Alias(target, alias_scope) => self.head(&target, alias_scope),
                Resolved::Bound(ty, from) => self.head(&ty, from),
                Resolved::Declared(path) => Some(Head::Declared(path)),
                Resolved::TypeParam => None,
            },
            _ => None,
        }
    }

    /// Follow a (possibly qualified) reference to what it names. Type
    /// arguments are substituted into an alias body so `Gen<Headers>`
    /// keeps its head; the body then resolves in the alias's own scope,
    /// where the substituted arguments are already concrete.
    fn resolve(&self, segments: &[String], args: &[TypeRef], scope: ScopeId) -> Option<Resolved> {
        let (head, rest) = segments.split_first()?;
        if rest.is_empty() {
            if let Some(i) = head.strip_prefix(BOUND_PREFIX) {
                let (ty, from) = self.bound.borrow()[i.parse::<usize>().ok()?].clone();
                return Some(Resolved::Bound(ty, from));
            }
            return match self.gctx.scopes.resolve_binding(scope, head)? {
                Binding::TypeParam => Some(Resolved::TypeParam),
                // A constrained type parameter reads as its primitive.
                Binding::Erased(erasure) => Some(Resolved::Alias(erasure.type_ref(), scope)),
                Binding::Declared(id) => Some(self.declared(id, segments, args, scope)),
            };
        }
        let id = self.gctx.resolve_path(scope, &segments.join("."))?;
        Some(self.declared(id, segments, args, scope))
    }

    /// What a declared name is. Arguments to an alias are recorded with
    /// the scope they were written in and stand in the body as
    /// placeholders, since they must not resolve in the alias's scope.
    fn declared(
        &self,
        id: crate::context::TypeId,
        segments: &[String],
        args: &[TypeRef],
        scope: ScopeId,
    ) -> Resolved {
        let kind = &self.gctx.get_type(id).kind;
        let alias = match (self.aliases.get(&id), kind) {
            (Some(a), _) => Some(*a),
            (None, TypeKind::TypeAlias(a)) => Some(a),
            _ => None,
        };
        match (alias, kind) {
            (Some(alias), _) => {
                let placeholders: Vec<TypeRef> = args
                    .iter()
                    .map(|a| {
                        let mut bound = self.bound.borrow_mut();
                        bound.push((a.clone(), scope));
                        TypeRef::ident(format!("{BOUND_PREFIX}{}", bound.len() - 1))
                    })
                    .collect();
                let subst: HashMap<&str, &TypeRef> = alias
                    .type_params
                    .iter()
                    .zip(&placeholders)
                    .map(|(p, a)| (p.name.as_str(), a))
                    .collect();
                Resolved::Alias(substitute(&alias.target, &subst), alias.body_scope)
            }
            (None, _) => Resolved::Declared(
                self.paths
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| segments.join(".")),
            ),
        }
    }

    fn param(&self, name: &str, ty: &TypeRef, scope: ScopeId) -> AbiParam {
        let wasm = self.of(ty, scope);
        AbiParam {
            name: name.into(),
            ty: wasm,
            head: self.head(ty, scope).filter(|_| wasm.is_ref()),
        }
    }

    fn result(&self, ty: &TypeRef, scope: ScopeId) -> Option<WasmType> {
        match self.target(ty, scope) {
            (TypeRef::Void | TypeRef::Undefined, _) => None,
            (other, scope) => Some(self.of(&other, scope)),
        }
    }

    /// Which empties a TS type admits, through aliases, type arguments
    /// and union arms; `None` for neither. A type parameter may be
    /// instantiated with anything, so it admits both, as does an
    /// unresolved name rather than promising non-null.
    fn admits_empty(&self, ty: &TypeRef, scope: ScopeId) -> Option<Nullability> {
        fn join(a: Option<Nullability>, b: Option<Nullability>) -> Option<Nullability> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a.with(b)),
                (a, None) => a,
                (None, b) => b,
            }
        }
        match ty {
            TypeRef::Null => Some(Nullability::Null),
            TypeRef::Undefined | TypeRef::Void => Some(Nullability::Undefined),
            TypeRef::Any | TypeRef::Unknown => Some(Nullability::Both),
            TypeRef::Union(union) => union
                .members()
                .iter()
                .fold(None, |acc, m| join(acc, self.admits_empty(m, scope))),
            TypeRef::Reference {
                segments,
                generic_args,
            } => match self.resolve(segments, generic_args, scope) {
                Some(Resolved::Alias(target, alias_scope)) => {
                    self.admits_empty(&target, alias_scope)
                }
                Some(Resolved::Bound(ty, from)) => self.admits_empty(&ty, from),
                Some(Resolved::Declared(_)) => None,
                Some(Resolved::TypeParam) | None => Some(Nullability::Both),
            },
            _ => None,
        }
    }
}

/// Replace bare references to the substitution's names, through the
/// shapes an alias body can carry.
fn substitute(ty: &TypeRef, subst: &HashMap<&str, &TypeRef>) -> TypeRef {
    if subst.is_empty() {
        return ty.clone();
    }
    let go = |t: &TypeRef| substitute(t, subst);
    match ty {
        TypeRef::Reference {
            segments,
            generic_args,
        } if segments.len() == 1 && generic_args.is_empty() => subst
            .get(segments[0].as_str())
            .map(|t| (*t).clone())
            .unwrap_or_else(|| ty.clone()),
        TypeRef::Reference {
            segments,
            generic_args,
        } => TypeRef::Reference {
            segments: segments.clone(),
            generic_args: generic_args.iter().map(go).collect(),
        },
        TypeRef::Array(inner) => TypeRef::Array(Box::new(go(inner))),
        TypeRef::Union(ms) => TypeRef::union(ms.members().iter().map(go)),
        TypeRef::Intersection(ms) => TypeRef::Intersection(ms.iter().map(go).collect()),
        TypeRef::Tuple(ms) => TypeRef::Tuple(ms.iter().map(go).collect()),
        other => other.clone(),
    }
}

type DeclaredIndex<'a> = (
    HashMap<crate::context::TypeId, String>,
    HashMap<crate::context::TypeId, &'a crate::ir::TypeAliasDecl>,
);

/// Dotted JS paths of the input's declared types and the aliases nested in
/// namespaces, walking namespaces.
fn declared_index<'a>(
    module: &Module,
    gctx: &'a GlobalContext,
    surface: &Surface,
) -> DeclaredIndex<'a> {
    let mut paths = HashMap::new();
    let mut aliases = HashMap::new();
    for &id in &module.types {
        match &gctx.get_type(id).kind {
            TypeKind::Class(c) => {
                paths.insert(id, surface.js_name(id, &c.js_name).to_string());
            }
            TypeKind::Interface(i) => {
                paths.insert(id, i.js_name.clone());
            }
            TypeKind::Namespace(ns) => namespace_index(
                ns,
                &[surface.js_name(id, &ns.name).to_string()],
                gctx,
                surface,
                &mut paths,
                &mut aliases,
            ),
            _ => {}
        }
    }
    (paths, aliases)
}

/// Member types of a namespace are reached by name in its child scope.
fn namespace_index<'a>(
    ns: &'a crate::ir::NamespaceDecl,
    prefix: &[String],
    gctx: &'a GlobalContext,
    surface: &Surface,
    paths: &mut HashMap<crate::context::TypeId, String>,
    aliases: &mut HashMap<crate::context::TypeId, &'a crate::ir::TypeAliasDecl>,
) {
    for &inner in &ns.declarations {
        let (name, js_name) = match &gctx.get_type(inner).kind {
            TypeKind::Class(c) => (&c.js_name, surface.js_name(inner, &c.js_name)),
            TypeKind::Interface(i) => (&i.js_name, i.js_name.as_str()),
            TypeKind::TypeAlias(a) => {
                // Only the alias's own placeholder: a re-export stub's name
                // may resolve straight to the declaration it re-exports.
                if let Some(id) = gctx.scopes.resolve(ns.child_scope, &a.name) {
                    if matches!(&gctx.get_type(id).kind, TypeKind::TypeAlias(p) if p.name == a.name)
                    {
                        aliases.insert(id, a);
                    }
                }
                continue;
            }
            TypeKind::Namespace(child) => {
                let mut p = prefix.to_vec();
                p.push(surface.js_name(inner, &child.name).to_string());
                namespace_index(child, &p, gctx, surface, paths, aliases);
                continue;
            }
            _ => continue,
        };
        if let Some(id) = gctx.scopes.resolve(ns.child_scope, name) {
            let mut p = prefix.to_vec();
            p.push(js_name.to_string());
            paths.insert(id, p.join("."));
        }
    }
}

/// What a reference resolved to.
enum Resolved {
    /// An alias body and the scope it reads in, with type arguments applied.
    Alias(TypeRef, ScopeId),
    /// A substituted type argument and the scope it was written in.
    Bound(TypeRef, ScopeId),
    /// A class, interface or enum, as its dotted path.
    Declared(String),
    TypeParam,
}

/// ABI functions every language gets regardless of the `.d.ts`: the parts of
/// wasm-bindgen's `sys` surface that need JS to answer.
fn sys_functions() -> Vec<AbiFunction> {
    let sys =
        |name: &str, op: AbiOp, param: Option<WasmType>, result: WasmType, ts: &str| AbiFunction {
            name: name.into(),
            aliases: Vec::new(),
            owner: None,
            op,
            params: param
                .map(|ty| AbiParam {
                    name: "x".into(),
                    ty,
                    head: None,
                })
                .into_iter()
                .collect(),
            result: Some(result),
            result_head: None,
            ts: ts.into(),
            doc: None,
            variadic: false,
            module: None,
        };
    vec![
        sys(
            "JsValue.is_undefined",
            AbiOp::IsUndefined,
            Some(WasmType::JS),
            WasmType::I32,
            "x === undefined",
        ),
        sys(
            "JsValue.undefined",
            AbiOp::Undefined,
            None,
            WasmType::ExternRef {
                null: false,
                undefined: true,
            },
            "undefined",
        ),
        sys(
            "Number.value",
            AbiOp::NumberValue,
            Some(WasmType::JS_NON_NULL),
            WasmType::F64,
            "Number(x)",
        ),
    ]
}

pub fn build(
    module: &Module,
    gctx: &GlobalContext,
    options: &GenerateOptions,
    provided: &[ProvidedFn],
) -> Abi {
    let mut abi = Abi {
        functions: sys_functions(),
        ..Default::default()
    };
    let surface = Surface::build(module, gctx, options);
    let (paths, aliases) = declared_index(module, gctx, &surface);
    let lower = Lower {
        gctx,
        paths,
        aliases,
        surface,
        bound: Default::default(),
    };
    // The wasm export name drops `default.` unless a top-level provider
    // has the same path underneath, as the Rust name does.
    let top_level: HashSet<String> = provided
        .iter()
        .filter(|e| e.path.first().is_some_and(|s| s != "default"))
        .map(|e| e.js_path())
        .collect();
    for e in provided {
        abi.handlers.push(build_handler(e, &lower, &top_level));
    }
    for &id in &module.types {
        for decl in lower.surface.get(id) {
            let start = abi.functions.len();
            let owners_start = abi.owners.len();
            build_declaration(id, decl, &[], &lower, &mut abi);
            // The specifier is a property of the declaration, so it is stamped
            // on everything the declaration produced rather than threaded
            // through every builder.
            if let ModuleContext::Module(spec) = &decl.module_context {
                for f in &mut abi.functions[start..] {
                    f.module = Some(spec.to_string());
                }
                for o in &mut abi.owners[owners_start..] {
                    o.module = Some(spec.to_string());
                }
            }
        }
    }
    // A merged declaration repeats its members; the first occurrence wins.
    // The same name in two modules is two imports, told apart by module.
    let mut seen: HashSet<(Option<String>, String)> = HashSet::new();
    let mut kept: Vec<AbiFunction> = Vec::with_capacity(abi.functions.len());
    for f in std::mem::take(&mut abi.functions) {
        let names: Vec<_> = std::iter::once(&f.name)
            .chain(&f.aliases)
            .map(|n| (f.module.clone(), n.clone()))
            .collect();
        if names.iter().any(|n| seen.contains(n)) {
            continue;
        }
        seen.extend(names);
        kept.push(f);
    }
    abi.functions = kept;
    // A merged declaration is one owner, inheriting from every base and
    // exported under every name its parts are.
    let mut owners: Vec<AbiOwner> = Vec::new();
    for o in std::mem::take(&mut abi.owners) {
        match owners
            .iter_mut()
            .find(|p| p.path == o.path && p.module == o.module)
        {
            Some(p) => {
                for e in o.extends {
                    if !p.extends.contains(&e) {
                        p.extends.push(e);
                    }
                }
                for a in o.aliases {
                    if !p.aliases.contains(&a) {
                        p.aliases.push(a);
                    }
                }
                if p.doc.is_none() {
                    p.doc = o.doc;
                }
            }
            None => owners.push(o),
        }
    }
    abi.owners = owners;
    for f in &mut abi.functions {
        if !f.result.is_some_and(WasmType::is_ref) {
            f.result_head = None;
        }
    }
    for h in &mut abi.handlers {
        if !h.result.is_some_and(WasmType::is_ref) {
            h.result_head = None;
        }
    }
    abi.canonicalize();
    abi
}

fn build_declaration(
    id: crate::context::TypeId,
    decl: &TypeDeclaration,
    path: &[String],
    lower: &Lower,
    abi: &mut Abi,
) {
    let start = abi.functions.len();
    let owner_index = abi.owners.len();
    let owner = match &decl.kind {
        TypeKind::Class(class) => {
            push_owner(
                abi,
                path,
                &class.js_name,
                OwnerKind::Class,
                class.extends.iter(),
                decl,
                lower,
            );
            build_class(class, path, lower, abi);
            class.js_name.as_str()
        }
        TypeKind::Interface(iface) => {
            let kind = if iface.classification == InterfaceClassification::Dictionary {
                OwnerKind::Dictionary
            } else {
                OwnerKind::Interface
            };
            push_owner(
                abi,
                path,
                &iface.js_name,
                kind,
                iface.extends.iter(),
                decl,
                lower,
            );
            build_interface(iface, path, lower, abi);
            iface.js_name.as_str()
        }
        TypeKind::Function(func) => {
            let overloads: Vec<Overload> = std::iter::once(Overload {
                params: &func.params,
                return_type: &func.return_type,
                body_scope: func.body_scope,
                throws: &func.throws,
                async_modifier: func.async_modifier,
                doc: &decl.doc,
            })
            .chain(func.overloads.iter().map(|o| Overload {
                params: &o.params,
                return_type: &o.return_type,
                body_scope: o.body_scope,
                throws: &o.throws,
                async_modifier: o.async_modifier,
                doc: &o.doc,
            }))
            .collect();
            let mut used = LocalScope::default();
            // A free function is `globalThis[name]`; a namespace function
            // is `namespace[name]` with the namespace as receiver.
            push_callable(
                abi,
                &join_name(path, &func.js_name),
                SignatureKind::Function,
                &overloads,
                &[],
                AbiOp::StaticCall {
                    path: path.to_vec(),
                    name: func.js_name.clone(),
                },
                &mut used,
                lower,
            );
            return;
        }
        TypeKind::Namespace(ns) => {
            let js_name = lower.surface.js_name(id, &ns.name);
            push_owner(
                abi,
                path,
                js_name,
                OwnerKind::Namespace,
                [].iter(),
                decl,
                lower,
            );
            let mut child = path.to_vec();
            child.push(js_name.to_string());
            for &inner in &ns.declarations {
                for d in lower.surface.get(inner) {
                    build_declaration(inner, d, &child, lower, abi);
                }
            }
            js_name
        }
        TypeKind::Variable(var) => {
            // A global variable is a getter: `globalThis[name]` or `namespace[name]`.
            abi.functions.push(AbiFunction {
                name: join_name(path, &var.js_name),
                aliases: Vec::new(),
                owner: (!path.is_empty()).then(|| path.join(".")),
                doc: decl.doc.clone(),
                op: AbiOp::StaticGet {
                    path: path.to_vec(),
                    name: var.js_name.clone(),
                },
                params: vec![],
                result: Some(lower.of(&var.type_ref, decl.scope_id)),
                result_head: lower.head(&var.type_ref, decl.scope_id),
                ts: format!(
                    "{} {}: {}",
                    if var.is_const { "const" } else { "var" },
                    var.js_name,
                    var.type_ref.format_ts()
                ),
                variadic: false,
                module: None,
            });
            return;
        }
        TypeKind::TypeAlias(_)
        | TypeKind::DiscriminatedUnion(_)
        | TypeKind::StringEnum(_)
        | TypeKind::NumericEnum(_)
        | TypeKind::TsEnum(_) => return,
    };
    // Members are named `Owner.member`; each other public name of the
    // owner binds the same function, under every name the member has
    // within the owner.
    let prefix = join_name(path, owner);
    let others: Vec<String> = lower
        .surface
        .names(id)
        .iter()
        .filter(|&n| n != owner)
        .map(|n| join_name(path, n))
        .collect();
    abi.owners[owner_index].aliases = others.clone();
    for f in &mut abi.functions[start..] {
        let names: Vec<String> = std::iter::once(&f.name)
            .chain(&f.aliases)
            .cloned()
            .collect();
        for other in &others {
            for name in &names {
                f.aliases.push(format!("{other}{}", &name[prefix.len()..]));
            }
        }
    }
}

/// Record the owner a class, interface or namespace declaration binds
/// members under. Its aliases are set once its other names are known.
fn push_owner<'h>(
    abi: &mut Abi,
    path: &[String],
    js_name: &str,
    kind: OwnerKind,
    extends: impl Iterator<Item = &'h crate::ir::Heritage>,
    decl: &TypeDeclaration,
    lower: &Lower,
) {
    abi.owners.push(AbiOwner {
        path: join_name(path, js_name),
        kind,
        module: None,
        aliases: Vec::new(),
        extends: extends
            .filter_map(|h| match lower.head(&h.type_ref, h.scope) {
                Some(Head::Declared(p)) => Some(p),
                _ => None,
            })
            .collect(),
        doc: decl.doc.clone(),
    });
}

/// The property's type as declared: the `?` already says `| undefined`.
fn declared_ts(g: &crate::ir::GetterMember) -> String {
    if g.optional {
        g.type_ref.without_undefined().format_ts()
    } else {
        g.type_ref.format_ts()
    }
}

fn join_name(path: &[String], name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", path.join("."), name)
    }
}

fn build_class(class: &ClassDecl, path: &[String], lower: &Lower, abi: &mut Abi) {
    let mut js_path = path.to_vec();
    js_path.push(class.js_name.clone());
    build_members(&js_path, &class.name, &class.members, false, lower, abi);
}

fn build_interface(iface: &InterfaceDecl, path: &[String], lower: &Lower, abi: &mut Abi) {
    let mut js_path = path.to_vec();
    js_path.push(iface.js_name.clone());
    let dictionary = iface.classification == InterfaceClassification::Dictionary;
    build_members(
        &js_path,
        &iface.name,
        &iface.members,
        dictionary,
        lower,
        abi,
    );
}

/// A parameter name with non-ASCII characters as `_`, since only ASCII
/// is legal in a WAT identifier.
fn ascii(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii() { c } else { '_' })
        .collect()
}

fn build_handler(e: &ProvidedFn, lower: &Lower, top_level: &HashSet<String>) -> AbiHandler {
    let (promise, result_ty) = match e.promise_inner() {
        Some(inner) => (true, inner),
        None => (false, &e.return_type),
    };
    let name = match e.path.as_slice() {
        [first, rest @ ..]
            if first == "default" && !rest.is_empty() && !top_level.contains(&rest.join(".")) =>
        {
            rest.join(".")
        }
        path => path.join("."),
    };
    let names = param_names(e.params.iter().map(|p| ascii(&p.name)), &[], None);
    AbiHandler {
        name,
        path: e.path.clone(),
        params: e
            .params
            .iter()
            .zip(&names)
            .map(|(p, name)| lower.param(name, &p.type_ref, e.scope))
            .collect(),
        // A promise handler always has a result slot: the promise itself
        // without JSPI, the settled value (`undefined` for `Promise<void>`)
        // under it. Only a plain `void` export has none.
        result: match (promise, lower.result(result_ty, e.scope)) {
            (true, None) => Some(WasmType::JS),
            (_, r) => r,
        },
        result_head: lower.head(result_ty, e.scope),
        promise,
        ts: format_method_ts(e.leaf(), &e.params, &names, &e.return_type),
        doc: e.doc.clone(),
    }
}

/// Members are named by the owner's dotted JS path (`WebAssembly.Module`),
/// so same-named types in different namespaces stay distinct. `local` is
/// the name the owner is declared under, which a public name need not be.
fn build_members(
    js_path: &[String],
    local: &str,
    members: &[Member],
    dictionary: bool,
    lower: &Lower,
    abi: &mut Abi,
) {
    let type_name = &js_path.join(".");
    let mut used = LocalScope::default();
    let this = AbiParam {
        name: "this".into(),
        ty: WasmType::JS_NON_NULL,
        head: Some(Head::Declared(type_name.to_string())),
    };

    if dictionary {
        abi.functions.push(AbiFunction {
            name: format!("{type_name}.new"),
            aliases: Vec::new(),
            owner: Some(type_name.to_string()),
            doc: None,
            op: AbiOp::NewObject,
            params: vec![],
            result: Some(WasmType::JS_NON_NULL),
            result_head: Some(Head::Declared(type_name.to_string())),
            ts: format!("{{}} as {type_name}"),
            variadic: false,
            module: None,
        });
        used.claim("new");
    }

    // Overloads of one JS callable expand together so their variants carry
    // the Rust bindings' `_with_x` suffixes rather than numeric dedup.
    let constructed = TypeRef::ident(local);
    let mut method_overloads: HashMap<&str, Vec<Overload>> = HashMap::new();
    let mut static_overloads: HashMap<&str, Vec<Overload>> = HashMap::new();
    let mut ctor_overloads: Vec<Overload> = Vec::new();
    for member in members {
        match member {
            Member::Method(m) => method_overloads
                .entry(m.js_name.as_str())
                .or_default()
                .push(Overload {
                    params: &m.params,
                    return_type: &m.return_type,
                    body_scope: m.body_scope,
                    throws: &m.throws,
                    async_modifier: m.async_modifier,
                    doc: &m.doc,
                }),
            Member::StaticMethod(m) => static_overloads
                .entry(m.js_name.as_str())
                .or_default()
                .push(Overload {
                    params: &m.params,
                    return_type: &m.return_type,
                    body_scope: m.body_scope,
                    throws: &m.throws,
                    async_modifier: m.async_modifier,
                    doc: &m.doc,
                }),
            Member::Constructor(c) => ctor_overloads.push(Overload {
                params: &c.params,
                return_type: &constructed,
                body_scope: c.scope,
                throws: &c.throws,
                async_modifier: false,
                doc: &c.doc,
            }),
            _ => {}
        }
    }
    let void = TypeRef::Void;
    let mut expanded: HashSet<(bool, &str)> = HashSet::new();
    let mut ctor_expanded = false;

    for member in members {
        match member {
            Member::Getter(g) => {
                let name = used.declare(
                    &base_rust_name(&g.js_name, SignatureKind::Method),
                    Suffix::Snake,
                );
                // A read of an optional property admits `undefined`
                // whatever the type says.
                let read = if g.optional {
                    g.type_ref.clone().with_empties(Nullability::Undefined)
                } else {
                    g.type_ref.clone()
                };
                abi.functions.push(AbiFunction {
                    name: format!("{type_name}.{name}"),
                    aliases: Vec::new(),
                    owner: Some(type_name.to_string()),
                    doc: g.doc.clone(),
                    op: AbiOp::Get {
                        name: g.js_name.clone(),
                    },
                    params: vec![this.clone()],
                    result: Some(lower.of(&read, g.scope)),
                    result_head: lower.head(&read, g.scope),
                    ts: format!(
                        "{}{}: {}",
                        g.js_name,
                        if g.optional { "?" } else { "" },
                        declared_ts(g)
                    ),
                    variadic: false,
                    module: None,
                });
            }
            Member::Setter(s) => {
                let params = [Param {
                    name: "value".into(),
                    type_ref: s.type_ref.clone(),
                    optional: false,
                    variadic: false,
                    slice_mode: s.slice_mode,
                }];
                push_callable(
                    abi,
                    &format!(
                        "{type_name}.{}",
                        base_rust_name(&s.js_name, SignatureKind::Setter)
                    ),
                    SignatureKind::Setter,
                    &[Overload {
                        params: &params,
                        return_type: &void,
                        body_scope: s.scope,
                        throws: &Throws::None,
                        async_modifier: false,
                        doc: &s.doc,
                    }],
                    std::slice::from_ref(&this),
                    AbiOp::Set {
                        name: s.js_name.clone(),
                    },
                    &mut used,
                    lower,
                );
            }
            Member::Method(m) => {
                if !expanded.insert((false, m.js_name.as_str())) {
                    continue;
                }
                push_callable(
                    abi,
                    &format!(
                        "{type_name}.{}",
                        base_rust_name(&m.js_name, SignatureKind::Method)
                    ),
                    SignatureKind::Method,
                    &method_overloads[m.js_name.as_str()],
                    std::slice::from_ref(&this),
                    AbiOp::Call {
                        name: m.js_name.clone(),
                    },
                    &mut used,
                    lower,
                );
            }
            Member::Constructor(_) => {
                if std::mem::replace(&mut ctor_expanded, true) {
                    continue;
                }
                push_callable(
                    abi,
                    &format!("{type_name}.new"),
                    SignatureKind::Constructor,
                    &ctor_overloads,
                    &[],
                    AbiOp::Construct {
                        path: js_path.to_vec(),
                    },
                    &mut used,
                    lower,
                );
            }
            Member::StaticGetter(g) => {
                let name = used.declare(
                    &base_rust_name(&g.js_name, SignatureKind::StaticMethod),
                    Suffix::Snake,
                );
                abi.functions.push(AbiFunction {
                    name: format!("{type_name}.{name}"),
                    aliases: Vec::new(),
                    owner: Some(type_name.to_string()),
                    doc: g.doc.clone(),
                    op: AbiOp::StaticGet {
                        path: js_path.to_vec(),
                        name: g.js_name.clone(),
                    },
                    params: vec![],
                    result: Some(lower.of(&g.type_ref, g.scope)),
                    result_head: lower.head(&g.type_ref, g.scope),
                    ts: format!("static {}: {}", g.js_name, g.type_ref.format_ts()),
                    variadic: false,
                    module: None,
                });
            }
            Member::StaticSetter(s) => {
                let params = [Param {
                    name: "value".into(),
                    type_ref: s.type_ref.clone(),
                    optional: false,
                    variadic: false,
                    slice_mode: None,
                }];
                push_callable(
                    abi,
                    &format!(
                        "{type_name}.{}",
                        base_rust_name(&s.js_name, SignatureKind::StaticSetter)
                    ),
                    SignatureKind::StaticSetter,
                    &[Overload {
                        params: &params,
                        return_type: &void,
                        body_scope: s.scope,
                        throws: &Throws::None,
                        async_modifier: false,
                        doc: &s.doc,
                    }],
                    &[],
                    AbiOp::StaticSet {
                        path: js_path.to_vec(),
                        name: s.js_name.clone(),
                    },
                    &mut used,
                    lower,
                );
            }
            Member::StaticMethod(m) => {
                if !expanded.insert((true, m.js_name.as_str())) {
                    continue;
                }
                push_callable(
                    abi,
                    &format!(
                        "{type_name}.{}",
                        base_rust_name(&m.js_name, SignatureKind::StaticMethod)
                    ),
                    SignatureKind::StaticMethod,
                    &static_overloads[m.js_name.as_str()],
                    &[],
                    AbiOp::StaticCall {
                        path: js_path.to_vec(),
                        name: m.js_name.clone(),
                    },
                    &mut used,
                    lower,
                );
            }
            Member::IndexSignature(_) => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_callable(
    abi: &mut Abi,
    base_name: &str,
    kind: SignatureKind,
    overloads: &[Overload],
    leading: &[AbiParam],
    op: AbiOp,
    used: &mut LocalScope,
    lower: &Lower,
) {
    let (owner, member) = match base_name.rsplit_once('.') {
        Some((o, m)) => (Some(o), m),
        None => (None, base_name),
    };
    let ts_name = match &op {
        AbiOp::Construct { .. } => "constructor".to_string(),
        AbiOp::Set { name } | AbiOp::StaticSet { name, .. } => format!("set {name}"),
        AbiOp::Call { name } | AbiOp::StaticCall { name, .. } => name.clone(),
        _ => member.to_string(),
    };

    // Without a codegen context, expansion keeps a type alias as one
    // parameter and renders every reference outside `js_sys` as `JsValue`,
    // so arms that differ only there are one function taking any of them.
    for sig in fold_null_arms(expand_signatures(overloads, kind, None), overloads, lower) {
        let Overload {
            return_type,
            body_scope: scope,
            ..
        } = overloads[sig.overload];
        let variant = used.declare(&format!("{member}{}", sig.name_suffix), Suffix::Snake);
        let name = match owner {
            Some(o) => format!("{o}.{variant}"),
            None => variant,
        };
        let variadic = sig.params.last().is_some_and(|p| p.variadic);
        let reserved: Vec<&str> = leading.iter().map(|p| p.name.as_str()).collect();
        let names = param_names(sig.params.iter().map(|p| ascii(&p.name)), &reserved, None);
        let mut params = leading.to_vec();
        params.extend(sig.params.iter().zip(&names).map(|(p, name)| {
            if p.variadic {
                AbiParam {
                    name: name.clone(),
                    ty: WasmType::JS,
                    head: None,
                }
            } else {
                lower.param(name, &p.type_ref, scope)
            }
        }));
        let ts_params = sig
            .params
            .iter()
            .zip(&names)
            .map(|(p, name)| {
                format!(
                    "{}{name}: {}",
                    if p.variadic { "..." } else { "" },
                    p.type_ref.format_ts()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let ts = match kind {
            SignatureKind::Constructor | SignatureKind::Setter | SignatureKind::StaticSetter => {
                format!("{ts_name}({ts_params})")
            }
            _ => format!("{ts_name}({ts_params}): {}", return_type.format_ts()),
        };
        abi.functions.push(AbiFunction {
            name,
            aliases: Vec::new(),
            owner: owner.map(String::from),
            doc: overloads[sig.overload].doc.clone(),
            op: op.clone(),
            params,
            result: match kind {
                SignatureKind::Setter | SignatureKind::StaticSetter => None,
                SignatureKind::Constructor => Some(WasmType::JS_NON_NULL),
                _ => lower.result(return_type, scope),
            },
            result_head: match kind {
                SignatureKind::Setter | SignatureKind::StaticSetter => None,
                // A constructor's result is the owner itself.
                SignatureKind::Constructor => owner.map(|o| Head::Declared(o.to_string())),
                _ => lower.head(return_type, scope),
            },
            ts,
            variadic,
            module: None,
        });
    }
}

/// Expansion gives a written `null` its own arm, a parameter typed `null`.
/// The ABI keeps one nullable `externref` per position instead: a `null`
/// arm folds into each arm that differs from it only at its `null`
/// positions, with the same result, and those take `T | null` there (a
/// scalar `number | null` crosses as `externref`). One with no such arm stays.
fn fold_null_arms(
    sigs: Vec<ExpandedSignature>,
    overloads: &[Overload],
    lower: &Lower,
) -> Vec<ExpandedSignature> {
    let is_null = |p: &ConcreteParam| matches!(p.type_ref, TypeRef::Null);
    let result = |s: &ExpandedSignature| {
        let o = &overloads[s.overload];
        lower.result(o.return_type, o.body_scope)
    };
    let mut nulled: Vec<Vec<bool>> = sigs.iter().map(|s| vec![false; s.params.len()]).collect();
    let mut folded = vec![false; sigs.len()];
    for (i, arm) in sigs.iter().enumerate() {
        if !arm.params.iter().any(is_null) {
            continue;
        }
        for (j, base) in sigs.iter().enumerate() {
            let sibling = !base.params.iter().any(is_null)
                && base.params.len() == arm.params.len()
                && base.params.iter().zip(&arm.params).all(|(b, a)| {
                    is_null(a) || (b.type_ref == a.type_ref && b.variadic == a.variadic)
                })
                && result(base) == result(arm);
            if sibling {
                folded[i] = true;
                for (k, a) in arm.params.iter().enumerate() {
                    nulled[j][k] |= is_null(a);
                }
            }
        }
    }
    sigs.into_iter()
        .zip(nulled)
        .zip(folded)
        .filter(|(_, folded)| !folded)
        .map(|((mut sig, nulled), _)| {
            for (p, nulled) in sig.params.iter_mut().zip(nulled) {
                if nulled {
                    let (core, n) = p
                        .type_ref
                        .split_empties()
                        .map_or((p.type_ref.clone(), Nullability::Null), |(core, n)| {
                            (core, n.with(Nullability::Null))
                        });
                    p.type_ref = core.with_empties(n);
                }
            }
            sig
        })
        .collect()
}

fn format_method_ts(name: &str, params: &[Param], names: &[String], ret: &TypeRef) -> String {
    let ps = params
        .iter()
        .zip(names)
        .map(|(p, param)| {
            format!(
                "{}{}{}: {}",
                if p.variadic { "..." } else { "" },
                param,
                if p.optional { "?" } else { "" },
                p.type_ref.format_ts()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({ps}): {}", ret.format_ts())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(path: &str) -> Option<Head> {
        Some(Head::Declared(path.into()))
    }

    /// Every declared module is selected, as `--export <specifier>` would.
    fn abi(src: &str) -> Abi {
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let exports = module
            .types
            .iter()
            .filter_map(|&id| match &gctx.get_type(id).module_context {
                ModuleContext::Module(m) => Some(crate::codegen::ExportSpec::Module(m.to_string())),
                ModuleContext::Global => None,
            })
            .chain([crate::codegen::ExportSpec::File("<input>".into())])
            .collect();
        let options = GenerateOptions::new().exports(exports);
        build(&module, &gctx, &options, &[])
    }

    #[test]
    fn module_declarations_carry_their_specifier() {
        let abi = abi(r#"
            declare function fetch(input: string): Promise<Response>;
            declare module "cloudflare:sockets" {
                function connect(address: string): Socket;
                class Socket { startTls(): Socket; }
            }
        "#);
        assert_eq!(abi.function("fetch").unwrap().module, None);
        assert_eq!(
            abi.function("connect").unwrap().module.as_deref(),
            Some("cloudflare:sockets")
        );
        assert_eq!(
            abi.function("Socket.start_tls").unwrap().module.as_deref(),
            Some("cloudflare:sockets")
        );
        assert_eq!(abi.modules(), ["cloudflare:sockets"]);
    }

    #[test]
    fn global_interface_collapses_into_module_constructor() {
        let abi = abi(r#"
            interface Email { readonly from: string; }
            declare module "cloudflare:email" {
                let _Email: { new (from: string): Email };
                export { _Email as Email };
            }
        "#);
        let new = abi.import("cloudflare:email", "Email.new").unwrap();
        assert!(matches!(new.op, AbiOp::Construct { .. }), "{:?}", new.op);
        assert_eq!(new.module.as_deref(), Some("cloudflare:email"));
        // The global instance interface collapses into the module's
        // constructor class, so its members are the class's.
        assert_eq!(
            abi.function("Email.from").unwrap().module.as_deref(),
            Some("cloudflare:email")
        );
    }

    #[test]
    fn nullability_sees_through_aliases() {
        let abi = abi(r#"
            type MaybeStr = string | null;
            type Alias2 = MaybeStr;
            type Body = Blob | MaybeStr;
            type Plain = string;
            type Gen<T> = T | null;
            declare class Blob {}
            declare function a(): MaybeStr;
            declare function b(): Alias2;
            declare function c(): Body;
            declare function d(): Plain;
            declare function e(x: Alias2): void;
            declare function g(): Gen<Blob>;
            declare function clone<T>(value: T): T;
            declare function unknownName(): NotDeclaredAnywhere;
            declare class Event<T = any> { readonly detail: T; }
        "#);
        let result = |n: &str| abi.function(n).unwrap().result.unwrap();
        assert!(result("a").admits_null());
        assert!(result("b").admits_null());
        assert!(result("c").admits_null());
        assert_eq!(result("d"), WasmType::JS_NON_NULL);
        assert!(abi.function("e").unwrap().params[0].ty.admits_null());
        assert!(result("g").admits_null(), "generic alias instantiation");
        assert_eq!(result("clone"), WasmType::JS, "type parameter");
        assert_eq!(abi.function("clone").unwrap().params[0].ty, WasmType::JS);
        assert_eq!(result("unknownName"), WasmType::JS, "unresolved name");
        assert_eq!(result("Event.detail"), WasmType::JS, "class type parameter");
    }

    #[test]
    fn scalars_through_aliases() {
        let abi = abi(r#"
            type Millis = number;
            type Stamp = Millis;
            type Flag = boolean;
            type Big = bigint;
            type Nothing = void;
            type Id<T> = T;
            declare function now(): Stamp;
            declare function enabled(): Flag;
            declare function big(): Big;
            declare function reset(): Nothing;
            declare function wrapped(): Id<number>;
            declare function wait(ms: Millis): void;
            declare function same<T>(value: T): T;
        "#);
        let result = |n: &str| abi.function(n).unwrap().result;
        assert_eq!(result("now"), Some(WasmType::F64), "alias of an alias");
        assert_eq!(result("enabled"), Some(WasmType::I32));
        assert_eq!(result("big"), Some(WasmType::I64));
        assert_eq!(result("reset"), None, "alias of void");
        assert_eq!(result("wrapped"), Some(WasmType::F64), "type argument");
        assert_eq!(abi.function("wait").unwrap().params[0].ty, WasmType::F64);
        assert_eq!(result("same"), Some(WasmType::JS), "type parameter");
    }

    #[test]
    fn namespaced_types_keep_their_path() {
        let abi = abi(r#"
            declare class Header { a(): void; }
            declare namespace TailStream { class Header { b(): void; } }
            declare namespace WebAssembly { class Module { constructor(bytes: ArrayBuffer); static exports(m: Module): string[]; } }
        "#);
        let names: Vec<&str> = abi
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .filter(|n| !n.starts_with("JsValue.") && !n.starts_with("Number."))
            .collect();
        assert_eq!(
            names,
            [
                "Header.a",
                "TailStream.Header.b",
                "WebAssembly.Module.new",
                "WebAssembly.Module.exports"
            ]
        );
        let ctor = abi.function("WebAssembly.Module.new").unwrap();
        assert_eq!(ctor.result_head, declared("WebAssembly.Module"));
        let exports = abi.function("WebAssembly.Module.exports").unwrap();
        assert_eq!(exports.params[0].head, declared("WebAssembly.Module"));
    }

    #[test]
    fn overload_variants_use_the_rust_suffixes() {
        let abi = abi(r#"
            declare class K {
                constructor();
                constructor(init: string);
                m(a: string): void;
                m(a: number): void;
                static s(x: string): void;
                static s(x: string, y: number): void;
            }
        "#);
        let names: Vec<&str> = abi
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .filter(|n| n.starts_with("K."))
            .collect();
        assert_eq!(
            names,
            [
                "K.new",
                "K.new_with_init",
                "K.m",
                "K.m_with_f64",
                "K.s",
                "K.s_with_y"
            ]
        );
    }

    #[test]
    fn nullability_through_type_params_qualified_paths_and_generic_aliases() {
        let abi = abi(r#"
            declare class Headers { constructor(); }
            declare class Gen<T> {
                optT(): T | undefined;
                nullableT(): T | null;
                plainT(): T;
            }
            declare namespace NS { type Inner = Headers | null; class K { constructor(); } }
            type Gen2<T> = T | null;
            type Id<T> = T;
            interface Uses {
                qualified: NS.Inner;
                qualifiedClass: NS.K;
                viaGen: Gen2<Headers>;
                viaId: Id<Headers>;
                partial: Gen2;
            }
        "#);
        let f = |n: &str| abi.function(n).unwrap();
        // The type parameter may be `X | null`, so the other empty is admitted too.
        assert_eq!(f("Gen.opt_t").result, Some(WasmType::JS));
        assert_eq!(f("Gen.nullable_t").result, Some(WasmType::JS));
        assert_eq!(f("Gen.plain_t").result, Some(WasmType::JS));
        // Qualified alias resolves through the namespace scope.
        assert!(f("Uses.qualified").result.unwrap().admits_null());
        assert_eq!(f("Uses.qualified").result_head, declared("Headers"));
        assert_eq!(
            f("Uses.qualified_class").result,
            Some(WasmType::JS_NON_NULL)
        );
        assert_eq!(f("Uses.qualified_class").result_head, declared("NS.K"));
        // Generic alias keeps the head once the argument is substituted.
        assert!(f("Uses.via_gen").result.unwrap().admits_null());
        assert_eq!(f("Uses.via_gen").result_head, declared("Headers"));
        assert_eq!(f("Uses.via_id").result, Some(WasmType::JS_NON_NULL));
        assert_eq!(f("Uses.via_id").result_head, declared("Headers"));
        // An un-instantiated alias parameter is a type parameter: BOTH, no head.
        assert_eq!(f("Uses.partial").result, Some(WasmType::JS));
        assert_eq!(f("Uses.partial").result_head, None);
    }

    #[test]
    fn members_bind_under_every_export_name() {
        let class = abi(r#"
            declare module "m:a" {
                class _Foo { constructor(); get(): string; }
                export { _Foo as Foo, _Foo as Bar };
            }
        "#);
        for (name, alias) in [("Foo.new", "Bar.new"), ("Foo.get", "Bar.get")] {
            assert_eq!(class.function(name).unwrap().aliases, [alias]);
        }

        let ns = abi(r#"
            declare module "m:a" {
                namespace _N {
                    function nf(): void;
                    class C { constructor(); }
                }
                export { _N as N1, _N as N2 };
            }
        "#);
        for (name, alias) in [("N1.nf", "N2.nf"), ("N1.C.new", "N2.C.new")] {
            assert_eq!(ns.function(name).unwrap().aliases, [alias]);
        }
    }

    #[test]
    fn module_binds_only_its_export_map() {
        let abi = abi(r#"
            declare module "m:a" {
                function open(): Hidden;
                function hidden(): void;
                const secret: number;
                class Hidden { static make(): Hidden; m(): void; }
                export { open };
            }
        "#);
        assert!(abi.function("open").is_some());
        assert!(abi.function("hidden").is_none());
        assert!(abi.function("secret").is_none());
        // An unexported class keeps only what binds through a value.
        assert!(abi.function("Hidden.m").is_some());
        assert!(abi.function("Hidden.make").is_none());
    }

    #[test]
    fn renamed_exports_bind_by_their_public_name() {
        let abi = abi(r#"
            declare module "m:a" {
                class _Foo { constructor(); }
                namespace _N { function f(): void; }
                export { _Foo as Foo, _N as N };
            }
        "#);
        let ctor = abi.function("Foo.new").unwrap();
        assert_eq!(ctor.result, Some(WasmType::JS_NON_NULL));
        assert_eq!(ctor.result_head, declared("Foo"));
        assert!(abi.function("N.f").is_some());
    }

    #[test]
    fn module_scalar_exports_keep_their_type() {
        let abi = abi(r#"
            declare module "m:a" { const n: number; let b: boolean; const l: bigint; }
        "#);
        for (name, ty) in [
            ("n", WasmType::F64),
            ("b", WasmType::I32),
            ("l", WasmType::I64),
        ] {
            assert_eq!(abi.import("m:a", name).unwrap().result, Some(ty), "{name}");
        }
    }

    #[test]
    fn null_and_undefined_bits_and_heads() {
        let abi = abi(r#"
            declare class Headers { constructor(); }
            type MaybeHeaders = Headers | null;
            interface Init {
                headers?: Headers;
                nullable: MaybeHeaders;
                both?: Headers | null;
            }
            declare function f(h: MaybeHeaders): Headers;
        "#);
        let f = |n: &str| abi.function(n).unwrap();
        let und = WasmType::ExternRef {
            null: false,
            undefined: true,
        };
        let nul = WasmType::ExternRef {
            null: true,
            undefined: false,
        };
        let both = WasmType::JS;
        assert_eq!(f("Init.headers").result, Some(und));
        assert_eq!(f("Init.nullable").result, Some(nul));
        assert_eq!(f("Init.both").result, Some(both));
        assert_eq!(f("f").params[0].ty, nul);
        assert_eq!(f("f").result, Some(WasmType::JS_NON_NULL));
        assert_eq!(f("Init.nullable").result_head, declared("Headers"));
        assert_eq!(f("f").params[0].head, declared("Headers"));
        assert_eq!(f("f").result_head, declared("Headers"));
        assert_eq!(f("Headers.new").result_head, declared("Headers"));
    }

    #[test]
    fn string_positions_have_the_string_head() {
        let abi = abi(r#"
            type S = string;
            declare class Blob {}
            declare function a(x: string | null): S;
            declare function b(): "x" | "y";
            declare function c(): string | number;
            declare function d(): string | Blob;
        "#);
        let f = |n: &str| abi.function(n).unwrap();
        assert_eq!(f("a").params[0].head, Some(Head::String));
        assert_eq!(f("a").result_head, Some(Head::String));
        assert_eq!(f("b").result_head, Some(Head::String));
        assert_eq!(f("c").result_head, None);
        assert_eq!(f("d").result_head, None);
    }

    #[test]
    fn null_arms_fold_into_one_nullable_position() {
        let abi = abi(r#"
            declare class Body { constructor(); }
            declare class Response {
                constructor(body?: Body | null, init?: string);
            }
            interface Init { body?: Body | null; limit?: number | null; empties?: null; }
            declare function key(x: string | number | null): void;
        "#);
        let names: Vec<&str> = abi.functions.iter().map(|f| f.name.as_str()).collect();
        let f = |n: &str| abi.function(n).unwrap();
        let ty = |n: &str, i: usize| f(n).params[i].ty;
        let nul = WasmType::ExternRef {
            null: true,
            undefined: false,
        };
        for gone in [
            "Response.new_with_null",
            "Response.new_with_null_and_init",
            "Init.set_body_with_null",
            "Init.set_limit_with_null",
            "key_with_null",
        ] {
            assert!(!names.contains(&gone), "{gone} folds into its sibling");
        }
        assert_eq!(ty("Response.new_with_body", 0), nul);
        assert_eq!(ty("Response.new_with_body_and_init", 0), WasmType::JS);
        assert_eq!(
            f("Response.new_with_body").ts,
            "constructor(body: Body | null)"
        );
        assert_eq!(ty("Init.set_body", 1), nul);
        // A scalar admitting `null` crosses as a nullable reference.
        assert_eq!(ty("Init.set_limit", 1), nul);
        assert_eq!(ty("key", 0), nul);
        assert_eq!(ty("key_with_f64", 0), nul);
        // A `null` arm with no sibling keeps its name.
        assert_eq!(ty("Init.set_empties_with_null", 1), nul);
    }

    #[test]
    fn same_name_in_two_roots_is_two_imports() {
        let abi = abi(r#"
            declare function connect(address: string): void;
            declare module "cloudflare:sockets" {
                function connect(address: string): void;
            }
            declare module "node:net" {
                namespace constants { const O_RDONLY: Sock; }
                class Sock {}
            }
            declare module "node:fs" {
                namespace constants { const O_RDONLY: Sock; }
                class Sock {}
            }
        "#);
        for module in [BINDINGS_SPECIFIER, "cloudflare:sockets"] {
            assert!(abi.import(module, "connect").is_some(), "{module}");
        }
        for module in ["node:net", "node:fs"] {
            let f = abi.import(module, "constants.O_RDONLY").unwrap();
            assert_eq!(
                f.op,
                AbiOp::StaticGet {
                    path: vec!["constants".into()],
                    name: "O_RDONLY".into()
                }
            );
        }
    }

    #[test]
    fn param_names_are_numbered_where_they_collide_once_sanitised() {
        let abi = abi(
            "declare function collideArgs(a: string, $a: number, fooBar: string, foo_bar: number): void;",
        );
        let names: Vec<&str> = abi
            .function("collideArgs")
            .unwrap()
            .params
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["a", "a2", "foo_bar", "foo_bar2"]);
    }

    fn param_names_of(params: &[AbiParam]) -> Vec<&str> {
        params.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn receiver_name_is_reserved() {
        let abi = abi("declare class Thing { method(_this: string, value: number): void; }");
        let method = abi.function("Thing.method").unwrap();
        assert_eq!(param_names_of(&method.params), ["this", "this2", "value"]);
    }

    #[test]
    fn non_ascii_param_names_are_legal_wat_identifiers() {
        let abi = abi("declare function uni(café: string, cafè: number): void;");
        let uni = abi.function("uni").unwrap();
        assert_eq!(param_names_of(&uni.params), ["caf_", "caf_2"]);
    }

    #[test]
    fn handler_params_are_numbered_where_they_collide_once_sanitised() {
        let (module, mut gctx) = crate::parse_source(
            "interface Handler { handle(fooBar: string, foo_bar: number, café: string): void; }",
            None,
        )
        .unwrap();
        let provided = crate::provides::resolve("Handler.handle", &module, &mut gctx).unwrap();
        let abi = build(&module, &gctx, &GenerateOptions::new(), &provided);
        assert_eq!(
            param_names_of(&abi.handlers[0].params),
            ["foo_bar", "foo_bar2", "caf_"]
        );
        assert_eq!(
            abi.handlers[0].ts,
            "handle(foo_bar: string, foo_bar2: number, caf_: string): void"
        );
    }
}
