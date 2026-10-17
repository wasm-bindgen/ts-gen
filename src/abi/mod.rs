//! The Wasm-level ABI derived from a `.d.ts`: one function per member, all
//! JS values as `externref`, scalars as Wasm numeric types.
//!
//! [`wat`] prints the ABI as an import template a language module is
//! written against (`--target wat`).

pub mod wat;

use std::collections::{HashMap, HashSet};

use crate::codegen::signatures::{base_rust_name, expand_signatures, Overload, SignatureKind};
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

/// Specifier a language module imports the bindings under.
pub const BINDINGS_SPECIFIER: &str = "./bindings.wasm";
/// Specifier for the fixed JS primitive module.
pub const BUILTINS_SPECIFIER: &str = "./builtins.js";
/// Import namespace for imported string constants (js-string builtins).
pub const STRING_CONSTANTS: &str = "wasm:js/string-constants";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WasmType {
    /// Only ever a TS `boolean`; `embed` relies on that to wrap writes in `Boolean()`.
    I32,
    I64,
    F64,
    /// A JS value. `null` / `undefined` record which empties the TS type
    /// admits; the wire type is `externref` either way. Only `null` is a
    /// wasm null, so `embed` lets a module take any position that does not
    /// admit `null` as `(ref extern)`.
    ExternRef {
        null: bool,
        undefined: bool,
    },
    /// Only used by the wrapper to hand a Wasm function to JS as a function object.
    FuncRef,
    /// Used in builtin import signatures for variadic array elements. GC languages
    /// use `anyref` arrays, and JS-bound anyref values convert at the import boundary.
    AnyRef,
}

impl WasmType {
    pub fn wat(self) -> &'static str {
        match self {
            WasmType::I32 => "i32",
            WasmType::I64 => "i64",
            WasmType::F64 => "f64",
            WasmType::ExternRef { .. } => "externref",
            WasmType::FuncRef => "funcref",
            WasmType::AnyRef => "anyref",
        }
    }

    /// A JS value position with no nullability claim: intermediates in
    /// the bindings, `any`, type parameters.
    pub const JS: WasmType = WasmType::ExternRef {
        null: true,
        undefined: true,
    };
    /// A JS value position the TS type says is never `null`/`undefined`.
    pub const JS_NON_NULL: WasmType = WasmType::ExternRef {
        null: false,
        undefined: false,
    };

    /// A reference position that may hold wasm `null`.
    pub fn admits_null(self) -> bool {
        matches!(
            self,
            WasmType::ExternRef { null: true, .. } | WasmType::AnyRef
        )
    }
}

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
        match ty {
            TypeRef::Boolean | TypeRef::BooleanLiteral(_) => WasmType::I32,
            TypeRef::Number | TypeRef::NumberLiteral(_) => WasmType::F64,
            TypeRef::BigInt => WasmType::I64,
            _ => {
                let n = self.admits_empty(ty, scope);
                WasmType::ExternRef {
                    null: n.is_some_and(Nullability::admits_null),
                    undefined: n.is_some_and(Nullability::admits_undefined),
                }
            }
        }
    }

    /// The declared type a reference position names, through aliases and
    /// ignoring empty arms: `Headers | null`, `type H = Headers` and
    /// `type Gen<T> = T | null; Gen<Headers>` all give `Headers`. A type
    /// in a namespace is its dotted path (`WebAssembly.Module`), matching
    /// the owner names. Unions of different heads and non-references give
    /// `None`.
    fn head(&self, ty: &TypeRef, scope: ScopeId) -> Option<String> {
        match ty {
            TypeRef::Union(union) => {
                let mut heads = union.values().iter().map(|m| self.head(m, scope));
                let first = heads.next()??;
                heads.all(|h| h.as_deref() == Some(&first)).then_some(first)
            }
            TypeRef::Reference {
                segments,
                generic_args,
            } => match self.resolve(segments, generic_args, scope)? {
                Resolved::Alias(target, alias_scope) => self.head(&target, alias_scope),
                Resolved::Bound(ty, from) => self.head(&ty, from),
                Resolved::Declared(path) => Some(path),
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
        AbiParam {
            name: name.into(),
            ty: self.of(ty, scope),
            head: self.head(ty, scope),
            ts: ty.format_ts(),
        }
    }

    fn result(&self, ty: &TypeRef, scope: ScopeId) -> Option<WasmType> {
        match ty {
            TypeRef::Void | TypeRef::Undefined => None,
            other => Some(self.of(other, scope)),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiParam {
    pub name: String,
    pub ty: WasmType,
    /// The declared type a reference names, through aliases and ignoring
    /// empty arms (`Headers` for `Headers | null`); a target can wrap it.
    pub head: Option<String>,
    /// TS spelling, for the template's comments.
    pub ts: String,
}

/// What the bridge has to do to implement a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiOp {
    /// `receiver[name]`
    Get { name: String },
    /// `receiver[name] = value`
    Set { name: String },
    /// `receiver[name](...args)`
    Call { name: String },
    /// `new path(...args)`
    Construct { path: Vec<String> },
    /// `path[name]`, where `path` is a global object such as a class or
    /// namespace.
    StaticGet { path: Vec<String>, name: String },
    /// `path[name] = value`
    StaticSet { path: Vec<String>, name: String },
    /// `path[name](...args)` with `path` as the receiver.
    StaticCall { path: Vec<String>, name: String },
    /// `{}`
    NewObject,
    /// `x === undefined`; `null` needs nothing, `ref.is_null` in the language.
    IsUndefined,
    /// `Number(x)`: unboxes a `number` that crossed as a reference
    /// (a nullable scalar, or a union arm) to `f64`.
    NumberValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiFunction {
    /// Export name in `bindings.wasm` and import field in the language
    /// module: `Type.member` with wasm-bindgen's variant suffixes.
    pub name: String,
    /// Further names `bindings.wasm` exports the same function under: a
    /// type exported under several names has each member under each.
    pub aliases: Vec<String>,
    pub op: AbiOp,
    /// Includes the receiver as the first parameter for instance ops.
    pub params: Vec<AbiParam>,
    pub result: Option<WasmType>,
    /// As [`AbiParam::head`], for the result.
    pub result_head: Option<String>,
    /// TS declaration this came from, for the template's comments.
    pub ts: String,
    /// Trailing parameter is a JS array spread into the call.
    pub variadic: bool,
    /// For declarations inside `declare module "spec"`: the specifier whose
    /// export roots the op's path (or is the callee itself when the path is
    /// empty), and which the language module imports the function from.
    /// `None` roots at `globalThis` and imports from `./bindings.wasm`.
    pub module: Option<String>,
}

impl AbiFunction {
    /// The module a language module imports this function from.
    pub fn import_module(&self) -> &str {
        self.module.as_deref().unwrap_or(BINDINGS_SPECIFIER)
    }
}

/// A function the module provides, lowered from a [`ProvidedFn`]: the
/// reverse of an [`AbiFunction`]. Parameters arrive by value (the language
/// owns them), results are owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiHandler {
    /// Wasm export name the module provides: the JS path joined with `.`,
    /// with a leading `default.` dropped unless a top-level provider has
    /// the same path (`fetch`, `foo.bar.baz`, `default.add` beside `add`).
    pub name: String,
    /// JS path from the module namespace, which the wrapper synthesises
    /// (`["default", "fetch"]`, `["foo", "bar", "baz"]`).
    pub path: Vec<String>,
    pub params: Vec<AbiParam>,
    /// Wasm type of the result; for a `Promise<T>` result this is `T`'s,
    /// a nullable reference when `T` is `void`.
    pub result: Option<WasmType>,
    pub result_head: Option<String>,
    /// The TS result was `Promise<T>`: under JSPI the module returns `T`
    /// and the wrapper's `promising` supplies the promise; otherwise the
    /// module returns the promise itself.
    pub promise: bool,
    pub ts: String,
    pub doc: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Abi {
    pub functions: Vec<AbiFunction>,
    pub handlers: Vec<AbiHandler>,
}

impl Abi {
    /// The first function named `name`, from any module.
    pub fn function(&self, name: &str) -> Option<&AbiFunction> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// The function a language module imports as `name` from `module`:
    /// its own name or an alias.
    pub fn import(&self, module: &str, name: &str) -> Option<&AbiFunction> {
        self.functions.iter().find(|f| {
            f.import_module() == module && (f.name == name || f.aliases.iter().any(|a| a == name))
        })
    }

    /// Module specifiers the ABI imports from, sorted and deduplicated.
    pub fn modules(&self) -> Vec<&str> {
        let mut specs: Vec<&str> = self
            .functions
            .iter()
            .filter_map(|f| f.module.as_deref())
            .collect();
        specs.sort_unstable();
        specs.dedup();
        specs
    }
}

/// Name the language module imports `undefined` under from `builtins.js`,
/// as an immutable `externref` global.
pub const UNDEFINED_GLOBAL: &str = "undefined";

/// ABI functions every language gets regardless of the `.d.ts`: the parts of
/// wasm-bindgen's `sys` surface that need JS to answer.
fn sys_functions() -> Vec<AbiFunction> {
    let sys = |name: &str, op: AbiOp, param: WasmType, result: WasmType, ts: &str| AbiFunction {
        name: name.into(),
        aliases: Vec::new(),
        op,
        params: vec![AbiParam {
            name: "x".into(),
            ty: param,
            head: None,
            ts: ts.into(),
        }],
        result: Some(result),
        result_head: None,
        ts: ts.into(),
        variadic: false,
        module: None,
    };
    vec![
        sys(
            "JsValue.is_undefined",
            AbiOp::IsUndefined,
            WasmType::JS,
            WasmType::I32,
            "x === undefined",
        ),
        sys(
            "Number.value",
            AbiOp::NumberValue,
            WasmType::JS_NON_NULL,
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
            build_declaration(id, decl, &[], &lower, &mut abi);
            // The specifier is a property of the declaration, so it is stamped
            // on everything the declaration produced rather than threaded
            // through every builder.
            if let ModuleContext::Module(spec) = &decl.module_context {
                for f in &mut abi.functions[start..] {
                    f.module = Some(spec.to_string());
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
    let owner = match &decl.kind {
        TypeKind::Class(class) => {
            build_class(class, path, lower, abi);
            class.js_name.as_str()
        }
        TypeKind::Interface(iface) => {
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
    AbiHandler {
        name,
        path: e.path.clone(),
        params: e
            .params
            .iter()
            .map(|p| lower.param(&p.name, &p.type_ref, e.scope))
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
        ts: format_method_ts(e.leaf(), &e.params, &e.return_type),
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
        head: Some(type_name.to_string()),
        ts: type_name.to_string(),
    };

    if dictionary {
        abi.functions.push(AbiFunction {
            name: format!("{type_name}.new"),
            aliases: Vec::new(),
            op: AbiOp::NewObject,
            params: vec![],
            result: Some(WasmType::JS_NON_NULL),
            result_head: Some(type_name.to_string()),
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
                        doc: &None,
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
                        doc: &None,
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
    for sig in expand_signatures(overloads, kind, None) {
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
        let mut params = leading.to_vec();
        params.extend(sig.params.iter().map(|p| {
            if p.variadic {
                AbiParam {
                    name: p.name.clone(),
                    ty: WasmType::JS,
                    head: None,
                    ts: format!("...{}", p.type_ref.format_ts()),
                }
            } else {
                lower.param(&p.name, &p.type_ref, scope)
            }
        }));
        let ts_params = sig
            .params
            .iter()
            .map(|p| {
                format!(
                    "{}{}: {}",
                    if p.variadic { "..." } else { "" },
                    p.name,
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
                SignatureKind::Constructor => owner.map(str::to_string),
                _ => lower.head(return_type, scope),
            },
            ts,
            variadic,
            module: None,
        });
    }
}

fn format_method_ts(name: &str, params: &[Param], ret: &TypeRef) -> String {
    let ps = params
        .iter()
        .map(|p| {
            format!(
                "{}{}{}: {}",
                if p.variadic { "..." } else { "" },
                p.name,
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
    fn dictionary_new_and_module_constructor_are_two_imports() {
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
        assert_eq!(ctor.result_head.as_deref(), Some("WebAssembly.Module"));
        let exports = abi.function("WebAssembly.Module.exports").unwrap();
        assert_eq!(
            exports.params[0].head.as_deref(),
            Some("WebAssembly.Module")
        );
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
        assert_eq!(f("Uses.qualified").result_head.as_deref(), Some("Headers"));
        assert_eq!(
            f("Uses.qualified_class").result,
            Some(WasmType::JS_NON_NULL)
        );
        assert_eq!(
            f("Uses.qualified_class").result_head.as_deref(),
            Some("NS.K")
        );
        // Generic alias keeps the head once the argument is substituted.
        assert!(f("Uses.via_gen").result.unwrap().admits_null());
        assert_eq!(f("Uses.via_gen").result_head.as_deref(), Some("Headers"));
        assert_eq!(f("Uses.via_id").result, Some(WasmType::JS_NON_NULL));
        assert_eq!(f("Uses.via_id").result_head.as_deref(), Some("Headers"));
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
        assert_eq!(ctor.result_head.as_deref(), Some("Foo"));
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
        assert_eq!(f("Init.nullable").result_head.as_deref(), Some("Headers"));
        assert_eq!(f("f").params[0].head.as_deref(), Some("Headers"));
        assert_eq!(f("f").result_head.as_deref(), Some("Headers"));
        assert_eq!(f("Headers.new").result_head.as_deref(), Some("Headers"));
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
}
