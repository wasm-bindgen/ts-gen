//! Rust bindings from the ts-gen IR (`IR.md`).
//!
//! The IR the producer lowers is the generator's input. Declaration kinds
//! move over one at a time: a kind handled here is emitted from its IR
//! declaration, and every other kind still reads the parse.

use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::quote;
use ts_gen_abi::enums::{self, EnumRef, Shape};
use ts_gen_abi::ir as abi;
use ts_gen_abi::ir::JsEntries;
use ts_gen_abi::resolve::{Context, Location, Symbols, TypeDecl};

use super::{ir_callables, ir_types};
use crate::context::TypeId;

/// The lowered IR, and where each parsed declaration is in it.
pub struct IrDecls {
    ir: abi::Ir,
    locations: HashMap<TypeId, Location>,
    ids: HashMap<Location, TypeId>,
    /// Whether the bindings are per monomorphization.
    mono: bool,
    pub families: super::families::Families,
}

impl IrDecls {
    /// The IR's declarations, then std's, which it resolves against.
    fn symbols(&self) -> Symbols<'_> {
        Symbols::with_libs(&self.ir, &[super::std_ir()])
    }

    pub fn new(lowered: crate::lower::Lowered) -> Self {
        let ids = lowered
            .locations
            .iter()
            .map(|(id, loc)| (loc.clone(), *id))
            .collect();
        IrDecls {
            ir: lowered.ir,
            locations: lowered.locations,
            ids,
            mono: false,
            families: Default::default(),
        }
    }

    /// Read associated types per monomorphization, through the traits of
    /// their families.
    pub fn monomorphize(&mut self) {
        self.mono = true;
        self.families = super::families::Families::new(&self.ir);
    }

    /// The Rust type of the IR enum `t`, named at `at`: the parse's, or
    /// one of string literals the IR synthesised, under the IR's name.
    pub fn enum_type(
        &self,
        cgctx: &super::typemap::CodegenContext<'_>,
        t: &abi::Type,
        at: &Location,
        scope: crate::parse::scope::ScopeId,
        from_module: &crate::ir::ModuleContext,
    ) -> Option<crate::ir::TypeRef> {
        let abi::Type::Named(r) = t else {
            return None;
        };
        let segments = super::ir_types::ts_segments(&r.path);
        let name = segments.last()?;
        if cgctx
            .gctx
            .resolve_path(scope, &segments.join("."))
            .is_some()
        {
            return Some(crate::ir::TypeRef::Reference {
                segments: segments.clone(),
                generic_args: Vec::new(),
            });
        }
        let symbols = self.symbols();
        let cx = ts_gen_abi::resolve::Context {
            module: at.module.as_deref(),
            namespace: at.path[..at.path.len().saturating_sub(1)]
                .iter()
                .map(String::as_str)
                .collect(),
            ..Default::default()
        };
        let (loc, TypeDecl::Enum(decl)) = symbols.resolve_decl(&r.path, &cx)? else {
            return None;
        };
        let flat = enums::flatten(
            &symbols,
            EnumRef {
                loc: Some(loc),
                decl,
            },
        )
        .ok()?;
        if flat.shape != Shape::Unit {
            return None;
        }
        let literals: Vec<crate::ir::TypeRef> = flat
            .arms
            .iter()
            .map(|a| literal(a.arm).map(|l| crate::ir::TypeRef::StringLiteral(l.into())))
            .collect::<Option<_>>()?;
        let name = cgctx.synthesise_literal_enum(name, &literals, scope, from_module)?;
        Some(crate::ir::TypeRef::ident(name))
    }

    /// Each error class the throws enums of the IR's members match, first
    /// seen first: an IR class as its parse declaration, any other (a
    /// builtin, an `--external`) by name. An arm with no class, `err`'s
    /// fallback, matches `Error`.
    pub fn error_classes(&self) -> Vec<Result<TypeId, String>> {
        let symbols = self.symbols();
        let mut enums = Vec::new();
        throws_enums(&self.ir.items, None, &mut Vec::new(), &mut enums);
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for (path, module, namespace) in enums {
            let cx = Context {
                module,
                namespace,
                ..Default::default()
            };
            let Some(e) = enums::resolve(&symbols, &path, &cx) else {
                continue;
            };
            if seen.contains(&(e.loc.clone(), e.decl.name.clone())) {
                continue;
            }
            seen.push((e.loc.clone(), e.decl.name.clone()));
            let Ok(flat) = enums::flatten(&symbols, e) else {
                continue;
            };
            for arm in &flat.arms {
                let class = arm.arm.meta.attrs.js.iter().find_map(|j| match j {
                    abi::JsEntry::Class(preds) => preds.first(),
                    _ => None,
                });
                let class = match class {
                    Some(pred) => {
                        let owner = arm.owner.loc.as_ref();
                        let cx = Context {
                            module: owner.and_then(|l| l.module.as_deref()),
                            namespace: owner
                                .map(|l| {
                                    l.path[..l.path.len() - 1]
                                        .iter()
                                        .map(String::as_str)
                                        .collect()
                                })
                                .unwrap_or_default(),
                            ..Default::default()
                        };
                        match symbols.resolve_decl(&pred.class, &cx) {
                            Some((loc, _)) if self.ids.contains_key(&loc) => Ok(self.ids[&loc]),
                            _ => Err(pred.class.segments.last().cloned().unwrap_or_default()),
                        }
                    }
                    None if arm.is_fallback() => Err("Error".to_string()),
                    None => continue,
                };
                if !out.contains(&class) {
                    out.push(class);
                }
            }
        }
        out
    }

    /// Where the parsed declaration `id` is in the IR.
    pub fn location(&self, id: TypeId) -> Option<&Location> {
        self.locations.get(&id)
    }

    /// The name of `id`'s constructor through the JS value at `value`
    /// (`@js(new = Wrap2) new wrap2(...)`), `None` when unnamed, if it
    /// has one.
    pub fn value_constructor(&self, id: TypeId, value: &str) -> Option<Option<&str>> {
        let loc = self.locations.get(&id)?;
        let (name, namespace) = loc.path.split_last()?;
        let class = self
            .container(loc.module.as_deref(), namespace)?
            .iter()
            .find_map(|d| match d {
                abi::Decl::Class(c) if &c.name == name => Some(c),
                _ => None,
            })?;
        class.members.iter().find_map(|m| match &m.kind {
            abi::MemberKind::New(n)
                if m.meta
                    .attrs
                    .js
                    .new_value()
                    .is_some_and(|p| p.segments.join(".") == value) =>
            {
                Some(n.as_deref())
            }
            _ => None,
        })
    }

    /// The items of the container at `module` and `namespace`.
    fn container(&self, module: Option<&str>, namespace: &[String]) -> Option<&[abi::Decl]> {
        let mut items: &[abi::Decl] = match module {
            None => &self.ir.items,
            Some(spec) => self.ir.items.iter().find_map(|d| match d {
                abi::Decl::Module(m) if m.spec == spec => Some(m.items.as_slice()),
                _ => None,
            })?,
        };
        for name in namespace {
            items = items.iter().find_map(|d| match d {
                abi::Decl::Namespace(n) if &n.name == name => Some(n.items.as_slice()),
                _ => None,
            })?;
        }
        Some(items)
    }

    /// Whether the declaration at `loc` is `public` in JS: named so, by its
    /// `@js(name)`, or exported as it.
    pub fn exports_as(&self, loc: &Location, public: &str) -> bool {
        let Some((local, namespace)) = loc.path.split_last() else {
            return false;
        };
        let module = loc.module.as_deref();
        let js = self.container(module, namespace).is_some_and(|items| {
            items.iter().any(|d| match d {
                abi::Decl::Class(c) if &c.name == local => c
                    .meta
                    .attrs
                    .js
                    .contains(&abi::JsEntry::Name(public.to_string())),
                _ => false,
            })
        });
        local == public || js || self.local(module, public) == local
    }

    /// The declaration `module` exports as `public`.
    fn local<'a>(&'a self, module: Option<&str>, public: &'a str) -> &'a str {
        let exports = self.ir.items.iter().find_map(|d| match d {
            abi::Decl::Module(m) if Some(m.spec.as_str()) == module => m.exports.as_ref(),
            _ => None,
        });
        exports
            .into_iter()
            .flatten()
            .find(|e| e.alias.as_deref().unwrap_or(&e.name) == public)
            .map_or(public, |e| &e.name)
    }

    /// `namespace` in the container `module`, borrowed from the IR.
    fn namespace(&self, module: Option<&str>, namespace: &[String]) -> Option<Vec<&str>> {
        let mut items = self.container(module, &[])?;
        let mut out = Vec::with_capacity(namespace.len());
        for name in namespace {
            let n = items.iter().find_map(|d| match d {
                abi::Decl::Namespace(n) if &n.name == name => Some(n),
                _ => None,
            })?;
            out.push(n.name.as_str());
            items = &n.items;
        }
        Some(out)
    }

    /// The overloads of the callable `js_name` at `site`, stated in the
    /// signature model. `None` when any can't be, or the IR declares none.
    pub fn callable(
        &self,
        gctx: &crate::context::GlobalContext,
        site: &ir_callables::Site<'_>,
        js_name: &str,
        scope: crate::parse::scope::ScopeId,
        bound: &[crate::parse::scope::ScopeId],
    ) -> Option<Vec<ir_types::Overload>> {
        self.members(gctx, site, js_name, scope, bound, |cx, members| {
            let overloads: Vec<_> = null_arms(members)
                .iter()
                .map(|m| ir_types::overload(cx, m))
                .collect::<Option<_>>()?;
            (!overloads.is_empty()).then_some(overloads)
        })
    }

    /// The type of the accessor at `site`: what a getter reads, or what a
    /// setter writes. `None` when the setter writes more than one type.
    pub fn accessor(
        &self,
        gctx: &crate::context::GlobalContext,
        site: &ir_callables::Site<'_>,
        js_name: &str,
        scope: crate::parse::scope::ScopeId,
    ) -> Option<crate::ir::TypeRef> {
        match &self.accessors(gctx, site, js_name, scope)?.0[..] {
            [t] => Some(t.clone()),
            _ => None,
        }
    }

    /// What the getter `js_name` at `site` throws.
    pub fn getter_throws(
        &self,
        gctx: &crate::context::GlobalContext,
        site: &ir_callables::Site<'_>,
        js_name: &str,
        scope: crate::parse::scope::ScopeId,
    ) -> Option<crate::ir::Throws> {
        self.members(
            gctx,
            site,
            js_name,
            scope,
            &[],
            |cx, members| match &members[..] {
                [m] => Some(ir_types::getter_throws(cx, m)),
                _ => None,
            },
        )
    }

    /// What each accessor `js_name` at `site` reads or writes: one per
    /// setter of a union the IR writes arm by arm. `None` when any can't be
    /// stated. With it, whether it's a parent's the child doesn't pass its
    /// parameters to, which the child reads as the parent declares it.
    pub fn accessors(
        &self,
        gctx: &crate::context::GlobalContext,
        site: &ir_callables::Site<'_>,
        js_name: &str,
        scope: crate::parse::scope::ScopeId,
    ) -> Option<(Vec<crate::ir::TypeRef>, bool)> {
        self.members(gctx, site, js_name, scope, &[], |cx, members| {
            let ts = null_arms(members)
                .iter()
                .map(|m| ir_types::accessor(cx, m))
                .collect::<Option<Vec<_>>>()
                .filter(|ts| !ts.is_empty())?;
            Some((ts, !cx.erased.is_empty()))
        })
    }

    /// The members at `site` that bind `js_name` in its form.
    fn members<R>(
        &self,
        gctx: &crate::context::GlobalContext,
        site: &ir_callables::Site<'_>,
        js_name: &str,
        scope: crate::parse::scope::ScopeId,
        bound: &[crate::parse::scope::ScopeId],
        f: impl Fn(&ir_types::Cx<'_>, Vec<&abi::Member>) -> Option<R>,
    ) -> Option<R> {
        // A module's export list binds its declarations to their public
        // names, which the site names them by, unless the module declares
        // one of that name too (`namespace M` beside `export { N as M }`).
        let mapped = std::iter::once(js_name)
            .chain(site.path.first().map(String::as_str))
            .any(|n| self.local(site.module, n) != n);
        self.members_at(gctx, site, js_name, scope, bound, true, &f)
            .or_else(|| {
                mapped
                    .then(|| self.members_at(gctx, site, js_name, scope, bound, false, &f))
                    .flatten()
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn members_at<R>(
        &self,
        gctx: &crate::context::GlobalContext,
        site: &ir_callables::Site<'_>,
        js_name: &str,
        scope: crate::parse::scope::ScopeId,
        bound: &[crate::parse::scope::ScopeId],
        public: bool,
        f: &impl Fn(&ir_types::Cx<'_>, Vec<&abi::Member>) -> Option<R>,
    ) -> Option<R> {
        use ir_callables::Form;
        let local = |name: &str| match public {
            true => self.local(site.module, name).to_string(),
            false => name.to_string(),
        };
        let path: Vec<String> = match site.path.split_first() {
            Some((first, rest)) => std::iter::once(local(first))
                .chain(rest.iter().cloned())
                .collect(),
            None => Vec::new(),
        };
        let js_name = match site.form {
            Form::Function | Form::Variable if path.is_empty() => local(js_name),
            _ => js_name.to_string(),
        };
        let js_name = js_name.as_str();
        let site = &ir_callables::Site {
            path: &path,
            ..*site
        };
        let form = |m: &abi::Member| match &m.kind {
            abi::MemberKind::Method(_) if m.is_static => Form::Static,
            abi::MemberKind::Method(_) => Form::Method,
            abi::MemberKind::Get(_) if m.is_static => Form::StaticGet,
            abi::MemberKind::Get(_) => Form::Get,
            abi::MemberKind::Set(_) if m.is_static => Form::StaticSet,
            abi::MemberKind::Set(_) => Form::Set,
            abi::MemberKind::New(_) if m.meta.attrs.js.new_value().is_some() => Form::Value,
            abi::MemberKind::New(_) => Form::Constructor,
        };
        let binds = |m: &&abi::Member| {
            let f = form(m);
            (f == site.form
                || site.form == Form::Function && f == Form::Method
                || site.form == Form::Variable && f == Form::Get)
                && match site.form {
                    Form::Constructor => true,
                    Form::Value => m
                        .meta
                        .attrs
                        .js
                        .new_value()
                        .is_some_and(|p| p.segments.last().is_some_and(|n| n == js_name)),
                    _ => ir_types::is_named(m, js_name),
                }
        };
        let symbols = self.symbols();
        let mut tparams: &[abi::TypeParam] = &[];
        let mut erased = Vec::new();
        let mut assoc: &[abi::Assoc] = &[];
        let mut module = site.module;
        let mut instance = Vec::new();
        let (namespace, members): (Vec<&str>, Vec<&abi::Member>) = match site.form {
            Form::Function | Form::Variable => {
                match self.container(site.module, site.path).map(|items| {
                    items
                        .iter()
                        .filter_map(|d| match d {
                            abi::Decl::Member(m) if !m.export => Some(m),
                            _ => None,
                        })
                        .filter(binds)
                        .collect::<Vec<_>>()
                }) {
                    Some(members) if !members.is_empty() || site.path.is_empty() => {
                        (site.path.iter().map(String::as_str).collect(), members)
                    }
                    // A namespace merged into a class or enum, whose functions
                    // are the declaration's static members (its namespace
                    // holding only its types, if any).
                    _ => {
                        let (name, namespace) = site.path.split_last()?;
                        let items = self.container(site.module, namespace)?;
                        let statics = items.iter().find_map(|d| match d {
                            abi::Decl::Class(c) if &c.name == name => Some(&c.members),
                            abi::Decl::Enum(e) if &e.name == name => Some(&e.members),
                            _ => None,
                        })?;
                        let of = match site.form {
                            Form::Function => Form::Static,
                            _ => Form::StaticGet,
                        };
                        let members = statics
                            .iter()
                            .filter(|m| form(m) == of && ir_types::is_named(m, js_name))
                            .collect();
                        (self.namespace(module, namespace)?, members)
                    }
                }
            }
            _ => {
                let (class, namespace) = site.path.split_last()?;
                let items = self.container(site.module, namespace)?;
                let class = items.iter().find_map(|d| match d {
                    abi::Decl::Class(c) if &c.name == class => Some(c),
                    _ => None,
                })?;
                let at = Location {
                    module: site.module.map(Into::into),
                    path: site.path.to_vec(),
                };
                // An instance member the class inherits from a parent
                // the parse flattens into it.
                let inherits = matches!(site.form, Form::Get | Form::Set | Form::Method);
                let (loc, class, passed, args) = match class.members.iter().any(|m| binds(&m))
                    || !inherits
                {
                    true => (at, class, true, Vec::new()),
                    // Without mono, through a parent passed literal-only
                    // arguments too.
                    false => inherited(&symbols, &at, class, &binds, !self.mono, &mut Vec::new())?,
                };
                tparams = &class.tparams;
                instance = args;
                if !passed && instance.is_empty() {
                    erased = tparams.iter().map(|p| p.name.as_str()).collect();
                }
                assoc = &class.assoc;
                module = loc.module.as_deref().and_then(|m| {
                    self.ir.items.iter().find_map(|d| match d {
                        abi::Decl::Module(x) if x.spec == m => Some(x.spec.as_str()),
                        _ => None,
                    })
                });
                let namespace = self.namespace(module, &loc.path[..loc.path.len() - 1])?;
                (namespace, class.members.iter().filter(binds).collect())
            }
        };
        let cx = ir_types::Cx {
            gctx,
            symbols: &symbols,
            ids: &self.ids,
            locations: &self.locations,
            module,
            namespace,
            tparams: tparams.iter().map(|p| p.name.as_str()).collect(),
            assoc: assoc.iter().collect(),
            mono: self.mono,
            scope,
            erased,
            own: Vec::new(),
            bound,
        };
        if instance.is_empty() {
            return f(&cx, members);
        }
        // A parent's members at the concrete arguments the child passes.
        let arg = |n: &str| instance.iter().find(|(p, _)| p == n).map(|(_, a)| a);
        // An associated type (`PluralizeUnits`, or `Units.PluralizeUnits`),
        // at its parameter's argument's row.
        let row = |param: Option<&str>, name: &str| {
            let assoc = assoc
                .iter()
                .find(|x| x.name == name && param.is_none_or(|p| x.param == p))?;
            let a = arg(&assoc.param)?;
            assoc
                .rows
                .iter()
                .find(|(e, _)| e == a)
                .map(|(_, t)| t.clone())
        };
        let at = |t: &abi::Type| {
            t.map_named(&|r| match &r.path.segments[..] {
                [n] if r.args.is_empty() => arg(n).cloned().or_else(|| row(None, n)),
                [n, name] if r.args.is_empty() => row(Some(n), name),
                _ => None,
            })
        };
        let members: Vec<abi::Member> = members
            .into_iter()
            .map(|m| abi::Member {
                params: m
                    .params
                    .iter()
                    .map(|p| abi::Param {
                        ty: at(&p.ty),
                        ..p.clone()
                    })
                    .collect(),
                result: at(&m.result),
                ..m.clone()
            })
            .collect();
        f(&cx, members.iter().collect())
    }

    /// Whether the parameter `name` of the declaration `id` is a
    /// family's literal-only parameter, which an argument of literals
    /// instantiates at their enum.
    pub fn family_param(&self, id: TypeId, name: &str) -> bool {
        self.locations
            .get(&id)
            .and_then(|loc| self.families.bounded.get(loc))
            .is_some_and(|b| b.param == name)
    }

    /// The enum `id` lowered to, with its hierarchy flattened.
    fn with_enum<R>(&self, id: TypeId, f: impl FnOnce(&enums::Flat<'_>) -> R) -> Option<R> {
        let loc = self.locations.get(&id)?;
        let symbols = self.symbols();
        let path = abi::Path::new(loc.path.clone());
        let cx = ts_gen_abi::resolve::Context {
            module: loc.module.as_deref(),
            namespace: Vec::new(),
            tparams: Vec::new(),
            ..Default::default()
        };
        let (loc, decl) = symbols.resolve_decl(&path, &cx)?;
        let TypeDecl::Enum(decl) = decl else {
            return None;
        };
        let flat = enums::flatten(
            &symbols,
            EnumRef {
                loc: Some(loc),
                decl,
            },
        )
        .ok()?;
        Some(f(&flat))
    }

    /// The string or numeric enum `id` lowered to, as a wasm-bindgen enum
    /// named `name`. `None` when it isn't a unit enum, which the parse
    /// still emits.
    pub fn unit_enum(
        &self,
        id: TypeId,
        name: &str,
        js_namespace: &[String],
    ) -> Option<TokenStream> {
        self.with_enum(id, |flat| match flat.shape {
            Shape::Unit => Some(string_enum(flat, name)),
            Shape::Carrier(_) => Some(numeric_enum(flat, name, js_namespace)),
            Shape::Payload => None,
        })
        .flatten()
    }
}

/// The JS literal of a string enum arm.
fn literal(arm: &abi::Arm) -> Option<&str> {
    arm.meta.attrs.js.iter().find_map(|e| match e {
        abi::JsEntry::Literal(s) => Some(s.as_str()),
        _ => None,
    })
}

fn alias_consts(name: &syn::Ident, flat: &enums::Flat<'_>, docs: bool) -> TokenStream {
    let consts: Vec<_> = flat
        .aliases
        .iter()
        .map(|(alias, target)| {
            let doc = if docs {
                super::doc_tokens(&alias.meta.doc)
            } else {
                quote! {}
            };
            let alias = super::typemap::make_ident(&alias.name);
            let target = super::typemap::make_ident(&flat.arms[*target].arm.name);
            quote! {
                #doc
                #[allow(non_upper_case_globals)]
                pub const #alias: #name = #name::#target;
            }
        })
        .collect();
    if consts.is_empty() {
        return quote! {};
    }
    quote! {
        impl #name {
            #(#consts)*
        }
    }
}

/// A string enum: each variant's discriminant is its JS literal. wasm-bindgen
/// never names one in JS, so it takes no `js_namespace`.
fn string_enum(flat: &enums::Flat<'_>, name: &str) -> TokenStream {
    let ident = super::typemap::make_ident(name);
    let derives = super::enums::enum_derives(false);
    let variants = flat.arms.iter().filter_map(|a| {
        let variant = super::typemap::make_ident(&a.arm.name);
        let value = literal(a.arm)?;
        Some(quote! { #variant = #value })
    });
    let aliases = alias_consts(&ident, flat, false);
    quote! {
        #[wasm_bindgen(private)]
        #derives
        pub enum #ident {
            #(#variants,)*
        }
        #aliases
    }
}

/// A numeric enum, `repr(u32)` unless a value is negative. wasm-bindgen
/// defines a JS object for it, placed by its `js_namespace`.
fn numeric_enum(flat: &enums::Flat<'_>, name: &str, js_namespace: &[String]) -> TokenStream {
    let ident = super::typemap::make_ident(name);
    let derives = super::enums::enum_derives(false);
    let js_namespace = super::js_namespace_attr(js_namespace).map(|ns| quote! { , #ns });
    let negative = flat.arms.iter().any(|a| a.value < 0);
    let repr = if negative {
        quote! { i32 }
    } else {
        quote! { u32 }
    };
    let variants = flat.arms.iter().map(|a| {
        let variant = super::typemap::make_ident(&a.arm.name);
        let doc = super::doc_tokens(&a.arm.meta.doc);
        let value = if negative {
            let v = a.value as i32;
            quote! { #v }
        } else {
            let v = a.value as u32;
            quote! { #v }
        };
        quote! {
            #doc
            #variant = #value
        }
    });
    let aliases = alias_consts(&ident, flat, true);
    quote! {
        #[wasm_bindgen(private #js_namespace)]
        #derives
        #[repr(#repr)]
        pub enum #ident {
            #(#variants,)*
        }
        #aliases
    }
}

/// Each enum a member throws or a future rejects with, and the container
/// naming it.
fn throws_enums<'a>(
    items: &'a [abi::Decl],
    module: Option<&'a str>,
    namespace: &mut Vec<&'a str>,
    out: &mut Vec<(abi::Path, Option<&'a str>, Vec<&'a str>)>,
) {
    fn futures(t: &abi::Type, out: &mut Vec<abi::Path>) {
        match t {
            abi::Type::Future(t, e) => {
                out.push(e.clone());
                futures(t, out);
            }
            abi::Type::Array(t) | abi::Type::Nullable(t, _) => futures(t, out),
            abi::Type::Tuple(ts) | abi::Type::TupleBox(ts) => {
                ts.iter().for_each(|t| futures(t, out))
            }
            abi::Type::Named(r) => r.args.iter().for_each(|t| futures(t, out)),
            _ => {}
        }
    }
    let member = |m: &abi::Member, namespace: &[&'a str], out: &mut Vec<_>| {
        let mut paths: Vec<abi::Path> = m.throws.iter().cloned().collect();
        futures(&m.result, &mut paths);
        out.extend(paths.into_iter().map(|p| (p, module, namespace.to_vec())));
    };
    for d in items {
        match d {
            abi::Decl::Member(m) => member(m, namespace, out),
            abi::Decl::Class(c) => c.members.iter().for_each(|m| member(m, namespace, out)),
            abi::Decl::Namespace(n) => {
                namespace.push(&n.name);
                throws_enums(&n.items, module, namespace, out);
                namespace.pop();
            }
            abi::Decl::Module(m) => throws_enums(&m.items, Some(&m.spec), &mut Vec::new(), out),
            _ => {}
        }
    }
}

/// The parent declaring a member, whether the child passes it its own
/// parameters, and the concrete arguments it passes instead.
type Parent<'a> = (Location, &'a abi::Class, bool, Vec<(String, abi::Type)>);

/// The parent of `class` (at `at`), or its parent in turn, declaring a
/// member `binds` accepts, and whether each parent on the way passes its
/// child's parameters on unchanged. Only one that does, or one with none,
/// reads alike from the child, unless `any`; a parent passed concrete
/// arguments, or one it passes them on to, reads at them.
fn inherited<'a>(
    symbols: &Symbols<'a>,
    at: &Location,
    class: &'a abi::Class,
    binds: &impl Fn(&&abi::Member) -> bool,
    any: bool,
    seen: &mut Vec<Location>,
) -> Option<Parent<'a>> {
    if seen.contains(at) {
        return None;
    }
    seen.push(at.clone());
    let cx = Context {
        module: at.module.as_deref(),
        namespace: at.path[..at.path.len() - 1]
            .iter()
            .map(String::as_str)
            .collect(),
        ..Default::default()
    };
    for r in &class.extends {
        let Some((loc, TypeDecl::Class(parent))) = symbols.resolve_decl(&r.path, &cx) else {
            continue;
        };
        let passed = r.args.len() == parent.tparams.len()
            && r.args.iter().zip(&parent.tparams).all(|(a, p)| {
                matches!(a, abi::Type::Named(n) if n.args.is_empty() && n.path.segments == [p.name.clone()])
            });
        // A literal-only parameter reads at its bound, alike at any argument.
        let literal = parent.tparams.iter().zip(&r.args).all(|(p, a)| {
            matches!(p.bound, Some(abi::Bound::In(_)))
                || matches!(a, abi::Type::Named(n) if n.args.is_empty() && n.path.segments == [p.name.clone()])
        });
        // Concrete arguments, which the parent's members read at
        // (`extends Box<string>`).
        let mentions_own = |t: &abi::Type| {
            let mut found = false;
            t.walk(&mut |t| {
                found |= matches!(t, abi::Type::Named(n) if n.args.is_empty()
                    && class.tparams.iter().any(|p| n.path.segments == [p.name.clone()]));
            });
            found
        };
        // Without mono, a literal family reads through its associated
        // types instead; with it, at the argument's rows.
        let family = !parent.assoc.is_empty()
            || parent
                .tparams
                .iter()
                .any(|p| matches!(p.bound, Some(abi::Bound::In(_))));
        let concrete = !passed
            && r.args.len() == parent.tparams.len()
            && !r.args.iter().any(mentions_own)
            && (!family || !any);
        if !passed && !(any && literal) && !concrete {
            continue;
        }
        let args: Vec<(String, abi::Type)> = match concrete && !(any && literal) {
            true => parent
                .tparams
                .iter()
                .map(|p| p.name.clone())
                .zip(r.args.iter().cloned())
                .collect(),
            false => Vec::new(),
        };
        if parent.members.iter().any(|m| binds(&m)) {
            return Some((loc, parent, passed, args));
        }
        match inherited(symbols, &loc, parent, binds, any, seen) {
            Some((loc, class, further, deeper)) if args.is_empty() => {
                return Some((loc, class, passed && further, deeper));
            }
            // Through parents passing the parameters on, whose names the
            // arguments keep.
            Some((loc, class, true, deeper)) if deeper.is_empty() => {
                return Some((loc, class, false, args));
            }
            _ => {}
        }
    }
    None
}

/// Rust's bindings of `members`: each `@js(null) T?` parameter or
/// setter value is a value overload beside a constant `null` one (`f(x)`
/// and `f_with_null()`), and a `T??` one omittable as well (`f()`,
/// `f_with_x(x)`, `f_with_null()`).
fn null_arms(members: Vec<&abi::Member>) -> Vec<abi::Member> {
    members
        .into_iter()
        .flat_map(|m| expand(m.clone()))
        .collect()
}

fn expand(mut m: abi::Member) -> Vec<abi::Member> {
    for p in &mut m.params {
        p.ty = plain_inner(&p.ty, true);
    }
    m.result = plain_inner(&m.result, true);
    if m.export {
        return vec![m];
    }
    let last = m.params.len().saturating_sub(1);
    let Some((i, value)) = m.params.iter().enumerate().find_map(|(i, p)| match &p.ty {
        abi::Type::Nullable(t, true) => Some((i, (**t).clone())),
        abi::Type::Nullable(t, false) => match &**t {
            abi::Type::Nullable(v, _) if i < last => {
                Some((i, abi::Type::Nullable(v.clone(), false)))
            }
            abi::Type::Nullable(v, _) => Some((i, (**v).clone())),
            _ => None,
        },
        _ => None,
    }) else {
        return vec![m];
    };
    let mut with_value = m.clone();

    with_value.params[i].ty = value;
    let mut null = m;
    let name = null.params.remove(i).name;
    let template = null.meta.attrs.js.iter_mut().find_map(|e| match e {
        abi::JsEntry::Args(t) => Some(t),
        _ => None,
    });
    let lit = abi::Telem::Lit(abi::Literal::Null);
    match template {
        Some(t) => t
            .iter_mut()
            .filter(|e| matches!(e, abi::Telem::Param(n) if *n == name))
            .for_each(|e| *e = lit.clone()),
        None => {
            let args = with_value
                .params
                .iter()
                .map(|p| match p.name == name {
                    true => lit.clone(),
                    false => abi::Telem::Param(p.name.clone()),
                })
                .collect();
            null.meta.attrs.js.push(abi::JsEntry::Args(args));
        }
    }
    let renamed = null
        .meta
        .attrs
        .js
        .iter()
        .any(|e| matches!(e, abi::JsEntry::Name(_)));
    if let abi::MemberKind::Set(n) = &mut null.kind {
        if !renamed {
            null.meta.attrs.js.insert(0, abi::JsEntry::Name(n.clone()));
        }
        n.push_str("WithNull");
    }
    let mut out = expand(with_value);
    out.extend(expand(null));
    out
}

/// `t` with `@js(null)` and `T??` read as plain `T?` below its top node,
/// or below a top `future`'s value.
fn plain_inner(t: &abi::Type, top: bool) -> abi::Type {
    use abi::Type as T;
    let inner = |t: &T| plain_inner(t, false);
    let b = |t: &T| Box::new(inner(t));
    match t {
        T::Nullable(v, null) if top => match &**v {
            T::Nullable(w, _) => T::Nullable(Box::new(T::Nullable(b(w), false)), false),
            v => T::Nullable(b(v), *null),
        },
        T::Nullable(v, null) => T::Nullable(b(v), *null),
        T::Vec(e) => T::Vec(b(e)),
        T::VecBox(e) => T::VecBox(b(e)),
        T::MutVec(e) => T::MutVec(b(e)),
        T::Array(e) => T::Array(b(e)),
        T::ArrBox(e) => T::ArrBox(b(e)),
        T::Tuple(ts) => T::Tuple(ts.iter().map(inner).collect()),
        T::TupleBox(ts) => T::TupleBox(ts.iter().map(inner).collect()),
        T::Future(v, e) => T::Future(Box::new(plain_inner(v, top)), e.clone()),
        T::Fn(f) => T::Fn(Box::new(abi::FnType {
            params: f.params.iter().map(inner).collect(),
            rest: f.rest,
            result: inner(&f.result),
            throws: f.throws.clone(),
        })),
        T::View { mutable, elem } => T::View {
            mutable: *mutable,
            elem: b(elem),
        },
        T::Named(r) => T::Named(abi::TypeRef {
            path: r.path.clone(),
            args: r.args.iter().map(inner).collect(),
        }),
        t => t.clone(),
    }
}
