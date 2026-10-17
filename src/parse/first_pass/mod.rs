//! Two-phase first pass over the oxc AST to build the IR.
//!
//! - Phase 1 (`collect`): Collect all type names into the `TypeRegistry`.
//! - Phase 2 (`populate`): Fully populate IR declarations using the registry for resolution.
//! - `converters`: Pure AST → IR conversion functions shared by both phases.

use oxc_ast::ast;

use crate::ir;
use crate::parse::scope::ScopeId;

mod collect;
pub mod converters;
mod populate;

// Re-export public entry points.
pub use collect::collect_type_names;
pub use populate::{populate_declarations, NamespaceMembers, Populated, StaticSides};

// ─── Script vs Module Detection ─────────────────────────────────────

/// Detect whether a file is a TypeScript "script" or "module".
///
/// A file is a **module** if any top-level statement is an `import` or `export`.
/// Script files have all top-level declarations in global scope.
/// Module files have declarations local to the file — only `declare global {}`
/// blocks affect the global scope.
pub fn is_module(program: &ast::Program<'_>) -> bool {
    program.body.iter().any(|stmt| {
        matches!(
            stmt,
            ast::Statement::ImportDeclaration(_)
                | ast::Statement::ExportAllDeclaration(_)
                | ast::Statement::ExportDefaultDeclaration(_)
                | ast::Statement::ExportDeclaration(_)
                | ast::Statement::ExportNamedDeclaration(_)
                | ast::Statement::ExportFromDeclaration(_)
                | ast::Statement::TSExportAssignment(_)
        )
    })
}

/// The module context of a file's top-level declarations. In a module file
/// every top-level declaration belongs to the module whether or not it has
/// an `export` prefix: an unexported one can still be exported by name
/// through `export { local as public }`, or be referred to by exported
/// declarations, and in either case it must merge and resolve with them.
pub fn file_context(program: &ast::Program<'_>, lib_name: Option<&str>) -> ir::ModuleContext {
    match lib_name {
        Some(lib) if is_module(program) => ir::ModuleContext::Module(lib.into()),
        _ => ir::ModuleContext::Global,
    }
}

// ─── Shared Helpers ─────────────────────────────────────────────────

/// The Rust name of the binding `local` exported as `public`. A default
/// export has no name of its own, so it keeps the local one.
pub(crate) fn rust_name<'n>(local: &'n str, public: &'n str) -> &'n str {
    if public == "default" {
        local
    } else {
        public
    }
}

/// Whether a declaration block decides its exports explicitly. TypeScript
/// exports every declaration of a `.d.ts` file, `declare module` body or
/// namespace body implicitly, unless the block contains an export
/// declaration (`export { … }`, `export … from`, `export *`) or an export
/// assignment (`export =`, `export default <expr>`); `export` modifiers on
/// declarations do not switch implicit export off, and nor does `export
/// default class C {}`, which is a declaration with modifiers. The rule is
/// per block, so a second `namespace test { function after(); }` next to a
/// `namespace test { export { test }; }` still exports `after`.
pub fn block_exports_explicitly(stmts: &[ast::Statement<'_>]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        ast::Statement::ExportNamedDeclaration(_)
        | ast::Statement::ExportFromDeclaration(_)
        | ast::Statement::ExportAllDeclaration(_)
        | ast::Statement::TSExportAssignment(_) => true,
        ast::Statement::ExportDefaultDeclaration(export) => !matches!(
            export.declaration,
            ast::ExportDefaultDeclarationKind::ClassDeclaration(_)
                | ast::ExportDefaultDeclarationKind::FunctionDeclaration(_)
                | ast::ExportDefaultDeclarationKind::TSInterfaceDeclaration(_)
        ),
        _ => false,
    })
}

/// The binding a block's `export = <name>` assigns, if any.
pub fn export_assigned_name<'s>(stmts: &'s [ast::Statement<'_>]) -> Option<&'s str> {
    stmts.iter().find_map(|stmt| match stmt {
        ast::Statement::TSExportAssignment(assign) => match &assign.expression {
            ast::Expression::Identifier(ident) => Some(ident.name.as_str()),
            _ => None,
        },
        _ => None,
    })
}

/// The local names a block's imports bind.
pub fn block_imported_names(stmts: &[ast::Statement<'_>]) -> Vec<String> {
    let mut names = Vec::new();
    for stmt in stmts {
        match stmt {
            ast::Statement::ImportDeclaration(import) => {
                for spec in import.specifiers.iter().flatten() {
                    names.push(spec.local().name.to_string());
                }
            }
            ast::Statement::TSImportEqualsDeclaration(import) => {
                names.push(import.id.name.to_string());
            }
            _ => {}
        }
    }
    names
}

/// The names a block's own declarations bind, with or without an `export`
/// modifier. Imports and nested `declare module` / `declare global` blocks
/// bind nothing here.
pub fn block_declared_names(stmts: &[ast::Statement<'_>]) -> Vec<String> {
    let mut names = Vec::new();
    for stmt in stmts {
        let decl = match stmt {
            ast::Statement::ExportDeclaration(export) => Some(&export.declaration),
            _ => None,
        };
        match decl {
            Some(decl) => declaration_names(decl, &mut names),
            None => statement_names(stmt, &mut names),
        }
    }
    names
}

/// The names a `declare module` block declares as exports of the module:
/// every declaration of a block without an export declaration, else those
/// with an `export` modifier. TypeScript's binder puts these in the module
/// symbol's exports, which every block of the module shares, and the rest
/// of the block's declarations, with its imports, in the block's locals.
pub fn block_module_exports(stmts: &[ast::Statement<'_>]) -> Vec<String> {
    if !block_exports_explicitly(stmts) {
        return block_implicit_exports(stmts);
    }
    let mut names = Vec::new();
    for stmt in stmts {
        if let ast::Statement::ExportDeclaration(export) = stmt {
            declaration_names(&export.declaration, &mut names);
        }
    }
    names
}

/// The names a block exports when it doesn't decide its exports explicitly:
/// its own declarations, except that an `export default` declaration's
/// local name is exported only as `default`.
pub fn block_implicit_exports(stmts: &[ast::Statement<'_>]) -> Vec<String> {
    stmts
        .iter()
        .filter(|stmt| !matches!(stmt, ast::Statement::ExportDefaultDeclaration(_)))
        .flat_map(|stmt| block_declared_names(std::slice::from_ref(stmt)))
        .collect()
}

fn statement_names(stmt: &ast::Statement<'_>, names: &mut Vec<String>) {
    match stmt {
        ast::Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ast::ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                names.extend(c.id.as_ref().map(|id| id.name.to_string()))
            }
            ast::ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                names.extend(f.id.as_ref().map(|id| id.name.to_string()))
            }
            _ => {}
        },
        ast::Statement::ClassDeclaration(c) => {
            names.extend(c.id.as_ref().map(|id| id.name.to_string()))
        }
        ast::Statement::FunctionDeclaration(f) => {
            names.extend(f.id.as_ref().map(|id| id.name.to_string()))
        }
        ast::Statement::TSInterfaceDeclaration(i) => names.push(i.id.name.to_string()),
        ast::Statement::TSTypeAliasDeclaration(a) => names.push(a.id.name.to_string()),
        ast::Statement::TSEnumDeclaration(e) => names.push(e.id.name.to_string()),
        ast::Statement::TSNamespaceDeclaration(ns) => names.push(ns.id.name.to_string()),
        ast::Statement::VariableDeclaration(v) => names.extend(
            v.declarations
                .iter()
                .filter_map(crate::parse::merge::var_declarator_name),
        ),
        _ => {}
    }
}

fn declaration_names(decl: &ast::Declaration<'_>, names: &mut Vec<String>) {
    match decl {
        ast::Declaration::ClassDeclaration(c) => {
            names.extend(c.id.as_ref().map(|id| id.name.to_string()))
        }
        ast::Declaration::FunctionDeclaration(f) => {
            names.extend(f.id.as_ref().map(|id| id.name.to_string()))
        }
        ast::Declaration::TSInterfaceDeclaration(i) => names.push(i.id.name.to_string()),
        ast::Declaration::TSTypeAliasDeclaration(a) => names.push(a.id.name.to_string()),
        ast::Declaration::TSEnumDeclaration(e) => names.push(e.id.name.to_string()),
        ast::Declaration::TSNamespaceDeclaration(ns) => names.push(ns.id.name.to_string()),
        ast::Declaration::VariableDeclaration(v) => names.extend(
            v.declarations
                .iter()
                .filter_map(crate::parse::merge::var_declarator_name),
        ),
        _ => {}
    }
}

/// Statements of `namespace N { ... }`. Dotted `namespace A.B {}` bodies
/// are not descended into.
fn namespace_block_statements<'s, 'a>(
    ns: &'s ast::TSNamespaceDeclaration<'a>,
) -> &'s [ast::Statement<'a>] {
    match &ns.body {
        ast::TSNamespaceDeclarationBody::TSModuleBlock(block) => &block.body,
        ast::TSNamespaceDeclarationBody::TSNamespaceDeclaration(_) => &[],
    }
}

fn is_string_literal_union(ts_type: &ast::TSType<'_>) -> bool {
    match ts_type {
        ast::TSType::TSUnionType(union) => union.types.iter().all(|t| {
            matches!(
                t,
                ast::TSType::TSLiteralType(lit) if matches!(&lit.literal, ast::TSLiteral::StringLiteral(_))
            )
        }),
        ast::TSType::TSLiteralType(lit) => matches!(&lit.literal, ast::TSLiteral::StringLiteral(_)),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn register_type(
    name: &str,
    kind: ir::RegisteredKind,
    ctx: &ir::ModuleContext,
    registry: &mut ir::TypeRegistry,
    gctx: &mut crate::context::GlobalContext,
    scope: ScopeId,
    exported: bool,
    bind: bool,
) {
    registry.types.insert(
        (scope, name.to_string()),
        ir::TypeInfo {
            kind: kind.clone(),
            primary_context: ctx.clone(),
        },
    );

    // Create a placeholder TypeDeclaration based on the registered kind.
    // Phase 2 (`populate_declarations`) overwrites these with the
    // fully-populated IR — including a proper body_scope holding the
    // declaration's type-parameter bindings — so the placeholder
    // body_scope is just the enclosing scope. It's never observed.
    let name_str = name.to_string();
    let placeholder_kind = match kind {
        ir::RegisteredKind::Class | ir::RegisteredKind::MergedClassLike => {
            ir::TypeKind::Class(ir::ClassDecl {
                name: name_str.clone(),
                js_name: name_str.clone(),
                type_params: vec![],
                extends: None,
                implements: vec![],
                is_abstract: false,
                members: vec![],
                type_module_context: ctx.clone(),
                body_scope: scope,
            })
        }
        ir::RegisteredKind::Interface => ir::TypeKind::Interface(ir::InterfaceDecl {
            construct: Vec::new(),
            name: name_str.clone(),
            js_name: name_str.clone(),
            type_params: vec![],
            extends: vec![],
            members: vec![],
            classification: ir::InterfaceClassification::Unclassified,
            body_scope: scope,
        }),
        ir::RegisteredKind::StringEnum => ir::TypeKind::StringEnum(ir::StringEnumDecl {
            name: name_str.clone(),
            variants: vec![],
        }),
        ir::RegisteredKind::TsEnum => ir::TypeKind::TsEnum(ir::TsEnumDecl {
            name: name_str.clone(),
            members: vec![],
            global: false,
        }),
        ir::RegisteredKind::TypeAlias => ir::TypeKind::TypeAlias(ir::TypeAliasDecl {
            name: name_str.clone(),
            type_params: vec![],
            target: ir::TypeRef::Any,
            from_module: None,
            body_scope: scope,
        }),
        ir::RegisteredKind::Function => ir::TypeKind::Function(ir::FunctionDecl {
            name: name_str.clone(),
            js_name: name_str.clone(),
            type_params: vec![],
            params: vec![],
            return_type: ir::TypeRef::Any,
            overloads: vec![],
            throws: ir::Throws::None,
            body_scope: scope,
        }),
        ir::RegisteredKind::Variable => ir::TypeKind::Variable(ir::VariableDecl {
            name: name_str.clone(),
            js_name: name_str.clone(),
            type_ref: ir::TypeRef::Any,
            is_const: false,
            constructs: vec![],
        }),
        ir::RegisteredKind::Namespace => {
            let ns_scope = gctx.scopes.create_child(scope);
            ir::TypeKind::Namespace(ir::NamespaceDecl {
                name: name_str.clone(),
                declarations: vec![],
                child_scope: ns_scope,
            })
        }
    };

    let type_id = gctx.insert_type(ir::TypeDeclaration {
        kind: placeholder_kind,
        module_context: ctx.clone(),
        doc: None,
        scope_id: scope,
    });
    if exported {
        gctx.exports.insert(scope, name, name);
    }
    if bind {
        gctx.scopes.insert(scope, name_str, type_id);
    }
}

/// Register an import as a pending import.
fn register_import(
    local_name: &str,
    original_name: &str,
    from_module: &str,
    gctx: &mut crate::context::GlobalContext,
    scope: ScopeId,
) {
    gctx.pending_imports
        .push(crate::parse::scope::PendingImport {
            scope,
            local_name: local_name.to_string(),
            from_module: from_module.to_string(),
            original_name: original_name.to_string(),
        });
}
