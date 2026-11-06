//! `.d.ts` → [`Ir`]: the bindings the Rust emitters report for the surface
//! (see [`crate::codegen::record`]), with their types lowered to the IR's.
//!
//! Names, expansion and forms are the Rust output's own. A binding's Rust
//! presentation forms (`try_`, `_promise`, `_js_string`, the inline
//! wrappers) are dropped; each derives from the member it presents.

use std::collections::{HashMap, HashSet};

use ts_gen_abi::ir::{
    Arm, Class, ClassKind, Function, Head as IrHead, Ir, JsType, Kind, Literal, Member, MemberKind,
    Named, Param as IrParam, Provide, Type,
};

use crate::codegen::record::{Binding, ClassRecord, GetterRecord, Record};
use crate::codegen::signatures::{param_names, ConcreteParam, FunctionSignature, SigForm};
use crate::codegen::surface::Surface;
use crate::codegen::typemap::{read_enum, CodegenContext, ReadEnum};
use crate::codegen::GenerateOptions;
use crate::context::{GlobalContext, TypeId};
use crate::ir::{Module, ModuleContext, Nullability, TypeDeclaration, TypeKind, TypeRef};
use crate::parse::scope::{Binding as ScopeBinding, ScopeId};
use crate::provides::ProvidedFn;

use super::OwnerRef;

pub(crate) fn build(
    module: &Module,
    gctx: &GlobalContext,
    options: &GenerateOptions,
    provided: &[ProvidedFn],
) -> Ir {
    let options = options.without_error_type();
    let surface = Surface::build(module, gctx, &options);
    let cgctx = CodegenContext::from_module_full(module, gctx, &options, Some(&surface))
        .expect("only `--error-type` fails to parse, and it is not set");
    let records = crate::codegen::record_bindings(module, &surface, &cgctx);
    // The Rust pass reports these when it emits Rust.
    drop(cgctx.take_diagnostics());
    let walk = Walk::new(module, gctx, &surface);
    let lower = Lower {
        gctx,
        cgctx: &cgctx,
        paths: walk.places.paths.clone(),
        modules: walk.places.modules.clone(),
        aliases: walk.aliases.clone(),
        depth: Default::default(),
        bound: Default::default(),
    };
    let mut builder = Builder {
        lower: &lower,
        walk: &walk,
        ir: Ir::default(),
        ranks: Vec::new(),
    };
    builder.namespaces();
    builder.records(records);
    builder.kinds();
    builder.provides(provided);
    let mut ir = builder.finish();
    options.ir_naming().apply(&mut ir);
    ir
}

/// Lowers TS types to IR types in the scope they read in, seeing through
/// type aliases (`type Foo = string | null` admits `null`) and the type
/// arguments substituted into their bodies.
struct Lower<'a> {
    gctx: &'a GlobalContext,
    cgctx: &'a CodegenContext<'a>,
    /// Dotted JS path and `declare module` of every declared type, which a
    /// reference names whatever it spells.
    paths: HashMap<TypeId, String>,
    modules: HashMap<TypeId, String>,
    /// Aliases declared inside namespaces: the arena entry a namespace
    /// member resolves to is a placeholder, the body lives in the
    /// namespace's declaration list.
    aliases: HashMap<TypeId, &'a crate::ir::TypeAliasDecl>,
    /// How deep in nested types the current lowering is.
    depth: std::cell::Cell<usize>,
    /// Type arguments substituted into alias bodies, each with the scope
    /// it came from, keyed by the placeholder name standing in for it.
    bound: std::cell::RefCell<Vec<(TypeRef, ScopeId)>>,
}

/// Placeholder prefix for substituted type arguments; `#` cannot start a
/// TS identifier, so it never clashes with a real name.
const BOUND_PREFIX: &str = "#bound:";

/// What a reference resolved to.
enum Resolved {
    /// An alias body and the scope it reads in, with type arguments applied.
    Alias(TypeRef, ScopeId),
    /// A substituted type argument and the scope it was written in.
    Bound(TypeRef, ScopeId),
    /// A class, interface, enum or undeclared global.
    Declared(OwnerRef),
    TypeParam,
}

impl Lower<'_> {
    /// What `ty` finally names through aliases and substituted type
    /// arguments, with the scope it reads in. Type parameters stay as
    /// they are, and so does a union alias the Rust bindings name as an
    /// enum when `keep_kinds`.
    fn target(&self, ty: &TypeRef, scope: ScopeId, keep_kinds: bool) -> (TypeRef, ScopeId) {
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
            if keep_kinds && self.union_kind(&ty, scope).is_some() {
                break;
            }
            match self.resolve(segments, generic_args, scope) {
                Some(Resolved::Alias(t, s) | Resolved::Bound(t, s)) => (ty, scope) = (t, s),
                _ => break,
            }
        }
        (ty, scope)
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
                ScopeBinding::TypeParam => Some(Resolved::TypeParam),
                // A constrained type parameter reads as its primitive.
                ScopeBinding::Erased(erasure) => Some(Resolved::Alias(erasure.type_ref(), scope)),
                ScopeBinding::Declared(id) => Some(self.declared(id, segments, args, scope)),
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
        id: TypeId,
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
        if let Some(alias) = alias {
            // A missing argument takes its default, which reads in the
            // alias's scope with the earlier arguments, else erases to `any`.
            let mut subst: HashMap<&str, TypeRef> = HashMap::new();
            for (i, p) in alias.type_params.iter().enumerate() {
                let arg = match (args.get(i), &p.default) {
                    (Some(a), _) => {
                        let mut bound = self.bound.borrow_mut();
                        bound.push((a.clone(), scope));
                        TypeRef::ident(format!("{BOUND_PREFIX}{}", bound.len() - 1))
                    }
                    (None, Some(default)) => default.substitute(&subst),
                    (None, None) => TypeRef::Any,
                };
                subst.insert(p.name.as_str(), arg);
            }
            return Resolved::Alias(alias.target.substitute(&subst), alias.body_scope);
        }
        Resolved::Declared(OwnerRef {
            module: self.modules.get(&id).cloned(),
            path: self
                .paths
                .get(&id)
                .cloned()
                .unwrap_or_else(|| segments.join(".")),
        })
    }

    /// The enum the Rust bindings name a union alias reference as, by the
    /// alias's identity.
    fn union_kind(&self, ty: &TypeRef, scope: ScopeId) -> Option<OwnerRef> {
        let TypeRef::Reference { segments, .. } = ty else {
            return None;
        };
        self.cgctx.union_alias(ty, scope)?;
        let id = self.gctx.resolve_path(scope, &segments.join("."))?;
        Some(OwnerRef {
            module: self.modules.get(&id).cloned(),
            path: self
                .paths
                .get(&id)
                .cloned()
                .unwrap_or_else(|| segments.join(".")),
        })
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
                // A synthesised enum, which a setter takes for its literals.
                None if segments.len() == 1 && self.cgctx.is_synthesised_enum(&segments[0]) => None,
                None if !matches!(segments[0].as_str(), "Iterable" | "AsyncIterable")
                    && self.known_global(&segments.join(".")) =>
                {
                    None
                }
                Some(Resolved::TypeParam) | None => Some(Nullability::Both),
            },
            _ => None,
        }
    }

    /// The IR type of a position that is not a read: an argument, or a
    /// value inside another type when `!top`.
    fn ty(&self, ty: &TypeRef, scope: ScopeId, top: bool) -> Type {
        let (core, core_scope) = self.target(ty, scope, top);
        match core {
            TypeRef::Boolean | TypeRef::BooleanLiteral(_) => Type::Bool,
            TypeRef::Number | TypeRef::NumberLiteral(_) => Type::F64,
            TypeRef::BigInt => Type::I64,
            _ => Type::Js(self.js(&core, core_scope, top)),
        }
    }

    /// A position's type as a JS value: scalars as their reference heads.
    fn js(&self, ty: &TypeRef, scope: ScopeId, top: bool) -> JsType {
        let (ty, scope) = self.target(ty, scope, top);
        let n = self.admits_empty(&ty, scope);
        // A recursive alias (`type Json = string | Json[]`) erases where it
        // recurs.
        let depth = self.depth.get();
        let head = if depth > 16 {
            IrHead::Any
        } else {
            self.depth.set(depth + 1);
            let head = self.head(&ty, scope, top);
            self.depth.set(depth);
            head
        };
        JsType {
            head,
            null: n.is_some_and(Nullability::admits_null),
            undefined: n.is_some_and(Nullability::admits_undefined),
        }
    }

    /// A named head; `Record<K, V>` keeps only `V`, as its keys are
    /// strings on the wire.
    fn named(&self, name: OwnerRef, args: &[TypeRef], scope: ScopeId) -> IrHead {
        let args = if name.module.is_none() && name.path == "Record" {
            &args[args.len().min(1)..]
        } else {
            args
        };
        IrHead::Named(Named {
            name,
            args: args.iter().map(|a| self.js(a, scope, false)).collect(),
        })
    }

    /// Whether an undeclared name is a type the Rust bindings map to a
    /// value type of their own, rather than to any value.
    fn known_global(&self, name: &str) -> bool {
        crate::codegen::typemap::DEDICATED_HEADS.contains(&name)
            || name == "ArrayLike"
            || self.gctx.external_map.resolve_type(name).is_some()
            || self.gctx.external_map.resolve_qualified(name).is_some()
    }

    fn head(&self, ty: &TypeRef, scope: ScopeId, top: bool) -> IrHead {
        let global = |name: &str| {
            IrHead::Named(Named {
                name: OwnerRef::global(name),
                args: Vec::new(),
            })
        };
        match ty {
            TypeRef::String => IrHead::String,
            TypeRef::StringLiteral(s) => IrHead::Literal(Literal::String(s.clone())),
            TypeRef::Number | TypeRef::NumberLiteral(_) => IrHead::Number,
            TypeRef::Boolean | TypeRef::BooleanLiteral(_) => IrHead::Boolean,
            TypeRef::BigInt => IrHead::BigInt,
            TypeRef::Null | TypeRef::Undefined | TypeRef::Void => IrHead::Empty,
            TypeRef::Object => global("Object"),
            TypeRef::Symbol => global("Symbol"),
            TypeRef::ArrayBufferView => global("ArrayBufferView"),
            TypeRef::PropertyKey => global("PropertyKey"),
            TypeRef::TypedSlice(element, _) => global(element.typed_array()),
            TypeRef::Array(element) => IrHead::Named(Named {
                name: OwnerRef::global("Array"),
                args: vec![self.js(element, scope, false)],
            }),
            TypeRef::Tuple(elements) => {
                IrHead::Tuple(elements.iter().map(|e| self.js(e, scope, false)).collect())
            }
            TypeRef::Function(sig) => IrHead::Callback {
                params: sig
                    .params
                    .iter()
                    .map(|p| self.ty(&p.type_ref, scope, false))
                    .collect(),
                result: Box::new(match self.target(&sig.return_type, scope, false) {
                    (TypeRef::Void | TypeRef::Undefined, _) => Type::Void,
                    _ => self.ty(&sig.return_type, scope, false),
                }),
            },
            TypeRef::Union(union) => {
                let core: Vec<&TypeRef> = union
                    .members()
                    .iter()
                    .filter(|m| !matches!(m, TypeRef::Null | TypeRef::Undefined | TypeRef::Void))
                    .collect();
                match core.as_slice() {
                    [] => IrHead::Empty,
                    [only] => self.js(only, scope, top).head,
                    _ => {
                        let heads: Vec<IrHead> =
                            core.iter().map(|m| self.js(m, scope, false).head).collect();
                        if heads.iter().all(|h| *h == heads[0]) {
                            return heads[0].clone();
                        }
                        let members: Vec<TypeRef> = core.into_iter().cloned().collect();
                        match crate::codegen::subtyping::lub_union(
                            &members,
                            Some(self.cgctx),
                            scope,
                        ) {
                            Some(lub) => self.js(&lub, scope, false).head,
                            None => IrHead::Any,
                        }
                    }
                }
            }
            TypeRef::Intersection(members) => {
                match crate::codegen::subtyping::glb_intersection(members, Some(self.cgctx), scope)
                {
                    Some(bound) => self.js(&bound, scope, false).head,
                    None => IrHead::Any,
                }
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => {
                if top {
                    if let Some(kind) = self.union_kind(ty, scope) {
                        return IrHead::Named(Named {
                            name: kind,
                            args: Vec::new(),
                        });
                    }
                }
                match self.resolve(segments, generic_args, scope) {
                    Some(Resolved::Alias(t, s) | Resolved::Bound(t, s)) => self.head(&t, s, top),
                    Some(Resolved::TypeParam) => IrHead::Var(segments.join(".")),
                    Some(Resolved::Declared(name)) => self.named(name, generic_args, scope),
                    None => match segments.join(".").as_str() {
                        // Not a type the Rust bindings can name.
                        "Iterable" | "AsyncIterable" => IrHead::Any,
                        name => {
                            let name = match name {
                                "ArrayLike" | "ReadonlyArray" => "Array",
                                "ReadonlySet" => "Set",
                                "ReadonlyMap" => "Map",
                                "PromiseLike" => "Promise",
                                "IterableIterator" => "Iterator",
                                "AsyncIterableIterator" => "AsyncIterator",
                                other => other,
                            };
                            self.named(OwnerRef::global(name), generic_args, scope)
                        }
                    },
                }
            }
            TypeRef::Any
            | TypeRef::Unknown
            | TypeRef::Never
            | TypeRef::KeyOf(_)
            | TypeRef::IndexedAccess(..)
            | TypeRef::Mapped(_)
            | TypeRef::ObjectLiteral(_)
            | TypeRef::Unresolved(_) => IrHead::Any,
        }
    }

    /// The type of a read: a getter (`getter`) or a callable's result, the
    /// awaited value of an `async fn` when `is_async`. A union the Rust
    /// bindings read as an enum reads as that kind.
    fn read(
        &self,
        ty: &TypeRef,
        scope: ScopeId,
        from_module: &ModuleContext,
        getter: bool,
        is_async: bool,
    ) -> Type {
        if let (TypeRef::Void | TypeRef::Undefined, _) = self.target(ty, scope, false) {
            return Type::Void;
        }
        let empties = |n: Option<Nullability>| {
            (
                n.is_some_and(Nullability::admits_null),
                n.is_some_and(Nullability::admits_undefined),
            )
        };
        match read_enum(ty, self.cgctx, scope, from_module, getter, is_async) {
            Some(ReadEnum::UnionAlias { alias, nullability }) => {
                let (null, undefined) = empties(nullability);
                let name = self
                    .union_kind(&alias, scope)
                    .unwrap_or_else(|| OwnerRef::global(alias.format_ts()));
                Type::Js(JsType {
                    head: IrHead::Named(Named {
                        name,
                        args: Vec::new(),
                    }),
                    null,
                    undefined,
                })
            }
            Some(ReadEnum::Dynamic { name, nullability }) => {
                let (null, undefined) = empties(nullability);
                Type::Js(JsType {
                    head: IrHead::Named(Named {
                        name: OwnerRef::global(name),
                        args: Vec::new(),
                    }),
                    null,
                    undefined,
                })
            }
            None => self.ty(ty, scope, false),
        }
    }

    /// `Promise<T>`, the value an `async fn` awaits read as its own result.
    fn promise(&self, promise: &TypeRef, scope: ScopeId, from_module: &ModuleContext) -> Type {
        let Type::Js(mut js) = self.ty(promise, scope, false) else {
            return self.ty(promise, scope, false);
        };
        if let (IrHead::Named(named), Some(inner)) = (&mut js.head, promise.as_promise_inner()) {
            named.args = vec![as_js(self.read(inner, scope, from_module, false, true))];
        }
        Type::Js(js)
    }
}

/// A type as a type argument: scalars as their reference heads.
impl Lower<'_> {
    /// The arms of a `@throws` type, each once.
    fn errors(&self, throws: Option<&TypeRef>, scope: ScopeId) -> Vec<JsType> {
        let arms = match throws {
            Some(TypeRef::Union(u)) => u.values().to_vec(),
            Some(t) => vec![t.clone()],
            None => Vec::new(),
        };
        // One entry per arm: `null` and `undefined` stand alone rather than
        // qualify another arm.
        let mut out: Vec<JsType> = Vec::new();
        for arm in &arms {
            let js = as_js(self.ty(arm, scope, false));
            let empty = |null, undefined| JsType {
                head: IrHead::Empty,
                null,
                undefined,
            };
            let bare = |head| JsType {
                head,
                null: false,
                undefined: false,
            };
            let mut split = Vec::new();
            if js.head != IrHead::Empty {
                split.push(bare(js.head.clone()));
            }
            if js.null || (js.head == IrHead::Empty && !js.undefined) {
                split.push(empty(true, false));
            }
            if js.undefined {
                split.push(empty(false, true));
            }
            for js in split {
                if !out.contains(&js) {
                    out.push(js);
                }
            }
        }
        out
    }

    /// The type parameters a signature names that its class does not
    /// declare, as the Rust binding's own generics.
    fn type_params(&self, sig: &FunctionSignature, class_params: &[String]) -> Vec<String> {
        let mut names = Vec::new();
        for p in &sig.params {
            crate::codegen::signatures::collect_type_params(
                &p.type_ref,
                self.cgctx,
                sig.body_scope,
                &mut names,
            );
        }
        crate::codegen::signatures::collect_type_params(
            &sig.return_type,
            self.cgctx,
            sig.body_scope,
            &mut names,
        );
        names.retain(|n| !class_params.contains(n));
        names
    }
}

fn as_js(ty: Type) -> JsType {
    let head = match ty {
        Type::Js(js) => return js,
        Type::Void => {
            return JsType {
                head: IrHead::Empty,
                null: false,
                undefined: true,
            }
        }
        Type::F64 => IrHead::Number,
        Type::I64 => IrHead::BigInt,
        Type::Bool => IrHead::Boolean,
    };
    JsType {
        head,
        null: false,
        undefined: false,
    }
}

/// Where each declared type is: its binding name and, for one inside a
/// `declare module`, the module.
#[derive(Default, Clone)]
struct Places {
    paths: HashMap<TypeId, String>,
    modules: HashMap<TypeId, String>,
}

impl Places {
    /// Place `id`, unless an earlier form of it is placed.
    fn insert(&mut self, id: TypeId, path: String, module: Option<&str>) {
        if self.paths.contains_key(&id) {
            return;
        }
        self.paths.insert(id, path);
        if let Some(m) = module {
            self.modules.insert(id, m.to_string());
        }
    }
}

/// A namespace the surface emits members of.
struct Namespace<'a> {
    rank: usize,
    decl: &'a TypeDeclaration,
    module: Option<String>,
    name: String,
    paths: Vec<String>,
}

/// A namespace's path segment, with the suffix its Rust module takes
/// apart from a namesake's (`M` as `m_2` is `M_2`).
fn renamed_segment(own: &str, module: &str) -> String {
    let base = crate::util::naming::to_snake_case(own);
    match module.strip_prefix(&base) {
        Some(rest)
            if rest.len() > 1
                && rest.starts_with('_')
                && rest[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("{own}{rest}")
        }
        _ => own.to_string(),
    }
}

/// The surface in declaration order, with namespaces walked in place.
struct Walk<'a> {
    /// Each declaration's position, by address.
    ranks: HashMap<usize, usize>,
    decls: HashMap<usize, &'a TypeDeclaration>,
    /// Each declaration's dotted paths under every name it and its
    /// namespaces are exported by, the one it binds by first.
    paths: HashMap<usize, Vec<String>>,
    /// Each declaration's binding name: its first path with the segments
    /// the Rust bindings rename apart from a namesake renamed.
    names: HashMap<usize, String>,
    /// The binding name of each namespace member's namespace.
    containers: HashMap<usize, String>,
    namespaces: Vec<Namespace<'a>>,
    places: Places,
    aliases: HashMap<TypeId, &'a crate::ir::TypeAliasDecl>,
}

impl<'a> Walk<'a> {
    fn new(module: &Module, gctx: &'a GlobalContext, surface: &'a Surface<'a>) -> Self {
        let mut walk = Walk {
            ranks: HashMap::new(),
            decls: HashMap::new(),
            paths: HashMap::new(),
            names: HashMap::new(),
            containers: HashMap::new(),
            namespaces: Vec::new(),
            places: Places::default(),
            aliases: HashMap::new(),
        };
        for &id in &module.types {
            for decl in surface.get(id) {
                walk.visit(id, decl, &[Vec::new()], &[], None, gctx, surface, false);
            }
        }
        // A type outside the surface is still named by where it is.
        for &id in &module.types {
            if surface.get(id).is_empty() {
                let decl = gctx.get_type(id);
                walk.visit(id, decl, &[Vec::new()], &[], None, gctx, surface, true);
            }
        }
        walk
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        &mut self,
        id: TypeId,
        decl: &'a TypeDeclaration,
        prefixes: &[Vec<String>],
        name_prefix: &[String],
        parent: Option<ScopeId>,
        gctx: &'a GlobalContext,
        surface: &'a Surface<'a>,
        index_only: bool,
    ) {
        let key = decl as *const TypeDeclaration as usize;
        if !index_only {
            if self.ranks.contains_key(&key) {
                return;
            }
            self.ranks.insert(key, self.ranks.len());
            self.decls.insert(key, decl);
            if !name_prefix.is_empty() {
                self.containers.insert(key, name_prefix.join("."));
            }
        }
        let spec = match &decl.module_context {
            ModuleContext::Module(spec) => Some(spec.to_string()),
            ModuleContext::Global => None,
        };
        let own = match &decl.kind {
            TypeKind::Class(c) => surface.js_name(id, &c.js_name),
            TypeKind::Interface(i) => i.js_name.as_str(),
            TypeKind::Namespace(ns) => surface.js_name(id, &ns.name),
            TypeKind::Function(f) => f.js_name.as_str(),
            TypeKind::Variable(v) => v.js_name.as_str(),
            TypeKind::DiscriminatedUnion(d) => d.js_name.as_str(),
            TypeKind::TypeAlias(a) => a.name.as_str(),
            TypeKind::StringEnum(e) => e.name.as_str(),
            TypeKind::NumericEnum(e) => e.name.as_str(),
            TypeKind::TsEnum(e) => e.name.as_str(),
        };
        let names: Vec<&str> = std::iter::once(own)
            .chain(
                surface
                    .names(id)
                    .iter()
                    .map(String::as_str)
                    .filter(|&n| n != own),
            )
            .collect();
        let paths: Vec<Vec<String>> = prefixes
            .iter()
            .flat_map(|p| {
                names.iter().map(move |n| {
                    let mut p = p.clone();
                    p.push(n.to_string());
                    p
                })
            })
            .collect();
        let dotted: Vec<String> = paths.iter().map(|p| p.join(".")).collect();
        let segment = match &decl.kind {
            TypeKind::Namespace(ns) => renamed_segment(own, &surface.mod_name(id, &ns.name)),
            // The Rust type's name: the declaration's own, `_Conn` for
            // `export { _Conn as Conn }`, as an identifier (`$Store` is
            // `Store`).
            kind => surface
                .type_names()
                .get(&id)
                .map(String::as_str)
                .or_else(|| crate::codegen::typemap::type_decl_name(kind))
                .map_or_else(|| own.to_string(), rust_ident),
        };
        let mut name = name_prefix.to_vec();
        name.push(segment);
        // A namespace member is reached by name in the namespace's scope
        // (one synthesised in a block scope is not); a top-level
        // declaration is its own type id.
        let bound_by = match &decl.kind {
            TypeKind::Class(c) => c.js_name.as_str(),
            TypeKind::Interface(i) => i.js_name.as_str(),
            other => own_name(other),
        };
        let rid = parent
            .and_then(|scope| gctx.scopes.resolve(scope, bound_by))
            .unwrap_or(id);
        self.places.insert(rid, name.join("."), spec.as_deref());
        if let (Some(scope), TypeKind::TypeAlias(a)) = (parent, &decl.kind) {
            // Only the alias's own placeholder: a re-export stub's name
            // may resolve straight to the declaration it re-exports.
            if let Some(pid) = gctx.scopes.resolve(scope, &a.name) {
                if matches!(&gctx.get_type(pid).kind, TypeKind::TypeAlias(p) if p.name == a.name) {
                    self.aliases.insert(pid, a);
                }
            }
        }
        let TypeKind::Namespace(ns) = &decl.kind else {
            if !index_only {
                self.paths.insert(key, dotted);
                self.names.insert(key, name.join("."));
            }
            return;
        };
        let scope = Some(ns.child_scope);
        if index_only {
            for &inner in &ns.declarations {
                let d = gctx.get_type(inner);
                self.visit(inner, d, &paths, &name, scope, gctx, surface, true);
            }
            return;
        }
        self.paths.insert(key, dotted.clone());
        self.names.insert(key, name.join("."));
        self.namespaces.push(Namespace {
            rank: self.ranks[&key],
            decl,
            module: spec,
            name: name.join("."),
            paths: dotted,
        });
        for &inner in &ns.declarations {
            for d in surface.get(inner) {
                self.visit(inner, d, &paths, &name, scope, gctx, surface, false);
            }
        }
    }

    fn rank(&self, decl: usize) -> usize {
        self.ranks.get(&decl).copied().unwrap_or(usize::MAX)
    }
}

/// The name a declaration is bound by in its scope.
fn own_name(kind: &TypeKind) -> &str {
    match kind {
        TypeKind::Class(c) => &c.name,
        TypeKind::Interface(i) => &i.name,
        TypeKind::Namespace(ns) => &ns.name,
        TypeKind::Function(f) => &f.name,
        TypeKind::Variable(v) => &v.name,
        TypeKind::DiscriminatedUnion(d) => &d.name,
        TypeKind::TypeAlias(a) => &a.name,
        TypeKind::StringEnum(e) => &e.name,
        TypeKind::NumericEnum(e) => &e.name,
        TypeKind::TsEnum(e) => &e.name,
    }
}

fn is_array(ty: &Type) -> bool {
    matches!(ty, Type::Js(JsType { head: IrHead::Named(n), .. })
        if n.name.module.is_none() && n.name.path == "Array")
}

/// `name` as Rust spells it, a keyword or digit escaped (`self_`, `_0`).
fn rust_ident(name: &str) -> String {
    unraw(&crate::codegen::typemap::make_ident(name))
}

/// A Rust ident's name, without the `r#` of a raw one.
fn unraw(ident: &syn::Ident) -> String {
    let ident = ident.to_string();
    ident.strip_prefix("r#").unwrap_or(&ident).to_string()
}

fn spec_of(module: &ModuleContext) -> Option<String> {
    match module {
        ModuleContext::Module(spec) => Some(spec.to_string()),
        ModuleContext::Global => None,
    }
}

/// A parameter name with non-ASCII characters as `_`, since only ASCII
/// is legal in a WAT identifier.
fn ascii(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii() { c } else { '_' })
        .collect()
}

/// Where the bindings being lowered go.
#[derive(Clone)]
enum Owner {
    /// A class's members, by index.
    Class(usize),
    /// Top-level functions and variables of a module.
    Top(Option<String>),
}

struct Builder<'l, 'a> {
    lower: &'l Lower<'a>,
    walk: &'l Walk<'a>,
    ir: Ir,
    /// Each class's declaration rank, for ordering.
    ranks: Vec<usize>,
}

impl Builder<'_, '_> {
    fn class_index(&self, module: &Option<String>, name: &str) -> Option<usize> {
        self.ir
            .classes
            .iter()
            .position(|c| c.module == *module && c.name == name)
    }

    fn namespaces(&mut self) {
        for ns in &self.walk.namespaces {
            if self.class_index(&ns.module, &ns.name).is_some() {
                continue;
            }
            self.ir.classes.push(Class {
                name: ns.name.clone(),
                path: ns.paths[0].clone(),
                module: ns.module.clone(),
                kind: ClassKind::Namespace,
                params: Vec::new(),
                extends: Vec::new(),
                aliases: ns.paths[1..].to_vec(),
                doc: ns.decl.doc.clone(),
                members: Vec::new(),
            });
            self.ranks.push(ns.rank);
        }
    }

    /// The namespace class `decl` is in, or the module's top level.
    fn container(
        &mut self,
        module: &Option<String>,
        decl: usize,
        js_namespace: &[String],
    ) -> Owner {
        if js_namespace.is_empty() {
            return Owner::Top(module.clone());
        }
        let path = js_namespace.join(".");
        let name = self
            .walk
            .containers
            .get(&decl)
            .cloned()
            .unwrap_or_else(|| path.clone());
        Owner::Class(self.class_index(module, &name).unwrap_or_else(|| {
            self.ir.classes.push(Class {
                name,
                path,
                module: module.clone(),
                kind: ClassKind::Namespace,
                params: Vec::new(),
                extends: Vec::new(),
                aliases: Vec::new(),
                doc: None,
                members: Vec::new(),
            });
            self.ranks.push(usize::MAX);
            self.ir.classes.len() - 1
        }))
    }

    fn records(&mut self, records: Vec<Record>) {
        // Group by declaration, in declaration order.
        let mut groups: Vec<(usize, TypeId, Vec<String>, Vec<Record>)> = Vec::new();
        for record in records {
            match record {
                Record::Decl {
                    id,
                    decl,
                    js_namespace,
                } => groups.push((decl, id, js_namespace, Vec::new())),
                other => {
                    if let Some(group) = groups.last_mut() {
                        group.3.push(other);
                    }
                }
            }
        }
        groups.sort_by_key(|(decl, ..)| self.walk.rank(*decl));
        // A construct signature names the class it builds by type id.
        let mut targets: HashMap<TypeId, (Option<String>, String)> = HashMap::new();
        for (decl, id, js_namespace, records) in &groups {
            for record in records {
                if let Record::Class(c) = record {
                    let name = self.class_name(*decl, js_namespace, c);
                    targets.insert(*id, (spec_of(&c.module), name));
                }
            }
        }
        let mut constructs: Vec<(Option<String>, String, Vec<Member>)> = Vec::new();
        for (decl, _, js_namespace, records) in groups {
            let Some(&tdecl) = self.walk.decls.get(&decl) else {
                continue;
            };
            let module = spec_of(&tdecl.module_context);
            let mut owner = self.container(&module, decl, &js_namespace);
            let mut class: Option<ClassRecord> = None;
            for record in records {
                match record {
                    Record::Decl { .. } => {}
                    Record::Class(c) => {
                        owner = Owner::Class(self.open_class(&c, decl, &js_namespace, tdecl));
                        class = Some(c);
                    }
                    Record::Callable { kind, sigs } => {
                        let members = self.callable(kind, &sigs, &owner);
                        self.place(&owner, members);
                    }
                    Record::Getter(g) => {
                        let member = self.getter(&g, class.as_ref());
                        self.place(&owner, vec![member]);
                    }
                    Record::Static {
                        rust_name,
                        js_name,
                        type_ref,
                        scope,
                    } => {
                        let result = self.lower.ty(&type_ref, scope, false);
                        let member = Member {
                            name: rust_name,
                            js: js_name,
                            kind: MemberKind::StaticGet,
                            type_params: Vec::new(),
                            params: Vec::new(),
                            result,
                            throws: false,
                            errors: Vec::new(),
                            nothrow: false,
                            variadic: false,
                            cohort: None,
                            optional: false,
                            module: None,
                            aliases: Vec::new(),
                            doc: tdecl.doc.clone(),
                        };
                        self.place(&owner, vec![member]);
                    }
                    Record::Construct {
                        target,
                        js_class,
                        sigs,
                    } => {
                        let Some((tmodule, tpath)) = targets.get(&target).cloned() else {
                            continue;
                        };
                        let mut js = js_namespace.clone();
                        js.push(js_class);
                        let mut members = self.constructs(&sigs, js.join("."), &tmodule, &tpath);
                        if module.is_some() && module != tmodule {
                            for m in &mut members {
                                m.module = module.clone();
                            }
                        }
                        constructs.push((tmodule, tpath, members));
                    }
                }
            }
        }
        for (module, path, members) in constructs {
            let Some(class) = self
                .class_index(&module, &path)
                .and_then(|i| self.ir.classes.get_mut(i))
            else {
                continue;
            };
            for mut member in members.into_iter().rev() {
                if class.members.iter().any(|m| m.name == member.name) {
                    continue;
                }
                member.type_params.retain(|p| !class.params.contains(p));
                class.members.insert(0, member);
            }
        }
    }

    fn open_class(
        &mut self,
        c: &ClassRecord,
        decl: usize,
        js_namespace: &[String],
        tdecl: &TypeDeclaration,
    ) -> usize {
        let module = spec_of(&c.module);
        let path = class_path(js_namespace, c);
        let name = self.class_name(decl, js_namespace, c);
        let mut aliases: Vec<String> = Vec::new();
        let paths = self.walk.paths.get(&decl).into_iter().flatten();
        for p in std::iter::once(&path).chain(paths) {
            // A path that only sanitises to the name is the name, not an alias.
            if *p != name && rust_ident(p) != name && !aliases.contains(p) {
                aliases.push(p.clone());
            }
        }
        let extends: Vec<Named> = c
            .heritage
            .iter()
            .filter_map(|h| match self.lower.js(&h.type_ref, h.scope, false).head {
                IrHead::Named(n) => Some(n),
                _ => None,
            })
            .collect();
        let kind = if c.runtime_class {
            ClassKind::Class
        } else {
            ClassKind::Interface
        };
        let index = match self.class_index(&module, &name) {
            Some(index) => {
                let class = &mut self.ir.classes[index];
                if class.kind == ClassKind::Namespace {
                    class.kind = kind;
                }
                for e in extends {
                    if !class.extends.contains(&e) {
                        class.extends.push(e);
                    }
                }
                for a in aliases {
                    if !class.aliases.contains(&a) {
                        class.aliases.push(a);
                    }
                }
                if class.doc.is_none() {
                    class.doc = tdecl.doc.clone();
                }
                index
            }
            None => {
                self.ir.classes.push(Class {
                    name: name.clone(),
                    path: path.clone(),
                    module: module.clone(),
                    kind,
                    params: c.type_params.clone(),
                    extends,
                    aliases,
                    doc: tdecl.doc.clone(),
                    members: Vec::new(),
                });
                self.ranks.push(self.walk.rank(decl));
                self.ir.classes.len() - 1
            }
        };
        if c.dictionary {
            let own = OwnerRef {
                module: module.clone(),
                path: name.clone(),
            };
            let member = Member {
                name: "new".into(),
                js: path.clone(),
                kind: MemberKind::Object,
                type_params: Vec::new(),
                params: Vec::new(),
                result: Type::Js(JsType {
                    head: IrHead::Named(Named {
                        name: own.clone(),
                        args: Vec::new(),
                    }),
                    null: false,
                    undefined: false,
                }),
                throws: false,
                errors: Vec::new(),
                nothrow: false,
                variadic: false,
                cohort: None,
                optional: false,
                module: None,
                aliases: Vec::new(),
                doc: None,
            };
            self.place(&Owner::Class(index), vec![member]);
        }
        index
    }

    /// Add members to their owner, the first of a name winning, as a
    /// merged declaration repeats its members.
    fn place(&mut self, owner: &Owner, members: Vec<Member>) {
        for mut member in members {
            member.name = rust_ident(&member.name);
            let list = match owner {
                Owner::Class(i) => &mut self.ir.classes[*i].members,
                Owner::Top(module) => {
                    if self
                        .ir
                        .functions
                        .iter()
                        .any(|f| f.module == *module && f.member.name == member.name)
                    {
                        continue;
                    }
                    self.ir.functions.push(Function {
                        module: module.clone(),
                        member,
                    });
                    continue;
                }
            };
            if list.iter().any(|m| m.name == member.name) {
                continue;
            }
            list.push(member);
        }
    }

    /// The members of one callable's bindings: each primary form, or for
    /// an `async fn` its `_promise` form under the primary's name, and for
    /// a literal-keyed cohort one member per arm.
    fn callable(&self, kind: Binding, sigs: &[FunctionSignature], owner: &Owner) -> Vec<Member> {
        let primaries: Vec<&FunctionSignature> =
            sigs.iter().filter(|s| s.form == SigForm::Primary).collect();
        let mut promises = sigs.iter().filter(|s| s.form == SigForm::Promise);
        let (from_module, class_ref) = match owner {
            Owner::Class(i) => {
                let c = &self.ir.classes[*i];
                (
                    match &c.module {
                        Some(m) => ModuleContext::Module(m.as_str().into()),
                        None => ModuleContext::Global,
                    },
                    Some((
                        OwnerRef {
                            module: c.module.clone(),
                            path: c.name.clone(),
                        },
                        c.path.clone(),
                    )),
                )
            }
            Owner::Top(m) => (
                match m {
                    Some(m) => ModuleContext::Module(m.as_str().into()),
                    None => ModuleContext::Global,
                },
                None,
            ),
        };
        let member_kind = match kind {
            Binding::Constructor => MemberKind::Constructor,
            Binding::Method => MemberKind::Method,
            Binding::StaticMethod | Binding::Function => MemberKind::StaticMethod,
            Binding::Setter => MemberKind::Set,
            Binding::StaticSetter => MemberKind::StaticSet,
        };
        let mut out = Vec::new();
        let class_params = match owner {
            Owner::Class(i) => self.ir.classes[*i].params.clone(),
            Owner::Top(_) => Vec::new(),
        };
        for primary in primaries {
            let type_params = self.lower.type_params(primary, &class_params);
            let wire = if primary.is_async {
                match promises.next() {
                    Some(p) => p,
                    None => continue,
                }
            } else {
                primary
            };
            let ctor = kind == Binding::Constructor;
            let result = match kind {
                Binding::Setter | Binding::StaticSetter => Type::Void,
                Binding::Constructor => Type::Js(JsType {
                    head: IrHead::Named(Named {
                        name: class_ref
                            .clone()
                            .map_or_else(|| OwnerRef::global("Object"), |(r, _)| r),
                        args: Vec::new(),
                    }),
                    null: false,
                    undefined: false,
                }),
                _ if primary.is_async => {
                    self.lower
                        .promise(&wire.return_type, wire.body_scope, &from_module)
                }
                _ => self.lower.read(
                    &primary.return_type,
                    primary.body_scope,
                    &from_module,
                    false,
                    false,
                ),
            };
            let js = match (&class_ref, ctor) {
                (Some((_, path)), true) => path.clone(),
                _ => primary.js_name.clone(),
            };
            type Arm<'s> = (Option<(&'s [usize], &'s [String])>, String);
            let arms: Vec<Arm> = match &primary.literal {
                Some(spec) => spec
                    .arms
                    .iter()
                    .map(|(lits, name)| (Some((&spec.positions[..], &lits[..])), name.clone()))
                    .collect(),
                None => vec![(None, primary.rust_name.clone())],
            };
            for (literals, name) in arms {
                let setter = matches!(kind, Binding::Setter | Binding::StaticSetter);
                let params = self.params(
                    &primary.params,
                    primary.body_scope,
                    member_kind.has_receiver(),
                    setter,
                    literals,
                );
                out.push(Member {
                    name,
                    js: js.clone(),
                    kind: member_kind,
                    type_params: type_params.clone(),
                    params,
                    result: result.clone(),
                    throws: wire.catch,
                    errors: self
                        .lower
                        .errors(primary.throws_type.as_ref(), primary.body_scope),
                    nothrow: primary.nothrow,
                    variadic: primary.params.last().is_some_and(|p| p.variadic),
                    cohort: primary.literal.as_ref().map(|spec| spec.binding.clone()),
                    optional: false,
                    module: None,
                    aliases: Vec::new(),
                    doc: primary.doc.clone(),
                });
            }
        }
        out
    }

    /// A variable's construct signatures, as constructors of the class at
    /// `path` that `new` the variable at `js`.
    fn constructs(
        &self,
        sigs: &[FunctionSignature],
        js: String,
        module: &Option<String>,
        path: &str,
    ) -> Vec<Member> {
        let own = OwnerRef {
            module: module.clone(),
            path: path.to_string(),
        };
        // The class's own are dropped once it is known.
        let class_params = Vec::new();
        sigs.iter()
            .filter(|s| s.form == SigForm::Primary)
            .map(|sig| {
                let params = self.params(&sig.params, sig.body_scope, false, false, None);
                let result = match self.lower.ty(&sig.return_type, sig.body_scope, false) {
                    Type::Js(js) => Type::Js(js),
                    _ => Type::Js(JsType {
                        head: IrHead::Named(Named {
                            name: own.clone(),
                            args: Vec::new(),
                        }),
                        null: false,
                        undefined: false,
                    }),
                };
                Member {
                    name: sig.rust_name.clone(),
                    js: js.clone(),
                    kind: MemberKind::Constructor,
                    type_params: self.lower.type_params(sig, &class_params),
                    params,
                    result,
                    throws: sig.catch,
                    errors: self.lower.errors(sig.throws_type.as_ref(), sig.body_scope),
                    nothrow: sig.nothrow,
                    variadic: sig.params.last().is_some_and(|p| p.variadic),
                    cohort: None,
                    optional: false,
                    module: None,
                    aliases: Vec::new(),
                    doc: sig.doc.clone(),
                }
            })
            .collect()
    }

    /// A binding's parameters, named as in a Rust parameter list (after the
    /// receiver's `this`). A literal-keyed arm takes its literals at
    /// `literals`' positions.
    fn params(
        &self,
        params: &[ConcreteParam],
        scope: ScopeId,
        receiver: bool,
        setter: bool,
        literals: Option<(&[usize], &[String])>,
    ) -> Vec<IrParam> {
        let reserved: &[&str] = if receiver { &["this"] } else { &[] };
        let names: Vec<String> = if setter {
            vec!["value".to_string(); params.len()]
        } else {
            param_names(params.iter().map(|p| ascii(&p.name)), reserved, None)
        };
        let mut out = Vec::new();
        for (i, (p, name)) in params.iter().zip(&names).enumerate() {
            let literal = literals.and_then(|(positions, lits)| {
                positions
                    .iter()
                    .position(|&q| q == i)
                    .map(|k| TypeRef::StringLiteral(lits[k].clone()))
            });
            let type_ref = literal.as_ref().unwrap_or(&p.type_ref);
            let mut ty = self.lower.ty(type_ref, scope, true);
            if p.variadic && !is_array(&ty) {
                // A rest parameter typed by a type parameter, `...args: A`.
                ty = Type::js(IrHead::Named(Named {
                    name: OwnerRef::global("Array"),
                    args: vec![JsType {
                        head: IrHead::Any,
                        null: true,
                        undefined: true,
                    }],
                }));
            }
            out.push(IrParam {
                name: name.clone(),
                ty,
            });
        }
        out
    }

    fn getter(&self, g: &GetterRecord, class: Option<&ClassRecord>) -> Member {
        let from_module = class.map_or(ModuleContext::Global, |c| c.module.clone());
        Member {
            name: g.rust_name.clone(),
            js: g.js_name.clone(),
            kind: if g.is_static {
                MemberKind::StaticGet
            } else {
                MemberKind::Get
            },
            type_params: Vec::new(),
            params: Vec::new(),
            result: self
                .lower
                .read(&g.type_ref, g.scope, &from_module, true, false),
            throws: g.throws.as_type().is_some(),
            errors: self.lower.errors(g.throws.as_type(), g.scope),
            nothrow: g.throws.is_never(),
            variadic: false,
            cohort: None,
            optional: g.optional,
            module: None,
            aliases: Vec::new(),
            doc: g.doc.clone(),
        }
    }

    /// Named unions the Rust bindings emit as enums: string and numeric
    /// enums, union aliases, and the read enums synthesised for unions.
    fn kinds(&mut self) {
        let l = self.lower;
        let mut decls: Vec<(&usize, &&TypeDeclaration)> = self.walk.decls.iter().collect();
        decls.sort_by_key(|(k, _)| self.walk.rank(**k));
        let mut seen = HashSet::new();
        for (key, decl) in decls {
            let module = spec_of(&decl.module_context);
            let path = self.walk.paths[key][0].clone();
            let name = self
                .walk
                .names
                .get(key)
                .cloned()
                .unwrap_or_else(|| path.clone());
            let (arms, fallback) = match &decl.kind {
                TypeKind::StringEnum(e) => {
                    let (variants, aliases) =
                        crate::codegen::enums::split_aliases(&e.variants, |v| v.js_value.clone());
                    let arms = variants
                        .iter()
                        .map(|v| Arm {
                            name: rust_ident(&v.rust_name),
                            ty: literal(Literal::String(v.js_value.clone())),
                            aliases: aliases
                                .iter()
                                .filter(|(_, t)| std::ptr::eq(*t, *v))
                                .map(|(a, _)| rust_ident(&a.rust_name))
                                .collect(),
                            doc: None,
                        })
                        .collect();
                    (arms, false)
                }
                TypeKind::NumericEnum(e) => {
                    let (variants, aliases) =
                        crate::codegen::enums::split_aliases(&e.variants, |v| v.value);
                    let arms = variants
                        .iter()
                        .map(|v| Arm {
                            name: rust_ident(&v.rust_name),
                            ty: literal(Literal::Int(v.value)),
                            aliases: aliases
                                .iter()
                                .filter(|(_, t)| std::ptr::eq(*t, *v))
                                .map(|(a, _)| rust_ident(&a.rust_name))
                                .collect(),
                            doc: v.doc.clone(),
                        })
                        .collect();
                    (arms, false)
                }
                TypeKind::TypeAlias(alias) if alias.from_module.is_none() => {
                    let Some(union) = l.cgctx.union_alias_of(alias) else {
                        continue;
                    };
                    let arms: Vec<TypeRef> =
                        crate::codegen::union_alias_arms(&union, l.cgctx, &decl.module_context)
                            .into_iter()
                            .map(|(a, _)| a.clone())
                            .collect();
                    (self.arms(&arms, union.scope), union.fallback)
                }
                _ => continue,
            };
            if !seen.insert((module.clone(), name.clone())) {
                continue;
            }
            self.ir.kinds.push(Kind {
                name,
                path,
                module,
                arms,
                fallback,
                doc: decl.doc.clone(),
            });
        }
        let registry = l.cgctx.dynamic_unions.borrow();
        for info in registry.by_key.values() {
            self.ir.kinds.push(Kind {
                name: info.rust_name.clone(),
                path: info.rust_name.clone(),
                module: None,
                arms: self.arms(&info.members, info.scope),
                fallback: info.fallback,
                doc: None,
            });
        }
    }

    /// A union's arms under the Rust variant names: an integer or string
    /// literal is its own value. An arm admits no empties, as no Rust
    /// variant is one: a position admitting them says so itself.
    fn arms(&self, arms: &[TypeRef], scope: ScopeId) -> Vec<Arm> {
        crate::codegen::dynamic_union_variant_idents(arms)
            .into_iter()
            .zip(arms)
            .map(|(ident, ty)| Arm {
                name: unraw(&ident),
                ty: match ty {
                    TypeRef::NumberLiteral(n) if n.fract() == 0.0 => {
                        literal(Literal::Int(*n as i64))
                    }
                    _ => JsType {
                        null: false,
                        undefined: false,
                        ..self.lower.js(ty, scope, false)
                    },
                },
                aliases: Vec::new(),
                doc: None,
            })
            .collect()
    }

    fn provides(&mut self, provided: &[ProvidedFn]) {
        // The wasm export name drops `default.` unless a top-level provider
        // has the same path underneath, as the Rust name does.
        let top_level: HashSet<String> = provided
            .iter()
            .filter(|e| e.path.first().is_some_and(|s| s != "default"))
            .map(|e| e.js_path())
            .collect();
        for e in provided {
            let (promise, result_ty) = match e.promise_inner() {
                Some(inner) => (true, inner),
                None => (false, &e.return_type),
            };
            let name = match e.path.as_slice() {
                [first, rest @ ..]
                    if first == "default"
                        && !rest.is_empty()
                        && !top_level.contains(&rest.join(".")) =>
                {
                    rest.join(".")
                }
                path => path.join("."),
            };
            let names = param_names(e.params.iter().map(|p| ascii(&p.name)), &[], None);
            let l = self.lower;
            let params: Vec<IrParam> = e
                .params
                .iter()
                .zip(&names)
                .map(|(p, name)| IrParam {
                    name: name.clone(),
                    ty: l.ty(&p.type_ref, e.scope, true),
                })
                .collect();
            let result = match l.target(result_ty, e.scope, false) {
                (TypeRef::Void | TypeRef::Undefined, _) => Type::Void,
                _ => l.ty(result_ty, e.scope, false),
            };
            self.ir.provides.push(Provide {
                name,
                js: e.path.clone(),
                params,
                result,
                is_async: promise,
                doc: e.doc.clone(),
            });
        }
    }

    fn finish(mut self) -> Ir {
        let mut keyed: Vec<(usize, usize, Class)> = std::mem::take(&mut self.ir.classes)
            .into_iter()
            .zip(self.ranks)
            .enumerate()
            .map(|(i, (c, r))| (r, i, c))
            .collect();
        keyed.sort_by_key(|(r, i, _)| (*r, *i));
        self.ir.classes = keyed.into_iter().map(|(_, _, c)| c).collect();
        // A path is no alias where another class has it as its name.
        let names: HashSet<(Option<String>, String)> = self
            .ir
            .classes
            .iter()
            .map(|c| (c.module.clone(), c.name.clone()))
            .collect();
        for c in &mut self.ir.classes {
            let module = c.module.clone();
            c.aliases
                .retain(|a| !names.contains(&(module.clone(), a.clone())));
        }
        self.ir
    }
}

fn literal(literal: Literal) -> JsType {
    JsType {
        head: IrHead::Literal(literal),
        null: false,
        undefined: false,
    }
}

impl Builder<'_, '_> {
    fn class_name(&self, decl: usize, js_namespace: &[String], c: &ClassRecord) -> String {
        self.walk
            .names
            .get(&decl)
            .cloned()
            .unwrap_or_else(|| class_path(js_namespace, c))
    }
}

/// A class's path: the namespaces its declaration is in, which a
/// discriminated union's extern block does not spell, then its own name.
fn class_path(js_namespace: &[String], c: &ClassRecord) -> String {
    let mut path = js_namespace.to_vec();
    path.push(c.js_name.clone());
    path.join(".")
}
