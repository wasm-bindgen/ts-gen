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
use crate::util::naming::{to_snake_case, LocalScope};

use super::converters::{
    convert_class_decl, convert_function_decl, convert_interface_decl, convert_string_enum,
    convert_ts_enum, export_default_kind_name,
};
use super::namespace_block_statements;

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
    used_type_names: LocalScope,
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
    file_scope: ScopeId,
    namespace_members: NamespaceMembers,
    /// The ambient module scope being populated and the name its
    /// `export =` assigns, if any.
    export_assignment: Option<(ScopeId, String)>,
    /// Whether declarations in the global module context are in the global
    /// scope: in a script file, or in a `declare global` block.
    global_scope: bool,
    static_sides: StaticSides,
    /// The classes of constructor-shaped `var`s with no instance interface,
    /// by scope and name.
    constructor_classes: Vec<(ScopeId, String)>,
}

/// The static side of each constructor-shaped `var` with no instance
/// interface in its file, as a class without the construct signature's
/// instance, by the var's scope and name. Another file may declare the
/// interface, which then merges with it.
pub type StaticSides = std::collections::HashMap<(ScopeId, String), ir::TypeDeclaration>;

/// Populated members of each global namespace, keyed by its member scope.
/// They are inserted into the type arena after population, when the
/// namespace takes their ids; every block of a merged namespace shares the
/// one scope, so their members land together.
pub type NamespaceMembers = std::collections::HashMap<ScopeId, Vec<ir::TypeDeclaration>>;

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
            type_arena: self.type_arena,
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
) -> Populated {
    let mut declarations = Vec::new();
    let file_ctx = super::file_context(program, lib_name);
    let used_type_names =
        LocalScope::with_claimed(registry.types.keys().map(|(_, name)| name.clone()));
    let mut pcx = PopulateCtx {
        registry,
        lib_name,
        type_arena,
        used_type_names,
        synth: Vec::new(),
        scopes,
        diag,
        docs,
        file_scope: scope,
        namespace_members: NamespaceMembers::new(),
        export_assignment: None,
        global_scope: !matches!(file_ctx, ir::ModuleContext::Module(_)),
        static_sides: StaticSides::new(),
        constructor_classes: Vec::new(),
    };

    if let ir::ModuleContext::Module(_) = file_ctx {
        pcx.export_assignment = assigned_name(&program.body).map(|name| (scope, name));
    }
    for stmt in &program.body {
        pcx.populate_statement(stmt, &file_ctx, &mut declarations, scope);
    }

    Populated {
        declarations,
        namespace_members: pcx.namespace_members,
        static_sides: pcx.static_sides,
        constructor_classes: pcx.constructor_classes,
    }
}

/// What population yields for a file.
pub struct Populated {
    pub declarations: Vec<ir::TypeDeclaration>,
    pub namespace_members: NamespaceMembers,
    pub static_sides: StaticSides,
    /// The classes of constructor-shaped `var`s with no instance interface,
    /// by scope and name: a value nothing refers to by its local name, so
    /// such a class takes the name its module exports it under.
    pub constructor_classes: Vec<(ScopeId, String)>,
}

/// The binding a module body's `export = <name>` assigns.
fn assigned_name(body: &[ast::Statement<'_>]) -> Option<String> {
    body.iter().find_map(|stmt| match stmt {
        ast::Statement::TSExportAssignment(assign) => match &assign.expression {
            ast::Expression::Identifier(ident) => Some(ident.name.to_string()),
            _ => None,
        },
        _ => None,
    })
}

/// Per-declaration context passed to `populate_declaration`.
struct DeclCtx<'a> {
    module_context: &'a ir::ModuleContext,
    export_span_start: Option<u32>,
    scope: ScopeId,
}

impl<'a> DeclCtx<'a> {
    /// Build a `TypeDeclaration` with this context's module and scope.
    fn decl(&self, kind: ir::TypeKind, doc: Option<String>) -> ir::TypeDeclaration {
        ir::TypeDeclaration {
            kind,
            module_context: self.module_context.clone(),
            doc,
            scope_id: self.scope,
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
    ) {
        let dcx = DeclCtx {
            module_context: ctx,
            export_span_start: None,
            scope,
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
            ast::Statement::TSExternalModuleDeclaration(module) => {
                self.populate_external_module(module, declarations);
            }
            ast::Statement::TSNamespaceDeclaration(ns) => {
                self.populate_namespace(ns, ctx, declarations, scope);
            }
            ast::Statement::TSEnumDeclaration(enum_decl) => {
                self.populate_ts_enum(enum_decl, &dcx, declarations);
            }
            ast::Statement::TSGlobalDeclaration(global) => {
                let global_scope = std::mem::replace(&mut self.global_scope, true);
                for s in &global.body.body {
                    self.populate_statement(
                        s,
                        &ir::ModuleContext::Global,
                        declarations,
                        self.file_scope,
                    );
                }
                self.global_scope = global_scope;
            }

            // ModuleDeclaration variants
            ast::Statement::ExportDeclaration(export) => {
                // `--lib-name` names the file's own module; an export inside
                // a `declare module` body belongs to that module.
                let export_ctx = match (ctx, self.lib_name) {
                    (ir::ModuleContext::Global, Some(lib)) => ir::ModuleContext::Module(lib.into()),
                    _ => ctx.clone(),
                };
                self.populate_oxc_declaration(
                    &export.declaration,
                    &export_ctx,
                    declarations,
                    Some(export.span.start),
                    scope,
                );
            }
            ast::Statement::ExportFromDeclaration(export) => {
                for spec in &export.specifiers {
                    self.push_export_alias(
                        spec,
                        Some(export.source.value.to_string()),
                        ctx,
                        declarations,
                        scope,
                    );
                }
            }
            ast::Statement::ExportNamedDeclaration(export) => {
                for spec in &export.specifiers {
                    let exported_name = spec.exported.name().to_string();
                    let local = spec.local.name().to_string();
                    // The output emits a binding the block declares under each
                    // public name; one from outside it, such as an import, is
                    // re-exported through an alias.
                    if exported_name == local || self.scopes.get(scope).get(&local).is_some() {
                        continue;
                    }
                    self.push_export_alias(spec, None, ctx, declarations, scope);
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
                    let (doc, jsinfo) = match info {
                        Some((d, i)) => (Some(d), i),
                        None => (None, Default::default()),
                    };
                    if let Some(mut decl) =
                        convert_function_decl(func, jsinfo.throws(), scope, &mut self.parse_ctx())
                    {
                        crate::parse::members::apply_slice_hints(
                            &mut decl.params,
                            &jsinfo,
                            &decl.js_name,
                            self.diag,
                        );
                        declarations.push(ir::TypeDeclaration {
                            kind: ir::TypeKind::Function(decl),
                            module_context: ctx.clone(),
                            doc,
                            scope_id: scope,
                        });
                    }
                }
                // Bound by its declaration, which takes `default` from the
                // export map.
                ast::ExportDefaultDeclarationKind::Identifier(_) => {}
                other => {
                    self.diag.warn(format!(
                        "Unsupported export default declaration kind: {}",
                        export_default_kind_name(other)
                    ));
                }
            },

            // The assigned binding is bound as the default export, and an
            // assigned namespace is emitted flat by `populate_namespace`.
            ast::Statement::TSExportAssignment(assign) => {
                let declared = match &assign.expression {
                    ast::Expression::Identifier(ident) => {
                        matches!(ctx, ir::ModuleContext::Module(_))
                            && (self.scopes.get(scope).get(&ident.name).is_some()
                                || self
                                    .scopes
                                    .resolve_namespace(scope, &ident.name)
                                    .and_then(|ns| self.registry.assigned_namespaces.get(&ns))
                                    .is_some_and(|assigned| {
                                        matches!(ctx, ir::ModuleContext::Module(m)
                                            if **m == **assigned)
                                    }))
                    }
                    _ => false,
                };
                if !declared {
                    self.diag.info(
                        "Skipping `export =` (only a binding declared in the same \
                         module is supported)",
                    );
                }
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
            ast::Declaration::TSExternalModuleDeclaration(module) => {
                self.populate_external_module(module, declarations);
            }
            ast::Declaration::TSNamespaceDeclaration(ns) => {
                self.populate_namespace(ns, ctx, declarations, scope);
            }
            ast::Declaration::TSEnumDeclaration(enum_decl) => {
                self.populate_ts_enum(enum_decl, &dcx, declarations);
            }
            ast::Declaration::TSGlobalDeclaration(global) => {
                let global_scope = std::mem::replace(&mut self.global_scope, true);
                for s in &global.body.body {
                    self.populate_statement(
                        s,
                        &ir::ModuleContext::Global,
                        declarations,
                        self.file_scope,
                    );
                }
                self.global_scope = global_scope;
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

        if self.registry.kind(dcx.scope, &name) == Some(&ir::RegisteredKind::StringEnum) {
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
        if let Some(kind) = self.try_synthesize_alias_decl(alias, dcx.scope) {
            declarations.push(dcx.decl(kind, doc));
            // Hoisted nested interfaces accumulated in `self.synth` —
            // drain them alongside the promoted alias declaration.
            let module_context = dcx.module_context.clone();
            let scope = dcx.scope;
            declarations.extend(self.drain_synth(&module_context, scope));
            return;
        }

        let mut type_params = convert_type_params(alias.type_parameters.as_ref(), self.diag);
        // The alias's body scope holds its own type parameters so the
        // target type can resolve them lexically.
        let body_scope = crate::parse::members::create_body_scope(
            &mut type_params,
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

    /// Promote `type Foo = { ... }`, `type Foo = { ... } | { ... }` or
    /// `type Foo = A & { ... }` into a named `InterfaceDecl` (or a
    /// `DiscriminatedUnionDecl` for a union whose branches share a
    /// required literal-typed property — see
    /// [`literal_union::detect_discriminators`]), going through the same
    /// [`synthesize_inline`] as anonymous-parameter hoisting. Returns
    /// `None` if the target shape matches none of these forms.
    ///
    /// [`synthesize_inline`]: crate::parse::members::synthesize_inline
    fn try_synthesize_alias_decl(
        &mut self,
        alias: &ast::TSTypeAliasDeclaration<'_>,
        parent_scope: ScopeId,
    ) -> Option<ir::TypeKind> {
        let shape = crate::parse::members::inline_shape(&alias.type_annotation)?;
        let name = alias.id.name.to_string();
        let type_params = convert_type_params(alias.type_parameters.as_ref(), self.diag);
        let synth = crate::parse::members::synthesize_inline(
            &shape,
            name.clone(),
            type_params,
            parent_scope,
            &mut self.parse_ctx(),
        )?;
        if matches!(shape, crate::parse::members::InlineShape::Union(_)) {
            let discriminators =
                crate::parse::literal_union::detect_discriminators(&synth.branches);
            if !discriminators.is_empty() {
                return Some(ir::TypeKind::DiscriminatedUnion(
                    ir::DiscriminatedUnionDecl {
                        name: name.clone(),
                        js_name: name,
                        type_params: synth.iface.type_params,
                        branches: synth.branches,
                        members: synth.iface.members,
                        discriminators,
                        body_scope: synth.iface.body_scope,
                    },
                ));
            }
        }
        Some(ir::TypeKind::Interface(synth.iface))
    }

    fn populate_function(
        &mut self,
        func: &ast::Function<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let (doc, info) = self.lookup_callable_doc(dcx.export_span_start, func.span.start);
        if let Some(mut d) =
            convert_function_decl(func, info.throws(), dcx.scope, &mut self.parse_ctx())
        {
            crate::parse::members::apply_slice_hints(&mut d.params, &info, &d.js_name, self.diag);
            declarations.push(dcx.decl(ir::TypeKind::Function(d), doc));
        }
    }

    /// Emit the class a `declare var X: { new(...) }` / `declare var X:
    /// XConstructor` static side describes. The instance interface `X`,
    /// if any, is folded in by `merge_class_pairs`.
    fn push_static_side_class(
        &mut self,
        name: &str,
        static_side: &[ast::TSSignature<'_>],
        dcx: &DeclCtx<'_>,
        doc: Option<String>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let mut class = self.static_side_class(name, static_side, dcx, doc);
        // With an instance interface, the interface's members refer to the
        // local name and `merge_class_pairs` pairs the two by it. A var
        // alone is a value nothing refers to by its local name.
        let has_instance_interface =
            self.registry.kind(dcx.scope, name) == Some(&ir::RegisteredKind::MergedClassLike);
        if !has_instance_interface {
            // An instance interface another file declares replaces the
            // construct signature's instance, as it would within one file.
            self.static_sides
                .insert((dcx.scope, name.to_string()), class.clone());
            self.add_construct_instance(&mut class, name, static_side, dcx);
            self.constructor_classes.push((dcx.scope, name.to_string()));
        }
        declarations.push(class);
    }

    /// Nothing else describes the instance of `class`, so the construct
    /// signature's return type does: an inline literal supplies the
    /// members, a named type the parent.
    fn add_construct_instance(
        &mut self,
        class: &mut ir::TypeDeclaration,
        name: &str,
        static_side: &[ast::TSSignature<'_>],
        dcx: &DeclCtx<'_>,
    ) {
        let ir::TypeKind::Class(c) = &mut class.kind else {
            return;
        };
        let instance = static_side.iter().find_map(|m| match m {
            ast::TSSignature::TSConstructSignatureDeclaration(c) => c.return_type.as_ref(),
            _ => None,
        });
        match instance.map(|rt| &rt.type_annotation) {
            Some(ast::TSType::TSTypeLiteral(instance_lit)) => {
                for sig in &instance_lit.members {
                    c.members
                        .extend(crate::parse::members::convert_ts_signature(
                            sig,
                            Some(name),
                            dcx.scope,
                            &mut self.parse_ctx(),
                        ));
                }
            }
            Some(other @ ast::TSType::TSTypeReference(_)) => {
                let parent = convert_ts_type_scoped(other, dcx.scope, &mut self.parse_ctx());
                if parent.as_ident() != Some(name) {
                    c.extends = Some(ir::Heritage {
                        type_ref: parent,
                        scope: dcx.scope,
                    });
                }
            }
            _ => {}
        }
    }

    /// The class a static side describes.
    fn static_side_class(
        &mut self,
        name: &str,
        members: &[ast::TSSignature<'_>],
        dcx: &DeclCtx<'_>,
        doc: Option<String>,
    ) -> ir::TypeDeclaration {
        let (ctor, static_members) = extract_var_members(members, dcx.scope, &mut self.parse_ctx());

        let mut members: Vec<ir::Member> = ctor.into_iter().map(ir::Member::Constructor).collect();
        members.extend(static_members);

        // Var-promoted classes have no surface
        // type parameters; body scope = enclosing.
        dcx.decl(
            ir::TypeKind::Class(ir::ClassDecl {
                name: name.to_string(),
                js_name: name.to_string(),
                type_params: vec![],
                extends: None,
                implements: vec![],
                is_abstract: false,
                members,
                type_module_context: dcx.module_context.clone(),
                body_scope: dcx.scope,
            }),
            doc,
        )
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
                let merged_with_iface = self.registry.kind(dcx.scope, &name)
                    == Some(&ir::RegisteredKind::MergedClassLike);
                let alias_ctor = self
                    .registry
                    .alias_constructors
                    .contains(&(dcx.scope, name.clone()));
                let constructs = if alias_ctor {
                    self.alias_constructs(declarator, dcx.scope)
                } else {
                    Vec::new()
                };

                // Promote `let X: { new(...): T }` to a class when either:
                //   (a) it merged with an interface (script `var + interface` pattern), or
                //   (b) it lives in a module context and has constructor shape — the
                //       `declare module "cloudflare:email" { let _X: { new(...): T }; }`
                //       pattern, where the class is exported (often via rename).
                //   (c) it has constructor shape and nothing else describes the
                //       instance — `declare const WebSocketPair: { new (): { 0:
                //       WebSocket; 1: WebSocket } }`. The construct signature's
                //       return type then supplies the instance members (an inline
                //       literal) or the parent (a named type).
                let is_ctor_var = is_class_constructor_var(declarator);

                if (merged_with_iface || is_ctor_var) && !alias_ctor {
                    if let Some(type_ann) = &declarator.type_annotation {
                        if let ast::TSType::TSTypeLiteral(lit) = &type_ann.type_annotation {
                            self.push_static_side_class(
                                &name,
                                &lit.members,
                                dcx,
                                doc.clone(),
                                declarations,
                            );
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

                let rust_name = to_snake_case(&name);
                if let ir::TypeRef::Function(sig) = type_ref {
                    declarations.push(dcx.decl(
                        ir::TypeKind::Function(ir::FunctionDecl {
                            name: rust_name,
                            js_name: name,
                            type_params: vec![],
                            params: sig.params,
                            return_type: *sig.return_type,
                            overloads: vec![],
                            // Variable-as-function form (`var foo: () => T`) doesn't
                            // typically carry @throws JSDoc; leave empty.
                            throws: ir::Throws::None,
                            // Var-as-function bindings have no surface type
                            // parameters; their body scope is the enclosing.
                            body_scope: dcx.scope,
                        }),
                        doc.clone(),
                    ));
                } else {
                    let is_const = matches!(var_decl.kind, ast::VariableDeclarationKind::Const);
                    declarations.push(dcx.decl(
                        ir::TypeKind::Variable(ir::VariableDecl {
                            name: rust_name,
                            js_name: name,
                            type_ref,
                            is_const,
                            constructs,
                        }),
                        doc.clone(),
                    ));
                }
            }
        }
    }

    /// The construct signatures of `var X: { new(...): T }`, each with its
    /// own type parameters, or none when the literal has other members.
    fn alias_constructs(
        &mut self,
        declarator: &ast::VariableDeclarator<'_>,
        scope: ScopeId,
    ) -> Vec<ir::FunctionDecl> {
        let Some(ast::TSType::TSTypeLiteral(lit)) = declarator
            .type_annotation
            .as_ref()
            .map(|ann| &ann.type_annotation)
        else {
            return Vec::new();
        };
        let mut constructs = Vec::new();
        for member in &lit.members {
            let sig = match member {
                ast::TSSignature::TSConstructSignatureDeclaration(sig) => sig,
                ast::TSSignature::TSPropertySignature(prop)
                    if crate::parse::members::property_key_name(&prop.key).as_deref()
                        == Some("prototype") =>
                {
                    continue;
                }
                // Statics would be unreachable from the constructors, so the
                // variable binds as a value instead.
                _ => return Vec::new(),
            };
            let mut pcx = self.parse_ctx();
            let mut type_params = convert_type_params(sig.type_parameters.as_ref(), pcx.diag);
            let body_scope =
                crate::parse::members::create_body_scope(&mut type_params, scope, &mut pcx);
            let params = crate::parse::types::convert_formal_params_scoped(
                &sig.params,
                body_scope,
                &mut pcx,
            );
            let return_type = sig
                .return_type
                .as_ref()
                .map(|rt| convert_ts_type_scoped(&rt.type_annotation, body_scope, &mut pcx))
                .unwrap_or(ir::TypeRef::Any);
            constructs.push(ir::FunctionDecl {
                name: "new".to_string(),
                js_name: "new".to_string(),
                type_params,
                params,
                return_type,
                overloads: vec![],
                throws: ir::Throws::None,
                body_scope,
            });
        }
        constructs
    }

    /// Re-export alias stub for `export { local as exported } [from "m"]`.
    fn push_export_alias(
        &self,
        spec: &ast::ExportSpecifier<'_>,
        from_module: Option<String>,
        ctx: &ir::ModuleContext,
        declarations: &mut Vec<ir::TypeDeclaration>,
        scope: ScopeId,
    ) {
        declarations.push(ir::TypeDeclaration {
            kind: ir::TypeKind::TypeAlias(ir::TypeAliasDecl {
                name: spec.exported.name().to_string(),
                type_params: vec![],
                target: ir::TypeRef::ident(spec.local.name().to_string()),
                from_module,
                // Re-export aliases have no body — pin to the
                // enclosing scope.
                body_scope: scope,
            }),
            module_context: ctx.clone(),
            doc: None,
            scope_id: scope,
        });
    }

    fn populate_external_module(
        &mut self,
        module: &ast::TSExternalModuleDeclaration<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let specifier = module.id.value.as_str();
        let module_ctx = ir::ModuleContext::Module(specifier.into());
        let module_scope = self
            .registry
            .module_blocks
            .get(&module.span.start)
            .expect("phase 1 creates a scope for every ambient module block")
            .1;
        if let Some(block) = &module.body {
            let outer = std::mem::replace(
                &mut self.export_assignment,
                assigned_name(&block.body).map(|name| (module_scope, name)),
            );
            for stmt in &block.body {
                self.populate_statement(stmt, &module_ctx, declarations, module_scope);
            }
            self.export_assignment = outer;
        }
    }

    fn populate_namespace(
        &mut self,
        ns: &ast::TSNamespaceDeclaration<'_>,
        parent_ctx: &ir::ModuleContext,
        declarations: &mut Vec<ir::TypeDeclaration>,
        scope: ScopeId,
    ) {
        let doc = self.docs.for_span(ns.span.start);
        let ns_name = ns.id.name.to_string();
        // Use the namespace's child scope from Phase 1 (fixes Item 10).
        let ns_scope = self.resolve_namespace_scope(&ns_name, scope);
        let block = *self
            .registry
            .namespace_blocks
            .get(&ns.span.start)
            .expect("phase 1 creates a scope for every namespace declaration");
        let stmts = namespace_block_statements(ns);

        // A namespace the module assigns with `export =` is the module
        // itself, so its members are module exports and are emitted flat,
        // also when the namespace is declared outside the module.
        if self
            .export_assignment
            .as_ref()
            .is_some_and(|(module_scope, name)| *module_scope == scope && *name == ns_name)
        {
            for stmt in stmts {
                self.populate_statement(stmt, parent_ctx, declarations, block);
            }
            return;
        }
        if let Some(specifier) = self.registry.assigned_namespaces.get(&ns_scope) {
            let module_ctx = ir::ModuleContext::Module(specifier.as_str().into());
            for stmt in stmts {
                self.populate_statement(stmt, &module_ctx, declarations, block);
            }
            return;
        }

        // Members keep the parent's module context so their bindings carry
        // `module = "…"` as well as the namespace path.
        let mut ns_decls = Vec::new();
        for stmt in stmts {
            self.populate_statement(stmt, parent_ctx, &mut ns_decls, block);
        }
        self.namespace_members
            .entry(ns_scope)
            .or_default()
            .extend(ns_decls);
        declarations.push(ir::TypeDeclaration {
            kind: ir::TypeKind::Namespace(ir::NamespaceDecl {
                name: ns_name,
                declarations: vec![],
                child_scope: ns_scope,
            }),
            module_context: parent_ctx.clone(),
            doc,
            scope_id: scope,
        });
    }

    fn populate_ts_enum(
        &mut self,
        enum_decl: &ast::TSEnumDeclaration<'_>,
        dcx: &DeclCtx<'_>,
        declarations: &mut Vec<ir::TypeDeclaration>,
    ) {
        let doc = self.docs.for_span(enum_decl.span.start);
        let mut decl = convert_ts_enum(enum_decl, self.docs);
        decl.global = self.global_scope && matches!(dcx.module_context, ir::ModuleContext::Global);
        declarations.push(dcx.decl(ir::TypeKind::TsEnum(decl), doc));
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
    ) -> (Option<String>, crate::parse::docs::JsDocInfo) {
        let info = export_span_start
            .and_then(|s| self.docs.info_for_span(s))
            .or_else(|| self.docs.info_for_span(inner_span_start));
        match info {
            Some((doc, info)) => (Some(doc), info),
            None => (None, Default::default()),
        }
    }

    /// Look up the child scope that Phase 1 created for a namespace.
    /// Falls back to the parent scope if not found.
    fn resolve_namespace_scope(&self, ns_name: &str, parent_scope: ScopeId) -> ScopeId {
        self.scopes
            .get(parent_scope)
            .namespace(ns_name)
            .unwrap_or(parent_scope)
    }
}
