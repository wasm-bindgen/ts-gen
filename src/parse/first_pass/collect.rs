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

use super::converters::{classify_ts_enum_kind, export_default_kind_name};
use super::{is_string_literal_union, namespace_block_statements};

/// Shared context for Phase 1 name collection.
struct CollectCtx<'a> {
    lib_name: Option<&'a str>,
    registry: &'a mut ir::TypeRegistry,
    gctx: &'a mut crate::context::GlobalContext,
    diag: &'a mut DiagnosticCollector,
    file_scope: ScopeId,
    /// `(scope, local, public)` for each source-less export specifier.
    /// Resolved once the whole file is collected, since a specifier may
    /// precede the declaration it names.
    export_specifiers: Vec<(ScopeId, String, String)>,
    /// Interfaces with a construct signature or `prototype` member.
    constructor_shaped: std::collections::HashSet<String>,
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
    let mut ccx = CollectCtx {
        lib_name,
        registry: &mut registry,
        gctx,
        diag,
        file_scope: root_scope,
        export_specifiers: Vec::new(),
        constructor_shaped: Default::default(),
        named_ctor_vars: Vec::new(),
    };

    let file_ctx = super::file_context(program, lib_name);
    ccx.collect_block(&program.body, &file_ctx, root_scope, false);
    if !matches!(file_ctx, ir::ModuleContext::Module(_)) {
        // A script file's globals are not a module's exports.
        ccx.gctx.exports.clear(root_scope);
    }

    for (name, iface, ctx, scope, exported) in std::mem::take(&mut ccx.named_ctor_vars) {
        ccx.resolve_named_ctor_var(&name, &iface, &ctx, scope, exported);
    }

    for (scope, local, public) in std::mem::take(&mut ccx.export_specifiers) {
        // Only a binding declared directly in the exporting scope takes the
        // public name; an outer one is re-exported through an alias stub.
        if let Some(type_id) = ccx.gctx.scopes.get(scope).get(&local) {
            ccx.registry
                .export_names
                .entry(type_id)
                .or_default()
                .push(public);
        }
    }

    registry
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
                let kind = classify_ts_enum_kind(enum_decl);
                self.register_type(&name, kind, ctx, scope, exported);
            }
            ast::Statement::TSGlobalDeclaration(global) => {
                for s in &global.body.body {
                    self.collect_statement(s, &ir::ModuleContext::Global, self.file_scope, true);
                }
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
                    if exported_name != local {
                        self.export_specifiers.push((scope, local, exported_name));
                    }
                }
            }
            ast::Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                // `export default class Def` exports `default` only; `Def`
                // is a local name (TS2614 on `import { Def }`).
                ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    if let Some(ref id) = class.id {
                        self.register_type(&id.name, ir::RegisteredKind::Class, ctx, scope, false);
                        self.gctx.exports.insert(scope, "default", id.name.as_str());
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
                        self.gctx.exports.insert(scope, "default", id.name.as_str());
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
                    self.gctx
                        .exports
                        .insert(scope, "default", ident.name.as_str());
                    self.export_specifiers.push((
                        scope,
                        ident.name.to_string(),
                        "default".to_string(),
                    ));
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
                let kind = classify_ts_enum_kind(enum_decl);
                self.register_type(&name, kind, ctx, scope, exported);
            }
            ast::Declaration::TSGlobalDeclaration(global) => {
                for s in &global.body.body {
                    self.collect_statement(s, &ir::ModuleContext::Global, self.file_scope, true);
                }
            }
            ast::Declaration::TSImportEqualsDeclaration(_) => {}
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
            self.constructor_shaped.insert(name.to_string());
        }
        if let Some(info) = self.registry.types.get_mut(name) {
            if info.kind == ir::RegisteredKind::Variable {
                info.kind = ir::RegisteredKind::MergedClassLike;
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
        // interface and registers as one; an alias to a non-object type
        // (`type Id = string`) records that in its target, since the
        // default `any` means "not yet known".
        let kind = if is_string_literal_union(&alias.type_annotation) {
            ir::RegisteredKind::StringEnum
        } else if crate::parse::members::inline_shape(&alias.type_annotation).is_some() {
            ir::RegisteredKind::Interface
        } else {
            ir::RegisteredKind::TypeAlias
        };
        self.register_type(&name, kind, ctx, scope, exported);
        if let Some(target) = non_object_target(&alias.type_annotation) {
            if let Some(id) = self.gctx.scopes.get(scope).get(&name) {
                if let ir::TypeKind::TypeAlias(a) = &mut self.gctx.get_type_mut(id).kind {
                    a.target = target;
                }
            }
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
                    if let Some(info) = self.registry.types.get_mut(&name) {
                        if info.kind == ir::RegisteredKind::Interface {
                            info.kind = ir::RegisteredKind::MergedClassLike;
                            continue;
                        }
                    }
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

    /// `declare var X: XConstructor` merges like the inline literal form
    /// when `XConstructor` has constructor shape and there is an instance
    /// interface `X` to carry the members (or the var is module-scoped,
    /// where the class may be constructor-only). Otherwise it is a plain
    /// variable of that type; a same-named namespace or interface
    /// collected meanwhile keeps the name, as it would have by
    /// overwriting the var in source order.
    fn resolve_named_ctor_var(
        &mut self,
        name: &str,
        iface: &str,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) {
        let instance = self
            .registry
            .types
            .get_mut(name)
            .filter(|info| info.kind == ir::RegisteredKind::Interface);
        if self.constructor_shaped.contains(iface) && iface != name {
            if let Some(info) = instance {
                info.kind = ir::RegisteredKind::MergedClassLike;
                self.registry
                    .ctor_interfaces
                    .insert(name.to_string(), iface.to_string());
                return;
            }
            if matches!(ctx, ir::ModuleContext::Module(_)) {
                self.registry
                    .ctor_interfaces
                    .insert(name.to_string(), iface.to_string());
                self.register_type(name, ir::RegisteredKind::Variable, ctx, scope, exported);
                return;
            }
        }
        if !self.registry.types.contains_key(name) {
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
        let module_scope = *self
            .registry
            .module_scopes
            .entry(specifier.to_string())
            .or_insert_with(|| self.gctx.scopes.create_child(scope));
        if let Some(block) = &module.body {
            self.collect_block(&block.body, &module_ctx, module_scope, false);
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
            for name in super::block_declared_names(stmts) {
                self.gctx.exports.insert(scope, &name, &name);
            }
        }
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
        // but still gets its own scope so `NS.Member` resolves.
        let ns_scope = match self.gctx.scopes.get(scope).namespace(&name) {
            Some(ns_scope) => ns_scope,
            None => self.create_namespace_scope(&name, parent_ctx, scope, exported),
        };
        self.collect_block(namespace_block_statements(ns), parent_ctx, ns_scope, false);
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
        super::register_type(name, kind, ctx, self.registry, self.gctx, scope, exported);
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

    /// Create a new namespace child scope and register it.
    fn create_namespace_scope(
        &mut self,
        name: &str,
        ctx: &ir::ModuleContext,
        scope: ScopeId,
        exported: bool,
    ) -> ScopeId {
        let ns_scope = self.gctx.scopes.create_child(scope);
        self.gctx
            .scopes
            .insert_namespace(scope, name.to_string(), ns_scope);
        if exported {
            self.gctx.exports.insert(scope, name, name);
        }
        // A same-named class, function or interface stays the name's type.
        if self.gctx.scopes.get(scope).get(name).is_some() {
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
            exported,
        });
        self.gctx.scopes.insert(scope, name.to_string(), ns_type_id);
        self.registry.types.insert(
            name.to_string(),
            ir::TypeInfo {
                kind: ir::RegisteredKind::Namespace,
                primary_context: ctx.clone(),
            },
        );
        ns_scope
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
