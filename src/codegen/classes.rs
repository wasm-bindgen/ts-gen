//! ClassDecl / ClassLike InterfaceDecl → wasm_bindgen `extern "C"` block generation.
//!
//! Generates the standard pattern seen in worker-sys:
//!
//! ```rust,ignore
//! #[wasm_bindgen]
//! extern "C" {
//!     #[wasm_bindgen(extends = js_sys::Object, js_name = "MyClass")]
//!     #[derive(Debug, Clone, PartialEq, Eq)]
//!     pub type MyClass;
//!
//!     #[wasm_bindgen(constructor, catch)]
//!     pub fn new(arg: &str) -> Result<MyClass, JsValue>;
//!
//!     #[wasm_bindgen(method, getter)]
//!     pub fn name(this: &MyClass) -> String;
//!
//!     #[wasm_bindgen(method, js_name = "doThing")]
//!     pub fn do_thing(this: &MyClass, x: f64);
//! }
//! ```

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use std::collections::HashMap;

use crate::codegen::signatures::{
    build_signatures, generate_concrete_params_with_mono_bounds, generate_dictionary_params,
    is_void_return, public_rust_name, render_generic_bounds, CallableSpec, ConcreteParam,
    DictionaryParams, FunctionSignature, Overload, SignatureKind,
};
use crate::codegen::typemap::{to_return_type, CodegenContext, TypePosition};
use crate::ir::{
    ClassDecl, GetterMember, InterfaceClassification, InterfaceDecl, Member, MethodMember,
    ModuleContext, SetterMember, StaticGetterMember, StaticMethodMember, StaticSetterMember,
    TypeKind, TypeRef,
};
use crate::parse::scope::ScopeId;
use crate::util::naming::{naming_priority, to_snake_case, LocalScope, Suffix};

/// Configuration for generating a class-like extern block.
pub(crate) struct ClassConfig<'a> {
    /// Rust type name.
    rust_name: String,
    /// JS class name (for `js_name` / `js_class` attributes).
    js_name: String,
    /// The `extends` parents (wasm_bindgen supports chained `extends = ...`).
    extends: Vec<TokenStream>,
    /// Module specifier for `#[wasm_bindgen(module = "...")]`.
    module: Option<std::rc::Rc<str>>,
    /// JS namespace path (e.g., `["WebAssembly"]`) for types inside a
    /// namespace, outermost first; empty at the top level.
    js_namespace: Vec<String>,
    /// Whether this is an abstract class (skip constructor).
    is_abstract: bool,
    /// Members to generate.
    members: Vec<Member>,
    /// See [`member_names`].
    member_names: MemberNames,
    /// Type parameters declared on the type itself (`<T, U, …>`).
    /// Rendered as `<T: JsGeneric, …>` on the `pub type` decl and
    /// propagated to every `this: &Type<T, …>` reference inside the
    /// extern block.
    type_params: Vec<crate::ir::TypeParam>,
    /// Codegen context for type resolution.
    cgctx: Option<&'a CodegenContext<'a>>,
    /// Scope for type reference resolution. For class-like configs
    /// this is the type's body scope (holds its `<T, …>`).
    scope: ScopeId,
    /// The item type a declared `[Symbol.iterator]()` yields; see
    /// [`take_symbol_methods`].
    iterable: Option<TypeRef>,
}

/// Splits declared well-known-symbol methods out of `members` (the parser
/// names them `[Symbol.iterator]`, where a synthesized wrapper's method is
/// `iterator`), returning the item type of a sync `[Symbol.iterator]()`.
/// Per-mono it becomes `IntoIterator for &Type`; otherwise it is dropped,
/// as before the parser kept it.
fn take_symbol_methods(members: &mut Vec<Member>) -> Option<TypeRef> {
    let mut item = None;
    members.retain(|m| match m {
        Member::Method(m) if m.name.starts_with('[') => {
            if m.js_name == "[Symbol.iterator]" && m.params.is_empty() && m.type_params.is_empty() {
                item = match &m.return_type {
                    TypeRef::Reference {
                        segments,
                        generic_args,
                    } if segments.len() == 1
                        && matches!(
                            segments[0].as_str(),
                            "IterableIterator" | "Iterator" | "IteratorObject" | "ArrayIterator"
                        ) =>
                    {
                        generic_args.first().cloned()
                    }
                    _ => None,
                };
            }
            false
        }
        _ => true,
    });
    item
}

/// What a member base names; instance and static members of the same JS
/// name are distinct members of one extern block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MemberKind {
    Property,
    StaticProperty,
    Method,
    StaticMethod,
}

type MemberKey = (MemberKind, String);

/// The names a type's members are emitted under; see [`member_names`].
#[derive(Default, Clone)]
struct MemberNames {
    /// The snake_case base of each property and method: a method's and a
    /// getter's own name, and a dictionary's factory parameter.
    bases: HashMap<MemberKey, String>,
    /// The `set_` name of each property that has a setter, which a
    /// dictionary's builder method follows.
    setters: HashMap<MemberKey, String>,
    /// The base a property named like a generated name had before taking
    /// its trailing underscore, which its other names derive from.
    stems: HashMap<MemberKey, String>,
}

/// Name a type's properties and methods up front.
///
/// A property's getter, `set_` setter, and a dictionary's factory parameter
/// and builder method are named in separate passes, so names that convert
/// alike (`data` / `$data`, `fooBar` / `foo_bar`) are made distinct here,
/// once. Declared names claim first, instance members before statics and
/// each in [`naming_priority`] order after the constructor's `new`, then
/// the setters' derived names: `set_` plus the
/// property's base (`$data` → `data_2` / `set_data_2`), numbered on its own
/// when a declared name already has it (`setFoo()` keeps `set_foo` and
/// property `foo`'s setter becomes `set_foo_2`).
///
/// A property named like one of the type's `generated` names (`new`) can
/// only clash with that one name, so it takes a trailing underscore instead
/// (`new_`, still `set_new`). A method's names (`try_new`, `new_with_x`) can
/// clash more than once, so it's numbered.
///
/// A static named like an instance member takes `_static`, MDN's spelling
/// for the clash (`Response.json` → `json_static`, `response.json()` →
/// `json`).
fn member_names(members: &[Member], generated: &[&str]) -> MemberNames {
    let mut keyed: Vec<(MemberKind, &str)> = members
        .iter()
        .filter_map(|member| {
            Some(match member {
                Member::Getter(GetterMember { js_name, .. })
                | Member::Setter(SetterMember { js_name, .. }) => (MemberKind::Property, js_name),
                Member::StaticGetter(StaticGetterMember { js_name, .. })
                | Member::StaticSetter(StaticSetterMember { js_name, .. }) => {
                    (MemberKind::StaticProperty, js_name)
                }
                Member::Method(MethodMember { js_name, .. }) => (MemberKind::Method, js_name),
                Member::StaticMethod(StaticMethodMember { js_name, .. }) => {
                    (MemberKind::StaticMethod, js_name)
                }
                _ => return None,
            })
        })
        .map(|(kind, js_name)| (kind, js_name.as_str()))
        .collect();
    let is_static =
        |kind: &MemberKind| matches!(kind, MemberKind::StaticProperty | MemberKind::StaticMethod);
    keyed.sort_by_key(|(kind, js_name)| {
        (
            is_static(kind),
            naming_priority(js_name, &member_snake(js_name)),
        )
    });

    let mut scope = LocalScope::default();
    for name in generated {
        scope.claim(*name);
    }
    let mut names = MemberNames::default();
    let mut instance = HashSet::new();
    for (kind, js_name) in &keyed {
        let key = (*kind, js_name.to_string());
        if names.bases.contains_key(&key) {
            continue;
        }
        let mut snake = member_snake(js_name);
        if !is_static(kind) {
            instance.insert(snake.clone());
        } else if instance.contains(&snake) {
            snake.push_str("_static");
        }
        let is_property = matches!(kind, MemberKind::Property | MemberKind::StaticProperty);
        let base = if is_property && generated.contains(&public_rust_name(&snake).as_str()) {
            names.stems.insert(key.clone(), snake.clone());
            scope.declare(&format!("{snake}_"), Suffix::Snake)
        } else {
            scope.declare(&snake, Suffix::Snake)
        };
        // A keyword is declared under its public spelling (`type_`).
        scope.claim(public_rust_name(&base));
        names.bases.insert(key, base);
    }
    // Every setter that can keep `set_<base>` does before any is numbered,
    // so `set_data_2` can't go to `data`'s setter while `data_2` has one.
    let mut numbered = Vec::new();
    for member in members {
        let key = match member {
            Member::Setter(SetterMember { js_name, .. }) => (MemberKind::Property, js_name),
            Member::StaticSetter(StaticSetterMember { js_name, .. }) => {
                (MemberKind::StaticProperty, js_name)
            }
            _ => continue,
        };
        let key = (key.0, key.1.clone());
        if names.setters.contains_key(&key) || numbered.contains(&key) {
            continue;
        }
        let setter = format!("set_{}", names.stem(&key));
        if scope.claim(setter.clone()) {
            names.setters.insert(key, setter);
        } else {
            numbered.push(key);
        }
    }
    for key in numbered {
        let setter = scope.declare(&format!("set_{}", names.stem(&key)), Suffix::Snake);
        names.setters.insert(key, setter);
    }
    names
}

impl MemberNames {
    /// The base `key`'s derived names follow; see [`Self::stems`].
    fn stem(&self, key: &MemberKey) -> &str {
        self.stems.get(key).unwrap_or(&self.bases[key])
    }
}

/// The names a type's generated API takes; see [`member_names`].
fn generated_names(
    members: &[Member],
    is_abstract: bool,
    dictionary: bool,
) -> &'static [&'static str] {
    if dictionary {
        &["new", "builder", "build"]
    } else if !is_abstract && members.iter().any(|m| matches!(m, Member::Constructor(_))) {
        &["new"]
    } else {
        &[]
    }
}

fn member_snake(js_name: &str) -> String {
    super::signatures::base_rust_name(js_name, SignatureKind::Method)
}

impl ClassConfig<'_> {
    /// The base a member is named from; see [`member_names`].
    fn member_base(&self, kind: MemberKind, js_name: &str) -> String {
        self.member_names
            .bases
            .get(&(kind, js_name.to_string()))
            .cloned()
            .unwrap_or_else(|| member_snake(js_name))
    }

    /// A property's setter name; see [`member_names`].
    fn setter_name(&self, kind: MemberKind, js_name: &str) -> String {
        self.member_names
            .setters
            .get(&(kind, js_name.to_string()))
            .cloned()
            .unwrap_or_else(|| format!("set_{}", self.member_base(kind, js_name)))
    }

    /// The base a member's derived names follow; see [`MemberNames::stems`].
    fn member_stem(&self, kind: MemberKind, js_name: &str) -> String {
        let key = (kind, js_name.to_string());
        match self.member_names.stems.get(&key) {
            Some(stem) => stem.clone(),
            None => self.member_base(kind, js_name),
        }
    }

    /// An extern block's names with every member's own names reserved, so
    /// emitting members in declaration order still gives each the name
    /// [`member_names`] chose.
    fn reserved_names(&self) -> LocalScope {
        let mut names = LocalScope::default();
        for base in self.member_names.bases.values() {
            names.reserve(base.clone());
            names.reserve(public_rust_name(base));
        }
        for setter in self.member_names.setters.values() {
            names.reserve(setter.clone());
        }
        names
    }

    /// The enum a property's setter shares with its getter; see
    /// [`super::typemap::setter_enum`].
    fn setter_enum(&self, js_name: &str, ty: &TypeRef) -> Option<super::typemap::SetterEnum> {
        super::typemap::setter_enum(
            ty,
            self.cgctx,
            self.scope,
            &self.from_module(),
            super::typemap::ReturnAnchor {
                base: js_name,
                kind: super::typemap::ReturnAnchorKind::Getter,
                parent: Some(&self.rust_name),
            },
        )
    }

    /// A setter's single parameter, typed so literal members route through
    /// the getter's enum.
    fn setter_param(
        &self,
        js_name: &str,
        ty: &TypeRef,
        slice_mode: Option<crate::ir::SliceModeHint>,
    ) -> crate::ir::Param {
        let mut type_ref = match self.setter_enum(js_name, ty) {
            Some(e) => e.param_type(ty),
            None => ty.clone(),
        };
        if let Some(widened) = slice_mode.and_then(|m| m.widen_bare_typed_array(&type_ref)) {
            type_ref = widened;
        }
        crate::ir::Param {
            name: "val".to_string(),
            type_ref,
            optional: false,
            variadic: false,
            slice_mode,
        }
    }
}

impl<'a> ClassConfig<'a> {
    fn from_class(
        decl: &ClassDecl,
        ctx: &ModuleContext,
        cgctx: Option<&'a CodegenContext>,
        _enclosing_scope: ScopeId,
    ) -> Self {
        // The class's body scope (holding its `<T, …>`) is the right
        // scope to thread through name resolution everywhere inside
        // its extern block.
        let scope = decl.body_scope;
        let extends = extends_list(decl.extends.as_slice(), cgctx, ctx);
        let module = match ctx {
            ModuleContext::Module(m) => Some(m.clone()),
            ModuleContext::Global => None,
        };

        let mut members = decl.members.clone();
        let iterable = take_symbol_methods(&mut members);
        super::type_params::erase_uninferrable_in_members(&mut members, cgctx);
        ClassConfig {
            rust_name: decl.name.clone(),
            js_name: decl.js_name.clone(),
            extends,
            module,
            js_namespace: Vec::new(),
            is_abstract: decl.is_abstract,
            member_names: member_names(
                &members,
                generated_names(&members, decl.is_abstract, false),
            ),
            members,
            type_params: decl.type_params.clone(),
            cgctx,
            scope,
            iterable,
        }
    }

    fn from_interface(
        decl: &InterfaceDecl,
        ctx: &ModuleContext,
        cgctx: Option<&'a CodegenContext>,
        _enclosing_scope: ScopeId,
        dictionary: bool,
    ) -> Self {
        let scope = decl.body_scope;
        let extends = extends_list(&decl.extends, cgctx, ctx);
        let module = match ctx {
            ModuleContext::Module(m) => Some(m.clone()),
            ModuleContext::Global => None,
        };

        let inherited = inherited_members(&decl.extends, cgctx);
        let mut members = inherit_into(inherited.beyond_first, &decl.members);
        let iterable = take_symbol_methods(&mut members);
        super::type_params::erase_uninferrable_in_members(&mut members, cgctx);

        ClassConfig {
            rust_name: decl.name.clone(),
            js_name: decl.js_name.clone(),
            extends,
            module,
            js_namespace: Vec::new(),
            is_abstract: false,
            member_names: member_names(&members, generated_names(&members, false, dictionary)),
            members,
            type_params: decl.type_params.clone(),
            cgctx,
            scope,
            iterable,
        }
    }

    /// The config with the first parent chain's members merged in as well,
    /// for building the dictionary factory: wasm-bindgen derefs to the
    /// first parent so its accessors are not re-declared, but the factory
    /// has to set its fields.
    fn with_first_parent_members(&self, decl: &InterfaceDecl) -> Self {
        let inherited = inherited_members(&decl.extends, self.cgctx);
        let members = inherit_into(inherited.first_chain, &self.members);
        ClassConfig {
            rust_name: self.rust_name.clone(),
            js_name: self.js_name.clone(),
            extends: self.extends.clone(),
            module: self.module.clone(),
            js_namespace: self.js_namespace.clone(),
            is_abstract: self.is_abstract,
            members,
            member_names: self.member_names.clone(),
            type_params: self.type_params.clone(),
            cgctx: self.cgctx,
            scope: self.scope,
            iterable: self.iterable.clone(),
        }
    }

    /// Build a `ClassConfig` from a [`DiscriminatedUnionDecl`]. Members
    /// are the merged shape — used for the extern block (one getter /
    /// setter set covering every property across branches). Per-branch
    /// information is passed alongside through
    /// [`generate_dictionary_factory_with_passes`] in the caller.
    pub(crate) fn from_discriminated_union(
        decl: &crate::ir::DiscriminatedUnionDecl,
        ctx: &ModuleContext,
        cgctx: Option<&'a CodegenContext>,
        _enclosing_scope: ScopeId,
    ) -> Self {
        let scope = decl.body_scope;
        let module = match ctx {
            ModuleContext::Module(m) => Some(m.clone()),
            ModuleContext::Global => None,
        };
        let mut members = decl.members.clone();
        let iterable = take_symbol_methods(&mut members);
        super::type_params::erase_uninferrable_in_members(&mut members, cgctx);
        ClassConfig {
            rust_name: decl.name.clone(),
            js_name: decl.js_name.clone(),
            extends: vec![quote! { Object }],
            module,
            js_namespace: Vec::new(),
            is_abstract: false,
            member_names: member_names(&members, generated_names(&members, false, true)),
            members,
            type_params: decl.type_params.clone(),
            cgctx,
            scope,
            iterable,
        }
    }

    /// Tokens for the type-level generic declaration
    /// (`<T: ::wasm_bindgen::JsGeneric, …>`) or empty when the type
    /// has no parameters.
    fn type_generics_decl(&self) -> TokenStream {
        render_generic_bounds(&self.type_param_bounds())
    }

    /// The `pub type` form of the generic declaration: every parameter
    /// carries a default so the bare name (`Request`, `ReadableStream`)
    /// stays usable at every site that does not care about the
    /// parameter. The default is the TS default when one is declared,
    /// otherwise `JsValue`.
    fn type_generics_decl_with_defaults(&self) -> TokenStream {
        if self.type_params.is_empty() {
            return quote! {};
        }
        let params: Vec<TokenStream> = self
            .type_params
            .iter()
            .map(|tp| {
                let decl = CodegenContext::type_param_decl(self.cgctx, &tp.name);
                let default = self.type_param_default_tokens(tp);
                quote! { #decl = #default }
            })
            .collect();
        quote! { <#(#params),*> }
    }

    /// Lowering of a type parameter's default in generic-argument
    /// position; `JsValue` when the TS declaration has none. When the
    /// default's target itself erases to `JsValue` (an alias of a union,
    /// an unresolved name) its type arguments go with it:
    /// `CfProperties<CfHostMetadata>` is just `JsValue`.
    fn type_param_default_tokens(&self, tp: &crate::ir::TypeParam) -> TokenStream {
        let Some(default) = &tp.default else {
            return quote! { JsValue };
        };
        let tokens = super::typemap::to_syn_type(
            default,
            TypePosition::ARGUMENT.to_inner(),
            self.cgctx,
            self.scope,
            &self.from_module(),
        );
        if let (TypeRef::Reference { segments, .. }, Some(ctx)) = (default, self.cgctx) {
            let uses = ctx.external_uses.borrow();
            if uses.get(&segments[0]).is_some_and(|v| v == "JsValue") {
                return quote! { JsValue };
            }
        }
        tokens
    }

    /// The TS default of a type parameter as a `TypeRef`, `any` when
    /// none is declared.
    fn type_param_default_ref(&self, tp: &crate::ir::TypeParam) -> TypeRef {
        tp.default.clone().unwrap_or(TypeRef::Any)
    }

    /// Type-level parameters mentioned by `tys`, in declaration order.
    fn mentioned_type_params(&self, scope: ScopeId, tys: &[&TypeRef]) -> Vec<String> {
        let names = self.mentioned_type_param_names(scope, tys);
        self.type_params
            .iter()
            .filter(|tp| names.contains(&tp.name))
            .map(|tp| tp.name.clone())
            .collect()
    }

    /// Every name in `tys` that resolves to a type parameter in `scope`
    /// (type-level or callable-level), in appearance order.
    fn mentioned_type_param_names(&self, scope: ScopeId, tys: &[&TypeRef]) -> Vec<String> {
        let Some(cgctx) = self.cgctx else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        for ty in tys {
            super::signatures::collect_type_params(ty, cgctx, scope, &mut names);
        }
        names
    }

    /// Under per-monomorphization, a construct signature's declared return
    /// when it instantiates the class with concrete arguments only
    /// (`new (source: UnderlyingByteSource): ReadableStream<Uint8Array>`),
    /// which the constructor then returns as written rather than at the
    /// class's defaults.
    fn concrete_construct_return<'c>(
        &self,
        c: &'c crate::ir::ConstructorMember,
    ) -> Option<&'c TypeRef> {
        if !CodegenContext::generic_mono(self.cgctx) || self.type_params.is_empty() {
            return None;
        }
        let ret = c.return_type.as_ref()?;
        let TypeRef::Reference {
            segments,
            generic_args,
        } = ret
        else {
            return None;
        };
        let names_class = segments.len() == 1
            && (segments[0] == self.rust_name || segments[0] == self.js_name)
            && generic_args.len() == self.type_params.len();
        (names_class && self.mentioned_type_param_names(c.scope, &[ret]).is_empty()).then_some(ret)
    }

    /// Generic declaration for Rust helper impls and builder structs. The
    /// erased path keeps the extern declaration's `JsGeneric` bound. The
    /// per-monomorphization path instead needs `IntoWasmAbi` because helper
    /// bodies call generated setters with values of these types.
    fn helper_generics_decl(&self) -> TokenStream {
        if !CodegenContext::generic_mono(self.cgctx) {
            return self.type_generics_decl();
        }
        let idents = self
            .type_params
            .iter()
            .map(|tp| {
                let ident = super::typemap::make_ident(&tp.name);
                quote! { #ident }
            })
            .collect::<Vec<_>>();
        render_generic_bounds(&idents)
    }

    /// The per-monomorphization helpers' `where` clause. Each helper passes
    /// its arguments to the setters, whose wasm-bindgen imports bound every
    /// argument shape naming a type parameter (`&T`, `Option<T>`, `&[T]`),
    /// so the impl restates those bounds for the helpers' own signatures.
    fn helper_where_clause<'f>(&self, fns: impl Iterator<Item = &'f TokenStream>) -> TokenStream {
        if !CodegenContext::generic_mono(self.cgctx) || self.type_params.is_empty() {
            return quote! {};
        }
        let idents: Vec<syn::Ident> = self
            .type_params
            .iter()
            .map(|tp| super::typemap::make_ident(&tp.name))
            .collect();
        let sigs: Vec<syn::Signature> = fns
            .filter_map(|f| syn::parse2::<syn::ImplItemFn>(f.clone()).ok())
            .map(|f| f.sig)
            .collect();
        let mut preds = super::inline_wrappers::mono_arg_bounds(
            sigs.iter().flat_map(|s| s.inputs.iter()),
            &idents,
        );
        let mut seen = std::collections::HashSet::new();
        preds.retain(|p| seen.insert(quote::ToTokens::to_token_stream(p).to_string()));
        if preds.is_empty() {
            return quote! {};
        }
        quote! { where #(#preds),* }
    }

    /// Tokens for the type's generic-argument list (`<T, …>`) used in
    /// `this: &Type<T, …>` references inside the extern block. Empty
    /// when there are no parameters.
    fn type_generics_args(&self) -> TokenStream {
        if self.type_params.is_empty() {
            return quote! {};
        }
        let idents = self
            .type_params
            .iter()
            .map(|tp| super::typemap::make_ident(&tp.name))
            .collect::<Vec<_>>();
        quote! { <#(#idents),*> }
    }

    /// One `T: ::wasm_bindgen::JsGeneric` token stream per type-level
    /// parameter. Used by per-callable emitters to merge type-level
    /// bounds with method-local bounds (`<T, ...>` collected from
    /// signature types) and ABV widening bounds (`<Tn: TypedArray>`)
    /// into a single `<...>` declaration on each emitted `fn`.
    fn type_param_bounds(&self) -> Vec<TokenStream> {
        self.type_params
            .iter()
            .map(|tp| CodegenContext::type_param_decl(self.cgctx, &tp.name))
            .collect()
    }

    /// Bounds for type params that appear in any of the supplied
    /// `TypeRef`s but are **not** type-level — i.e. method/getter/setter
    /// -local generics introduced by signature types. Each appears as
    /// `T: ::wasm_bindgen::JsGeneric`, matching the type-level form so
    /// the two lists can be concatenated by [`render_generic_bounds`].
    ///
    /// Names already covered by `self.type_params` are skipped — the
    /// caller composes the two lists in order (type-level first).
    fn local_generic_bounds(&self, scope: ScopeId, tys: &[&TypeRef]) -> Vec<TokenStream> {
        let cgctx = match self.cgctx {
            Some(c) => c,
            None => return Vec::new(),
        };
        let mut names: Vec<String> = Vec::new();
        for ty in tys {
            super::signatures::collect_type_params(ty, cgctx, scope, &mut names);
        }
        let type_level: std::collections::HashSet<&str> =
            self.type_params.iter().map(|tp| tp.name.as_str()).collect();
        names
            .into_iter()
            .filter(|n| !type_level.contains(n.as_str()))
            .map(|n| CodegenContext::type_param_decl(Some(cgctx), &n))
            .collect()
    }

    /// Rust name to use everywhere the class identifier appears in generated
    /// code (`pub type X`, `this: &X`, `static_method_of = X`, …).
    ///
    /// When the declared name collides with a `js_sys` reserved identifier,
    /// `CodegenContext::resolve_collisions` chose a suffixed alternative
    /// (`Global` → `Global_`); this method threads that through. Falls back
    /// to the original `rust_name` when no rename was registered.
    fn effective_rust_name(&self) -> String {
        self.cgctx
            .and_then(|ctx| ctx.renamed_locals.get(&self.rust_name).cloned())
            .unwrap_or_else(|| self.rust_name.clone())
    }

    /// wasm-bindgen resolves the JS class of a constructor or static member
    /// from the return type / `static_method_of` ident and the item's own
    /// `js_namespace`, ignoring the type's `js_name` and `js_namespace`. A
    /// sanitised, renamed, or namespaced type has to spell both out.
    fn js_class_parts(&self) -> Vec<TokenStream> {
        let mut parts = Vec::new();
        let ident = super::typemap::make_ident(&self.effective_rust_name());
        if super::typemap::needs_js_name(&ident, &self.js_name) {
            let js_class = &self.js_name;
            parts.push(quote! { js_class = #js_class });
        }
        parts.extend(super::js_namespace_attr(&self.js_namespace));
        parts
    }

    /// The `ModuleContext` this extern block lives in. Used as the
    /// `from_module` argument to type-emit helpers so cross-module
    /// references get the correct path qualifier.
    #[allow(clippy::wrong_self_convention)]
    fn from_module(&self) -> ModuleContext {
        match &self.module {
            Some(m) => ModuleContext::Module(m.clone()),
            None => ModuleContext::Global,
        }
    }
}

/// Generate a complete `extern "C"` block for a class-like declaration.
pub fn generate_class(
    decl: &ClassDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TokenStream {
    let config = ClassConfig::from_class(decl, ctx, cgctx, scope);
    generate_extern_block(&config)
}

/// Generate a complete `extern "C"` block for a class-like interface.
pub fn generate_class_like_interface(
    decl: &InterfaceDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    js_namespace: &[String],
    scope: ScopeId,
) -> TokenStream {
    let mut config = ClassConfig::from_interface(decl, ctx, cgctx, scope, false);
    config.js_namespace = js_namespace.to_vec();
    generate_extern_block(&config)
}

/// Generate a complete `extern "C"` block for a class inside a namespace, with `js_namespace`.
pub fn generate_class_with_js_namespace(
    decl: &ClassDecl,
    ctx: &ModuleContext,
    js_namespace: &[String],
    cgctx: Option<&CodegenContext<'_>>,
    scope: ScopeId,
) -> TokenStream {
    let mut config = ClassConfig::from_class(decl, ctx, cgctx, scope);
    config.js_namespace = js_namespace.to_vec();
    generate_extern_block(&config)
}

/// Generate a simple extern "C" block for a dictionary interface.
/// Temporary until M5 implements proper dictionary builders.
pub fn generate_dictionary_extern(
    decl: &InterfaceDecl,
    ctx: &ModuleContext,
    cgctx: Option<&CodegenContext<'_>>,
    js_namespace: &[String],
    scope: ScopeId,
) -> TokenStream {
    let mut config = ClassConfig::from_interface(decl, ctx, cgctx, scope, true);
    config.js_namespace = js_namespace.to_vec();

    let extern_block = generate_extern_block(&config);
    let factory = generate_dictionary_factory(&config.with_first_parent_members(decl));

    quote! {
        #extern_block
        #factory
    }
}

/// Generate a Rust `impl` block with factory constructors for a dictionary interface.
///
/// Produces `new()` plus expanded variants like `new_with_status(status: f64)`,
/// `new_with_status_and_status_text(status: f64, status_text: &str)`, etc.
/// Each factory creates a bare `Object`, sets the provided properties via their
/// setters, and returns it cast to the dictionary type.
/// Generate a Rust `impl` block with `new()` and `builder()` for a dictionary interface.
///
/// Produces:
/// ```ignore
/// impl ResponseInit {
///     pub fn new() -> Self { ... }
///     pub fn builder() -> ResponseInitBuilder { ... }
/// }
///
/// pub struct ResponseInitBuilder { inner: ResponseInit }
/// impl ResponseInitBuilder {
///     pub fn status(self, val: f64) -> Self { ... }
///     pub fn headers(self, val: &Headers) -> Self { ... }
///     pub fn build(self) -> ResponseInit { ... }
/// }
/// ```
fn generate_dictionary_factory(config: &ClassConfig) -> TokenStream {
    generate_dictionary_factory_with_passes(config, None)
}

/// Same as [`generate_dictionary_factory`] but lets the caller override
/// the **required-field passes** that drive `new_*` / `builder_*`
/// generation.
///
/// * `None` — single pass over the merged required-getter set. This is
///   the regular-interface and dictionary path.
/// * `Some(passes)` — one pass per entry; each entry's getters drive
///   one cohort of `new_*` / `builder_*` variants. Used for
///   discriminated unions, where each branch contributes a distinct
///   required-field pass and produces its own per-discriminant
///   factories.
///
/// The wrapper struct's fluent setters (the
/// `optional`-fields-on-the-merged-shape side) are unchanged across
/// passes — they always reflect the merged optional set, which is the
/// runtime-permissive view callers can always tweak.
pub(crate) fn generate_dictionary_factory_with_passes(
    config: &ClassConfig,
    factory_required_overrides: Option<Vec<Vec<&crate::ir::GetterMember>>>,
) -> TokenStream {
    let rust_type_ident = super::typemap::make_ident(&config.effective_rust_name());
    let builder_name_ident = super::typemap::make_ident(&format!("{}Builder", config.rust_name));
    // The type's generic-arg list (`<T, …>`) — applied to every
    // mention of `rust_type` / `builder_name` as a *type*. Empty
    // for non-generic dictionaries, in which case `quote!`
    // interpolation leaves the bare identifier intact.
    let type_args = config.type_generics_args();
    let type_decl = config.helper_generics_decl();
    let builder_struct_decl = config.type_generics_decl();
    let rust_type = quote! { #rust_type_ident #type_args };
    let builder_name = quote! { #builder_name_ident #type_args };

    // If any getter lacks a corresponding setter the type has readonly
    // properties, which means it is not constructible via setters — skip
    // the builder entirely and only emit a bare `new()`.
    let setter_types: std::collections::HashMap<&str, &crate::ir::TypeRef> = config
        .members
        .iter()
        .filter_map(|m| {
            if let Member::Setter(s) = m {
                Some((s.js_name.as_str(), &s.type_ref))
            } else {
                None
            }
        })
        .collect();
    let has_readonly = config.members.iter().any(|m| {
        if let Member::Getter(g) = m {
            !setter_types.contains_key(g.js_name.as_str())
        } else {
            false
        }
    });
    if has_readonly {
        return quote! {
            impl #type_decl #rust_type {
                #[allow(clippy::new_without_default)]
                pub fn new() -> #rust_type {
                    #[allow(unused_unsafe)]
                    unsafe { JsValue::from(js_sys::Object::new()).unchecked_into() }
                }
            }
        };
    }

    // Collect getter properties for builder methods
    let getters: Vec<&crate::ir::GetterMember> = config
        .members
        .iter()
        .filter_map(|m| {
            if let Member::Getter(g) = m {
                Some(g)
            } else {
                None
            }
        })
        .collect();

    // Partition getters by the merged-shape `optional` flag. This
    // partition drives the wrapper's fluent setters (optional set) and
    // is the default required-field pass when no per-branch override is
    // supplied.
    let mut required_getters: Vec<&crate::ir::GetterMember> = Vec::new();
    let mut optional_getters: Vec<&crate::ir::GetterMember> = Vec::new();
    for g in &getters {
        if g.optional {
            optional_getters.push(g);
        } else {
            required_getters.push(g);
        }
    }

    // The list of required-field passes drives `new_*` / `builder_*`
    // generation. Plain interfaces / dictionaries get a single pass
    // (the merged required set). Discriminated unions thread one pass
    // per branch via `factory_required_overrides` so that each
    // branch's required fields surface as positional args in its own
    // `new_<discriminator>(...)` variant — see
    // [`generate_dictionary_factory_with_passes`].
    let required_passes: Vec<Vec<&crate::ir::GetterMember>> =
        factory_required_overrides.unwrap_or_else(|| vec![required_getters.clone()]);

    // When every property is required, the builder type is dead weight: it
    // would carry only `pub fn build(self) -> Foo` with nothing to chain,
    // so we suppress it. `new(reqs)` still gets emitted but inlines the
    // construction directly. See the dictionary-builder convention in
    // `CONVENTIONS.md` for the rule.
    //
    // For discriminated unions the merged-shape optional set is what
    // populates the wrapper's setters, so the decision keys off
    // `optional_getters` regardless of the per-branch `required_passes`.
    let emit_builder = !optional_getters.is_empty();

    // Resolve each getter through the setter pipeline. Multi-overload
    // setters (e.g. `set_from(&str)` + `set_from_with_email_address(&EmailAddress)`
    // for `from: string | EmailAddress`) produce multiple sigs — we
    // cartesian-product across required fields below to expose every
    // combination as its own `new` / `builder` variant.
    // A factory/builder argument takes the property's `{@slice mode}`
    // hint, carried on the corresponding setter member.
    let slice_mode_of = |js_name: &str| {
        config.members.iter().find_map(|m| match m {
            Member::Setter(s) if s.js_name == js_name => s.slice_mode,
            _ => None,
        })
    };
    // Factories call the setter the extern block emits, so they take its
    // type, not the getter's: an optional property reads `T | undefined`,
    // and a discriminated-union branch narrows its getter
    // (`disposition: "inline"`), while the setter keeps the merged type
    // (`"inline" | "attachment"`) and with it the enum a literal is passed as.
    let assigned_type = |g: &crate::ir::GetterMember| -> crate::ir::TypeRef {
        setter_types
            .get(g.js_name.as_str())
            .map_or_else(|| g.type_ref.clone(), |t| (*t).clone())
    };
    let resolve_setter_sigs = |g: &crate::ir::GetterMember| -> Vec<FunctionSignature> {
        let setter_param =
            config.setter_param(&g.js_name, &assigned_type(g), slice_mode_of(&g.js_name));
        let mut setter_used = LocalScope::default();
        let void = crate::ir::TypeRef::Void;
        let setter_overloads = [Overload {
            params: &[setter_param],
            return_type: &void,
            body_scope: g.scope,
            throws: &crate::ir::Throws::None,
            doc: &None,
            string_mode: None,
        }];
        let setter_name = config.setter_name(MemberKind::Property, &g.js_name);
        build_signatures(
            &CallableSpec {
                js_name: &g.js_name,
                rust_name: Some(&setter_name),
                kind: SignatureKind::Setter,
                overloads: &setter_overloads,
            },
            &mut setter_used,
            config.cgctx,
        )
    };

    // Per required field, decompose the type ref into a list of
    // *options*. Each option becomes one cell in the cartesian product
    // across fields; literal-typed members are special-cased so the
    // user picks the discriminant via the function name (`new_inline`)
    // rather than passing the literal as a `&str` argument.
    enum FieldOption {
        /// String/number/boolean literal baked into the body. No param
        /// contributed; the suffix part is the literal value.
        Literal {
            /// JS field name (`disposition`) — used for the doc bullet.
            field_js_name: String,
            /// JSDoc on the original getter (if any).
            field_doc: Option<String>,
            /// TS source form of the literal — `"inline"`, `42`, `true` —
            /// rendered into the bullet as
            /// `` `disposition: "inline"` ``.
            literal_display: String,
            suffix: String,
            setter_ident: syn::Ident,
            literal_expr: TokenStream,
        },
        /// Free-form value: contributes a normal positional parameter.
        Value {
            field_doc: Option<String>,
            param: ConcreteParam,
            /// The scope the field's type resolves in.
            scope: ScopeId,
            setter_ident: syn::Ident,
        },
    }
    struct FieldDim {
        options: Vec<FieldOption>,
    }

    // Decompose a getter's `type_ref` into union members + ABI
    // fan-out flavors.
    let split_union = |ty: &crate::ir::TypeRef, scope: ScopeId| -> Vec<crate::ir::TypeRef> {
        crate::codegen::signatures::flatten_dictionary_field(ty, config.cgctx, scope)
    };

    // Helper that turns one required-getter list into a list of
    // `FieldDim`s — the cartesian product axes for that pass.
    let build_field_dims = |required: &[&crate::ir::GetterMember]| -> Vec<FieldDim> {
        let mut field_dims: Vec<FieldDim> = Vec::new();
        for g in required {
            let param_name = config.member_base(MemberKind::Property, &g.js_name);
            let setters = resolve_setter_sigs(g);
            if setters.is_empty() {
                continue;
            }

            // Each alternative (union member or ABI fan-out flavor)
            // maps to one option. Literals find their setter by
            // matching the setter's param type_ref structurally;
            // non-literals get their own option with an
            // `expand_signatures`-compatible param. Fan-out flavors
            // emitted by `flatten_type` (e.g. `JsString` next to
            // `String`) appear as additional Value alternatives and
            // pick up their matching setter (`set_from_with_js_string`)
            // via the same `pick_setter` lookup.
            let mut options: Vec<FieldOption> = Vec::new();
            let members = split_union(&g.type_ref, g.scope);
            let setter_enum = config.setter_enum(&g.js_name, &assigned_type(g));
            let any_literal = members.iter().any(|m| {
                matches!(
                    m,
                    crate::ir::TypeRef::StringLiteral(_)
                        | crate::ir::TypeRef::NumberLiteral(_)
                        | crate::ir::TypeRef::BooleanLiteral(_)
                )
            });
            let value_members: Vec<&crate::ir::TypeRef> = members
                .iter()
                .filter(|m| {
                    !matches!(
                        m,
                        crate::ir::TypeRef::StringLiteral(_)
                            | crate::ir::TypeRef::NumberLiteral(_)
                            | crate::ir::TypeRef::BooleanLiteral(_)
                    )
                })
                .collect();

            // Setter expansion dedups by rendered Rust type, keeping only the
            // first member of each (`"x" | "y"` leaves one setter typed
            // `"x"`), so match on the rendered type rather than the IR. A
            // merged optional field (`contentId?: undefined` in another
            // branch) has a `Nullable` setter param; the member matches
            // through it and the factory takes the setter's own type so the
            // call type-checks. Per-mono primitive-union grouping gives
            // setters and factory options different groupings of the same
            // members (the factory keeps literals apart), so fall back to the
            // grouped setter whose marker trait accepts the option's type.
            fn param_type(sig: &FunctionSignature) -> Option<&crate::ir::TypeRef> {
                sig.params.first().map(|p| &p.type_ref)
            }
            let find_setter = |target: &crate::ir::TypeRef| -> Option<&FunctionSignature> {
                let strip = |t: &crate::ir::TypeRef| match t {
                    crate::ir::TypeRef::Nullable(inner, _) => (**inner).clone(),
                    other => other.clone(),
                };
                let render = |param: &ConcreteParam| {
                    super::signatures::render_param_type(
                        param,
                        config.cgctx,
                        g.scope,
                        &crate::ir::ModuleContext::Global,
                    )
                    .to_string()
                };
                let target_rendered = render(&ConcreteParam {
                    name: String::new(),
                    type_ref: target.clone(),
                    variadic: false,
                    slice_mode: None,
                });
                setters
                    .iter()
                    .find(|sig| {
                        sig.params
                            .first()
                            .is_some_and(|p| render(p) == target_rendered)
                    })
                    .or_else(|| {
                        setters
                            .iter()
                            .find(|sig| param_type(sig).map(strip).as_ref() == Some(target))
                    })
                    .or_else(|| {
                        setters.iter().find(|sig| {
                            param_type(sig)
                                .map(strip)
                                .as_ref()
                                .and_then(super::primitive_unions::PrimitiveUnion::classify)
                                .is_some_and(|u| u.accepts(target))
                        })
                    })
            };
            let pick_setter = |target: &crate::ir::TypeRef| -> Option<syn::Ident> {
                find_setter(target).map(|sig| super::typemap::make_ident(&sig.rust_name))
            };
            // Fallback for fields without per-member setter granularity:
            // the first setter handles everything (typical when the IR
            // only synthesised one setter for the whole union).
            let default_setter = super::typemap::make_ident(&setters[0].rust_name);

            // A literal the setter routes through its enum is passed as
            // that enum's variant.
            let enum_literal = |m: &crate::ir::TypeRef| {
                let e = setter_enum.as_ref()?;
                let variant = super::typemap::make_ident(e.variant(m)?);
                let enum_ident = super::typemap::make_ident(&e.name);
                let setter_ident = pick_setter(&crate::ir::TypeRef::ident(e.name.clone()))?;
                Some((setter_ident, quote! { #enum_ident::#variant }))
            };
            for m in &members {
                match m {
                    crate::ir::TypeRef::StringLiteral(s) => {
                        let (setter_ident, literal_expr) = enum_literal(m).unwrap_or_else(|| {
                            let setter = pick_setter(m).unwrap_or_else(|| default_setter.clone());
                            (setter, quote! { #s })
                        });
                        options.push(FieldOption::Literal {
                            field_js_name: g.js_name.clone(),
                            field_doc: g.doc.clone(),
                            literal_display: format!("\"{s}\""),
                            suffix: format!("_{}", to_snake_case(s)),
                            setter_ident,
                            literal_expr,
                        });
                    }
                    crate::ir::TypeRef::NumberLiteral(n) => {
                        let (setter_ident, literal_expr) = enum_literal(m).unwrap_or_else(|| {
                            let setter = pick_setter(m).unwrap_or_else(|| default_setter.clone());
                            let lit = proc_macro2::Literal::f64_unsuffixed(n.abs());
                            let sign = n.is_sign_negative().then(|| quote! { - });
                            (setter, quote! { #sign #lit })
                        });
                        options.push(FieldOption::Literal {
                            field_js_name: g.js_name.clone(),
                            field_doc: g.doc.clone(),
                            literal_display: n.to_string(),
                            suffix: format!("_{}", crate::util::naming::number_literal_snake(*n)),
                            setter_ident,
                            literal_expr,
                        });
                    }
                    crate::ir::TypeRef::BooleanLiteral(b) => {
                        let setter_ident = pick_setter(m).unwrap_or_else(|| default_setter.clone());
                        options.push(FieldOption::Literal {
                            field_js_name: g.js_name.clone(),
                            field_doc: g.doc.clone(),
                            literal_display: b.to_string(),
                            suffix: format!("_{}", b),
                            setter_ident,
                            literal_expr: quote! { #b },
                        });
                    }
                    _ => {}
                }
            }
            // Non-literal members: if the union mixed literals with
            // concrete types, those concrete types still need their own
            // value options (e.g. `disposition: "inline" | string` →
            // `new_inline()` plus a catch-all `new(disposition: &str)`).
            // For purely non-literal unions we emit one value option
            // per member.
            let value_targets: Vec<&crate::ir::TypeRef> = if any_literal {
                value_members
            } else {
                members.iter().collect()
            };
            for m in value_targets {
                let setter = find_setter(m);
                let setter_ident = setter
                    .map(|sig| super::typemap::make_ident(&sig.rust_name))
                    .unwrap_or_else(|| default_setter.clone());
                let param = ConcreteParam {
                    name: param_name.clone(),
                    type_ref: setter
                        .and_then(param_type)
                        .cloned()
                        .unwrap_or_else(|| m.clone()),
                    variadic: false,
                    slice_mode: None,
                };
                options.push(FieldOption::Value {
                    field_doc: g.doc.clone(),
                    param,
                    scope: g.scope,
                    setter_ident,
                });
            }
            if options.is_empty() {
                continue;
            }
            field_dims.push(FieldDim { options });
        }
        field_dims
    };

    // For each combo: collect the value params (literals contribute
    // only to the suffix, not to the param list) plus the markdown
    // bullets that document this variant. Literal bullets render the
    // baked-in value; value bullets read off the original getter's
    // JSDoc.
    struct ComboPlan {
        literal_suffix: String,
        value_params: Vec<ConcreteParam>,
        value_scopes: Vec<ScopeId>,
        init_calls: Vec<TokenStream>,
        arg_idents: Vec<syn::Ident>,
        /// Lines like `` `disposition: "inline"`: <getter doc> `` that
        /// describe the literal discriminants baked into this variant.
        literal_doc_bullets: Vec<String>,
        /// Lines like `` `from`: <getter doc> `` describing the
        /// caller-provided fields.
        provided_doc_bullets: Vec<String>,
    }

    // A literal names its variant when its field offers a choice: several
    // options within a pass, or different literals across passes (each
    // branch of a discriminated union pins one).
    let mut literals_across_passes: HashMap<String, HashSet<String>> = HashMap::new();
    if required_passes.len() > 1 {
        for pass in &required_passes {
            for dim in build_field_dims(pass) {
                for opt in dim.options {
                    if let FieldOption::Literal {
                        field_js_name,
                        suffix,
                        ..
                    } = opt
                    {
                        literals_across_passes
                            .entry(field_js_name)
                            .or_default()
                            .insert(suffix);
                    }
                }
            }
        }
    }

    let mut builder_variants: Vec<TokenStream> = Vec::new();
    let mut new_variants: Vec<TokenStream> = Vec::new();
    // Shared across passes so a discriminator combo emitted by branch A
    // doesn't get re-emitted under the same name by branch B.
    let mut emitted_names: HashSet<String> = HashSet::new();

    for required_pass in &required_passes {
        // A pass that covers every merged-optional field doesn't need
        // a `builder_<suffix>` companion — going through it would
        // immediately call `.build()` since there are no fluent
        // setters left to chain. Inline the construction into `new_*`
        // and skip the builder variant for that pass.
        //
        // For plain interfaces (single pass over the merged required
        // set) this collapses to the existing `!emit_builder` rule:
        // when no merged optional exists at all, `pass_covers_optionals`
        // is trivially true and we fall through the inlined-body path.
        let pass_covers_optionals = optional_getters
            .iter()
            .all(|opt| required_pass.iter().any(|req| req.js_name == opt.js_name));
        let pass_emit_builder = emit_builder && !pass_covers_optionals;

        let mut field_dims = build_field_dims(required_pass);

        // Cap the dict-factory cartesian. The product across
        // required fields can blow up (a 13-required-field dict
        // with a few small unions each is the case that motivated
        // this cap — workers-types' `AIGatewayHeaders`).
        // Above the cap, every field's options collapse to its
        // single idiomatic-LUB Value option so the cartesian
        // degenerates to one combo per literal-prefix group.
        const DICT_FACTORY_COMBO_LIMIT: usize = 64;
        let combo_count: usize = field_dims
            .iter()
            .map(|d| d.options.len())
            .fold(1usize, |a, n| a.saturating_mul(n));
        if combo_count > DICT_FACTORY_COMBO_LIMIT {
            if let Some(c) = config.cgctx {
                c.warn(format!(
                    "dictionary `{}` factory cartesian is {} combinations (above the \
                     {}-combo limit) — collapsing each required field's setter \
                     overloads to its idiomatic form and suppressing ABI fan-out \
                     for this required set",
                    config.rust_name, combo_count, DICT_FACTORY_COMBO_LIMIT,
                ));
            }
            for dim in &mut field_dims {
                // Keep only the first option (the idiomatic /
                // first-listed setter for the field). All other
                // alternatives are dropped — the wrapper builder's
                // fluent setters still expose them on a per-field
                // basis, no combinatorial cost there.
                dim.options.truncate(1);
            }
        }

        // Cartesian product across field dimensions → list of combos
        // (each is a Vec<&FieldOption>).
        let combos: Vec<Vec<&FieldOption>> = if field_dims.is_empty() {
            vec![vec![]]
        } else {
            let mut acc: Vec<Vec<&FieldOption>> = vec![vec![]];
            for dim in &field_dims {
                acc = acc
                    .into_iter()
                    .flat_map(|prefix| {
                        dim.options.iter().map(move |opt| {
                            let mut next = prefix.clone();
                            next.push(opt);
                            next
                        })
                    })
                    .collect();
            }
            acc
        };

        let mut plans: Vec<ComboPlan> = Vec::with_capacity(combos.len());
        for combo in &combos {
            let mut literal_suffix = String::new();
            let mut value_params: Vec<ConcreteParam> = Vec::new();
            let mut value_scopes: Vec<ScopeId> = Vec::new();
            let mut init_calls: Vec<TokenStream> = Vec::new();
            let mut arg_idents: Vec<syn::Ident> = Vec::new();
            let mut literal_doc_bullets: Vec<String> = Vec::new();
            let mut provided_doc_bullets: Vec<String> = Vec::new();
            for (dim_index, opt) in combo.iter().enumerate() {
                match opt {
                    FieldOption::Literal {
                        field_js_name,
                        field_doc,
                        literal_display,
                        suffix,
                        setter_ident,
                        literal_expr,
                    } => {
                        // A field with a single literal option offers no
                        // choice, so it is baked in without naming the
                        // variant after it (`new`, not `new_0_none__`).
                        if field_dims[dim_index].options.len() > 1
                            || literals_across_passes
                                .get(field_js_name)
                                .is_some_and(|values| values.len() > 1)
                        {
                            literal_suffix.push_str(suffix);
                        }
                        init_calls.push(quote! { inner.#setter_ident(#literal_expr); });
                        let bullet_head = format!("`{field_js_name}: {literal_display}`");
                        literal_doc_bullets.push(match field_doc {
                            Some(doc) => format!("* {bullet_head} - {}", doc.trim()),
                            None => format!("* {bullet_head}"),
                        });
                    }
                    FieldOption::Value {
                        field_doc,
                        param,
                        scope,
                        setter_ident,
                    } => {
                        let arg_ident =
                            super::signatures::dictionary_param_ident(&param.name, config.cgctx);
                        arg_idents.push(arg_ident.clone());
                        init_calls.push(quote! { inner.#setter_ident(#arg_ident); });
                        value_params.push(param.clone());
                        value_scopes.push(*scope);
                        if let Some(doc) = field_doc {
                            let bullet_head = format!("`{}`", param.name);
                            provided_doc_bullets.push(format!("* {bullet_head} - {}", doc.trim()));
                        }
                    }
                }
            }
            plans.push(ComboPlan {
                literal_suffix,
                value_params,
                value_scopes,
                init_calls,
                arg_idents,
                literal_doc_bullets,
                provided_doc_bullets,
            });
        }

        // Compute `_with_X[_and_Y]` suffixes ONLY across combos sharing
        // the same literal prefix within this pass. Combos with
        // distinct literal prefixes are already disambiguated by the
        // prefix; combos in different passes can collide on prefix but
        // are deduped via `emitted_names` below.
        let mut value_suffixes: Vec<String> = vec![String::new(); plans.len()];
        let mut by_prefix: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, p) in plans.iter().enumerate() {
            by_prefix
                .entry(p.literal_suffix.clone())
                .or_default()
                .push(i);
        }
        for indices in by_prefix.values() {
            let group_params: Vec<Vec<ConcreteParam>> = indices
                .iter()
                .map(|&i| plans[i].value_params.clone())
                .collect();
            let group_suffixes = crate::codegen::signatures::compute_suffixes_pub(&group_params);
            for (suffix, &i) in group_suffixes.iter().zip(indices) {
                value_suffixes[i] = suffix.clone();
            }
        }

        for (plan, value_suffix) in plans.iter().zip(&value_suffixes) {
            let full_suffix = format!("{}{}", plan.literal_suffix, value_suffix);
            // Dedup combos that produce the same final name (e.g. when
            // a discriminator collapse plus a value collapse converge,
            // or when two passes happen to compute the same combo).
            if !emitted_names.insert(full_suffix.clone()) {
                continue;
            }
            let new_ident = super::typemap::make_ident(&format!("new{full_suffix}"));
            let DictionaryParams {
                bounds: abv_bounds,
                helper_where_clause,
                params: params_tokens,
                ..
            } = super::signatures::generate_dictionary_params_in(
                &plan.value_params,
                &plan.value_scopes,
                config.cgctx,
                config.scope,
                &config.from_module(),
            );
            // Factory functions live inside `impl<T: ...> Foo<T>`,
            // which already declares the type-level bounds — Rust
            // forbids redeclaring them on the `fn`. Only the
            // method-local bounds (collected from this combo's
            // value params) and ABV widening bounds belong on the
            // `fn`'s `<...>`. The extern block, by contrast,
            // redeclares all bounds because each `pub fn` there is
            // independent (see `generic_params_for_method`).
            let mut combo_bounds: Vec<TokenStream> = Vec::new();
            let combo_param_tys: Vec<&TypeRef> =
                plan.value_params.iter().map(|p| &p.type_ref).collect();
            combo_bounds.extend(config.local_generic_bounds(config.scope, &combo_param_tys));
            combo_bounds.extend(abv_bounds);
            let generics = render_generic_bounds(&combo_bounds);
            let init_calls = &plan.init_calls;
            let arg_idents = &plan.arg_idents;

            // Compose the doc block. `# Inlined fields` lists the
            // literal discriminants baked into the function name (no
            // parameter on the call). `# Parameters` lists the
            // caller-supplied fields that show up in the actual
            // signature. Either section is skipped when its bullet
            // list is empty.
            let mut doc_text = String::new();
            if !plan.literal_doc_bullets.is_empty() {
                doc_text.push_str("## Inlined fields\n\n");
                doc_text.push_str(&plan.literal_doc_bullets.join("\n"));
            }
            if !plan.provided_doc_bullets.is_empty() {
                if !doc_text.is_empty() {
                    doc_text.push_str("\n\n");
                }
                doc_text.push_str(&plan.provided_doc_bullets.join("\n"));
            }
            let doc_attr = if doc_text.is_empty() {
                quote! {}
            } else {
                super::doc_tokens(&Some(doc_text))
            };

            if pass_emit_builder {
                // `builder*` returns the wrapper struct so callers can
                // chain setters; `new*` is just
                // `<Type>::<builder>(reqs).build()`.
                //
                // No-required case has zero `init_calls`, so we inline
                // the factory directly into the struct literal where
                // the field type pins inference. The required-args
                // case keeps a `let inner: <Type>` so setter calls
                // type-resolve before construction.
                //
                // Returns are spelled out as `Foo<T>` /
                // `FooBuilder<T>` rather than `Self`, so the
                // generic and non-generic cases produce identical
                // structure and no `Self` substitution is needed
                // when the body is composed inside an
                // `impl<T> Foo<T>` block.
                let builder_body = if init_calls.is_empty() {
                    quote! {
                        #builder_name_ident {
                            inner: JsCast::unchecked_into(js_sys::Object::new()),
                        }
                    }
                } else {
                    quote! {
                        let inner: #rust_type = JsCast::unchecked_into(js_sys::Object::new());
                        #(#init_calls)*
                        #builder_name_ident { inner }
                    }
                };
                let builder_ident = super::typemap::make_ident(&format!("builder{full_suffix}"));
                builder_variants.push(quote! {
                    #doc_attr
                    pub fn #builder_ident #generics (#params_tokens) -> #builder_name
                    #helper_where_clause
                    {
                        #builder_body
                    }
                });
                new_variants.push(quote! {
                    #doc_attr
                    pub fn #new_ident #generics (#params_tokens) -> #rust_type
                    #helper_where_clause
                    {
                        Self::#builder_ident(#(#arg_idents),*).build()
                    }
                });
            } else {
                // No useful builder for this pass: either the type has
                // no optional fields at all (plain dictionary,
                // `!emit_builder`) or this branch's required set
                // already covers every merged-optional field
                // (`pass_covers_optionals`). Inline the construction
                // into `new_*` and skip the redundant `builder_*`.
                let body = if init_calls.is_empty() {
                    quote! {
                        JsCast::unchecked_into(js_sys::Object::new())
                    }
                } else {
                    quote! {
                        let inner: #rust_type = JsCast::unchecked_into(js_sys::Object::new());
                        #(#init_calls)*
                        inner
                    }
                };
                new_variants.push(quote! {
                    #doc_attr
                    pub fn #new_ident #generics (#params_tokens) -> #rust_type
                    #helper_where_clause
                    {
                        #body
                    }
                });
            }
        }
    }

    // Fluent methods on the wrapper for optional fields. Required fields
    // are intentionally absent — they're only settable through `builder`.
    // (Empty when `!emit_builder`, since there's nothing to chain.)
    let mut builder_methods: Vec<TokenStream> = Vec::new();
    let mut bound_shapes: Vec<TokenStream> = Vec::new();
    for g in &optional_getters {
        let base = config.member_base(MemberKind::Property, &g.js_name);
        let setter_name = config.setter_name(MemberKind::Property, &g.js_name);
        for sig in resolve_setter_sigs(g) {
            // Not calling the builder method already leaves the field unset.
            if sig
                .params
                .iter()
                .any(|p| matches!(p.type_ref, TypeRef::Null))
            {
                continue;
            }
            // The builder has no methods of its own to avoid, so it follows
            // the getter's name, with the setter's expansion suffix.
            let suffix = sig.rust_name.strip_prefix(&setter_name).unwrap_or_default();
            let builder_method_name = if suffix.is_empty() {
                public_rust_name(&base)
            } else {
                let stem = config.member_stem(MemberKind::Property, &g.js_name);
                public_rust_name(&format!("{stem}{suffix}"))
            };
            let method_ident = super::typemap::make_ident(&builder_method_name);
            let setter_ident = super::typemap::make_ident(&sig.rust_name);
            let (builder_params, wrapped) = builder_value_params(config, &sig);
            // Mirrors `generate_setter`: an `ArrayBufferView` param is a
            // `<T: TypedArray>` generic on the extern setter, so the fluent
            // method that forwards to it must carry the same bound.
            let DictionaryParams {
                bounds: abv_bounds,
                helper_where_clause,
                params,
                ..
            } = generate_dictionary_params(
                &builder_params,
                config.cgctx,
                sig.body_scope,
                &config.from_module(),
            );
            let generics = render_generic_bounds(&abv_bounds);
            let param_idents: Vec<_> = sig
                .params
                .iter()
                .zip(&wrapped)
                .map(|(p, &wrap)| {
                    let ident = super::signatures::dictionary_param_ident(&p.name, config.cgctx);
                    match wrap {
                        Wrap::None => quote! { #ident },
                        Wrap::Some => quote! { Some(#ident) },
                        Wrap::SomeObject => {
                            quote! { Some(::wasm_bindgen::JsCast::unchecked_ref(#ident)) }
                        }
                    }
                })
                .collect();
            let method = quote! {
                pub fn #method_ident #generics (self, #params) -> Self
                #helper_where_clause
                {
                    self.inner.#setter_ident(#(#param_idents),*);
                    self
                }
            };
            // A wrapped argument reaches the setter as `Some(…)`, so the
            // setter's shape (`Option<&T>`) is what the impl must bound.
            bound_shapes.push(if wrapped.iter().all(|w| matches!(w, Wrap::None)) {
                method.clone()
            } else {
                let setter_params = generate_dictionary_params(
                    &sig.params,
                    config.cgctx,
                    sig.body_scope,
                    &config.from_module(),
                )
                .params;
                quote! { fn setter(#setter_params) {} }
            });
            builder_methods.push(method);
        }
    }

    let helper_where = config.helper_where_clause(
        new_variants
            .iter()
            .chain(&builder_variants)
            .chain(&bound_shapes),
    );
    if emit_builder {
        quote! {
            impl #type_decl #rust_type #helper_where {
                #(#new_variants)*
                #(#builder_variants)*
            }

            pub struct #builder_name_ident #builder_struct_decl {
                inner: #rust_type,
            }

            impl #type_decl #builder_name #helper_where {
                #(#builder_methods)*

                pub fn build(self) -> #rust_type {
                    self.inner
                }
            }

            impl #type_decl From<#rust_type> for #builder_name {
                fn from(inner: #rust_type) -> Self {
                    Self { inner }
                }
            }
        }
    } else {
        // All-required dictionary: the builder would be empty, so we
        // emit only `new*` factories on the dictionary type itself.
        quote! {
            impl #type_decl #rust_type #helper_where {
                #(#new_variants)*
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Wrap {
    None,
    Some,
    SomeObject,
}

/// A builder method's parameters for an optional field's setter, and which
/// of them the setter takes wrapped in `Some`. Under per-monomorphization
/// the field's empty is not calling the method, so a nullable value is
/// taken bare (`.prefix("a")`, `.signal(&signal)`) and passed on as
/// `Some(…)`; `set_prefix(None)` still clears it.
fn builder_value_params(
    config: &ClassConfig,
    sig: &FunctionSignature,
) -> (Vec<ConcreteParam>, Vec<Wrap>) {
    let mono = CodegenContext::generic_mono(config.cgctx);
    sig.params
        .iter()
        .map(|p| {
            let ty = super::typemap::mono_argument_empty(&p.type_ref, config.cgctx);
            match ty.as_ref() {
                TypeRef::Nullable(inner, _) if mono && !p.variadic => {
                    let bare = ConcreteParam {
                        type_ref: (**inner).clone(),
                        ..p.clone()
                    };
                    // A nullable view's setter takes the `Object` handle,
                    // where the bare one is generic over `TypedArray`.
                    let wrap = if matches!(**inner, TypeRef::ArrayBufferView) {
                        Wrap::SomeObject
                    } else {
                        Wrap::Some
                    };
                    (bare, wrap)
                }
                _ => (p.clone(), Wrap::None),
            }
        })
        .unzip()
}

/// Free a member base reserved by [`ClassConfig::reserved_names`], in both
/// its spellings, for its member to declare.
fn unreserve_base(names: &mut LocalScope, base: &str) {
    names.unreserve(base);
    names.unreserve(&public_rust_name(base));
}

/// Build the full `#[wasm_bindgen] extern "C" { ... }` block.
///
/// All naming happens through a single `names` set that spans the entire
/// extern block. Members are processed in declaration order. Methods with the
/// same `js_name` (TypeScript overloads) are grouped and expanded together as
/// one unit — overloads feed into the same expansion, producing disambiguated
/// `_with_`/`_and_` suffixes across all overloads rather than opaque `_1` suffixes.
///
/// Each name — including `try_` variants — is declared in that scope, which
/// guarantees uniqueness by appending numeric suffixes on collision.
pub(crate) fn generate_extern_block(config: &ClassConfig) -> TokenStream {
    use crate::ir::{ConstructorMember, MethodMember, StaticMethodMember};
    use std::collections::HashMap;

    let mut items = Vec::new();
    let mut names = config.reserved_names();

    // Pre-group methods/statics/constructors by js_name.
    // We iterate config.members in declaration order, so the first occurrence of
    // each js_name determines where its expanded signatures appear in the output.
    let mut method_groups: HashMap<String, Vec<&MethodMember>> = HashMap::new();
    let mut static_method_groups: HashMap<String, Vec<&StaticMethodMember>> = HashMap::new();
    let mut constructor_overloads: Vec<&ConstructorMember> = Vec::new();

    for member in &config.members {
        match member {
            Member::Constructor(ctor) if !config.is_abstract => {
                constructor_overloads.push(ctor);
            }
            Member::Method(m) => {
                method_groups.entry(m.js_name.clone()).or_default().push(m);
            }
            Member::StaticMethod(m) => {
                static_method_groups
                    .entry(m.js_name.clone())
                    .or_default()
                    .push(m);
            }
            _ => {}
        }
    }

    // Track which method groups have been expanded (by js_name).
    let mut expanded_methods: HashSet<String> = HashSet::new();
    let mut expanded_static_methods: HashSet<String> = HashSet::new();
    let mut expanded_constructors = false;

    // 1. Type declaration with attributes
    items.push(generate_type_decl(config));

    // 2. Process all members in declaration order through the single naming pass.
    //    When we encounter the first member of a method group, expand all overloads
    //    of that group together. Skip subsequent members of the same group.
    for member in &config.members {
        match member {
            Member::Constructor(_) if !config.is_abstract => {
                if expanded_constructors {
                    continue;
                }
                expanded_constructors = true;

                let return_type = TypeRef::ident(config.rust_name.clone());
                let returns: Vec<&TypeRef> = constructor_overloads
                    .iter()
                    .map(|c| config.concrete_construct_return(c).unwrap_or(&return_type))
                    .collect();
                // Each constructor resolves in the body of the declaration
                // it is written in, or in its construct signature's own
                // scope when that declares `<T>` (`new <T>(…): Foo<T>`).
                let overloads: Vec<Overload> = constructor_overloads
                    .iter()
                    .zip(&returns)
                    .map(|(c, return_type)| Overload {
                        params: &c.params,
                        return_type,
                        body_scope: c.scope,
                        throws: &c.throws,
                        doc: &c.doc,
                        string_mode: None,
                    })
                    .collect();
                let sigs = build_signatures(
                    &CallableSpec {
                        js_name: &config.js_name,
                        rust_name: None,
                        kind: SignatureKind::Constructor,
                        overloads: &overloads,
                    },
                    &mut names,
                    config.cgctx,
                );
                for sig in &sigs {
                    items.push(generate_expanded_constructor(config, sig));
                }
            }
            Member::Method(m) => {
                if expanded_methods.contains(&m.js_name) {
                    continue;
                }
                expanded_methods.insert(m.js_name.clone());

                let group = &method_groups[&m.js_name];
                let overloads: Vec<Overload> = group
                    .iter()
                    .map(|m| Overload {
                        params: &m.params,
                        return_type: &m.return_type,
                        body_scope: m.body_scope,
                        throws: &m.throws,
                        doc: &m.doc,
                        string_mode: m.string_mode,
                    })
                    .collect();
                let base = config.member_base(MemberKind::Method, &m.js_name);
                unreserve_base(&mut names, &base);
                let sigs = build_signatures(
                    &CallableSpec {
                        js_name: &m.js_name,
                        rust_name: Some(&base),
                        kind: SignatureKind::Method,
                        overloads: &overloads,
                    },
                    &mut names,
                    config.cgctx,
                );
                for sig in &sigs {
                    items.push(generate_expanded_method(config, sig));
                }
            }
            Member::StaticMethod(m) => {
                if expanded_static_methods.contains(&m.js_name) {
                    continue;
                }
                expanded_static_methods.insert(m.js_name.clone());

                let group = &static_method_groups[&m.js_name];
                let overloads: Vec<Overload> = group
                    .iter()
                    .map(|m| Overload {
                        params: &m.params,
                        return_type: &m.return_type,
                        body_scope: m.body_scope,
                        throws: &m.throws,
                        doc: &m.doc,
                        string_mode: m.string_mode,
                    })
                    .collect();
                let base = config.member_base(MemberKind::StaticMethod, &m.js_name);
                unreserve_base(&mut names, &base);
                let sigs = build_signatures(
                    &CallableSpec {
                        js_name: &m.js_name,
                        rust_name: Some(&base),
                        kind: SignatureKind::StaticMethod,
                        overloads: &overloads,
                    },
                    &mut names,
                    config.cgctx,
                );
                for sig in &sigs {
                    items.push(generate_expanded_static_method(config, sig));
                }
            }
            Member::Getter(g) => {
                unreserve_base(
                    &mut names,
                    &config.member_base(MemberKind::Property, &g.js_name),
                );
                items.push(generate_getter(config, g, &mut names));
            }
            Member::Setter(s) => {
                names.unreserve(&config.setter_name(MemberKind::Property, &s.js_name));
                items.extend(generate_setter(config, s, &mut names));
            }
            Member::StaticGetter(g) => {
                let base = config.member_base(MemberKind::StaticProperty, &g.js_name);
                unreserve_base(&mut names, &base);
                items.push(generate_static_getter(config, g, &mut names));
            }
            Member::StaticSetter(s) => {
                names.unreserve(&config.setter_name(MemberKind::StaticProperty, &s.js_name));
                items.extend(generate_static_setter(config, s, &mut names));
            }
            Member::IndexSignature(_) | Member::Constructor(_) => {
                // IndexSignature: not yet supported in codegen
                // Constructor on abstract class: skip
            }
        }
    }
    let into_iterator = match &config.iterable {
        Some(item) if CodegenContext::generic_mono(config.cgctx) => {
            let (method, into_iterator) = generate_into_iterator(config, item, &mut names);
            items.push(method);
            into_iterator
        }
        _ => quote! {},
    };

    // Build the extern block with optional module attribute
    let wb_extern_attr = CodegenContext::extern_attr(config.cgctx, config.module.as_deref());

    // Re-export the type under its original (un-suffixed) name when collision
    // resolution renamed it. The suffixed name (`Foo_`) is purely an internal
    // disambiguator against the `use js_sys::*` glob inside the surrounding
    // module — it must never leak into the public Rust path consumers see.
    let public_alias = if config.effective_rust_name() != config.rust_name
        && !super::typemap::is_prelude_reserved(&config.rust_name)
    {
        let public = super::typemap::make_ident(&config.rust_name);
        let internal = super::typemap::make_ident(&config.effective_rust_name());
        quote! { pub use #internal as #public; }
    } else {
        quote! {}
    };

    quote! {
        #wb_extern_attr
        extern "C" {
            #(#items)*
        }
        #into_iterator
        #public_alias
    }
}

/// Per-mono, a declared `[Symbol.iterator]()` binds as `iterator()` and
/// makes `&Type` iterable, as js-sys's own collections are: a `[K, V]`
/// tuple item iterates as `(K, V)` through `js_sys::Entries`
/// (`for (name, value) in &headers`), any other as the item itself.
fn generate_into_iterator(
    config: &ClassConfig,
    item: &TypeRef,
    names: &mut LocalScope,
) -> (TokenStream, TokenStream) {
    let ident = super::typemap::make_ident(&names.declare("iterator", Suffix::Snake));
    let this_type = super::typemap::make_ident(&config.effective_rust_name());
    let args = config.type_generics_args();
    let generics = config.type_generics_decl();
    let from_module = config.from_module();
    let lower = |t: &TypeRef| {
        super::typemap::to_syn_type(
            t,
            TypePosition::RETURN.to_container(config.cgctx),
            config.cgctx,
            config.scope,
            &from_module,
        )
    };
    let (iter_ty, item_ty, into_iter, body, bound) = match item {
        TypeRef::Tuple(elems) if elems.len() == 2 => {
            let (k, v) = (lower(&elems[0]), lower(&elems[1]));
            (
                quote! { Iterator<ArrayTuple<(#k, #v)>> },
                quote! { (#k, #v) },
                quote! { ::js_sys::Entries<#k, #v> },
                quote! { ::js_sys::Entries::from(self.#ident()) },
                quote! { ::js_sys::Entries<#k, #v>: ::core::iter::Iterator<Item = (#k, #v)> },
            )
        }
        _ => {
            let t = lower(item);
            (
                quote! { Iterator<#t> },
                quote! { #t },
                quote! { <Iterator<#t> as ::core::iter::IntoIterator>::IntoIter },
                quote! { ::core::iter::IntoIterator::into_iter(self.#ident()) },
                quote! { Iterator<#t>: ::core::iter::IntoIterator<Item = #t> },
            )
        }
    };
    let where_clause = if config.type_params.is_empty() {
        quote! {}
    } else {
        quote! { where #bound }
    };
    let method = quote! {
        #[wasm_bindgen(method, js_name = "[Symbol.iterator]")]
        pub fn #ident #generics (this: &#this_type #args) -> #iter_ty;
    };
    let impl_generics = if config.type_params.is_empty() {
        quote! { <'a> }
    } else {
        let params = config
            .type_params
            .iter()
            .map(|tp| super::typemap::make_ident(&tp.name));
        quote! { <'a, #(#params),*> }
    };
    let into_iterator = quote! {
        impl #impl_generics ::core::iter::IntoIterator for &'a #this_type #args #where_clause {
            type Item = #item_ty;
            type IntoIter = #into_iter;
            fn into_iter(self) -> Self::IntoIter {
                #body
            }
        }
    };
    (method, into_iterator)
}

/// Generate the type declaration:
///
/// ```rust,ignore
/// #[wasm_bindgen(extends = ..., js_name = "FooBar")]
/// #[derive(Debug, Clone, PartialEq, Eq)]
/// pub type FooBar;
/// ```
fn generate_type_decl(config: &ClassConfig) -> TokenStream {
    // If the bare class name collided with a `js_sys` reserved name (e.g.
    // `Global`, `Function`), `CodegenContext::resolve_collisions` chose a
    // suffixed Rust name (`Global_`). The type decl needs to use that
    // renamed identifier so references through `to_syn_type` line up.
    // The original JS-side name still goes through `js_name = "..."` below.
    let rust_name = config
        .cgctx
        .and_then(|ctx| ctx.renamed_locals.get(&config.rust_name).cloned())
        .unwrap_or_else(|| config.rust_name.clone());
    let rust_ident = super::typemap::make_ident(&rust_name);
    let js_name = &config.js_name;

    // Build wasm_bindgen attribute parts
    let mut wb_parts: Vec<TokenStream> = Vec::new();

    let mut has_object = false;
    for extends in &config.extends {
        let extends_str = extends.to_string();
        // Skip extends that resolve to JsValue (implicit, causes conflicting impls)
        if extends_str == "JsValue" {
            continue;
        }
        if let Some(cgctx) = config.cgctx {
            let uses = cgctx.external_uses.borrow();
            if uses.get(&extends_str).is_some_and(|v| v == "JsValue") {
                continue;
            }
        }
        if extends_str == "Object" {
            has_object = true;
        }
        wb_parts.push(quote! { extends = #extends });
    }
    // Every type extends Object at minimum
    if !has_object {
        wb_parts.push(quote! { extends = Object });
    }

    // Emit `js_name` whenever the JS-side class name differs from the Rust
    // ident we'll use — that's true after a collision rename even if the
    // `config.rust_name` (pre-rename) matched `js_name`.
    if super::typemap::needs_js_name(&rust_ident, js_name) {
        wb_parts.push(quote! { js_name = #js_name });
    }

    // Namespace for types inside a JS namespace (e.g., WebAssembly.Module)
    wb_parts.extend(super::js_namespace_attr(&config.js_namespace));

    let wb_attr = if wb_parts.is_empty() {
        quote! {}
    } else {
        quote! { #[wasm_bindgen(#(#wb_parts),*)] }
    };

    let generics_decl = config.type_generics_decl_with_defaults();

    quote! {
        #wb_attr
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub type #rust_ident #generics_decl;
    }
}

/// Generic-parameter declaration for a method, covering both the
/// type-level parameters referenced in `this: &Foo<T, …>` and any
/// method-only parameters mentioned in arguments or the return type.
/// wasm-bindgen requires the redeclaration even for type-level
/// params (see `js_sys::Array::for_each<T: JsGeneric>` for the
/// canonical pattern).
///
/// Bounds are composed in a fixed order: type-level first
/// (declaration order), then method-local. ABV widening generics
/// (`<Tn: TypedArray>`) don't apply to methods — they're emitted
/// only at the dictionary-factory / setter sites that use
/// [`generate_dictionary_params`].
fn generic_bounds_for_method(config: &ClassConfig, sig: &FunctionSignature) -> Vec<TokenStream> {
    let mut bounds = config.type_param_bounds();
    let sig_tys: Vec<&TypeRef> = sig
        .params
        .iter()
        .map(|p| &p.type_ref)
        .chain(std::iter::once(&sig.return_type))
        .collect();
    bounds.extend(config.local_generic_bounds(sig.body_scope, &sig_tys));
    widen_async_return_param(bounds, config, sig)
}

/// An `async fn` resolving to a bare type parameter is converted from
/// the settled `JsValue` by wasm-bindgen's `JsFuture<T>`, which needs
/// `T: FromWasmAbi` on top of `JsGeneric`.
fn widen_async_return_param(
    bounds: Vec<TokenStream>,
    config: &ClassConfig,
    sig: &FunctionSignature,
) -> Vec<TokenStream> {
    if !sig.is_async || CodegenContext::generic_mono(config.cgctx) {
        return bounds;
    }
    let Some(cgctx) = config.cgctx else {
        return bounds;
    };
    let Some(name) = sig.return_type.as_ident() else {
        return bounds;
    };
    if !matches!(
        cgctx.gctx.scopes.resolve_binding(sig.body_scope, name),
        Some(crate::parse::scope::Binding::TypeParam)
    ) {
        return bounds;
    }
    let plain = CodegenContext::type_param_decl(config.cgctx, name).to_string();
    let ident = super::typemap::make_ident(name);
    bounds
        .into_iter()
        .map(|b| {
            if b.to_string() == plain {
                quote! { #ident: ::wasm_bindgen::JsGeneric + ::wasm_bindgen::convert::FromWasmAbi }
            } else {
                b
            }
        })
        .collect()
}

/// Generate a constructor binding from a resolved signature.
fn generate_expanded_constructor(config: &ClassConfig, sig: &FunctionSignature) -> TokenStream {
    let rust_ident = super::typemap::make_ident(&sig.rust_name);
    let scope = sig.body_scope;
    let params = generate_concrete_params_with_mono_bounds(
        &sig.params,
        config.cgctx,
        scope,
        &config.from_module(),
    );
    let doc = super::doc_tokens(&sig.doc);
    let has_variadic = sig.params.last().is_some_and(|p| p.variadic);

    // Type parameters the arguments mention — the class's own or the
    // construct signature's. A constructor-level `<T>` of the same
    // name as a class-level one (the `declare var` pattern) binds the
    // class parameter positionally by name.
    let param_tys: Vec<&TypeRef> = sig.params.iter().map(|p| &p.type_ref).collect();
    let mentioned = config.mentioned_type_param_names(scope, &param_tys);
    let return_type = if config.type_params.is_empty() || mentioned.is_empty() {
        sig.return_type.clone()
    } else {
        TypeRef::generic(
            config.rust_name.clone(),
            config
                .type_params
                .iter()
                .map(|tp| {
                    if mentioned.contains(&tp.name) {
                        TypeRef::ident(tp.name.clone())
                    } else {
                        config.type_param_default_ref(tp)
                    }
                })
                .collect(),
        )
    };
    // Constructors return the constructed type, wrapped in `Result` when
    // `catch` (the default; `@throws {never}` opts out).
    // The class is named from its own body, wherever the params resolve.
    let ret = to_return_type(
        &return_type,
        sig.catch,
        sig.is_async,
        sig.js_string_return,
        sig.error_type.as_ref(),
        config.cgctx,
        config.scope,
        &config.from_module(),
        super::typemap::ReturnAnchor {
            base: &sig.rust_name,
            kind: super::typemap::ReturnAnchorKind::Callable,
            parent: Some(&config.rust_name),
        },
    );

    let mut wb_parts = vec![quote! { constructor }];
    wb_parts.extend(config.js_class_parts());
    if has_variadic {
        wb_parts.push(quote! { variadic });
    }
    if sig.catch {
        wb_parts.push(quote! { catch });
    }
    // For non-"new" overloads, we need js_name so wasm_bindgen maps them
    // to the same JS constructor.
    if sig.rust_name != "new" {
        let js_name = &config.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    // A constructor only binds the type parameters its own arguments
    // mention; the rest are instantiated at their defaults in the return
    // type so `Foo::new()` needs no turbofish.
    let bounds: Vec<TokenStream> = mentioned
        .iter()
        .map(|n| CodegenContext::type_param_decl(config.cgctx, n))
        .collect();
    let generics = render_generic_bounds(&bounds);

    let literal = super::inline_wrappers::literal_attr(sig);
    let typed = typed_constructor(config, sig, &mentioned, &return_type, &params, &wb_parts);
    quote! {
        #doc
        #literal
        #[wasm_bindgen(#(#wb_parts),*)]
        pub fn #rust_ident #generics (#params) -> #ret;
        #typed
    }
}

/// Under per-monomorphization, a constructor that leaves class parameters
/// at their defaults (`TransformStream::new()` is `TransformStream<JsValue,
/// JsValue>`, so it needs no annotation) gets a `_typed` twin binding every
/// class parameter, for a typed stream from the same JS constructor:
/// `let t: TransformStream<Uint8Array, Uint8Array> = TransformStream::new_typed()?`.
fn typed_constructor(
    config: &ClassConfig,
    sig: &FunctionSignature,
    mentioned: &[String],
    return_type: &TypeRef,
    params: &TokenStream,
    wb_parts: &[TokenStream],
) -> TokenStream {
    let declared_concrete = mentioned.is_empty()
        && matches!(return_type, TypeRef::Reference { generic_args, .. } if !generic_args.is_empty());
    if !CodegenContext::generic_mono(config.cgctx)
        || config.type_params.is_empty()
        || sig.literal.is_some()
        || mentioned.len() >= config.type_params.len()
        || declared_concrete
    {
        return quote! {};
    }
    let typed_ident = super::typemap::make_ident(&format!("{}_typed", sig.rust_name));
    let typed_return = TypeRef::generic(
        config.rust_name.clone(),
        config
            .type_params
            .iter()
            .map(|tp| TypeRef::ident(tp.name.clone()))
            .collect(),
    );
    let ret = to_return_type(
        &typed_return,
        sig.catch,
        sig.is_async,
        false,
        sig.error_type.as_ref(),
        config.cgctx,
        config.scope,
        &config.from_module(),
        super::typemap::ReturnAnchor {
            base: &sig.rust_name,
            kind: super::typemap::ReturnAnchorKind::Callable,
            parent: Some(&config.rust_name),
        },
    );
    let mut wb_parts: Vec<TokenStream> = wb_parts
        .iter()
        .filter(|p| !p.to_string().starts_with("js_name"))
        .cloned()
        .collect();
    let js_name = &config.js_name;
    wb_parts.push(quote! { js_name = #js_name });
    let bounds: Vec<TokenStream> = config
        .type_params
        .iter()
        .map(|tp| CodegenContext::type_param_decl(config.cgctx, &tp.name))
        .collect();
    let generics = render_generic_bounds(&bounds);
    let doc = super::doc_tokens(&sig.doc);
    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub fn #typed_ident #generics (#params) -> #ret;
    }
}

/// Generate an instance method binding from an expanded signature.
fn generate_expanded_method(config: &ClassConfig, sig: &FunctionSignature) -> TokenStream {
    let rust_ident = super::typemap::make_ident(&sig.rust_name);
    let this_type = super::typemap::make_ident(&config.effective_rust_name());
    // Method-local type parameters live in `sig.body_scope`; resolve
    // names through that scope so `T` (etc.) lowers to a bare ident
    // rather than slipping through to `emit_type_name`.
    let method_scope = sig.body_scope;
    let params = generate_concrete_params_with_mono_bounds(
        &sig.params,
        config.cgctx,
        method_scope,
        &config.from_module(),
    );
    let doc = super::doc_tokens(&sig.doc);
    let has_variadic = sig.params.last().is_some_and(|p| p.variadic);

    let mut wb_parts: Vec<TokenStream> = vec![quote! { method }];
    if has_variadic {
        wb_parts.push(quote! { variadic });
    }
    if sig.catch {
        wb_parts.push(quote! { catch });
    }
    if super::typemap::needs_js_name(&rust_ident, &sig.js_name) {
        let js_name = &sig.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    let ret_ty = to_return_type(
        &sig.return_type,
        sig.catch,
        sig.is_async,
        sig.js_string_return,
        sig.error_type.as_ref(),
        config.cgctx,
        method_scope,
        &config.from_module(),
        super::typemap::ReturnAnchor {
            base: &sig.rust_name,
            kind: super::typemap::ReturnAnchorKind::Callable,
            parent: Some(&config.rust_name),
        },
    );
    let ret = if is_void_return(&sig.return_type) && !sig.catch {
        quote! {}
    } else {
        quote! { -> #ret_ty }
    };

    let async_kw = if sig.is_async {
        quote! { async }
    } else {
        quote! {}
    };

    let method_generics = render_generic_bounds(&generic_bounds_for_method(config, sig));
    let this_generics = config.type_generics_args();

    let literal = super::inline_wrappers::literal_attr(sig);
    quote! {
        #doc
        #literal
        #[wasm_bindgen(#(#wb_parts),*)]
        pub #async_kw fn #rust_ident #method_generics (this: &#this_type #this_generics, #params) #ret;
    }
}

/// Generate a static method binding from an expanded signature.
fn generate_expanded_static_method(config: &ClassConfig, sig: &FunctionSignature) -> TokenStream {
    let rust_ident = super::typemap::make_ident(&sig.rust_name);
    let class_ident = super::typemap::make_ident(&config.effective_rust_name());
    let scope = sig.body_scope;
    let params = generate_concrete_params_with_mono_bounds(
        &sig.params,
        config.cgctx,
        scope,
        &config.from_module(),
    );
    let doc = super::doc_tokens(&sig.doc);
    let has_variadic = sig.params.last().is_some_and(|p| p.variadic);

    let mut wb_parts: Vec<TokenStream> = vec![quote! { static_method_of = #class_ident }];
    wb_parts.extend(config.js_class_parts());
    if has_variadic {
        wb_parts.push(quote! { variadic });
    }
    if sig.catch {
        wb_parts.push(quote! { catch });
    }
    if super::typemap::needs_js_name(&rust_ident, &sig.js_name) {
        let js_name = &sig.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }

    let ret_ty = to_return_type(
        &sig.return_type,
        sig.catch,
        sig.is_async,
        sig.js_string_return,
        sig.error_type.as_ref(),
        config.cgctx,
        scope,
        &config.from_module(),
        super::typemap::ReturnAnchor {
            base: &sig.rust_name,
            kind: super::typemap::ReturnAnchorKind::Callable,
            parent: Some(&config.rust_name),
        },
    );
    let ret = if is_void_return(&sig.return_type) && !sig.catch {
        quote! {}
    } else {
        quote! { -> #ret_ty }
    };

    let async_kw = if sig.is_async {
        quote! { async }
    } else {
        quote! {}
    };

    // Static methods have no `this`, so type-level parameters are only
    // declared when the signature mentions them.
    let sig_tys: Vec<&TypeRef> = sig
        .params
        .iter()
        .map(|p| &p.type_ref)
        .chain(std::iter::once(&sig.return_type))
        .collect();
    let mut bounds: Vec<TokenStream> = config
        .mentioned_type_params(scope, &sig_tys)
        .iter()
        .map(|n| CodegenContext::type_param_decl(config.cgctx, n))
        .collect();
    bounds.extend(config.local_generic_bounds(scope, &sig_tys));
    let generics = render_generic_bounds(&bounds);

    let literal = super::inline_wrappers::literal_attr(sig);
    quote! {
        #doc
        #literal
        #[wasm_bindgen(#(#wb_parts),*)]
        pub #async_kw fn #rust_ident #generics (#params) #ret;
    }
}

/// Generate an instance getter binding.
fn generate_getter(
    config: &ClassConfig,
    getter: &GetterMember,
    names: &mut LocalScope,
) -> TokenStream {
    let this_type = super::typemap::make_ident(&config.effective_rust_name());
    let doc = super::doc_tokens(&getter.doc);

    let candidate = public_rust_name(&config.member_base(MemberKind::Property, &getter.js_name));
    let rust_name = names.declare(&candidate, Suffix::Snake);
    let rust_ident = super::typemap::make_ident(&rust_name);

    // Optional `?:` properties already read as `T | undefined` in the IR.
    let lowered_ty = getter.type_ref.clone();
    // Top-level erasing-union return synthesises a dynamic-union
    // enum (see [`ReturnAnchor`]). For getters the anchor is the
    // property name itself — no `Return` infix.
    let anchor = super::typemap::ReturnAnchor {
        base: &getter.js_name,
        kind: super::typemap::ReturnAnchorKind::Getter,
        parent: Some(&config.rust_name),
    };
    let getter_type = getter_string_ref_type(config, &lowered_ty, getter.scope, getter.string_mode)
        .unwrap_or_else(|| {
            super::typemap::to_getter_return_type(
                &lowered_ty,
                config.cgctx,
                getter.scope,
                &config.from_module(),
                anchor,
            )
        });

    let mut wb_parts: Vec<TokenStream> = vec![quote! { method }, quote! { getter }];
    if super::typemap::needs_js_name(&rust_ident, &getter.js_name) {
        let js_name = &getter.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }
    let error_ty = getter_error_type(config, &getter.throws);
    if error_ty.is_some() {
        wb_parts.push(quote! { catch });
    }
    let getter_type = wrap_getter_result(getter_type, error_ty.as_ref());

    // Thread the type's generic parameters through the binding so a
    // generic dictionary's getter (e.g. `value(): T` on
    // `Foo<T>`) compiles. The getter's own return type may also
    // mention bare type params — fold those in alongside type-level
    // ones via `local_generic_bounds`.
    let mut bounds = config.type_param_bounds();
    bounds.extend(config.local_generic_bounds(getter.scope, &[&getter.type_ref]));
    let fn_generics = render_generic_bounds(&bounds);
    let this_generics = config.type_generics_args();

    let source_name = &getter.js_name;
    let catch = error_ty.is_some().then(|| quote! { , catch });
    let js_string_variant = getter_js_string_variant(
        config,
        &lowered_ty,
        getter.scope,
        getter.string_mode,
        &rust_name,
        names,
    )
    .map(|(js_ident, js_type)| {
        let js_type = wrap_getter_result(js_type, error_ty.as_ref());
        quote! {
            #doc
            #[wasm_bindgen(method, getter, js_name = #source_name #catch)]
            pub fn #js_ident #fn_generics (this: &#this_type #this_generics) -> #js_type;
        }
    });

    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub fn #rust_ident #fn_generics (this: &#this_type #this_generics) -> #getter_type;
        #js_string_variant
    }
}

/// A getter annotated `@throws {X}` is the one accessor form that
/// catches; the `Result` error is `X` resolved like any `@throws` type.
fn getter_error_type(config: &ClassConfig, throws: &crate::ir::Throws) -> Option<TokenStream> {
    throws.as_type().map(|ty| {
        super::typemap::error_type_tokens(
            Some(ty),
            config.cgctx,
            config.scope,
            &config.from_module(),
        )
    })
}

fn wrap_getter_result(ty: TokenStream, error_ty: Option<&TokenStream>) -> TokenStream {
    match error_ty {
        Some(err) => quote! { Result<#ty, #err> },
        None => ty,
    }
}

/// A `{@string ref}` getter's type: its `_js_string` form, under the base
/// name.
fn getter_string_ref_type(
    config: &ClassConfig,
    ty: &TypeRef,
    scope: ScopeId,
    mode: Option<crate::ir::StringModeHint>,
) -> Option<TokenStream> {
    if mode != Some(crate::ir::StringModeHint::Ref) {
        return None;
    }
    let ctx = config
        .cgctx
        .filter(|ctx| super::signatures::has_js_string_return(ty, ctx, scope))?;
    Some(super::typemap::to_mono_js_string_return_type(
        ty,
        ctx,
        scope,
        &config.from_module(),
    ))
}

/// Ident and return type for a getter's additive `_js_string` variant, when
/// per-monomorphization mode is on and the getter type has a string leaf.
/// Instance and static getters differ only in attribute and receiver, which
/// callers supply.
fn getter_js_string_variant(
    config: &ClassConfig,
    ty: &TypeRef,
    scope: ScopeId,
    mode: Option<crate::ir::StringModeHint>,
    rust_name: &str,
    names: &mut LocalScope,
) -> Option<(syn::Ident, TokenStream)> {
    use crate::ir::StringModeHint;
    if matches!(mode, Some(StringModeHint::Copy | StringModeHint::Ref)) {
        return None;
    }
    let ctx = config
        .cgctx
        .filter(|ctx| super::signatures::has_js_string_return(ty, ctx, scope))?;
    let js_name = names.declare(
        &super::signatures::js_string_twin_name(rust_name),
        Suffix::Snake,
    );
    let js_type =
        super::typemap::to_mono_js_string_return_type(ty, ctx, scope, &config.from_module());
    Some((super::typemap::make_ident(&js_name), js_type))
}

/// Generate instance setter bindings, expanding union types into separate overloads.
fn generate_setter(
    config: &ClassConfig,
    setter: &SetterMember,
    names: &mut LocalScope,
) -> Vec<TokenStream> {
    let this_type = super::typemap::make_ident(&config.effective_rust_name());
    let doc = setter.doc.clone();

    // Treat the setter as a single-param method and expand through signatures
    let param = config.setter_param(&setter.js_name, &setter.type_ref, setter.slice_mode);

    let void = crate::ir::TypeRef::Void;
    let overloads = [Overload {
        params: &[param],
        return_type: &void,
        body_scope: setter.scope,
        throws: &crate::ir::Throws::None,
        doc: &doc,
        string_mode: None,
    }];
    let setter_name = config.setter_name(MemberKind::Property, &setter.js_name);
    let sigs = build_signatures(
        &CallableSpec {
            js_name: &setter.js_name,
            rust_name: Some(&setter_name),
            kind: SignatureKind::Setter,
            overloads: &overloads,
        },
        names,
        config.cgctx,
    );

    let this_generics = config.type_generics_args();
    sigs.iter()
        .map(|sig| {
            let rust_ident = super::typemap::make_ident(&sig.rust_name);
            let DictionaryParams {
                bounds: abv_bounds,
                extern_params,
                ..
            } = generate_dictionary_params(
                &sig.params,
                config.cgctx,
                sig.body_scope,
                &config.from_module(),
            );

            // Type-level first, then method-local (collected from
            // setter param types), then ABV widening bounds. The
            // first two share a JsGeneric bound; the third uses
            // `js_sys::TypedArray`. Composing them in this order
            // keeps method/getter/setter generics aligned.
            let mut bounds = config.type_param_bounds();
            let sig_tys: Vec<&TypeRef> = sig.params.iter().map(|p| &p.type_ref).collect();
            bounds.extend(config.local_generic_bounds(sig.body_scope, &sig_tys));
            bounds.extend(abv_bounds);
            let fn_generics = render_generic_bounds(&bounds);

            let mut wb_parts: Vec<TokenStream> = vec![quote! { method }, quote! { setter }];
            if super::typemap::needs_js_name(&rust_ident, &format!("set_{}", setter.js_name)) {
                let js_name = &setter.js_name;
                wb_parts.push(quote! { js_name = #js_name });
            }

            let doc = super::doc_tokens(&sig.doc);
            quote! {
                #doc
                #[wasm_bindgen(#(#wb_parts),*)]
                pub fn #rust_ident #fn_generics (this: &#this_type #this_generics, #extern_params);
            }
        })
        .collect()
}

/// Generate a static getter binding.
fn generate_static_getter(
    config: &ClassConfig,
    getter: &StaticGetterMember,
    names: &mut LocalScope,
) -> TokenStream {
    let class_ident = super::typemap::make_ident(&config.effective_rust_name());
    let doc = super::doc_tokens(&getter.doc);

    let candidate =
        public_rust_name(&config.member_base(MemberKind::StaticProperty, &getter.js_name));
    let rust_name = names.declare(&candidate, Suffix::Snake);
    let rust_ident = super::typemap::make_ident(&rust_name);

    let getter_type =
        getter_string_ref_type(config, &getter.type_ref, getter.scope, getter.string_mode)
            .unwrap_or_else(|| {
                super::typemap::to_getter_return_type(
                    &getter.type_ref,
                    config.cgctx,
                    getter.scope,
                    &config.from_module(),
                    super::typemap::ReturnAnchor {
                        base: &getter.js_name,
                        kind: super::typemap::ReturnAnchorKind::Getter,
                        parent: Some(&config.rust_name),
                    },
                )
            });

    let mut wb_parts: Vec<TokenStream> = vec![quote! { static_method_of = #class_ident }];
    wb_parts.extend(config.js_class_parts());
    wb_parts.push(quote! { getter });
    if super::typemap::needs_js_name(&rust_ident, &getter.js_name) {
        let js_name = &getter.js_name;
        wb_parts.push(quote! { js_name = #js_name });
    }
    let error_ty = getter_error_type(config, &getter.throws);
    if error_ty.is_some() {
        wb_parts.push(quote! { catch });
    }
    let getter_type = wrap_getter_result(getter_type, error_ty.as_ref());

    // Static getters take no receiver, so no `this` generic args
    // need plumbing — but the bounds still need redeclaring on the
    // `fn` for any type params mentioned by the return type.
    let mut bounds = config.type_param_bounds();
    bounds.extend(config.local_generic_bounds(getter.scope, &[&getter.type_ref]));
    let fn_generics = render_generic_bounds(&bounds);

    let source_name = &getter.js_name;
    let js_class = config.js_class_parts();
    let catch = error_ty.is_some().then(|| quote! { , catch });
    let js_string_variant = getter_js_string_variant(
        config,
        &getter.type_ref,
        getter.scope,
        getter.string_mode,
        &rust_name,
        names,
    )
    .map(|(js_ident, js_type)| {
        let js_type = wrap_getter_result(js_type, error_ty.as_ref());
        quote! {
            #doc
            #[wasm_bindgen(static_method_of = #class_ident, #(#js_class,)* getter, js_name = #source_name #catch)]
            pub fn #js_ident #fn_generics () -> #js_type;
        }
    });

    quote! {
        #doc
        #[wasm_bindgen(#(#wb_parts),*)]
        pub fn #rust_ident #fn_generics () -> #getter_type;
        #js_string_variant
    }
}

/// Generate static setter bindings, expanding union types into separate overloads.
fn generate_static_setter(
    config: &ClassConfig,
    setter: &StaticSetterMember,
    names: &mut LocalScope,
) -> Vec<TokenStream> {
    let class_ident = super::typemap::make_ident(&config.effective_rust_name());
    let doc = setter.doc.clone();

    let param = config.setter_param(&setter.js_name, &setter.type_ref, None);

    let void = crate::ir::TypeRef::Void;
    let overloads = [Overload {
        params: &[param],
        return_type: &void,
        body_scope: setter.scope,
        throws: &crate::ir::Throws::None,
        doc: &doc,
        string_mode: None,
    }];
    let setter_name = config.setter_name(MemberKind::StaticProperty, &setter.js_name);
    let sigs = build_signatures(
        &CallableSpec {
            js_name: &setter.js_name,
            rust_name: Some(&setter_name),
            kind: SignatureKind::StaticSetter,
            overloads: &overloads,
        },
        names,
        config.cgctx,
    );

    sigs.iter()
        .map(|sig| {
            let rust_ident = super::typemap::make_ident(&sig.rust_name);
            let DictionaryParams {
                bounds: abv_bounds,
                extern_params,
                ..
            } = generate_dictionary_params(
                &sig.params,
                config.cgctx,
                sig.body_scope,
                &config.from_module(),
            );

            // Static setters don't take a `this` receiver, so the
            // type's generic args don't propagate into the parameter
            // list, but the bounds must still be redeclared on the
            // `fn` when the setter param mentions them. ABV bounds
            // last (see [`generate_setter`]).
            let mut bounds = config.type_param_bounds();
            let sig_tys: Vec<&TypeRef> = sig.params.iter().map(|p| &p.type_ref).collect();
            bounds.extend(config.local_generic_bounds(sig.body_scope, &sig_tys));
            bounds.extend(abv_bounds);
            let fn_generics = render_generic_bounds(&bounds);

            let mut wb_parts: Vec<TokenStream> = vec![quote! { static_method_of = #class_ident }];
            wb_parts.extend(config.js_class_parts());
            wb_parts.push(quote! { setter });
            if super::typemap::needs_js_name(&rust_ident, &format!("set_{}", setter.js_name)) {
                let js_name = &setter.js_name;
                wb_parts.push(quote! { js_name = #js_name });
            }

            let doc = super::doc_tokens(&sig.doc);
            quote! {
                #doc
                #[wasm_bindgen(#(#wb_parts),*)]
                pub fn #rust_ident #fn_generics (#extern_params);
            }
        })
        .collect()
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Convert concrete params to `fn` parameter token stream.
/// Classification over the interface's own and inherited members: an
/// interface declaring nothing of its own but extending dictionaries
/// (`type EmailMessageBuilder = EmailReplyMessageBuilder & EmailDestinations`)
/// is a dictionary; one extending a class-like parent is class-like.
pub(crate) fn effective_classification(
    decl: &InterfaceDecl,
    cgctx: Option<&CodegenContext<'_>>,
) -> InterfaceClassification {
    if decl.extends.is_empty() {
        return decl.classification.clone();
    }
    let inherited = inherited_members(&decl.extends, cgctx);
    let members = inherit_into(inherited.first_chain, &inherited.beyond_first);
    let members = inherit_into(members, &decl.members);
    crate::parse::classify::classify_interface(&members)
}

/// Members a type inherits through `extends`, with the parents' type
/// parameters substituted by the arguments the `extends` clause passes.
struct Inherited {
    /// From the first parent and its ancestors. wasm-bindgen derefs to
    /// the first `extends =` target, so these are reachable without
    /// re-declaring them.
    first_chain: Vec<Member>,
    /// From every other parent (and their ancestors). Only `AsRef`
    /// reaches those, so the child declares them itself.
    beyond_first: Vec<Member>,
}

fn inherited_members(
    extends: &[crate::ir::Heritage],
    cgctx: Option<&CodegenContext<'_>>,
) -> Inherited {
    let mut out = Inherited {
        first_chain: Vec::new(),
        beyond_first: Vec::new(),
    };
    let Some(c) = cgctx else {
        return out;
    };
    for (i, parent) in extends.iter().enumerate() {
        let mut members = Vec::new();
        let mut visited = HashSet::new();
        collect_parent_members(
            &parent.type_ref,
            c,
            parent.scope,
            &mut members,
            &mut visited,
        );
        if i == 0 {
            out.first_chain = members;
        } else {
            extend_missing(&mut out.beyond_first, members);
        }
    }
    out
}

/// Append the members of `later` whose key no earlier sibling parent
/// already provides: siblings in `extends` order, first wins.
fn extend_missing(members: &mut Vec<Member>, later: Vec<Member>) {
    let present: HashSet<_> = members.iter().map(crate::parse::member_key).collect();
    members.extend(
        later
            .into_iter()
            .filter(|m| !present.contains(&crate::parse::member_key(m))),
    );
}

/// `inherited` followed by `own`, dropping inherited members that `own`
/// redeclares. Overloads share a key, so `own` is appended whole rather
/// than merged entry by entry.
fn inherit_into(inherited: Vec<Member>, own: &[Member]) -> Vec<Member> {
    let own_keys: HashSet<_> = own.iter().map(crate::parse::member_key).collect();
    let mut members: Vec<Member> = inherited
        .into_iter()
        .filter(|m| !own_keys.contains(&crate::parse::member_key(m)))
        .collect();
    members.extend(own.iter().cloned());
    members
}

fn collect_parent_members(
    parent: &TypeRef,
    c: &CodegenContext<'_>,
    scope: ScopeId,
    out: &mut Vec<Member>,
    visited: &mut HashSet<crate::context::TypeId>,
) {
    let TypeRef::Reference {
        segments,
        generic_args,
    } = parent
    else {
        return;
    };
    let Some(id) = c.gctx.resolve_path(scope, &segments.join(".")) else {
        return;
    };
    if !visited.insert(id) {
        return;
    }
    let (type_params, parent_extends, members) = match &c.gctx.get_type(id).kind {
        TypeKind::Interface(i) => (&i.type_params, i.extends.clone(), &i.members),
        TypeKind::Class(k) => (
            &k.type_params,
            k.extends.iter().cloned().collect(),
            &k.members,
        ),
        _ => return,
    };
    let args = c.live_generic_args(id, generic_args);
    let map: HashMap<String, TypeRef> = type_params
        .iter()
        .enumerate()
        .map(|(i, tp)| {
            let arg = args
                .get(i)
                .cloned()
                .or_else(|| tp.default.clone())
                .unwrap_or(TypeRef::Any);
            (tp.name.clone(), arg)
        })
        .collect();
    // Ancestors first so a parent's redeclaration overrides theirs.
    let mut ancestors = Vec::new();
    for grand in &parent_extends {
        collect_parent_members(
            &grand.type_ref.substitute(&map),
            c,
            grand.scope,
            &mut ancestors,
            visited,
        );
    }
    let own: Vec<Member> = members
        .iter()
        .map(|m| {
            let m = m.substitute(&map);
            // A method without its own `<T>` resolves everything in the
            // child's scope once the parent's parameters are substituted.
            match m {
                Member::Method(mut mm) if mm.type_params.is_empty() => {
                    mm.body_scope = scope;
                    Member::Method(mm)
                }
                other => other,
            }
        })
        .collect();
    let members = inherit_into(ancestors, &own);
    *out = inherit_into(std::mem::take(out), &members);
}

/// Lower the `extends` parents of a declaration. A parent that erases to
/// `JsValue` is dropped rather than replaced: `JsValue` is implicit, and
/// a stand-in `Object` would take the first slot, which wasm-bindgen
/// makes the `Deref` target. `Object` closes the list when nothing else
/// does.
fn extends_list(
    parents: &[crate::ir::Heritage],
    cgctx: Option<&CodegenContext<'_>>,
    from_module: &ModuleContext,
) -> Vec<TokenStream> {
    let mut out: Vec<TokenStream> = Vec::new();
    for crate::ir::Heritage {
        type_ref: parent,
        scope,
    } in parents
    {
        let TypeRef::Reference { segments, .. } = parent else {
            if let Some(ctx) = cgctx {
                ctx.warn(format!(
                    "unsupported extends type `{}`, ignored",
                    parent.format_ts()
                ));
            }
            continue;
        };
        // The attribute takes a bare path, so a generic parent is named
        // without its arguments.
        let tokens = super::typemap::to_syn_type(
            &TypeRef::Reference {
                segments: segments.clone(),
                generic_args: Vec::new(),
            },
            TypePosition::ARGUMENT.to_inner(),
            cgctx,
            *scope,
            from_module,
        );
        // Two declarations' clauses can name one parent.
        let rendered = tokens.to_string();
        if rendered == "JsValue" || out.iter().any(|t| t.to_string() == rendered) {
            continue;
        }
        out.push(tokens);
    }
    if out.is_empty() {
        out.push(quote! { Object });
    }
    out
}
