//! Phase 2: Fully populate IR declarations using the registry for resolution.
//!
//! Uses `PopulateCtx` to group shared state and avoid long parameter lists.
//! Declaration-level logic is in `populate_declaration`, shared between
//! top-level statements and `export` blocks (Item 21 dedup).

use oxc_ast::ast;

use crate::ir;
use crate::parse::ctx::ParseCtx;
use crate::parse::docs::DocComments;
use crate::parse::merge::{extract_var_members, is_class_constructor_var, var_declarator_name};
use crate::parse::scope::{ScopeArena, ScopeId};
use crate::parse::types::{convert_ts_type_scoped, convert_type_params};
use crate::util::diagnostics::DiagnosticCollector;
use crate::util::naming::to_snake_case;

use super::converters::{
    convert_class_decl, convert_function_decl, convert_interface_decl, convert_numeric_enum,
    convert_string_enum, convert_string_ts_enum, export_default_kind_name,
};

/// Shared context for Phase 2 declaration population.
///
/// Holds the registry + lib name + read-only type arena alongside an
/// owned `ParseCtx` (which itself is the bag of mutable parse state:
/// scopes, diagnostics, used type names, synthesized interfaces).
/// Member converters take `&mut ParseCtx` directly; this struct
/// handles the per-statement orchestration.
struct PopulateCtx<'a, 'docs> {
    registry: &'a ir::TypeRegistry,
    lib_name: Option<&'a str>,
    /// Read-only Phase 1 declarations (for looking up namespace child scopes).
    type_arena: &'a [ir::TypeDeclaration],
    /// Used-type-names set, owned by Phase 2 and lent to `ParseCtx`.
    used_type_names: std::collections::HashSet<String>,
    /// Synthesized-interface sink threaded through member conversion;
    /// the per-statement orchestration drains this between
    /// declarations.
    synth: Vec<ir::InterfaceDecl>,
    /// Borrows of the scope arena, diagnostic collector, and doc
    /// table. Held here so that callers like `populate_statement` can
    /// build a `ParseCtx` reborrow on demand.
    scopes: &'a mut ScopeArena,
    diag: &'a mut DiagnosticCollector,
    docs: &'a DocComments<'docs>,
}

impl<'a, 'docs> PopulateCtx<'a, 'docs> {
    /// Build a fresh `ParseCtx` reborrowing this struct's mutable
    /// fields. Use within a method to thread through to converters.
    fn parse_ctx<'b>(&'b mut self) -> ParseCtx<'b, 'docs> {
        ParseCtx {
            scopes: self.scopes,
            diag: self.diag,
            docs: self.docs,
            used_type_names: &mut self.used_type_names,
            synth: &mut self.synth,
        }
    }

    /// Drain any synthesized interfaces accumulated during a single
    /// declaration's conversion, wrapping each as a [`TypeDeclaration`]
    /// in the same module context as the parent declaration.
    fn drain_synth(
        &mut self,
        module_context: &ir::ModuleContext,
        scope: ScopeId,
    ) -> Vec<ir::TypeDeclaration> {
        self.synth
            .drain(..)
            .map(|iface| ir::TypeDeclaration {
                kind: ir::TypeKind::Interface(iface),
                module_context: module_context.clone(),
                doc: None,
                scope_id: scope,
                exported: true,
            })
            .collect()
    }
}

/// Walk the AST again and fully populate the IR declarations.
#[allow(clippy::too_many_arguments)]
pub fn populate_declarations<'a, 'docs>(
    program: &ast::Program<'_>,
    registry: &'a ir::TypeRegistry,
    lib_name: Option<&'a str>,
    docs: &'a DocComments<'docs>,
    diag: &'a mut DiagnosticCollector,
    scopes: &'a mut ScopeArena,
    type_arena: &'a [ir::TypeDeclaration],
    scope: ScopeId,
) -> Vec<ir::TypeDeclaration> {
    let mut declarations = Vec::new();
    let used_type_names: std::collections::HashSet<String> =
        registry.types.keys().cloned().collect();
    let mut pcx = PopulateCtx {
        registry,
        lib_name,
        type_arena,
        used_type_names,
        synth: Vec::new(),
        scopes,
        diag,
        docs,
    };

    for stmt in &program.body {
        pcx.populate_statement(
            stmt,
            &ir::ModuleContext::Global,
            &mut declarations,
            scope,
            false,
        );
    }

    declarations
}

/// Per-declaration context passed to `populate_declaration`.
struct DeclCtx<'a> {
    module_context: &'a ir::ModuleContext,
    export_span_start: Option<u32>,
    scope: ScopeId,
    exported: bool,
}

impl<'a> DeclCtx<'a> {
    /// Build a `TypeDeclaration` with this context's module/scope/exported fields.
    fn decl(&self, kind: ir::TypeKind, doc: Option<String>) -> ir::TypeDeclaration {
        ir::TypeDeclaration {
            kind,
            module_context: self.module_context.clone(),
            doc,
            scope_id: self.scope,
            exported: self.exported,
        }
    }
}

impl<'a, 'docs> PopulateCtx<'a, 'docs> {
    /// Populate from a top-level statement.
    fn populate_statement(
        &mut self,
        stmt: &ast::Statement<'_>,
        ctx: &ir::ModuleContext,
        declarations: &mut Vec<ir::TypeDeclaration>,
        scope: ScopeId,
        exported: bool,
    ) {
        let dcx = DeclCtx {
            module_context: ctx,
            export_span_start: None,
            scope,
            exported,
        };

        match stmt {
            // Declaration variants — delegate to shared handler
            ast::Statement::ClassDeclaration(class) => {
                self.populate_class(class, &dcx, declarations);
            }
            ast::Statement::TSInterfaceDeclaration(iface) => {
                self.populate_interface(iface, &dcx, declarations);
            }
            ast::Statement::TSTypeAliasDeclaration(alias) => {
                self.populate_type_alias(alias, &dcx, declarations);
            }
            ast::Statement::FunctionDeclaration(func) => {
                self.populate_function(func, &dcx, declarations);
            }
            ast::Statement::VariableDeclaration(var_decl) => {
                self.populate_variable_decl(var_decl, &dcx, declarations);
            }
            ast::Statement::TSModuleDeclaration(module) => {
                self.populate_module(module, ctx, declarations, scope, exported);
            }
            ast::Statement::TSEnumDeclaration(enum_decl) => {
                self.populate_ts_enum(enum_decl, &dcx, declarations);
            }
            ast::Statement::TSGlobalDeclaration(global) => {
                for s in &global.body.body {
                    self.populate_statement(
                        s,
                        &ir::ModuleContext::Global,
                        declarations,
                        scope,
                        true,
                    );
                }
            }

            // ModuleDeclaration variants
            ast::Statement::ExportNamedDeclaration(export) => {
                if let Some(ref decl) = export.declaration {
                    let export_ctx = if let Some(lib) = self.lib_name {
                        ir::ModuleContext::Module(lib.into())
                    } else {
                        ctx.clone()
                    };
                    self.populate_oxc_declaration(
                        decl,
                        &export_ctx,
                        declarations,
                        Some(export.span.start),
                        scope,
                    );
                }
                for spec in &export.specifiers {
                    let exported_name = spec.exported.name().to_string();
                    let local = spec.local.name().to_string();
                    if exported_name == local && export.source.is_none() {
                        continue;
                    }
                    // If `local` was promoted from a constructor-shaped variable to a
                    // class published under `exported_name` (recorded in phase 1's
                    // `export_renames`), the class itself already carries the public
                    // name — emitting an alias here would be self-referential.
                    if export.source.is_none()
                        && self
                            .registry
                            .export_renames
                            .get(&local)
                            .is_some_and(|renamed| renamed == &exported_name)
                        && matches!(
                            self.registry.types.get(&local).map(|info| &info.kind),
                            Some(ir::RegisteredKind::Variable)
                        )
                    {
                        continue;
                    }
                    let from_module = export.source.as_ref().map(|s| s.value.to_string());
                    declarations.push(ir::TypeDeclaration {
                        kind: ir::TypeKind::TypeAlias(ir::TypeAliasDecl {
                            name: exported_name,
                            type_params: vec![],
                            target: ir::TypeRef::ident(local),
                            from_module,
                            // Re-export aliases have no body — pin to the
                            // enclosing scope.
                            body_scope: scope,
                        }),
                        module_context: ctx.clone(),
                        doc: None,
                        scope_id: scope,
                        exported: true,
                    });
                }
            }
            ast::Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    let doc = self
                        .docs
                        .for_span(export.span.start)
                        .or_else(|| self.docs.for_span(class.span.start));
                    let class_decl = convert_class_decl(class, ctx, scope, &mut self.parse_ctx());
                    if let Some(decl) = class_decl {
                        declarations.push(ir::TypeDeclaration {
                            kind: ir::TypeKind::Class(decl),
                            module_context: ctx.clone(),
                            doc,
                            scope_id: scope,
                            exported: true,
                        });
                        declarations.extend(self.drain_synth(ctx, scope));
                    }
                }
                ast::ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    // Look up structured info on either the export span or the
                    // function span (JSDoc may be attached to either).
                    let info = self
                        .docs
                        .info_for_span(export.span.start)
                        .or_else(|| self.docs.info_for_span(func.span.start));
                    let (doc, throws) = match info {
                        Some((d, i)) => (Some(d), i.throws()),
                        None => (None, ir::Throws::None),
                    };
                    if let Some(decl) =
                        convert_function_decl(func, throws, scope, &mut self.parse_ctx())
                    {
                        declarations.push(ir::TypeDeclaration {
                            kind: ir::TypeKind::Function(decl),
                            module_context: ctx.clone(),
                            doc,
                            scope_id: scope,
                            exported: true,
                        });
                    }
                }
                other => {
                    self.diag.warn(format!(
                        "Unsupported export default declaration kind: {}",
                        export_default_kind_name(other)
                    ));
                }
            },

            ast::Statement::TSExportAssignment(_) => {
                self.diag
                    .info("Skipping `export =` (namespace contents already emitted)");
            }
            ast::Statement::TSNamespaceExportDeclaration(decl) => {
                self.diag.info(format!(
                    "Skipping `export as namespace {}` (UMD namespace export)",
                    decl.id.name
                ));
            }
            ast::Statement::ExportAllDeclaration(decl) => {
                let source = decl.source.value.as_str();
                self.diag.warn(format!(
                    "Re-export (`export * from \"{source}\"`) is not yet supported, skipping"
                ));
            }
            ast::Statement::ImportDeclaration(_) => {}
            ast::Statement::TSImportEqualsDeclaration(_) => {}
            _ => {}
        }
    }

    /// Populate from an `ast::Declaration` inside an export block.
    /// Dispatches to the same per-type methods as `populate_statement`.
    fn populate_oxc_declaration(
        &mut self,
        decl: &ast::Declaration<'_>,
        ctx: &ir::ModuleContext,
        declarations: &mut Vec<ir::TypeDeclaration>,
        export_span_start: Option<u32>,
        scope: ScopeId,
    ) {
        let dcx = DeclCtx {
            module_context: ctx,
            export_span_start,
            scope,
            exported: true,
        };

        match decl {
            ast::Declaration::ClassDeclaration(class) => {
                self.populate_class(class, &dcx, declarations);
            }
            ast::Declaration::TSInterfaceDeclaration(iface) => {
                self.populate_interface(iface, &dcx, declarations);
            }
            ast::Declaration::TSTypeAliasDeclaration(alias) => {
                self.populate_type_alias(alias, &dcx, declarations);
            }
            ast::Declaration::FunctionDeclaration(func) => {
                self.populate_function(func, &dcx, declarations);
            }
            ast::Declaration::VariableDeclaration(var_decl) => {
                self.populate_variable_decl(var_decl, &dcx, declarations);
            }
            ast::Declaration::TSModuleDeclaration(module) => {
                self.populate_module(module, ctx, declarations, scope, true);
            }
            ast::Declaration::TSEnumDeclaration(enum_decl) => {
                self.populate_ts_enum(enum_decl, &dcx, declarations);
            }
            ast::Declaration::TSGlobalDeclaration(global) => {
                for s in &global.body.body {
                    self.populate_statement(
                        s,
                        &ir::ModuleContext::Global,
                        declarations,
                        scope,
                        true,
                    );
                }
            }
            ast::Declaration::TSImportEqualsDeclaration(_) => {}
        }
    }

    // ─── Per-type populate methods (shared between statement & declaration) ───

    fn populate_class(
        &mut self,
        class: &ast::Class<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let doc = self.lookup_doc(dcx.export_span_start, class.span.start);
        let module_context = dcx.module_context.clone();
        let scope = dcx.scope;
        let class_decl = convert_class_decl(class, &module_context, scope, &mut self.parse_ctx());
        if let Some(d) = class_decl {
            declarations.push(dcx.decl(ir::TypeKind::Class(d), doc));
            declarations.extend(self.drain_synth(&module_context, scope));
        }
    }

    fn populate_interface(
        &mut self,
        iface: &ast::TSInterfaceDeclaration<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let doc = self.lookup_doc(dcx.export_span_start, iface.span.start);
        let module_context = dcx.module_context.clone();
        let scope = dcx.scope;
        let iface_decl = convert_interface_decl(iface, scope, &mut self.parse_ctx());
        declarations.push(dcx.decl(ir::TypeKind::Interface(iface_decl), doc));
        declarations.extend(self.drain_synth(&module_context, scope));
    }

    fn populate_type_alias(
        &mut self,
        alias: &ast::TSTypeAliasDeclaration<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let name = alias.id.name.to_string();
        let doc = self.lookup_doc(dcx.export_span_start, alias.span.start);

        if self
            .registry
            .types
            .get(&name)
            .map(|info| info.kind == ir::RegisteredKind::StringEnum)
            .unwrap_or(false)
        {
            if let Some(enum_decl) = convert_string_enum(&name, &alias.type_annotation) {
                declarations.push(dcx.decl(ir::TypeKind::StringEnum(enum_decl), doc));
                return;
            }
        }

        // `type Foo = { ... }` / `type Foo = { ... } | { ... }` are
        // structurally interfaces (or discriminated unions, if the
        // branches share a string-literal discriminator) — promote them
        // so consumers get the dictionary-builder / variant-factory
        // treatment instead of an opaque type alias to `Object`. The
        // alias's own name becomes the synthesized type.
        if let Some(kind) = self.try_synthesize_alias_decl(&name, &alias.type_annotation, dcx.scope)
        {
            declarations.push(dcx.decl(kind, doc));
            // Hoisted nested interfaces accumulated in `self.synth` —
            // drain them alongside the promoted alias declaration.
            let module_context = dcx.module_context.clone();
            let scope = dcx.scope;
            declarations.extend(self.drain_synth(&module_context, scope));
            return;
        }

        let type_params = convert_type_params(alias.type_parameters.as_ref(), self.diag);
        // The alias's body scope holds its own type parameters so the
        // target type can resolve them lexically.
        let body_scope = crate::parse::members::create_body_scope(
            &type_params,
            dcx.scope,
            &mut self.parse_ctx(),
        );
        let target =
            convert_ts_type_scoped(&alias.type_annotation, body_scope, &mut self.parse_ctx());
        declarations.push(dcx.decl(
            ir::TypeKind::TypeAlias(ir::TypeAliasDecl {
                name,
                type_params,
                target,
                from_module: None,
                body_scope,
            }),
            doc,
        ));
    }

    /// Promote `type Foo = { ... }` or `type Foo = { ... } | { ... }`
    /// into either a named `InterfaceDecl` or a `DiscriminatedUnionDecl`,
    /// going through the same merge rules as anonymous-parameter union
    /// synthesis. Returns `None` if the target shape matches neither
    /// form.
    ///
    /// Discriminated-union promotion happens when every branch is a
    /// type literal AND there's a shared required string-literal-typed
    /// property — see [`literal_union::detect_discriminators`].
    /// Otherwise the merge produces a plain `InterfaceDecl` as before.
    fn try_synthesize_alias_decl(
        &mut self,
        alias_name: &str,
        target: &ast::TSType<'_>,
        parent_scope: ScopeId,
    ) -> Option<ir::TypeKind> {
        match target {
            ast::TSType::TSTypeLiteral(literal) => {
                let iface = crate::parse::first_pass::converters::interface_from_signatures(
                    alias_name.to_string(),
                    Vec::new(),
                    Vec::new(),
                    &literal.members,
                    parent_scope,
                    &mut self.parse_ctx(),
                );
                self.used_type_names.insert(alias_name.to_string());
                // Any nested anonymous interfaces hoisted from member
                // params during `interface_from_signatures` accumulate
                // in `self.synth` — the caller drains them alongside
                // the alias-promoted interface.
                Some(ir::TypeKind::Interface(iface))
            }
            ast::TSType::TSUnionType(union)
                if union
                    .types
                    .iter()
                    .all(|t| matches!(t, ast::TSType::TSTypeLiteral(_))) =>
            {
                // Hoisted alias-union has no surface type parameters,
                // so the body scope is the parent scope directly.
                let body_scope = parent_scope;
                let alias_owned = alias_name.to_string();
                let branches: Vec<Vec<ir::Member>> = union
                    .types
                    .iter()
                    .filter_map(|t| match t {
                        ast::TSType::TSTypeLiteral(lit) => Some(
                            lit.members
                                .iter()
                                .flat_map(|sig| {
                                    crate::parse::members::convert_ts_signature(
                                        sig,
                                        Some(&alias_owned),
                                        body_scope,
                                        &mut self.parse_ctx(),
                                    )
                                })
                                .collect(),
                        ),
                        _ => None,
                    })
                    .collect();
                let merged = crate::parse::literal_union::merge_member_branches(&branches);
                self.used_type_names.insert(alias_name.to_string());

                let discriminators = crate::parse::literal_union::detect_discriminators(&branches);
                if !discriminators.is_empty() {
                    Some(ir::TypeKind::DiscriminatedUnion(
                        ir::DiscriminatedUnionDecl {
                            name: alias_name.to_string(),
                            js_name: alias_name.to_string(),
                            type_params: Vec::new(),
                            branches,
                            members: merged,
                            discriminators,
                            body_scope,
                        },
                    ))
                } else {
                    let classification = crate::parse::classify::classify_interface(&merged);
                    Some(ir::TypeKind::Interface(ir::InterfaceDecl {
                        name: alias_name.to_string(),
                        js_name: alias_name.to_string(),
                        type_params: Vec::new(),
                        extends: Vec::new(),
                        members: merged,
                        classification,
                        body_scope,
                    }))
                }
            }
            _ => None,
        }
    }

    fn populate_function(
        &mut self,
        func: &ast::Function<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let (doc, throws) = self.lookup_callable_doc(dcx.export_span_start, func.span.start);
        if let Some(d) = convert_function_decl(func, throws, dcx.scope, &mut self.parse_ctx()) {
            declarations.push(dcx.decl(ir::TypeKind::Function(d), doc));
        }
    }

    fn populate_variable_decl(
        &mut self,
        var_decl: &ast::VariableDeclaration<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let doc = self.lookup_doc(dcx.export_span_start, var_decl.span.start);
        for declarator in &var_decl.declarations {
            if let Some(name) = var_declarator_name(declarator) {
                let merged_with_iface = self
                    .registry
                    .types
                    .get(&name)
                    .map(|info| info.kind == ir::RegisteredKind::MergedClassLike)
                    .unwrap_or(false);

                // Promote `let X: { new(...): T }` to a class when either:
                //   (a) it merged with an interface (script `var + interface` pattern), or
                //   (b) it lives in a module context and has constructor shape — the
                //       `declare module "cloudflare:email" { let _X: { new(...): T }; }`
                //       pattern, where the class is exported (often via rename).
                let is_module_ctor_var = matches!(dcx.module_context, ir::ModuleContext::Module(_))
                    && is_class_constructor_var(declarator);

                if merged_with_iface || is_module_ctor_var {
                    if let Some(type_ann) = &declarator.type_annotation {
                        if let ast::TSType::TSTypeLiteral(lit) = &type_ann.type_annotation {
                            let (ctor, static_members) =
                                extract_var_members(lit, dcx.scope, &mut self.parse_ctx());

                            let mut members = Vec::new();
                            if let Some(c) = ctor {
                                members.push(ir::Member::Constructor(c));
                            }
                            members.extend(static_members);

                            // Resolve the public-facing class name. For module-scoped
                            // constructor vars, prefer the export rename (the JS name
                            // consumers see) when one was recorded in phase 1.
                            let public_name = self
                                .registry
                                .export_renames
                                .get(&name)
                                .cloned()
                                .unwrap_or_else(|| name.clone());

                            // Var-promoted classes have no surface
                            // type parameters; body scope = enclosing.
                            let body_scope = dcx.scope;
                            declarations.push(dcx.decl(
                                ir::TypeKind::Class(ir::ClassDecl {
                                    name: public_name.clone(),
                                    js_name: public_name,
                                    type_params: vec![],
                                    extends: None,
                                    implements: vec![],
                                    is_abstract: false,
                                    members,
                                    type_module_context: dcx.module_context.clone(),
                                    body_scope,
                                }),
                                doc.clone(),
                            ));
                            continue;
                        }
                    }
                }

                let type_ref = declarator
                    .type_annotation
                    .as_ref()
                    .map(|ann| {
                        convert_ts_type_scoped(
                            &ann.type_annotation,
                            dcx.scope,
                            &mut self.parse_ctx(),
                        )
                    })
                    .unwrap_or(ir::TypeRef::Any);

                if let ir::TypeRef::Function(sig) = type_ref {
                    // Var-as-function bindings have no surface type
                    // parameters; their body scope is the enclosing.
                    let body_scope = dcx.scope;
                    declarations.push(dcx.decl(
                        ir::TypeKind::Function(ir::FunctionDecl {
                            name: to_snake_case(&name),
                            js_name: name,
                            type_params: vec![],
                            params: sig.params,
                            return_type: *sig.return_type,
                            overloads: vec![],
                            // Variable-as-function form (`var foo: () => T`) doesn't
                            // typically carry @throws JSDoc; leave empty.
                            throws: ir::Throws::None,
                            body_scope,
                        }),
                        doc.clone(),
                    ));
                } else {
                    let is_const = matches!(var_decl.kind, ast::VariableDeclarationKind::Const);
                    declarations.push(dcx.decl(
                        ir::TypeKind::Variable(ir::VariableDecl {
                            name: to_snake_case(&name),
                            js_name: name,
                            type_ref,
                            is_const,
                        }),
                        doc.clone(),
                    ));
                }
            }
        }
    }

    fn populate_module(
        &mut self,
        module: &ast::TSModuleDeclaration<'_>,
        parent_ctx: &ir::ModuleContext,
        declarations: &mut Vec<ir::TypeDeclaration>,
        scope: ScopeId,
        exported: bool,
    ) {
        let doc = self.docs.for_span(module.span.start);
        match &module.id {
            ast::TSModuleDeclarationName::StringLiteral(s) => {
                let module_ctx = ir::ModuleContext::Module(s.value.as_str().into());
                if let Some(ast::TSModuleDeclarationBody::TSModuleBlock(block)) = &module.body {
                    for stmt in &block.body {
                        self.populate_statement(stmt, &module_ctx, declarations, scope, false);
                    }
                }
            }
            ast::TSModuleDeclarationName::Identifier(id) => {
                let ns_name = id.name.to_string();
                let is_inside_module = matches!(parent_ctx, ir::ModuleContext::Module(_));
                // Use the namespace's child scope from Phase 1 (fixes Item 10).
                let ns_scope = self.resolve_namespace_scope(&ns_name, scope);

                if is_inside_module {
                    // Flatten: emit declarations directly into the parent module.
                    if let Some(ast::TSModuleDeclarationBody::TSModuleBlock(block)) = &module.body {
                        for stmt in &block.body {
                            self.populate_statement(
                                stmt,
                                parent_ctx,
                                declarations,
                                ns_scope,
                                exported,
                            );
                        }
                    }
                } else {
                    let mut ns_decls = Vec::new();
                    if let Some(ast::TSModuleDeclarationBody::TSModuleBlock(block)) = &module.body {
                        for stmt in &block.body {
                            self.populate_statement(
                                stmt,
                                &ir::ModuleContext::Global,
                                &mut ns_decls,
                                ns_scope,
                                exported,
                            );
                        }
                    }
                    declarations.push(ir::TypeDeclaration {
                        kind: ir::TypeKind::Namespace(ir::NamespaceDecl {
                            name: ns_name,
                            declarations: ns_decls,
                            child_scope: ns_scope,
                        }),
                        module_context: ir::ModuleContext::Global,
                        doc,
                        scope_id: scope,
                        exported,
                    });
                }
            }
        }
    }

    fn populate_ts_enum(
        &mut self,
        enum_decl: &ast::TSEnumDeclaration<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let name = enum_decl.id.name.to_string();
        let doc = self.docs.for_span(enum_decl.span.start);

        let is_numeric = self
            .registry
            .types
            .get(&name)
            .map(|info| info.kind == ir::RegisteredKind::NumericEnum)
            .unwrap_or(false);

        if is_numeric {
            let decl = convert_numeric_enum(enum_decl, self.docs, self.diag);
            declarations.push(dcx.decl(ir::TypeKind::NumericEnum(decl), doc));
        } else {
            let decl = convert_string_ts_enum(enum_decl);
            declarations.push(dcx.decl(ir::TypeKind::StringEnum(decl), doc));
        }
    }

    // ─── Helpers ─────────────────────────────────────────────────────

    fn lookup_doc(&self, export_span_start: Option<u32>, inner_span_start: u32) -> Option<String> {
        export_span_start
            .and_then(|s| self.docs.for_span(s))
            .or_else(|| self.docs.for_span(inner_span_start))
    }

    /// Same fallback behavior as [`lookup_doc`], but also returns the
    /// `@throws` type (already lifted into a `TypeRef`) from the same
    /// JSDoc block. Used by callable declarations that need both the
    /// rendered doc and structured throws info.
    ///
    /// [`lookup_doc`]: Self::lookup_doc
    fn lookup_callable_doc(
        &self,
        export_span_start: Option<u32>,
        inner_span_start: u32,
    ) -> (Option<String>, ir::Throws) {
        let info = export_span_start
            .and_then(|s| self.docs.info_for_span(s))
            .or_else(|| self.docs.info_for_span(inner_span_start));
        match info {
            Some((doc, info)) => (Some(doc), info.throws()),
            None => (None, ir::Throws::None),
        }
    }

    /// Look up the child scope that Phase 1 created for a namespace.
    /// Falls back to the parent scope if not found.
    fn resolve_namespace_scope(&self, ns_name: &str, parent_scope: ScopeId) -> ScopeId {
        if let Some(type_id) = self.scopes.resolve(parent_scope, ns_name) {
            let decl = &self.type_arena[type_id.index()];
            if let ir::TypeKind::Namespace(ref ns) = decl.kind {
                return ns.child_scope;
            }
        }
        parent_scope
    }
}
