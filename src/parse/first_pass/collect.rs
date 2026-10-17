//! Phase 1: Collect all type names into the `TypeRegistry` and scope arena.
//!
//! Uses `CollectCtx` to group shared mutable state.

use oxc_ast::ast;

use crate::ir;
use crate::parse::merge::{
    has_constructor_shape, is_class_constructor_var, named_constructor_ref, var_declarator_name,
};
use crate::parse::scope::ScopeId;
use crate::util::diagnostics::DiagnosticCollector;

use super::converters::export_default_kind_name;
use super::{is_string_literal_union, namespace_block_statements};

/// Shared context for Phase 1 name collection.
struct CollectCtx<'a> {
    lib_name: Option<&'a str>,
    registry: &'a mut ir::TypeRegistry,
    gctx: &'a mut crate::context::GlobalContext,
    diag: &'a mut DiagnosticCollector,
    file_scope: ScopeId,
    /// Whether the file is read as a script, whose top-level declarations
    /// are globals visible to later files.
    is_script: bool,
    /// Key of the export map listing the globals this file declares: the
    /// file scope for a file read as a script, else a scope of its own,
    /// created at the first `declare global`.
    globals: Option<ScopeId>,
    /// Arena length before this file: a lower id is an earlier file's
    /// declaration.
    file_start: usize,
    /// `(module scope, specifier, name)` for each `export = name` that
    /// doesn't name a namespace of the module's own. Resolved through the
    /// scope chain once the whole file is collected, since the namespace
    /// may be declared after the module.
    outer_assignments: Vec<(ScopeId, String, String)>,
    /// `(scope, name)` for each `export default <name>` in a module, resolved
    /// once the whole file is collected as the declaration may follow.
    default_identifiers: Vec<(ScopeId, String)>,
    /// Interfaces with a construct signature or `prototype` member.
    constructor_shaped: std::collections::HashSet<(ScopeId, String)>,
    /// Constructor-shaped `var X: { new(...): T }` declarators, for a type
    /// alias `X` declared after them.
    ctor_vars: std::collections::HashSet<(ScopeId, String)>,
    /// `declare var X: Y` declarators, held back until the file is
    /// collected so `Y` can be checked against `constructor_shaped`
    /// regardless of declaration order.
    named_ctor_vars: Vec<(String, String, ir::ModuleContext, ScopeId, bool)>,
}

/// Walk the AST and collect all type names into the registry and scope arena.
pub fn collect_type_names(
    program: &ast::Program<'_>,
    lib_name: Option<&str>,
    diag: &mut DiagnosticCollector,
    gctx: &mut crate::context::GlobalContext,
    root_scope: ScopeId,
) -> ir::TypeRegistry {
    let mut registry = ir::TypeRegistry::default();
    let file_ctx = super::file_context(program, lib_name);
    let is_global = !matches!(file_ctx, ir::ModuleContext::Module(_));
    let file_start = gctx.type_arena().len();
    let mut ccx = CollectCtx {
        lib_name,
        registry: &mut registry,
        gctx,
        diag,
        file_scope: root_scope,
        is_script: is_global,
        globals: is_global.then_some(root_scope),
        file_start,
        outer_assignments: Vec::new(),
        default_identifiers: Vec::new(),
        constructor_shaped: Default::default(),
        ctor_vars: Default::default(),
        named_ctor_vars: Vec::new(),
    };

    ccx.collect_block(&program.body, &file_ctx, root_scope, false);
    if is_global {
        // A file read as a script exports every global it declares, and
        // a global has no default export.
        for name in super::block_declared_names(&program.body) {
            ccx.gctx.exports.insert(root_scope, &name, &name);
        }
        ccx.gctx.exports.remove(root_scope, "default");
    }

    for (name, iface, ctx, scope, exported) in std::mem::take(&mut ccx.named_ctor_vars) {
        ccx.resolve_named_ctor_var(&name, &iface, &ctx, scope, exported);
    }

    for (scope, specifier, name) in std::mem::take(&mut ccx.outer_assignments) {
        ccx.assign_namespace(scope, &specifier, &name);
    }

    // As in TypeScript, `export default x` exports every meaning of `x`, so
    // a namespace alone is the default too, its members bound through it.
    for (scope, name) in std::mem::take(&mut ccx.default_identifiers) {
        let s = ccx.gctx.scopes.get(scope);
        let bound = s.namespace(&name).is_some() || s.get(&name).is_some();
        if bound {
            ccx.export_default(scope, &name);
        } else {
            skip_default_identifier(ccx.diag, &name);
        }
    }

    // Every block's exports, its `export default` too, are the module's.
    // A declaration the block exports resolves in the module's exports,
    // merged with every block's, and an export specifier in its block.
    let blocks: Vec<ScopeId> = ccx.registry.module_blocks.values().map(|b| b.1).collect();
    for block_scope in blocks {
        ccx.move_block_exports(block_scope);
    }
    registry.globals = ccx.globals;
    registry
}

fn skip_default_identifier(diag: &mut DiagnosticCollector, name: &str) {
    diag.info(format!(
        "Skipping `export default {name}` (only a name declared in the same module is \
         supported)"
    ));
}

impl<'a> CollectCtx<'a> {
    /// Collect type names from a top-level statement.
    fn collect_statement(
        &mut self,
        stmt: &ast::Statement<'_>,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        match stmt {
            // Declaration variants
            ast::Statement::ClassDeclaration(class) => {
                if let Some(ref id) = class.id {
                    self.register_type(&id.name, ir::RegisteredKind::Class, ctx, scope, exported);
                }
            }
            ast::Statement::TSInterfaceDeclaration(iface) => {
                self.collect_interface_name(iface, ctx, scope, exported);
            }
            ast::Statement::TSTypeAliasDeclaration(alias) => {
                self.collect_type_alias_name(alias, ctx, scope, exported);
            }
            ast::Statement::FunctionDeclaration(func) => {
                if let Some(ref id) = func.id {
                    self.register_type(
                        &id.name,
                        ir::RegisteredKind::Function,
                        ctx,
                        scope,
                        exported,
                    );
                }
            }
            ast::Statement::VariableDeclaration(var_decl) => {
                self.collect_variable_names(var_decl, ctx, scope, exported);
            }
            ast::Statement::TSExternalModuleDeclaration(module) => {
                self.collect_external_module_names(module, scope);
            }
            ast::Statement::TSNamespaceDeclaration(ns) => {
                self.collect_namespace_names(ns, ctx, scope, exported);
            }
            ast::Statement::TSEnumDeclaration(enum_decl) => {
                let name = enum_decl.id.name.to_string();
                self.register_type(&name, ir::RegisteredKind::TsEnum, ctx, scope, exported);
            }
            ast::Statement::TSGlobalDeclaration(global) => {
                self.collect_global(&global.body.body);
            }

            // ModuleDeclaration variants
            ast::Statement::ExportDeclaration(export) => {
                let export_ctx = if let Some(lib) = self.lib_name {
                    ir::ModuleContext::Module(lib.into())
                } else {
                    self.diag
                        .warn("Found export declaration without --lib-name; treating as global");
                    ctx.clone()
                };
                self.collect_declaration(&export.declaration, &export_ctx, scope, true);
            }
            ast::Statement::ExportFromDeclaration(export) => {
                let source = export.source.value.to_string();
                for spec in &export.specifiers {
                    let exported_name = spec.exported.name().to_string();
                    let imported = spec.local.name().to_string();
                    // The re-export binds the public name locally once the
                    // import resolves, so it is exported under itself.
                    self.gctx
                        .exports
                        .insert(scope, &exported_name, &exported_name);
                    self.register_import(&exported_name, &imported, &source, scope);
                }
            }
            ast::Statement::ExportNamedDeclaration(export) => {
                for spec in &export.specifiers {
                    let exported_name = spec.exported.name().to_string();
                    let local = spec.local.name().to_string();
                    self.gctx.exports.insert(scope, &exported_name, &local);
                }
            }
            // `export default class Def` exports `default` only; `Def` is a
            // local name (TS2614 on `import { Def }`), so it binds by
            // `default` like a renamed export. Outside a module there is no
            // export to bind.
            ast::Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    if let Some(ref id) = class.id {
                        self.register_type(&id.name, ir::RegisteredKind::Class, ctx, scope, false);
                        if matches!(ctx, ir::ModuleContext::Module(_)) {
                            self.export_default(scope, &id.name);
                        }
                    }
                }
                ast::ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    if let Some(ref id) = func.id {
                        self.register_type(
                            &id.name,
                            ir::RegisteredKind::Function,
                            ctx,
                            scope,
                            false,
                        );
                        if matches!(ctx, ir::ModuleContext::Module(_)) {
                            self.export_default(scope, &id.name);
                        }
                    }
                }
                ast::ExportDefaultDeclarationKind::Identifier(ident) => {
                    if matches!(ctx, ir::ModuleContext::Module(_)) {
                        self.default_identifiers
                            .push((scope, ident.name.to_string()));
                    } else {
                        skip_default_identifier(self.diag, &ident.name);
                    }
                }
                other => {
                    self.diag.warn(format!(
                        "Unsupported export default declaration kind: {}",
                        export_default_kind_name(other)
                    ));
                }
            },

            // In the ESM shape of `module.exports = x`, `x` is the default
            // export.
            ast::Statement::TSExportAssignment(assign) => {
                if let (ir::ModuleContext::Module(_), ast::Expression::Identifier(ident)) =
                    (ctx, &assign.expression)
                {
                    self.export_default(scope, &ident.name);
                }
            }
            ast::Statement::TSNamespaceExportDeclaration(_) => {}

            ast::Statement::ExportAllDeclaration(export_all) => {
                let source = export_all.source.value.to_string();
                self.register_import("*", "*", &source, scope);
                self.diag.info(format!(
                    "Registered `export * from \"{source}\"` for dependency resolution"
                ));
            }

            ast::Statement::ImportDeclaration(import) => {
                let source = import.source.value.to_string();
                if let Some(ref specifiers) = import.specifiers {
                    for spec in specifiers {
                        match spec {
                            ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                                self.register_import(
                                    &s.local.name,
                                    &s.imported.name(),
                                    &source,
                                    scope,
                                );
                            }
                            ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                                self.register_import(&s.local.name, "default", &source, scope);
                            }
                            ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                                self.register_import(&s.local.name, "*", &source, scope);
                            }
                        }
                    }
                }
            }

            ast::Statement::TSImportEqualsDeclaration(decl) => {
                let local = decl.id.name.to_string();
                match &decl.module_reference {
                    ast::TSModuleReference::ExternalModuleReference(ext) => {
                        self.register_import(&local, "*", &ext.expression.value, scope);
                    }
                    ast::TSModuleReference::IdentifierReference(_)
                    | ast::TSModuleReference::QualifiedName(_) => {}
                }
            }

            _ => {}
        }
    }

    /// Collect names from an `ast::Declaration` (inside export blocks).
    fn collect_declaration(
        &mut self,
        decl: &ast::Declaration<'_>,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        match decl {
            ast::Declaration::ClassDeclaration(class) => {
                if let Some(ref id) = class.id {
                    self.register_type(&id.name, ir::RegisteredKind::Class, ctx, scope, exported);
                }
            }
            ast::Declaration::TSInterfaceDeclaration(iface) => {
                self.collect_interface_name(iface, ctx, scope, exported);
            }
            ast::Declaration::TSTypeAliasDeclaration(alias) => {
                self.collect_type_alias_name(alias, ctx, scope, exported);
            }
            ast::Declaration::FunctionDeclaration(func) => {
                if let Some(ref id) = func.id {
                    self.register_type(
                        &id.name,
                        ir::RegisteredKind::Function,
                        ctx,
                        scope,
                        exported,
                    );
                }
            }
            ast::Declaration::VariableDeclaration(var_decl) => {
                self.collect_variable_names(var_decl, ctx, scope, exported);
            }
            ast::Declaration::TSExternalModuleDeclaration(module) => {
                self.collect_external_module_names(module, scope);
            }
            ast::Declaration::TSNamespaceDeclaration(ns) => {
                self.collect_namespace_names(ns, ctx, scope, exported);
            }
            ast::Declaration::TSEnumDeclaration(enum_decl) => {
                let name = enum_decl.id.name.to_string();
                self.register_type(&name, ir::RegisteredKind::TsEnum, ctx, scope, exported);
            }
            ast::Declaration::TSGlobalDeclaration(global) => {
                self.collect_global(&global.body.body);
            }
            ast::Declaration::TSImportEqualsDeclaration(_) => {}
        }
    }

    /// A `declare global` block binds into the file scope, from which
    /// population hoists it to the global scope, and adds its names to the
    /// file's globals.
    fn collect_global(&mut self, stmts: &[ast::Statement<'_>]) {
        for s in stmts {
            self.collect_statement(s, &ir::ModuleContext::Global, self.file_scope, false);
        }
        let file_scope = self.file_scope;
        let globals = *self
            .globals
            .get_or_insert_with(|| self.gctx.scopes.create_child(file_scope));
        for name in super::block_declared_names(stmts) {
            self.gctx.exports.insert(globals, &name, &name);
        }
    }

    // ─── Per-type helpers ────────────────────────────────────────────

    fn collect_interface_name(
        &mut self,
        iface: &ast::TSInterfaceDeclaration<'_>,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        let name = iface.id.name.as_str();
        if has_constructor_shape(&iface.body.body) {
            self.constructor_shaped.insert((scope, name.to_string()));
        }
        if let Some(kind) = self.registry.kind_mut(scope, name) {
            if *kind == ir::RegisteredKind::Variable {
                *kind = ir::RegisteredKind::MergedClassLike;
                return;
            }
        }
        self.register_type(name, ir::RegisteredKind::Interface, ctx, scope, exported);
    }

    fn collect_type_alias_name(
        &mut self,
        alias: &ast::TSTypeAliasDeclaration<'_>,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        let name = alias.id.name.to_string();
        // Phase 2 reads a parent's placeholder when deciding whether an
        // intersection may `extends` it, so the placeholder says what can
        // be known syntactically: an alias over an inline shape becomes an
        // interface and registers as one, with an intersection's named
        // parents as its `extends` so they are checked in turn. One over its
        // own type parameter (`T & { … }`) can never hoist, so stays an
        // alias of that intersection. An alias records its type parameters,
        // and its target when that is a non-object type (`type Id = string`)
        // or another name, since the default `any` means "not yet known".
        let own_params: Vec<&str> = alias
            .type_parameters
            .iter()
            .flat_map(|tp| tp.params.iter().map(|p| p.name.name.as_str()))
            .collect();
        let shape = crate::parse::members::inline_shape(&alias.type_annotation);
        let parents: Vec<&str> = match &shape {
            Some(crate::parse::members::InlineShape::Intersection { parents, .. }) => {
                parents.iter().filter_map(|p| bare_reference(p)).collect()
            }
            _ => Vec::new(),
        };
        let over_own_param = parents.iter().any(|p| own_params.contains(p));
        let kind = if is_string_literal_union(&alias.type_annotation) {
            ir::RegisteredKind::StringEnum
        } else if shape.is_some() && !over_own_param {
            ir::RegisteredKind::Interface
        } else {
            ir::RegisteredKind::TypeAlias
        };
        if kind == ir::RegisteredKind::TypeAlias && self.ctor_vars.remove(&(scope, name.clone())) {
            self.registry
                .alias_constructors
                .insert((scope, name.clone()));
        }
        self.register_type(&name, kind, ctx, scope, exported);
        let Some(id) = self.gctx.scopes.get(scope).get(&name) else {
            return;
        };
        let target = if over_own_param {
            Some(ir::TypeRef::Intersection(
                parents.iter().copied().map(ir::TypeRef::ident).collect(),
            ))
        } else {
            non_object_target(&alias.type_annotation).or_else(|| {
                bare_reference(&alias.type_annotation)?;
                Some(placeholder_reference(&alias.type_annotation))
            })
        };
        match &mut self.gctx.get_type_mut(id).kind {
            ir::TypeKind::TypeAlias(a) => {
                if let Some(target) = target {
                    a.target = target;
                }
                a.type_params = own_params
                    .iter()
                    .map(|name| ir::TypeParam {
                        name: name.to_string(),
                        constraint: None,
                        default: None,
                    })
                    .collect();
            }
            ir::TypeKind::Interface(i) => {
                i.extends = parents
                    .into_iter()
                    .map(|parent| ir::Heritage {
                        type_ref: ir::TypeRef::ident(parent),
                        scope,
                    })
                    .collect();
            }
            _ => {}
        }
    }

    fn collect_variable_names(
        &mut self,
        var_decl: &ast::VariableDeclaration<'_>,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        for declarator in &var_decl.declarations {
            if let Some(name) = var_declarator_name(declarator) {
                if is_class_constructor_var(declarator) {
                    if let Some(kind) = self.registry.kind_mut(scope, &name) {
                        if *kind == ir::RegisteredKind::Interface {
                            *kind = ir::RegisteredKind::MergedClassLike;
                            continue;
                        }
                        // The alias stays the type, which the var constructs.
                        if *kind == ir::RegisteredKind::TypeAlias {
                            if exported {
                                self.gctx.exports.insert(scope, &name, &name);
                            }
                            self.registry.alias_constructors.insert((scope, name));
                            continue;
                        }
                    }
                    self.ctor_vars.insert((scope, name.clone()));
                    self.register_type(&name, ir::RegisteredKind::Variable, ctx, scope, exported);
                } else if let Some(iface) = named_constructor_ref(declarator) {
                    self.named_ctor_vars
                        .push((name, iface, ctx.clone(), scope, exported));
                } else {
                    self.register_type(&name, ir::RegisteredKind::Variable, ctx, scope, exported);
                }
            }
        }
    }

    /// `declare var X: XConstructor` next to an interface `X` registers `X`
    /// as a class when this file's `XConstructor` has constructor shape.
    /// That is only the name's phase 1 kind: the var is bound as a value,
    /// and the static side its type gives is resolved once every file is
    /// inserted (see `bind_constructor_vars`). Otherwise it is a plain
    /// variable; a same-named namespace or interface collected meanwhile
    /// keeps the name, as it would have by overwriting the var in source
    /// order.
    fn resolve_named_ctor_var(
        &mut self,
        name: &str,
        iface: &str,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        // `XConstructor` resolves from the var like any type reference.
        let ctor_scope = self
            .declaring_scope(scope, iface)
            .filter(|&s| self.constructor_shaped.contains(&(s, iface.to_string())));
        if ctor_scope.is_some() && iface != name {
            if let Some(kind) = self
                .registry
                .kind_mut(scope, name)
                .filter(|kind| **kind == ir::RegisteredKind::Interface)
            {
                *kind = ir::RegisteredKind::MergedClassLike;
                return;
            }
        }
        if self.registry.kind(scope, name).is_none() {
            self.register_type(name, ir::RegisteredKind::Variable, ctx, scope, exported);
        }
    }

    fn collect_external_module_names(
        &mut self,
        module: &ast::TSExternalModuleDeclaration<'_>,
        scope: ScopeId,
    ) {
        let specifier = module.id.value.as_str();
        let module_ctx = ir::ModuleContext::Module(specifier.into());
        // As in TypeScript's binder, what a block exports goes in the
        // module's exports, one table for every block of the module in any
        // file or augmentation, and its imports and other declarations in
        // the block's own locals, which see the module's exports.
        let global = self
            .gctx
            .scopes
            .get(self.file_scope)
            .parent
            .unwrap_or(self.file_scope);
        let exports = *self
            .gctx
            .ambient_modules
            .entry(specifier.to_string())
            .or_insert_with(|| self.gctx.scopes.create_child(global));
        let block_scope = self.gctx.scopes.create_child(scope);
        self.gctx.scopes.get_mut(block_scope).exports = Some(exports);
        self.registry
            .module_scopes
            .insert(specifier.to_string(), exports);
        self.registry
            .module_blocks
            .insert(module.span.start, (specifier.to_string(), block_scope));
        if let Some(block) = &module.body {
            let exported: std::collections::HashSet<String> =
                super::block_module_exports(&block.body)
                    .into_iter()
                    .collect();
            self.gctx
                .block_exports
                .insert(block_scope, exported.clone());
            self.collect_block(&block.body, &module_ctx, block_scope, false);
        }
    }

    /// Collect one declaration block (a file, a `declare module` body or a
    /// namespace body) into `scope`, recording what it exports. See
    /// [`super::block_exports_explicitly`] for the implicit-export rule.
    fn collect_block(
        &mut self,
        stmts: &[ast::Statement<'_>],
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        for stmt in stmts {
            self.collect_statement(stmt, ctx, scope, exported);
        }
        if !super::block_exports_explicitly(stmts) {
            for name in super::block_implicit_exports(stmts) {
                self.gctx.exports.insert(scope, &name, &name);
            }
        }
        // `export = X` makes the exports of the namespace merged with `X`
        // the module's named exports, alongside `default`.
        if let Some(assigned) = super::export_assigned_name(stmts) {
            if let Some(ns_scope) = self.gctx.scopes.get(scope).namespace(assigned) {
                self.gctx.exports.extend_from(scope, ns_scope);
            } else if let ir::ModuleContext::Module(specifier) = ctx {
                // A binding of the module's own, imported or declared, is
                // what it assigns.
                if !super::block_declared_names(stmts)
                    .iter()
                    .chain(&super::block_imported_names(stmts))
                    .any(|name| name == assigned)
                {
                    self.outer_assignments.push((
                        scope,
                        specifier.to_string(),
                        assigned.to_string(),
                    ));
                }
            }
        }
    }

    /// Make the export map of a block whose exports are shared, a
    /// `declare module` or namespace block, part of the shared one. A
    /// declaration the block exports resolves in the shared exports, merged
    /// with every block's, and an export specifier in its block.
    fn move_block_exports(&mut self, block_scope: ScopeId) {
        let Some(exports) = self.gctx.scopes.get(block_scope).exports else {
            return;
        };
        self.gctx.exports.move_into(block_scope, exports);
        let declared = self.gctx.block_exports.get(&block_scope).cloned();
        for name in declared.into_iter().flatten() {
            if self.gctx.exports.binding(exports, &name) == Some((name.as_str(), block_scope)) {
                self.gctx.exports.insert(exports, &name, &name);
            }
        }
    }

    /// The scope `name` is declared in, seen from `scope`.
    fn declaring_scope(&self, scope: ScopeId, name: &str) -> Option<ScopeId> {
        self.gctx
            .scopes
            .chain(scope)
            .into_iter()
            .find(|&id| self.gctx.scopes.get(id).get(name).is_some())
    }

    /// Make the namespace `name` resolves to from the module scope `scope`
    /// the module `specifier`, when it is the name's only runtime binding.
    /// A merged class, function or variable carries the value and stays
    /// where it is declared, so its namespace does too.
    ///
    /// Phase 2 populates one file at a time, so only a namespace this file
    /// declares first can be: an earlier file's is already populated as a
    /// global.
    fn assign_namespace(&mut self, scope: ScopeId, specifier: &str, name: &str) {
        let mut chain = self.gctx.scopes.chain(scope).into_iter();
        let (found_in, ns_scope) = loop {
            let Some(id) = chain.next() else { return };
            let s = self.gctx.scopes.get(id);
            let binding = s.get(name);
            if let Some(ns) = s.namespace(name) {
                // An interface merged with a constructor-shaped `var` is a
                // class, whose binding is still the interface placeholder.
                let class_like =
                    self.registry.kind(id, name) == Some(&ir::RegisteredKind::MergedClassLike);
                let type_only = binding.is_none_or(|t| match self.gctx.get_type(t).kind {
                    ir::TypeKind::Namespace(_) | ir::TypeKind::TypeAlias(_) => true,
                    ir::TypeKind::Interface(_) => !class_like,
                    _ => false,
                });
                if !type_only {
                    return;
                }
                break (id, ns);
            }
            if binding.is_some() {
                return;
            }
        };
        if let Some(other) = self.gctx.assigned_namespaces.get(&ns_scope) {
            self.diag.warn(format!(
                "`export = {name}` in \"{specifier}\" names the namespace \"{other}\" \
                 already assigns, skipping"
            ));
            return;
        }
        if found_in != self.file_scope || self.earlier_global_namespace(name).is_some() {
            self.diag.warn(format!(
                "`export = {name}` in \"{specifier}\" names a global namespace another \
                 file also declares, skipping"
            ));
            return;
        }
        self.gctx.exports.extend_from(scope, ns_scope);
        self.assign_global_namespace(name, ns_scope, specifier.to_string());
    }

    /// Record `ns_scope` as the module `specifier`. A global namespace
    /// assigned to a module is never populated as a `Namespace`, which is
    /// what hoists the rest, so it is hoisted here for later files to see.
    fn assign_global_namespace(&mut self, name: &str, ns_scope: ScopeId, specifier: String) {
        if self.is_script {
            if let Some(parent) = self.gctx.scopes.get(self.file_scope).parent {
                self.gctx
                    .scopes
                    .insert_namespace(parent, name.to_string(), ns_scope);
            }
        }
        self.gctx
            .assigned_namespaces
            .insert(ns_scope, specifier.clone());
        self.registry
            .assigned_namespaces
            .insert(ns_scope, specifier);
    }

    /// The member scope of a global namespace `name` an earlier script file
    /// declares, hoisted to the scope above this file's.
    fn earlier_global_namespace(&self, name: &str) -> Option<ScopeId> {
        let parent = self.gctx.scopes.get(self.file_scope).parent?;
        self.gctx.scopes.get(parent).namespace(name)
    }

    /// Record `local` as `scope`'s default export, binding by `default`.
    fn export_default(&mut self, scope: ScopeId, local: &str) {
        self.gctx.exports.insert(scope, "default", local);
    }

    fn collect_namespace_names(
        &mut self,
        ns: &ast::TSNamespaceDeclaration<'_>,
        parent_ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        let name = ns.id.name.to_string();
        // A namespace inside a module is emitted flat into the module,
        // but still gets its own scope so `NS.Member` resolves. A global
        // namespace another file declared already takes this file's
        // members into its scope, as the two merge.
        let global = (scope == self.file_scope && *parent_ctx == ir::ModuleContext::Global)
            .then(|| self.gctx.scopes.get(scope).parent)
            .flatten()
            .and_then(|global| self.gctx.scopes.get(global).namespace(&name));
        // Likewise a namespace a `declare module` block exports merges with
        // the module's other blocks' one.
        let module_exports = self.gctx.scopes.get(scope).exports.filter(|_| {
            self.gctx
                .block_exports
                .get(&scope)
                .is_some_and(|names| names.contains(&name))
        });
        let ns_scope = match self.gctx.scopes.get(scope).namespace(&name) {
            Some(ns_scope) => ns_scope,
            None => {
                let earlier = global.or_else(|| {
                    module_exports
                        .and_then(|exports| self.gctx.scopes.get(exports).namespace(&name))
                });
                let ns_scope =
                    self.create_namespace_scope(&name, parent_ctx, scope, exported, earlier);
                if let Some(exports) = module_exports {
                    self.gctx
                        .scopes
                        .insert_namespace(exports, name.clone(), ns_scope);
                }
                // Reopening a global namespace an earlier file assigned to
                // a module, whose declarations are the module's too.
                let assigned = global
                    .and_then(|earlier| self.gctx.assigned_namespaces.get(&earlier))
                    .cloned();
                if let Some(specifier) = assigned {
                    self.assign_global_namespace(&name, ns_scope, specifier);
                }
                ns_scope
            }
        };
        // As in TypeScript's binder, and as a `declare module` block, each
        // declaration of the namespace has its own scope for what it
        // doesn't export, which sees the members every declaration shares.
        let block_scope = self.gctx.scopes.create_child(scope);
        self.gctx.scopes.get_mut(block_scope).exports = Some(ns_scope);
        self.registry
            .namespace_blocks
            .insert(ns.span.start, block_scope);
        let stmts = namespace_block_statements(ns);
        let exported = super::block_module_exports(stmts).into_iter().collect();
        self.gctx.block_exports.insert(block_scope, exported);
        self.collect_block(stmts, parent_ctx, block_scope, false);
        self.move_block_exports(block_scope);
    }

    // ─── Registration helpers ────────────────────────────────────────

    fn register_type(
        &mut self,
        name: &str,
        kind: ir::RegisteredKind,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        // An earlier file's declaration in a shared namespace scope stays
        // bound, so this file's declaration of the name merges into it.
        let binds = self.gctx.binding_scope(scope, name);
        let earlier = self
            .gctx
            .scopes
            .get(binds)
            .get(name)
            .is_some_and(|id| id.index() < self.file_start);
        super::register_type(
            name,
            kind,
            ctx,
            self.registry,
            self.gctx,
            scope,
            exported,
            (!earlier).then_some(binds),
        );
    }

    fn register_import(
        &mut self,
        local_name: &str,
        original_name: &str,
        from_module: &str,
        scope: ScopeId,
    ) {
        super::register_import(local_name, original_name, from_module, self.gctx, scope);
    }

    /// Register a namespace in `scope`, with a new member scope unless it
    /// merges into the existing `members`.
    fn create_namespace_scope(
        &mut self,
        name: &str,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
        members: Option<ScopeId>,
    ) -> ScopeId {
        let ns_scope = members.unwrap_or_else(|| self.gctx.scopes.create_child(scope));
        self.gctx
            .scopes
            .insert_namespace(scope, name.to_string(), ns_scope);
        if exported {
            self.gctx.exports.insert(scope, name, name);
        }
        // A same-named class, function or interface stays the name's type.
        let binds = self.gctx.binding_scope(scope, name);
        if self.gctx.scopes.get(binds).get(name).is_some() {
            return ns_scope;
        }
        let ns_type_id = self.gctx.insert_type(ir::TypeDeclaration {
            kind: ir::TypeKind::Namespace(ir::NamespaceDecl {
                name: name.to_string(),
                declarations: vec![],
                child_scope: ns_scope,
            }),
            module_context: ctx.clone(),
            doc: None,
            scope_id: scope,
        });
        self.gctx.scopes.insert(binds, name.to_string(), ns_type_id);
        self.registry.types.insert(
            (scope, name.to_string()),
            ir::TypeInfo {
                kind: ir::RegisteredKind::Namespace,
                primary_context: ctx.clone(),
            },
        );
        ns_scope
    }
}

/// The name of a single-identifier type reference, with or without type
/// arguments.
fn bare_reference<'a>(ts_type: &'a ast::TSType<'_>) -> Option<&'a str> {
    match ts_type.without_parenthesized() {
        ast::TSType::TSTypeReference(r) => match &r.type_name {
            ast::TSTypeName::IdentifierReference(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// A single-identifier reference with its type arguments, which keep only
/// the names they mention (`Map<string, T[]>` → `Map<any, T[]>`): enough for
/// phase 2 to see whether a generic alias's target mentions its parameters.
fn placeholder_reference(ts_type: &ast::TSType<'_>) -> ir::TypeRef {
    match ts_type.without_parenthesized() {
        ast::TSType::TSTypeReference(r) => match &r.type_name {
            ast::TSTypeName::IdentifierReference(id) => ir::TypeRef::Reference {
                segments: vec![id.name.to_string()],
                generic_args: r
                    .type_arguments
                    .iter()
                    .flat_map(|args| args.params.iter().map(placeholder_reference))
                    .collect(),
            },
            _ => ir::TypeRef::Any,
        },
        ast::TSType::TSArrayType(arr) => {
            ir::TypeRef::Array(Box::new(placeholder_reference(&arr.element_type)))
        }
        _ => ir::TypeRef::Any,
    }
}

/// The IR type of an alias target that is syntactically not an object type,
/// so could never be a parent: primitives, literals, functions, arrays and
/// tuples. `None` for anything that might be one.
fn non_object_target(ts_type: &ast::TSType<'_>) -> Option<ir::TypeRef> {
    use ast::TSType as T;
    Some(match ts_type.without_parenthesized() {
        T::TSStringKeyword(_) | T::TSTemplateLiteralType(_) => ir::TypeRef::String,
        T::TSNumberKeyword(_) => ir::TypeRef::Number,
        T::TSBooleanKeyword(_) => ir::TypeRef::Boolean,
        T::TSBigIntKeyword(_) => ir::TypeRef::BigInt,
        T::TSSymbolKeyword(_) => ir::TypeRef::Symbol,
        T::TSNullKeyword(_) => ir::TypeRef::Null,
        T::TSUndefinedKeyword(_) | T::TSVoidKeyword(_) => ir::TypeRef::Undefined,
        T::TSLiteralType(lit) => match &lit.literal {
            ast::TSLiteral::StringLiteral(s) => ir::TypeRef::StringLiteral(s.value.to_string()),
            ast::TSLiteral::NumericLiteral(n) => ir::TypeRef::NumberLiteral(n.value),
            ast::TSLiteral::BooleanLiteral(b) => ir::TypeRef::BooleanLiteral(b.value),
            _ => return None,
        },
        T::TSFunctionType(_) | T::TSConstructorType(_) => ir::TypeRef::Function(ir::FunctionSig {
            params: vec![],
            return_type: Box::new(ir::TypeRef::Any),
        }),
        T::TSArrayType(_) => ir::TypeRef::Array(Box::new(ir::TypeRef::Any)),
        T::TSTupleType(_) => ir::TypeRef::Tuple(vec![]),
        _ => return None,
    })
}
