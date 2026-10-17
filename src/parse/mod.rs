//! Parser orchestration: parse `.d.ts` files into IR.

pub mod classify;
pub mod ctx;
pub mod docs;
mod enums;
pub mod first_pass;
pub mod literal_union;
pub mod members;
pub mod merge;
pub mod resolve;
pub mod scope;
pub mod types;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::context::GlobalContext;
use crate::ir::Module;
use crate::parse::scope::ScopeId;

/// Parse one or more `.d.ts` files and produce a `Module` + `GlobalContext`.
pub fn parse_dts_files(
    paths: &[impl AsRef<Path>],
    lib_name: Option<&str>,
) -> Result<(Module, GlobalContext)> {
    let mut gctx = GlobalContext::new();
    let mut module = Module::new(&mut gctx, lib_name);
    let global = module.global_scope;

    // Track which files we've already parsed to avoid cycles
    let mut parsed_files: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    // All file scopes (including deps) for import resolution
    let mut all_file_scopes = Vec::new();
    // Map scope → source file parent directory (for relative import resolution)
    let mut scope_dirs: std::collections::HashMap<ScopeId, PathBuf> =
        std::collections::HashMap::new();

    for path in paths {
        let path = path.as_ref();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        parsed_files.insert(canonical.clone());

        let source = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let file_scope = gctx.create_child_scope(global);
        all_file_scopes.push(file_scope);
        if let Some(parent) = path.parent() {
            scope_dirs.insert(file_scope, parent.to_path_buf());
        }
        let parsed = parse_single_file(&source, path, lib_name, &mut gctx, file_scope)?;
        module.add_file(canonical, file_scope, parsed);
    }

    // Resolve imports: parse dependency files for scope resolution.
    resolve_imports(
        &mut gctx,
        global,
        &mut all_file_scopes,
        &mut scope_dirs,
        &mut parsed_files,
        lib_name,
    )?;
    enums::evaluate_ts_enums(&mut gctx, &mut module.types);

    Ok((module, gctx))
}

/// Parse a single `.d.ts` source string into a `Module` + `GlobalContext`.
pub fn parse_single_source(
    source: &str,
    lib_name: Option<&str>,
) -> Result<(Module, GlobalContext)> {
    let mut gctx = GlobalContext::new();
    let mut module = Module::new(&mut gctx, lib_name);
    let file_scope = gctx.create_child_scope(module.global_scope);
    let path = Path::new("<input>");
    let parsed = parse_single_file(source, path, lib_name, &mut gctx, file_scope)?;
    module.add_file(path.to_path_buf(), file_scope, parsed);
    enums::evaluate_ts_enums(&mut gctx, &mut module.types);
    Ok((module, gctx))
}

impl Module {
    fn new(gctx: &mut GlobalContext, lib_name: Option<&str>) -> Self {
        // The global scope holds the built-in types (js_sys globals, web
        // platform types) and every input's globals.
        let global_scope = gctx.create_root_scope();
        populate_builtin_scope(gctx, global_scope);
        Module {
            types: Vec::new(),
            lib_name: lib_name.map(str::to_string),
            global_scope,
            file_scopes: Vec::new(),
            file_roots: Vec::new(),
            module_roots: Vec::new(),
        }
    }

    fn add_file(&mut self, path: PathBuf, file_scope: ScopeId, parsed: ParsedFile) {
        for id in parsed.types {
            if !self.types.contains(&id) {
                self.types.push(id);
            }
        }
        self.file_scopes.push(file_scope);
        if let Some(globals) = parsed.globals {
            self.file_roots.push((
                path,
                crate::ir::Root {
                    exports: globals,
                    bindings: self.global_scope,
                },
            ));
        }
        for (specifier, scope) in parsed.modules {
            let root = crate::ir::Root {
                exports: scope,
                bindings: scope,
            };
            if !self.module_roots.contains(&(specifier.clone(), root)) {
                self.module_roots.push((specifier, root));
            }
        }
    }
}

/// What parsing one file adds: its declarations, the key of the export map
/// of the globals it declares, and the modules it declares by specifier.
struct ParsedFile {
    types: Vec<crate::context::TypeId>,
    globals: Option<ScopeId>,
    modules: Vec<(String, ScopeId)>,
}

fn parse_single_file(
    source: &str,
    path: &Path,
    lib_name: Option<&str>,
    gctx: &mut GlobalContext,
    file_scope: ScopeId,
) -> Result<ParsedFile> {
    let allocator = Allocator::default();
    let source_type = SourceType::d_ts();

    gctx.diagnostics.set_file(path, source);

    let parser_return = Parser::new(&allocator, source, source_type).parse();

    for error in parser_return.diagnostics.iter() {
        let message = format!("Parse error: {}", error.message);
        let help = error.help.as_ref().map(|h| h.to_string());
        let label = error
            .labels
            .iter()
            .find(|l| l.primary())
            .or_else(|| error.labels.first());
        match label {
            Some(label) => gctx.diagnostics.warn_span(
                message,
                label.offset(),
                label.offset() + label.len(),
                label.label().map(str::to_string),
                help,
            ),
            None => {
                let help = help.map_or(String::new(), |h| format!(" (help: {h})"));
                gctx.warn(format!("{}: {message}{help}", path.display()))
            }
        }
    }
    if parser_return.fatal_error {
        gctx.diagnostics.error(format!(
            "Internal OXC parse error: {} could not be parsed, no bindings were generated for it",
            path.display()
        ));
    }

    let program = &parser_return.program;
    let doc_comments = docs::DocComments::new(&program.comments, source);

    // Phase 1: Collect type names and populate file scope
    // Swap out diagnostics temporarily to avoid double-borrowing gctx.
    let file_start = gctx.type_arena().len();
    let mut diag = std::mem::take(&mut gctx.diagnostics);
    let registry = first_pass::collect_type_names(program, lib_name, &mut diag, gctx, file_scope);
    gctx.diagnostics = diag;
    for (specifier, &scope) in &registry.module_scopes {
        gctx.ambient_module_scopes
            .entry(specifier.clone())
            .or_default()
            .push(scope);
    }
    // An assigned namespace's members are the module's own declarations, so
    // they resolve and qualify like the module's body.
    for (&scope, specifier) in &registry.assigned_namespaces {
        let scopes = gctx
            .ambient_module_scopes
            .entry(specifier.clone())
            .or_default();
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }

    gctx.info(format!(
        "Collected {} type names from {}",
        registry.types.len(),
        path.display()
    ));

    // Log pending imports found for this file's scope
    let import_logs: Vec<String> = gctx
        .pending_imports
        .iter()
        .filter(|imp| {
            imp.scope == file_scope || registry.module_scopes.values().any(|&s| s == imp.scope)
        })
        .map(|imp| {
            format!(
                "Import: {} from \"{}\" (pending)",
                imp.local_name, imp.from_module
            )
        })
        .collect();
    for msg in import_logs {
        gctx.info(msg);
    }

    // Phase 2: Populate declarations.
    //
    // Move `scopes` and `diagnostics` out of `gctx` for the duration
    // of the populate pass — both need exclusive `&mut` access (the
    // populate pass appends body scopes for type-parameter-bearing
    // declarations) and we still want a `&` to the type arena. The
    // updated scope arena is moved back into `gctx` afterward.
    let mut scopes = std::mem::take(&mut gctx.scopes);
    let mut diag = std::mem::take(&mut gctx.diagnostics);
    let type_arena_snapshot = gctx.type_arena().to_vec();
    let first_pass::Populated {
        declarations,
        mut namespace_members,
        mut static_sides,
        constructor_classes,
    } = first_pass::populate_declarations(
        program,
        &registry,
        lib_name,
        &doc_comments,
        &mut diag,
        &mut scopes,
        &type_arena_snapshot,
        file_scope,
    );
    gctx.scopes = scopes;
    gctx.diagnostics = diag;

    // Post-processing: merge, dedup, then insert into the global type arena
    let merged = merge_class_pairs(declarations);
    let merged = merge_namespaces(merged);
    let merged = dedup_function_overloads(merged);

    let mut cx = InsertCtx {
        gctx,
        namespace_members: &mut namespace_members,
        static_sides: &mut static_sides,
        module_scopes: &registry.module_scopes,
        assigned_namespaces: &registry.assigned_namespaces,
        constructor_classes: &constructor_classes,
        constructor_class_ids: std::collections::HashSet::new(),
        file_scope,
        file_start,
    };
    let types = merged
        .into_iter()
        .map(|decl| cx.insert(decl, true))
        .collect();
    let constructor_class_ids = cx.constructor_class_ids;

    // A global namespace a module assigns with `export =` exports the
    // module's names, also those a later file reopening it declares.
    let mut modules: Vec<(String, ScopeId)> = registry
        .module_scopes
        .iter()
        .map(|(specifier, &scope)| (specifier.clone(), scope))
        .chain(
            registry
                .assigned_namespaces
                .iter()
                .map(|(&scope, specifier)| (specifier.clone(), scope)),
        )
        .collect();
    modules.sort_by_key(|(specifier, scope)| (specifier.clone(), scope.0));
    if let crate::ir::ModuleContext::Module(lib) = first_pass::file_context(program, lib_name) {
        modules.insert(0, (lib.to_string(), file_scope));
    }
    name_constructor_classes(gctx, &modules, &constructor_class_ids);
    Ok(ParsedFile {
        types,
        globals: registry.globals,
        modules,
    })
}

struct InsertCtx<'a> {
    gctx: &'a mut GlobalContext,
    namespace_members: &'a mut first_pass::NamespaceMembers,
    static_sides: &'a mut first_pass::StaticSides,
    module_scopes: &'a std::collections::HashMap<String, ScopeId>,
    assigned_namespaces: &'a std::collections::HashMap<ScopeId, String>,
    constructor_classes: &'a [(ScopeId, String)],
    /// The inserted ids of `constructor_classes`.
    constructor_class_ids: std::collections::HashSet<crate::context::TypeId>,
    file_scope: ScopeId,
    /// Arena length before this file: a lower id is an earlier file's
    /// declaration.
    file_start: usize,
}

impl InsertCtx<'_> {
    /// Insert a populated declaration into the type arena and bind its name
    /// over the phase 1 placeholder, so scope lookups reach the populated
    /// declaration. A namespace's members are inserted the same way first,
    /// giving the namespace their ids. A global also binds in the global
    /// scope; there, and in a namespace scope the files share, a name an
    /// earlier file declared merges as within one file, and the id of the
    /// merged declaration is returned.
    fn insert(
        &mut self,
        mut decl: crate::ir::TypeDeclaration,
        top_level: bool,
    ) -> crate::context::TypeId {
        let members = match &mut decl.kind {
            crate::ir::TypeKind::Namespace(ns) => {
                let member_decls = self
                    .namespace_members
                    .remove(&ns.child_scope)
                    .unwrap_or_default();
                let mut ids = Vec::new();
                for member in merge_namespaces(member_decls) {
                    let id = self.insert(member, false);
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                ns.declarations = ids;
                Some(ns.child_scope)
            }
            _ => None,
        };
        let file_scope = self.file_scope;
        let decl_scope = decl.scope_id;
        let flat_scope = match &decl.module_context {
            crate::ir::ModuleContext::Module(m) => Some(
                self.module_scopes
                    .get(m.as_ref())
                    .copied()
                    .unwrap_or(file_scope),
            ),
            crate::ir::ModuleContext::Global => None,
        };
        let global = (flat_scope.is_none() && decl_scope == file_scope)
            .then(|| self.gctx.scopes.get(file_scope).parent)
            .flatten();
        let block = match (global, flat_scope) {
            (Some(global), _) => global,
            (None, Some(flat_scope))
                if top_level && !self.assigned_namespaces.contains_key(&decl_scope) =>
            {
                flat_scope
            }
            _ => decl_scope,
        };
        let static_side = match &decl.kind {
            crate::ir::TypeKind::Variable(v) => {
                self.static_sides.remove(&(decl_scope, v.js_name.clone()))
            }
            _ => None,
        };
        let placeholder = binding_name(&decl.kind);
        let constructor_class = matches!(&decl.kind, crate::ir::TypeKind::Class(c)
            if self.constructor_classes.contains(&(decl_scope, c.name.clone())));
        let mut type_id = self.gctx.insert_type(decl);
        if constructor_class {
            self.constructor_class_ids.insert(type_id);
        }
        if let Some(class) = static_side {
            self.gctx.static_sides.insert(type_id, class);
        }
        let earlier: Vec<_> = placeholder
            .as_ref()
            .map(|local| self.gctx.declarations(block, local))
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|id| id.index() < self.file_start)
            .collect();
        for existing in earlier {
            if self.merge_into(existing, type_id) {
                type_id = existing;
                break;
            }
        }
        let gctx = &mut *self.gctx;
        let merged = &gctx.get_type(type_id).kind;
        let name = declaration_name(merged);
        let local = binding_name(merged);
        let binds_value = is_value(merged);
        // A namespace merged with a same-named class, function or
        // interface leaves that declaration as the name's type, and a value
        // never takes a type's binding.
        let yields = |gctx: &GlobalContext, scope: ScopeId| {
            name.as_ref().is_some_and(|name| {
                gctx.scopes.get(scope).get(name).is_some_and(|id| {
                    id != type_id
                        && !gctx.builtin_type_ids.contains(&id)
                        && match &gctx.get_type(id).kind {
                            crate::ir::TypeKind::Namespace(_) => false,
                            kind => members.is_some() || (binds_value && !is_value(kind)),
                        }
                })
            })
        };
        // Members of a namespace inside a module are flattened into the
        // module but declared in the namespace scope, which also needs the
        // upgrade for `NS.Member` to reach the populated declaration.
        if let Some(name) = name.as_ref().filter(|_| !yields(gctx, decl_scope)) {
            gctx.scopes.insert(decl_scope, name.clone(), type_id);
            if let Some(flat_scope) = flat_scope.filter(|&s| s != decl_scope) {
                gctx.scopes.insert(flat_scope, name.clone(), type_id);
            }
        }
        if let (Some(global), Some(name)) = (global, name.as_ref()) {
            if let Some(members) = members {
                gctx.scopes.insert_namespace(global, name.clone(), members);
            }
            if !yields(gctx, global) {
                gctx.scopes.insert(global, name.clone(), type_id);
            }
        }
        if let Some(local) = local {
            gctx.declare(block, &local, type_id);
            if global.is_some() {
                prefer_global_type(gctx, block, &local);
            }
        }
        type_id
    }

    /// Merge the declaration `new` into `existing` where the two would merge
    /// within one file. Returns whether they merged.
    fn merge_into(
        &mut self,
        existing: crate::context::TypeId,
        new: crate::context::TypeId,
    ) -> bool {
        use crate::ir::TypeKind;
        let mut base = self.gctx.get_type(existing).clone();
        let incoming = self.gctx.get_type(new).clone();
        if let (TypeKind::Namespace(a), TypeKind::Namespace(b)) = (&mut base.kind, &incoming.kind) {
            if a.child_scope != b.child_scope {
                return false;
            }
            for id in &b.declarations {
                if !a.declarations.contains(id) {
                    a.declarations.push(*id);
                }
            }
            *self.gctx.get_type_mut(existing) = base;
            return true;
        }
        let sides = &mut self.gctx.static_sides;
        let pair = match (&base.kind, &incoming.kind) {
            // A redeclared `var` has the same type, so it is one variable.
            (TypeKind::Variable(_), TypeKind::Variable(_)) => {
                if let Some(side) = sides.remove(&new) {
                    sides.entry(existing).or_insert(side);
                }
                return true;
            }
            // An interface and a constructor-shaped `var` declared in
            // different files are one class, as within a file.
            (TypeKind::Interface(_), TypeKind::Variable(_)) => match sides.get(&new) {
                Some(side) => vec![base, side.clone()],
                None => return false,
            },
            (TypeKind::Variable(_), TypeKind::Interface(_)) => match sides.get(&existing) {
                Some(side) => vec![side.clone(), incoming],
                None => return false,
            },
            _ => vec![base, incoming],
        };
        let merged = dedup_function_overloads(merge_class_pairs(pair));
        match <[_; 1]>::try_from(merged) {
            Ok([merged]) => {
                *self.gctx.get_type_mut(existing) = merged;
                self.gctx.static_sides.remove(&existing);
                true
            }
            Err(_) => false,
        }
    }
}

/// Bind an import to what its name means in the exporting scope: a type, a
/// namespace, or both when they merge. Returns whether either was found.
///
/// `own_only` restricts the lookup to names `target` declares itself, for a
/// `declare module` block whose parent scope is the enclosing file's.
fn bind_import(
    gctx: &mut crate::context::GlobalContext,
    import: &scope::PendingImport,
    target: ScopeId,
    own_only: bool,
) -> bool {
    // `import * as A` makes `A` a namespace whose members are the module's
    // own scope. `export *` also registers as `*` but binds no local name.
    if import.original_name == "*" {
        if import.local_name == "*" {
            return false;
        }
        gctx.scopes
            .insert_namespace(import.scope, import.local_name.clone(), target);
        return true;
    }
    // An import names an export, which the module's export map binds to a
    // local name; they differ for `export { local as public }`.
    let Some(name) = gctx.exports.local_name(target, &import.original_name) else {
        return false;
    };
    let name = &name.to_string();
    let (type_id, members) = if own_only {
        let scope = gctx.scopes.get(target);
        (scope.get(name), scope.namespace(name))
    } else {
        (
            gctx.scopes.resolve(target, name),
            gctx.scopes.resolve_namespace(target, name),
        )
    };
    if let Some(type_id) = type_id {
        gctx.scopes
            .insert(import.scope, import.local_name.clone(), type_id);
    }
    if let Some(members) = members {
        gctx.scopes
            .insert_namespace(import.scope, import.local_name.clone(), members);
    }
    type_id.is_some() || members.is_some()
}

/// Deduplicate function overloads: when multiple `FunctionDecl` share the same
/// JS name and module context, keep only the one with the most parameters (the
/// most general signature). TypeScript overloads are a single JS function at
/// runtime.
fn dedup_function_overloads(
    declarations: Vec<crate::ir::TypeDeclaration>,
) -> Vec<crate::ir::TypeDeclaration> {
    use crate::ir::{ModuleContext, TypeDeclaration, TypeKind};

    let mut best: std::collections::HashMap<(String, ModuleContext), usize> =
        std::collections::HashMap::new();
    let mut result: Vec<TypeDeclaration> = Vec::new();
    let mut skip: std::collections::HashSet<usize> = std::collections::HashSet::new();

    // First pass: find the best (most params) overload for each name
    for (i, decl) in declarations.iter().enumerate() {
        if let TypeKind::Function(ref f) = decl.kind {
            let key = (f.js_name.clone(), decl.module_context.clone());
            if let Some(&existing_idx) = best.get(&key) {
                // Compare param counts — keep the one with more params
                if let TypeKind::Function(ref existing_f) = declarations[existing_idx].kind {
                    if f.params.len() > existing_f.params.len() {
                        skip.insert(existing_idx);
                        best.insert(key, i);
                    } else {
                        skip.insert(i);
                    }
                }
            } else {
                best.insert(key, i);
            }
        }
    }

    for (i, decl) in declarations.into_iter().enumerate() {
        if !skip.contains(&i) {
            result.push(decl);
        }
    }

    result
}

/// Resolve unresolved imports by finding and parsing dependency files.
///
/// Iterates until no new files are discovered (handles transitive deps).
fn resolve_imports(
    gctx: &mut GlobalContext,
    builtin: ScopeId,
    file_scopes: &mut Vec<ScopeId>,
    scope_dirs: &mut std::collections::HashMap<ScopeId, PathBuf>,
    parsed_files: &mut std::collections::HashSet<PathBuf>,
    lib_name: Option<&str>,
) -> Result<()> {
    // Track modules that we've already tried and failed to resolve,
    // so we don't re-attempt them on every iteration.
    let mut failed_modules: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        // Drain pending imports that haven't been resolved yet.
        let pending: Vec<scope::PendingImport> = gctx
            .pending_imports
            .drain(..)
            .filter(|p| !failed_modules.contains(&p.from_module))
            .collect();

        if pending.is_empty() {
            break;
        }

        let mut new_files_parsed = false;
        let mut still_pending = Vec::new();

        for import in pending {
            // A `declare module "x"` block in any parsed file can satisfy
            // the import. Its bindings live in the module's own scopes,
            // which aren't on the importer's scope chain. A name the
            // blocks don't declare falls through to file resolution, as
            // the block may only augment a module backed by a file.
            let ambient = gctx
                .ambient_module_scopes
                .get(&import.from_module)
                .cloned()
                .unwrap_or_default();
            if ambient
                .into_iter()
                .any(|scope| bind_import(gctx, &import, scope, true))
            {
                continue;
            }

            // Check if we already have this module registered
            if let Some(module_id) = gctx.find_module(&import.from_module) {
                // Look up the name in the target module's scope
                let target_scope = gctx.get_module(module_id).scope;
                if !bind_import(gctx, &import, target_scope, false) {
                    gctx.warn(format!(
                        "Import `{}` not found in module \"{}\"",
                        import.original_name, import.from_module
                    ));
                }
                continue;
            }

            // Try to resolve the module specifier to a file.
            let base_dir = scope_dirs
                .get(&import.scope)
                .cloned()
                .unwrap_or_else(|| PathBuf::from("."));
            let resolved_path = resolve::resolve_module(&import.from_module, &base_dir);

            if let Some(path) = resolved_path {
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());

                if !parsed_files.contains(&canonical) {
                    gctx.info(format!(
                        "Resolving import \"{}\" → {}",
                        import.from_module,
                        path.display()
                    ));

                    match std::fs::read_to_string(&path) {
                        Ok(source) => {
                            let dep_scope = gctx.create_child_scope(builtin);
                            file_scopes.push(dep_scope);
                            parsed_files.insert(canonical);
                            if let Some(parent) = path.parent() {
                                scope_dirs.insert(dep_scope, parent.to_path_buf());
                            }

                            let module_id =
                                gctx.register_module(import.from_module.clone(), dep_scope);

                            match parse_single_file(&source, &path, lib_name, gctx, dep_scope) {
                                Ok(dep_type_ids) => {
                                    gctx.get_module_mut(module_id).types = dep_type_ids.types;
                                    new_files_parsed = true;

                                    // Now resolve the imported name from the dep's scope
                                    if !bind_import(gctx, &import, dep_scope, false) {
                                        gctx.warn(format!(
                                            "Import `{}` not found in \"{}\"",
                                            import.original_name, import.from_module
                                        ));
                                    }
                                }
                                Err(e) => {
                                    gctx.warn(format!(
                                        "Failed to parse dependency \"{}\" ({}): {e}",
                                        import.from_module,
                                        path.display()
                                    ));
                                    failed_modules.insert(import.from_module.clone());
                                }
                            }
                        }
                        Err(e) => {
                            gctx.warn(format!(
                                "Failed to read dependency \"{}\" ({}): {e}",
                                import.from_module,
                                path.display()
                            ));
                            failed_modules.insert(import.from_module.clone());
                        }
                    }
                } else {
                    // File already parsed but module wasn't found — retry next iteration
                    still_pending.push(import);
                }
            } else {
                gctx.warn(format!(
                    "Could not resolve import \"{}\" — use --external to map this type",
                    import.from_module
                ));
                failed_modules.insert(import.from_module.clone());
            }
        }

        // Re-add imports that couldn't be resolved yet
        gctx.pending_imports.extend(still_pending);

        if !new_files_parsed {
            break;
        }
    }

    Ok(())
}

/// Populate the builtin scope with well-known global JS type names.
///
/// These are types that exist in every TypeScript environment without imports:
/// Array, Map, Set, Promise, Error, Date, typed arrays, etc.
/// The parser recognizes most of these and maps them to TypeRef variants directly,
/// but registering them in the builtin scope ensures that scope resolution
/// correctly identifies them as built-in rather than unresolved.
fn populate_builtin_scope(gctx: &mut GlobalContext, scope: ScopeId) {
    // js_sys types: real types available via `use js_sys::*`.
    // Registered as opaque interfaces — resolve_alias won't look through them.
    for &name in crate::codegen::typemap::JS_SYS_RESERVED {
        let type_id = gctx.insert_type(crate::ir::TypeDeclaration {
            kind: crate::ir::TypeKind::Interface(crate::ir::InterfaceDecl {
                name: name.to_string(),
                js_name: name.to_string(),
                type_params: vec![],
                extends: vec![],
                members: vec![],
                classification: crate::ir::InterfaceClassification::ClassLike,
                body_scope: scope,
            }),
            module_context: crate::ir::ModuleContext::Global,
            doc: None,
            scope_id: scope,
        });
        gctx.scopes.insert(scope, name.to_string(), type_id);
        gctx.builtin_type_ids.insert(type_id);
    }

    // Web platform built-in types (ReadableStream, Request, etc.) and
    // js_sys classes spelled differently in Rust. Registered as opaque
    // interfaces — codegen emits them as bare identifiers aliased through
    // the external map's defaults, `--external`, or input files.
    let defaults = crate::external_map::WEB_SYS_DEFAULTS
        .iter()
        .chain(crate::external_map::JS_SYS_RENAMES);
    for (name, _) in defaults {
        let type_id = gctx.insert_type(crate::ir::TypeDeclaration {
            kind: crate::ir::TypeKind::Interface(crate::ir::InterfaceDecl {
                name: name.to_string(),
                js_name: name.to_string(),
                type_params: vec![],
                extends: vec![],
                members: vec![],
                classification: crate::ir::InterfaceClassification::ClassLike,
                body_scope: scope,
            }),
            module_context: crate::ir::ModuleContext::Global,
            doc: None,
            scope_id: scope,
        });
        gctx.scopes.insert(scope, name.to_string(), type_id);
        gctx.builtin_type_ids.insert(type_id);
    }
}

/// Keep one declaration per namespace. TypeScript merges repeated
/// `namespace Foo { ... }` blocks, which already share one member scope and
/// so one entry in the populated members, so only the first block's
/// declaration is kept and codegen produces a single `mod foo { ... }`.
fn merge_namespaces(
    declarations: Vec<crate::ir::TypeDeclaration>,
) -> Vec<crate::ir::TypeDeclaration> {
    let mut seen = std::collections::HashSet::new();
    declarations
        .into_iter()
        .filter(|decl| match &decl.kind {
            crate::ir::TypeKind::Namespace(ns) => seen.insert(ns.child_scope),
            _ => true,
        })
        .collect()
}

/// The local name a declaration binds in its block: what an export map
/// refers to it by. A function or variable's own name is its Rust name, so
/// this is its JS name.
pub(crate) fn binding_name(kind: &crate::ir::TypeKind) -> Option<String> {
    match kind {
        crate::ir::TypeKind::Function(f) => Some(f.js_name.clone()),
        crate::ir::TypeKind::Variable(v) => Some(v.js_name.clone()),
        kind => declaration_name(kind),
    }
}

/// Name each constructor `var`'s class by its module's export map: the first
/// name it is exported under, unless it is exported under its own. A default
/// export has no name of its own, so it keeps the local one. Within the
/// module the class's name refers to it, as it would had it been declared
/// under that name.
fn name_constructor_classes(
    gctx: &mut GlobalContext,
    modules: &[(String, ScopeId)],
    classes: &std::collections::HashSet<crate::context::TypeId>,
) {
    let mut named = std::collections::HashSet::new();
    for &(_, scope) in modules {
        let mut entries: Vec<(String, String)> = gctx
            .exports
            .entries(scope)
            .map(|(public, local)| (public.to_string(), local.to_string()))
            .collect();
        entries.sort_by_key(|(public, _)| public == "default");
        for (public, local) in entries {
            if gctx.exports.local_name(scope, &local) == Some(local.as_str()) {
                continue;
            }
            for id in gctx.declarations(scope, &local).to_vec() {
                if !classes.contains(&id) || !named.insert(id) {
                    continue;
                }
                let name = first_pass::rust_name(&local, &public).to_string();
                let decl = gctx.get_type_mut(id);
                let decl_scope = decl.scope_id;
                if let crate::ir::TypeKind::Class(c) = &mut decl.kind {
                    c.name = name.clone();
                    c.js_name = public.clone();
                }
                if gctx.scopes.get(decl_scope).get(&name).is_none() {
                    gctx.scopes.insert(decl_scope, name, id);
                }
            }
        }
    }
}

/// Make the type among the global declarations of `name` what each declaring
/// file's scope binds it to. A value's phase 1 placeholder holds the name
/// there, and within one file the type's population replaces it, but a
/// type another file declares would be shadowed by it.
fn prefer_global_type(gctx: &mut GlobalContext, global: ScopeId, name: &str) {
    use crate::ir::TypeKind;
    let ids = gctx.declarations(global, name).to_vec();
    let Some(&ty) = ids.iter().find(|&&id| {
        !matches!(
            gctx.get_type(id).kind,
            TypeKind::Function(_) | TypeKind::Variable(_) | TypeKind::Namespace(_)
        )
    }) else {
        return;
    };
    for id in ids {
        let scope = gctx.get_type(id).scope_id;
        let shadows = gctx
            .scopes
            .get(scope)
            .get(name)
            .is_some_and(|bound| is_value(&gctx.get_type(bound).kind));
        if shadows {
            gctx.scopes.insert(scope, name.to_string(), ty);
        }
    }
}

fn is_value(kind: &crate::ir::TypeKind) -> bool {
    matches!(
        kind,
        crate::ir::TypeKind::Function(_) | crate::ir::TypeKind::Variable(_)
    )
}

/// Extract the name from a declaration kind, if it has one.
fn declaration_name(kind: &crate::ir::TypeKind) -> Option<String> {
    use crate::ir::TypeKind;
    match kind {
        TypeKind::Class(c) => Some(c.name.clone()),
        TypeKind::Interface(i) => Some(i.name.clone()),
        TypeKind::DiscriminatedUnion(d) => Some(d.name.clone()),
        TypeKind::TypeAlias(a) => Some(a.name.clone()),
        TypeKind::StringEnum(e) => Some(e.name.clone()),
        TypeKind::NumericEnum(e) => Some(e.name.clone()),
        TypeKind::TsEnum(e) => Some(e.name.clone()),
        TypeKind::Function(f) => Some(f.name.clone()),
        TypeKind::Variable(v) => Some(v.name.clone()),
        TypeKind::Namespace(n) => Some(n.name.clone()),
    }
}

/// Merge two member lists, deduplicating by (kind, name).
/// Members from `incoming` override same-named members in `base`.
fn merge_members(base: &mut Vec<crate::ir::Member>, incoming: Vec<crate::ir::Member>) {
    use std::collections::HashMap;

    // Build a map of existing members by key
    let mut by_key: HashMap<MemberKey, usize> = HashMap::new();
    for (i, m) in base.iter().enumerate() {
        by_key.insert(member_key(m), i);
    }

    for m in incoming {
        let key = member_key(&m);
        if let Some(&idx) = by_key.get(&key) {
            // Override existing
            base[idx] = m;
        } else {
            by_key.insert(key, base.len());
            base.push(m);
        }
    }
}

/// A lightweight key for member deduplication.
///
/// Getters and setters for the same JS property name get distinct keys,
/// since they are independent bindings that should not overwrite each other.
#[derive(Debug, PartialEq, Eq, Hash)]
enum MemberKey {
    Constructor,
    StaticMethod(String),
    StaticGetter(String),
    StaticSetter(String),
    Proto(String),
    ProtoGetter(String),
    ProtoSetter(String),
}

fn member_key(member: &crate::ir::Member) -> MemberKey {
    match member {
        crate::ir::Member::Constructor(_) => MemberKey::Constructor,
        crate::ir::Member::StaticMethod(m) => MemberKey::StaticMethod(m.name.clone()),
        crate::ir::Member::StaticGetter(g) => MemberKey::StaticGetter(g.js_name.clone()),
        crate::ir::Member::StaticSetter(s) => MemberKey::StaticSetter(s.js_name.clone()),
        crate::ir::Member::Method(m) => MemberKey::Proto(m.name.clone()),
        crate::ir::Member::Getter(g) => MemberKey::ProtoGetter(g.js_name.clone()),
        crate::ir::Member::Setter(s) => MemberKey::ProtoSetter(s.js_name.clone()),
        crate::ir::Member::IndexSignature(_) => MemberKey::Proto("[index]".to_string()),
    }
}

/// Merge declarations with the same name AND the same module context:
/// - Interface + Interface → merge members (TypeScript declaration merging)
/// - Interface + Class → merge interface members into class (var+interface pattern)
/// - Class + Class → merge members
///
/// Types in different module contexts (e.g. a global `interface EmailMessage`
/// and a `cloudflare:email`-scoped `class EmailMessage`) intentionally do NOT
/// merge — they're distinct Rust types that happen to share a JS name.
fn merge_class_pairs(
    declarations: Vec<crate::ir::TypeDeclaration>,
) -> Vec<crate::ir::TypeDeclaration> {
    use crate::ir::{ModuleContext, TypeDeclaration, TypeKind};
    use std::collections::HashMap;

    type Key = (String, ModuleContext);
    let mut class_map: HashMap<Key, usize> = HashMap::new();
    let mut iface_map: HashMap<Key, usize> = HashMap::new();
    let mut result: Vec<TypeDeclaration> = Vec::new();

    for decl in declarations {
        match &decl.kind {
            TypeKind::Class(class_decl) => {
                let key = (class_decl.name.clone(), decl.module_context.clone());

                // Merge into existing class
                if let Some(&existing_idx) = class_map.get(&key) {
                    if let TypeKind::Class(ref mut existing) = result[existing_idx].kind {
                        merge_members(&mut existing.members, class_decl.members.clone());
                    }
                    continue;
                }

                // Merge into existing interface (promote to class)
                if let Some(&iface_idx) = iface_map.get(&key) {
                    // Replace the interface with this class, merging members
                    let mut new_class = class_decl.clone();
                    if let TypeKind::Interface(ref iface) = result[iface_idx].kind {
                        merge_members(&mut new_class.members, iface.members.clone());
                        if new_class.extends.is_none() {
                            new_class.extends = iface.extends.first().cloned();
                        }
                        if new_class.type_params.is_empty() {
                            new_class.type_params = iface.type_params.clone();
                        }
                    }
                    result[iface_idx] = TypeDeclaration {
                        kind: TypeKind::Class(new_class),
                        module_context: decl.module_context.clone(),
                        doc: decl.doc.clone(),
                        scope_id: decl.scope_id,
                    };
                    class_map.insert(key.clone(), iface_idx);
                    iface_map.remove(&key);
                    continue;
                }

                let idx = result.len();
                class_map.insert(key, idx);
                result.push(decl);
            }
            TypeKind::Interface(iface_decl) => {
                let key = (iface_decl.name.clone(), decl.module_context.clone());

                // Merge into existing class
                if let Some(&class_idx) = class_map.get(&key) {
                    if let TypeKind::Class(ref mut class) = result[class_idx].kind {
                        merge_members(&mut class.members, iface_decl.members.clone());
                        if class.extends.is_none() {
                            class.extends = iface_decl.extends.first().cloned();
                        }
                        if class.type_params.is_empty() {
                            class.type_params = iface_decl.type_params.clone();
                        }
                    }
                    continue;
                }

                // Merge into existing interface
                if let Some(&existing_idx) = iface_map.get(&key) {
                    if let TypeKind::Interface(ref mut existing) = result[existing_idx].kind {
                        merge_members(&mut existing.members, iface_decl.members.clone());
                        // Merge extends
                        for ext in &iface_decl.extends {
                            if !existing.extends.contains(ext) {
                                existing.extends.push(ext.clone());
                            }
                        }
                    }
                    continue;
                }

                let idx = result.len();
                iface_map.insert(key, idx);
                result.push(decl);
            }
            _ => {
                result.push(decl);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        ClassDecl, GetterMember, InterfaceClassification, InterfaceDecl, Member, MethodMember,
        ModuleContext, TypeDeclaration, TypeKind, TypeRef,
    };
    use crate::parse::scope::ScopeId;
    use std::rc::Rc;

    fn method(name: &str) -> Member {
        Member::Method(MethodMember {
            name: name.to_string(),
            js_name: name.to_string(),
            type_params: vec![],
            params: vec![],
            return_type: TypeRef::Void,
            optional: false,
            doc: None,
            throws: crate::ir::Throws::None,
            body_scope: ScopeId::DUMMY,
        })
    }

    fn getter(name: &str) -> Member {
        Member::Getter(GetterMember {
            js_name: name.to_string(),
            type_ref: TypeRef::String,
            optional: false,
            doc: None,
        })
    }

    /// Helper for building a top-level `TypeDeclaration` with the given kind
    /// and module context. Defaults exported=true and scope=root for tests
    /// that don't care about those fields.
    fn decl(kind: TypeKind, ctx: ModuleContext) -> TypeDeclaration {
        TypeDeclaration {
            kind,
            module_context: ctx,
            doc: None,
            scope_id: ScopeId(0),
        }
    }

    fn iface(name: &str, members: Vec<Member>) -> InterfaceDecl {
        InterfaceDecl {
            name: name.to_string(),
            js_name: name.to_string(),
            type_params: vec![],
            extends: vec![],
            members,
            classification: InterfaceClassification::ClassLike,
            body_scope: ScopeId::DUMMY,
        }
    }

    fn class(name: &str, members: Vec<Member>, ctx: ModuleContext) -> ClassDecl {
        ClassDecl {
            name: name.to_string(),
            js_name: name.to_string(),
            type_params: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
            members,
            type_module_context: ctx,
            body_scope: ScopeId::DUMMY,
        }
    }

    fn module(spec: &str) -> ModuleContext {
        ModuleContext::Module(Rc::from(spec))
    }

    #[test]
    fn test_merge_members_dedup() {
        let mut base = vec![method("read"), method("write"), getter("name")];
        let incoming = vec![method("write"), method("end")];
        merge_members(&mut base, incoming);

        // write is overridden (not duplicated), end is appended
        assert_eq!(base.len(), 4);
        assert!(matches!(&base[0], Member::Method(m) if m.name == "read"));
        assert!(matches!(&base[1], Member::Method(m) if m.name == "write"));
        assert!(matches!(&base[2], Member::Getter(g) if g.js_name == "name"));
        assert!(matches!(&base[3], Member::Method(m) if m.name == "end"));
    }

    #[test]
    fn test_merge_members_no_overlap() {
        let mut base = vec![method("foo")];
        let incoming = vec![method("bar")];
        merge_members(&mut base, incoming);
        assert_eq!(base.len(), 2);
    }

    #[test]
    fn test_merge_members_getter_and_method_coexist() {
        // ProtoGetter and Proto are different keys — both survive
        let mut base = vec![getter("data")];
        let incoming = vec![method("data")];
        merge_members(&mut base, incoming);
        assert_eq!(base.len(), 2);
        assert!(matches!(&base[0], Member::Getter(g) if g.js_name == "data"));
        assert!(matches!(&base[1], Member::Method(m) if m.name == "data"));
    }

    // ─── merge_class_pairs ──────────────────────────────────────────────

    #[test]
    fn test_merge_class_pairs_interface_and_interface_same_context() {
        // Same name + same context (both global) → merge members.
        let a = decl(
            TypeKind::Interface(iface("Widget", vec![method("foo")])),
            ModuleContext::Global,
        );
        let b = decl(
            TypeKind::Interface(iface("Widget", vec![method("bar")])),
            ModuleContext::Global,
        );
        let merged = merge_class_pairs(vec![a, b]);
        assert_eq!(merged.len(), 1);
        let TypeKind::Interface(merged_iface) = &merged[0].kind else {
            panic!("expected merged interface");
        };
        assert_eq!(merged_iface.members.len(), 2);
    }

    #[test]
    fn test_merge_class_pairs_interface_and_class_same_context() {
        // var X: { new(...) } + interface X { ... } pattern: interface gets
        // promoted into the class, members merged.
        let iface_decl = decl(
            TypeKind::Interface(iface("Widget", vec![method("read")])),
            ModuleContext::Global,
        );
        let class_decl = decl(
            TypeKind::Class(class(
                "Widget",
                vec![method("write")],
                ModuleContext::Global,
            )),
            ModuleContext::Global,
        );
        let merged = merge_class_pairs(vec![iface_decl, class_decl]);
        assert_eq!(merged.len(), 1);
        let TypeKind::Class(merged_class) = &merged[0].kind else {
            panic!("expected merged class");
        };
        assert_eq!(merged_class.members.len(), 2);
    }

    #[test]
    fn test_merge_class_pairs_keeps_distinct_module_contexts() {
        // Regression guard for the email/cloudflare:email bug:
        //   global `interface EmailMessage { from; to }`
        // and
        //   declare module "cloudflare:email" { class EmailMessage { new(...) } }
        // share a JS name but live in different module contexts. They MUST
        // stay as two separate `TypeDeclaration`s — the global interface is
        // the receiver type for `SendEmail.send`/`reply`, the module-scoped
        // class carries the constructor.
        let global_iface = decl(
            TypeKind::Interface(iface("EmailMessage", vec![getter("from"), getter("to")])),
            ModuleContext::Global,
        );
        let module_class = decl(
            TypeKind::Class(class(
                "EmailMessage",
                vec![method("constructor")],
                module("cloudflare:email"),
            )),
            module("cloudflare:email"),
        );

        let merged = merge_class_pairs(vec![global_iface, module_class]);
        assert_eq!(
            merged.len(),
            2,
            "interface and class in different module contexts must not merge",
        );

        // Order is preserved by `merge_class_pairs` (stable insertion order),
        // so check both decls round-trip with their original kinds + contexts.
        assert!(matches!(merged[0].kind, TypeKind::Interface(ref i) if i.name == "EmailMessage"));
        assert_eq!(merged[0].module_context, ModuleContext::Global);
        assert!(matches!(merged[1].kind, TypeKind::Class(ref c) if c.name == "EmailMessage"));
        assert_eq!(merged[1].module_context, module("cloudflare:email"));
    }

    #[test]
    fn test_merge_class_pairs_class_and_class_same_context() {
        let a = decl(
            TypeKind::Class(class("Foo", vec![method("a")], ModuleContext::Global)),
            ModuleContext::Global,
        );
        let b = decl(
            TypeKind::Class(class("Foo", vec![method("b")], ModuleContext::Global)),
            ModuleContext::Global,
        );
        let merged = merge_class_pairs(vec![a, b]);
        assert_eq!(merged.len(), 1);
        let TypeKind::Class(c) = &merged[0].kind else {
            panic!("expected merged class");
        };
        assert_eq!(c.members.len(), 2);
    }

    #[test]
    fn globals_merge_across_files() {
        let dir = std::env::temp_dir().join(format!("ts-gen-globals-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.d.ts");
        let b = dir.join("b.d.ts");
        std::fs::write(
            &a,
            "interface Win { a(): void; } declare namespace N { interface S { x(): void; } }",
        )
        .unwrap();
        std::fs::write(
            &b,
            "interface Win { b(): void; } declare namespace N { interface S { y(): void; } interface B {} }",
        )
        .unwrap();
        let result = parse_dts_files(&[&a, &b], None);
        std::fs::remove_dir_all(&dir).unwrap();
        let (module, gctx) = result.unwrap();

        let members = |id| match &gctx.get_type(id).kind {
            TypeKind::Interface(i) => i.members.len(),
            other => panic!("expected an interface, got {other:?}"),
        };
        let wins = gctx.declarations(module.global_scope, "Win");
        assert_eq!(wins.len(), 1);
        assert_eq!(members(wins[0]), 2);
        assert_eq!(module.types.iter().filter(|&&id| id == wins[0]).count(), 1);

        let (file_a, file_b) = (module.file_scopes[0], module.file_scopes[1]);
        let s = gctx.resolve_path(file_b, "N.S").expect("N.S");
        assert_eq!(gctx.resolve_path(file_a, "N.S"), Some(s));
        assert_eq!(members(s), 2);
        assert!(gctx.resolve_path(file_a, "N.B").is_some());

        // Each file's globals are its exports, bound in the global scope.
        for (_, root) in &module.file_roots {
            assert_eq!(root.bindings, module.global_scope);
            let exports: Vec<_> = gctx.exports.entries(root.exports).collect();
            assert_eq!(exports, [("Win", "Win"), ("N", "N")]);
        }
    }

    #[test]
    fn test_ambient_module_scope_and_export_names() {
        let src = r#"
            declare function _open(): void;
            declare module "a" {
                interface Private {}
                function _open(): void;
                export { _open as open, _open as start };
            }
            declare module "b" {
                export function _open(): void;
            }
        "#;
        let (module, gctx) = crate::parse_source(src, None).unwrap();

        // Each block declares its own `_open`, once; `a` exports it twice.
        let file_scope = module.file_scopes[0];
        let a_scope = gctx.ambient_module_scopes["a"][0];
        let b_scope = gctx.ambient_module_scopes["b"][0];
        let declared = |scope| {
            let ids = gctx.declarations(scope, "_open");
            assert_eq!(ids.len(), 1);
            gctx.get_type(ids[0]).module_context.clone()
        };
        assert_eq!(declared(module.global_scope), ModuleContext::Global);
        assert_eq!(declared(a_scope), ModuleContext::Module("a".into()));
        assert_eq!(declared(b_scope), ModuleContext::Module("b".into()));
        let exports: Vec<_> = gctx.exports.entries(a_scope).collect();
        assert_eq!(exports, [("open", "_open"), ("start", "_open")]);

        assert!(gctx.scopes.resolve(a_scope, "Private").is_some());
        assert!(gctx.scopes.resolve(file_scope, "Private").is_none());
    }

    #[test]
    fn test_named_constructor_interface_merges_into_class() {
        let src = r#"
            declare var Later: LaterConstructor;
            interface Codec { encode(input: string): string; }
            interface CodecConstructor {
                new (name: string): Codec;
                readonly prototype: Codec;
                readonly version: number;
                isCodec(value: unknown): boolean;
            }
            declare var Codec: CodecConstructor;
            interface LaterConstructor { new (): Later; }
            interface Later {}
            interface Factory { new (id: number): Codec; label: string; }
            declare const Ns: Ns;
            declare namespace Ns { interface Inner {} }
        "#;
        let (module, gctx) = crate::parse_source(src, None).unwrap();

        let find = |name: &str| {
            module
                .types
                .iter()
                .map(|&id| &gctx.get_type(id).kind)
                .find(|k| declaration_name(k).as_deref() == Some(name))
        };
        let member_keys =
            |members: &[Member]| -> Vec<MemberKey> { members.iter().map(member_key).collect() };

        let Some(TypeKind::Class(codec)) = find("Codec") else {
            panic!("Codec should be a class");
        };
        assert_eq!(
            member_keys(&codec.members),
            vec![
                MemberKey::Constructor,
                MemberKey::StaticGetter("version".into()),
                MemberKey::StaticMethod("is_codec".into()),
                MemberKey::Proto("encode".into()),
            ]
        );
        assert!(find("CodecConstructor").is_none());
        assert!(matches!(find("Later"), Some(TypeKind::Class(c)) if c.members.len() == 1));
        assert!(find("LaterConstructor").is_none());

        let Some(TypeKind::Interface(factory)) = find("Factory") else {
            panic!("Factory should stay an interface");
        };
        assert!(!factory
            .members
            .iter()
            .any(|m| matches!(m, Member::Constructor(_))));

        // A deferred var must not clobber a later same-named namespace.
        let file_scope = module.file_scopes[0];
        let ns = gctx.scopes.resolve(file_scope, "Ns").unwrap();
        assert!(matches!(gctx.get_type(ns).kind, TypeKind::Namespace(_)));
    }

    #[test]
    fn test_import_from_ambient_module_in_other_file() {
        let dir =
            std::env::temp_dir().join(format!("ts-gen-ambient-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let decls = dir.join("decls.d.ts");
        let user = dir.join("user.d.ts");
        std::fs::write(
            &decls,
            r#"declare module "node:x" { export type Id = string; export class Thing {} }"#,
        )
        .unwrap();
        std::fs::write(
            &user,
            r#"
                import { Id, Thing } from "node:x";
                export declare function useIt(id: Id, t: Thing): void;
            "#,
        )
        .unwrap();
        let (module, gctx) = parse_dts_files(&[&decls, &user], Some("mylib")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let x_scope = gctx.ambient_module_scopes["node:x"][0];
        let user_scope = module.file_scopes[1];
        for name in ["Id", "Thing"] {
            let declared = gctx.scopes.get(x_scope).get(name);
            assert!(declared.is_some());
            assert_eq!(gctx.scopes.resolve(user_scope, name), declared);
        }
    }

    #[test]
    fn export_map_binds_imports_to_the_exported_local() {
        let dir = std::env::temp_dir().join(format!("ts-gen-export-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let decls = dir.join("decls.d.ts");
        std::fs::write(
            &decls,
            r#"
                declare module "dep" {
                    class _Foo { bar(): void; }
                    export { _Foo as Foo };
                    export default class Def {}
                    function hidden(): void;
                }
                declare module "bare" {
                    class Open {}
                    export function also(): void;
                    function too(): void;
                }
                declare module "blocks" {
                    namespace ns { const v: number; export { v as w }; }
                    namespace ns { function after(): void; }
                }
                declare module "assigned" {
                    class S { s(): void; }
                    namespace S { class R { r(): void; } }
                    export = S;
                }
                declare module "defaulted" {
                    export default class Def {}
                    function helper(): void;
                }
                declare module "main" {
                    import { Foo } from "dep";
                    import Def from "dep";
                    import * as D from "dep";
                    import { Open, too } from "bare";
                    import { ns } from "blocks";
                    import { R } from "assigned";
                    import * as A from "assigned";
                    import { helper } from "defaulted";
                }
            "#,
        )
        .unwrap();
        let result = parse_dts_files(&[&decls], None);
        std::fs::remove_dir_all(&dir).unwrap();
        let (_, gctx) = result.unwrap();

        let dep = gctx.ambient_module_scopes["dep"][0];
        let bare = gctx.ambient_module_scopes["bare"][0];
        let main = gctx.ambient_module_scopes["main"][0];
        // `export { … }` / `export default` decide the block's exports.
        assert_eq!(gctx.exports.local_name(dep, "Foo"), Some("_Foo"));
        assert_eq!(gctx.exports.local_name(dep, "default"), Some("Def"));
        assert_eq!(gctx.exports.local_name(dep, "Def"), None);
        let exports = |scope| -> Vec<(&str, &str)> { gctx.exports.entries(scope).collect() };
        assert_eq!(exports(dep), [("Foo", "_Foo"), ("default", "Def")]);
        // Without one, every declaration is exported, `export` modifier or not.
        assert_eq!(
            exports(bare),
            [("also", "also"), ("Open", "Open"), ("too", "too")]
        );

        let name_of = |id| {
            crate::codegen::typemap::type_decl_name(&gctx.get_type(id).kind).map(str::to_string)
        };
        let resolve = |path: &str| gctx.resolve_path(main, path).and_then(name_of);
        assert_eq!(resolve("Foo").as_deref(), Some("_Foo"));
        assert_eq!(resolve("Def").as_deref(), Some("Def"));
        assert_eq!(resolve("D.Foo").as_deref(), Some("_Foo"));
        assert_eq!(resolve("D.default").as_deref(), Some("Def"));
        assert_eq!(resolve("D._Foo"), None, "a local is not an export");
        assert_eq!(resolve("Open").as_deref(), Some("Open"));
        assert_eq!(resolve("too").as_deref(), Some("too"));
        // The rule is per block: the second `namespace ns` exports `after`
        // although the first decides its exports explicitly.
        assert_eq!(resolve("ns.w").as_deref(), Some("v"));
        assert_eq!(resolve("ns.after").as_deref(), Some("after"));
        assert_eq!(resolve("ns.v"), None);
        // `export = S` makes the namespace merged with `S` the module.
        assert_eq!(resolve("R").as_deref(), Some("R"));
        assert_eq!(resolve("A.R").as_deref(), Some("R"));
        assert_eq!(resolve("A.default").as_deref(), Some("S"));
        // `export default class` is a declaration, not an export assignment.
        assert_eq!(resolve("helper").as_deref(), Some("helper"));
    }

    #[test]
    fn export_assigned_global_namespace_is_the_module() {
        let dir = std::env::temp_dir().join(format!("ts-gen-assigned-ns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let decls = dir.join("decls.d.ts");
        std::fs::write(
            &decls,
            r#"
                declare module "m" {
                    export = Shared;
                }
                declare namespace Shared {
                    class Entry {}
                    function run(): void;
                }
                declare module "user" {
                    import { Entry } from "m";
                }
            "#,
        )
        .unwrap();
        let result = parse_dts_files(&[&decls], None);
        std::fs::remove_dir_all(&dir).unwrap();
        let (module, gctx) = result.unwrap();

        // Declared after the module, and still populated into it.
        let m_scope = gctx.ambient_module_scopes["m"][0];
        assert_eq!(gctx.exports.local_name(m_scope, "Entry"), Some("Entry"));
        assert_eq!(gctx.exports.local_name(m_scope, "run"), Some("run"));
        let contexts: Vec<_> = module
            .types
            .iter()
            .map(|&id| gctx.get_type(id))
            .filter(|d| !matches!(d.kind, TypeKind::Namespace(_)))
            .map(|d| d.module_context.clone())
            .collect();
        assert!(contexts
            .iter()
            .all(|c| *c == ModuleContext::Module("m".into())));
        assert!(!module
            .types
            .iter()
            .any(|&id| matches!(gctx.get_type(id).kind, TypeKind::Namespace(_))));

        let user = gctx.ambient_module_scopes["user"][0];
        let entry = gctx
            .resolve_path(user, "Entry")
            .map(|id| &gctx.get_type(id).kind);
        assert!(matches!(entry, Some(TypeKind::Class(_))));
    }

    #[test]
    fn export_assigned_global_namespace_limits() {
        let dir = std::env::temp_dir().join(format!("ts-gen-assigned-lim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let files = [
            r#"
                declare namespace S { function one(): void; }
                declare module "m" { export = S; }
                declare module "second" { export = S; }
                declare namespace NS { function g(): void; }
                declare module "own" { class NS {} export = NS; }
                declare function f(): void;
                declare namespace f { function h(): void; }
                declare module "merged" { export = f; }
                interface C { a(): void; }
                declare var C: { prototype: C; new(): C };
                declare namespace C { function k(): void; }
                declare module "classlike" { export = C; }
            "#,
            r#"
                declare namespace S { class Two {} }
                declare namespace X { function x(): void; }
            "#,
            r#"declare module "late" { export = X; }"#,
        ];
        let paths: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(i, src)| {
                let path = dir.join(format!("f{i}.d.ts"));
                std::fs::write(&path, src).unwrap();
                path
            })
            .collect();
        let result = parse_dts_files(&paths, None);
        std::fs::remove_dir_all(&dir).unwrap();
        let (module, gctx) = result.unwrap();

        // Where each declaration is emitted: a module, a global namespace,
        // or the global scope.
        let mut placed = std::collections::HashMap::new();
        for &id in &module.types {
            let d = gctx.get_type(id);
            if let TypeKind::Namespace(ns) = &d.kind {
                for &member in &ns.declarations {
                    let name = crate::codegen::typemap::type_decl_name(&gctx.get_type(member).kind);
                    placed.insert(name.unwrap().to_string(), format!("namespace {}", ns.name));
                }
            } else if let Some(name) = crate::codegen::typemap::type_decl_name(&d.kind) {
                let at = match &d.module_context {
                    ModuleContext::Module(m) => format!("module {m}"),
                    ModuleContext::Global => "global".to_string(),
                };
                placed.insert(name.to_string(), at);
            }
        }
        let place = |name: &str| placed[name].as_str();
        // A later file reopening an assigned namespace adds to the module.
        assert_eq!(place("one"), "module m");
        assert_eq!(place("Two"), "module m");
        // The module's own binding is what it assigns.
        assert_eq!(place("NS"), "module own");
        assert_eq!(place("g"), "namespace NS");
        // A merged function carries the value, so the namespace stays.
        assert_eq!(place("f"), "global");
        assert_eq!(place("h"), "namespace f");
        assert_eq!(place("k"), "namespace C");
        // An earlier file's namespace is already populated as a global.
        assert_eq!(place("x"), "namespace X");

        let exports = |m: &str| -> Vec<_> {
            gctx.exports
                .entries(gctx.ambient_module_scopes[m][0])
                .collect()
        };
        assert_eq!(exports("second"), [("default", "S")]);
        assert_eq!(exports("late"), [("default", "X")]);

        // `--export m` emits both files' members of the namespace.
        let options =
            crate::codegen::GenerateOptions::new().exports(std::collections::HashSet::from([
                crate::codegen::ExportSpec::Module("m".into()),
            ]));
        let surface = crate::codegen::surface::Surface::build(&module, &gctx, &options);
        for name in ["one", "Two"] {
            let id = module
                .types
                .iter()
                .copied()
                .find(|&id| {
                    crate::codegen::typemap::type_decl_name(&gctx.get_type(id).kind) == Some(name)
                })
                .unwrap();
            assert!(!surface.get(id).is_empty(), "{name}");
        }
    }

    #[test]
    fn default_exports_bind_by_default() {
        let dir = std::env::temp_dir().join(format!("ts-gen-default-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let decls = dir.join("decls.d.ts");
        std::fs::write(
            &decls,
            r#"
                declare module "a" { export default class C {} }
                declare module "b" { export default function f(): void; }
                declare module "c" { const v: string; export default v; }
                declare module "d" { export default later; function later(): void; }
                declare module "e" { namespace N { function x(): void; } export default N; }
                declare module "g" { interface I { a: string } namespace I { function y(): void; } export default I; }
                declare module "k" { enum E { A = 1 } namespace E { function y(): void; } export default E; }
                declare module "h" {
                    interface P { go(): void; }
                    const P: { new (): P; prototype: P };
                    namespace P { function z(): void; }
                    export default P;
                }
            "#,
        )
        .unwrap();
        let result = parse_dts_files(&[&decls], None);
        std::fs::remove_dir_all(&dir).unwrap();
        let (_, gctx) = result.unwrap();

        for (m, local) in [("a", "C"), ("b", "f"), ("c", "v"), ("d", "later")] {
            let scope = gctx.ambient_module_scopes[m][0];
            assert_eq!(
                gctx.exports.local_name(scope, "default"),
                Some(local),
                "{m}"
            );
        }
        // A namespace has no value to bind as `default`, so stays unexported.
        let e = gctx.ambient_module_scopes["e"][0];
        assert_eq!(gctx.exports.entries(e).count(), 0);
        // Nor does an interface merged with one, unless a `var` makes it a
        // class.
        let g = gctx.ambient_module_scopes["g"][0];
        assert_eq!(gctx.exports.local_name(g, "default"), None);
        let k = gctx.ambient_module_scopes["k"][0];
        assert_eq!(gctx.exports.local_name(k, "default"), None);
        let h = gctx.ambient_module_scopes["h"][0];
        assert_eq!(gctx.exports.local_name(h, "default"), Some("P"));
    }

    #[test]
    fn class_merges_are_per_scope() {
        let dir = std::env::temp_dir().join(format!("ts-gen-merge-scope-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let decls = dir.join("decls.d.ts");
        std::fs::write(
            &decls,
            r#"
                interface A { a(): void; }
                declare var A: { prototype: A; new(): A };
                interface B { b(): void; }
                interface BCtor { new(): B; readonly prototype: B; }
                declare var B: BCtor;
                declare module "z" {
                    interface A { x: string }
                    interface BCtor { y: string }
                    interface B { z: string }
                    const B: number;
                }
            "#,
        )
        .unwrap();
        let result = parse_dts_files(&[&decls], None);
        std::fs::remove_dir_all(&dir).unwrap();
        let (module, gctx) = result.unwrap();

        let globals: Vec<_> = module
            .types
            .iter()
            .map(|&id| gctx.get_type(id))
            .filter(|d| d.module_context == ModuleContext::Global)
            .collect();
        for name in ["A", "B"] {
            let class = globals.iter().find_map(|d| match &d.kind {
                TypeKind::Class(c) if c.name == name => Some(c),
                _ => None,
            });
            let class = class.unwrap_or_else(|| panic!("{name} is not a class"));
            assert!(
                class
                    .members
                    .iter()
                    .any(|m| matches!(m, crate::ir::Member::Constructor(_))),
                "{name} has no constructor"
            );
        }
        // The vars are the classes' constructors, not values of their own,
        // and `BCtor` is the class's static side rather than an interface.
        assert!(!globals
            .iter()
            .any(|d| matches!(&d.kind, TypeKind::Variable(_) | TypeKind::Interface(_))));
    }

    #[test]
    fn test_import_namespaces_between_ambient_modules() {
        let dir = std::env::temp_dir().join(format!("ts-gen-ambient-ns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let decls = dir.join("decls.d.ts");
        std::fs::write(
            &decls,
            r#"
                declare module "a" {
                    export interface Opts {}
                    export namespace inner { type Deep = "p" | "q"; }
                }
                declare module "b" {
                    import { inner } from "a";
                    import * as A from "a";
                }
            "#,
        )
        .unwrap();
        let result = parse_dts_files(&[&decls], None);
        std::fs::remove_dir_all(&dir).unwrap();
        let (_, gctx) = result.unwrap();

        let b_scope = gctx.ambient_module_scopes["b"][0];
        let kind = |path: &str| {
            gctx.resolve_path(b_scope, path)
                .map(|id| &gctx.get_type(id).kind)
        };
        assert!(matches!(kind("inner.Deep"), Some(TypeKind::StringEnum(_))));
        assert!(matches!(kind("A.Opts"), Some(TypeKind::Interface(_))));
        assert!(matches!(
            kind("A.inner.Deep"),
            Some(TypeKind::StringEnum(_))
        ));
    }

    #[test]
    fn test_merge_class_pairs_two_modules_with_same_name() {
        // Same name, two distinct modules — should remain separate even
        // though both are non-global.
        let a = decl(
            TypeKind::Interface(iface("Connection", vec![method("a")])),
            module("module:foo"),
        );
        let b = decl(
            TypeKind::Interface(iface("Connection", vec![method("b")])),
            module("module:bar"),
        );
        let merged = merge_class_pairs(vec![a, b]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_parse_errors_carry_source_location() {
        let source = "export interface A {\n  x: string;\n}\nexport declare function f(a: string: number): void;\n";
        let (_, gctx) = parse_single_source(source, Some("m")).unwrap();
        let diag = gctx
            .diagnostics
            .diagnostics
            .iter()
            .find(|d| d.message.starts_with("Parse error"))
            .expect("parse error diagnostic");
        assert_eq!(
            diag.message,
            "Parse error: Expected `,` or `)` but found `:`"
        );
        let loc = diag.location.as_ref().expect("parse error location");
        assert_eq!(loc.to_string(), "<input>:4:36");
        assert!(gctx.diagnostics.has_errors());
        assert!(gctx.diagnostics.render(false).contains(
            "\
  |
4 | export declare function f(a: string: number): void;
  |                                    ^ `,` or `)` expected
"
        ));
    }

    #[test]
    fn test_recoverable_parse_error_is_not_fatal() {
        let source = "export interface A { public x: string }\n";
        let (_, gctx) = parse_single_source(source, Some("m")).unwrap();
        assert!(gctx.diagnostics.has_warnings());
        assert!(!gctx.diagnostics.has_errors());
    }

    #[test]
    fn test_negative_literal_types() {
        let (module, gctx) =
            crate::parse_source("type A = -1; type B = -0.5; type C = -1n;", None).unwrap();
        let targets: Vec<TypeRef> = module
            .types
            .iter()
            .map(|id| match &gctx.get_type(*id).kind {
                TypeKind::TypeAlias(a) => a.target.clone(),
                other => panic!("expected alias, got {other:?}"),
            })
            .collect();
        assert_eq!(
            targets,
            [
                TypeRef::NumberLiteral(-1.0),
                TypeRef::NumberLiteral(-0.5),
                TypeRef::BigInt
            ]
        );
    }

    #[test]
    fn module_namespace_members_resolve_by_qualified_path() {
        let src = r#"
            export declare namespace Codes {
                type Region = "eu" | "us";
            }
        "#;
        let (module, gctx) = crate::parse_source(src, Some("m")).unwrap();
        let scope = module.file_scopes[0];
        let id = gctx
            .resolve_path(scope, "Codes.Region")
            .expect("Codes.Region resolves");
        // The populated declaration, not the phase-1 placeholder.
        match &gctx.get_type(id).kind {
            TypeKind::StringEnum(e) => assert_eq!(e.variants.len(), 2),
            other => panic!("expected a string enum, got {other:?}"),
        }
    }

    #[test]
    fn alias_named_constructor_vars_keep_the_alias() {
        let src = r#"
            interface Stub<T> { get(): T; }
            export type A<T> = Stub<T>;
            export declare const A: { new <T>(value: T): Stub<T> };
            export declare const B: { new (n: number): Stub<number> };
            export type B = Stub<number>;
        "#;
        let (module, gctx) = crate::parse_source(src, Some("m")).unwrap();
        let scope = module.file_scopes[0];
        for name in ["A", "B"] {
            let id = gctx.resolve_path(scope, name).unwrap();
            assert!(
                matches!(gctx.get_type(id).kind, TypeKind::TypeAlias(_)),
                "{name} resolves to its alias"
            );
            let constructs = module
                .types
                .iter()
                .find_map(|&id| match &gctx.get_type(id).kind {
                    TypeKind::Variable(v) if v.js_name == name => Some(&v.constructs),
                    _ => None,
                });
            let constructs = constructs.unwrap_or_else(|| panic!("{name} has a variable"));
            assert_eq!(constructs.len(), 1, "{name}");
        }
    }

    #[test]
    fn merged_namespaces_resolve_alongside_their_type() {
        for lib in [None, Some("m")] {
            let src = r#"
                export declare class Foo {}
                export declare namespace Foo { interface Options {} }
                export declare namespace Bar { type Mode = "on" | "off"; }
                export declare function Bar(): void;
                export type Local = "l";
            "#;
            let src = if lib.is_some() {
                src.to_string()
            } else {
                src.replace("export ", "")
            };
            let (module, gctx) = crate::parse_source(&src, lib).unwrap();
            let scope = module.file_scopes[0];
            let kind = |path: &str| {
                gctx.resolve_path(scope, path)
                    .map(|id| &gctx.get_type(id).kind)
            };

            assert!(matches!(kind("Foo"), Some(TypeKind::Class(_))), "{lib:?}");
            assert!(
                matches!(kind("Foo.Options"), Some(TypeKind::Interface(_))),
                "{lib:?}"
            );
            assert!(
                matches!(kind("Bar"), Some(TypeKind::Function(_))),
                "{lib:?}"
            );
            assert!(
                matches!(kind("Bar.Mode"), Some(TypeKind::StringEnum(_))),
                "{lib:?}"
            );
            // A member lookup does not fall back to the enclosing scope.
            assert!(kind("Foo.Local").is_none(), "{lib:?}");
        }
    }

    #[test]
    fn imported_namespaces_resolve_qualified_members() {
        let dir = std::env::temp_dir().join(format!("ts-gen-import-ns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("dep.d.ts"),
            r#"
                export declare namespace NS { type K = "a" | "b"; }
                export declare class Foo {}
                export declare namespace Foo { interface Options {} }
            "#,
        )
        .unwrap();
        let main = dir.join("main.d.ts");
        std::fs::write(
            &main,
            r#"import { NS, Foo as Bar } from "./dep"; import * as D from "./dep";"#,
        )
        .unwrap();
        let result = parse_dts_files(&[&main], Some("main"));
        std::fs::remove_dir_all(&dir).unwrap();

        let (module, gctx) = result.unwrap();
        let scope = module.file_scopes[0];
        let kind = |path: &str| {
            gctx.resolve_path(scope, path)
                .map(|id| &gctx.get_type(id).kind)
        };
        assert!(matches!(kind("NS.K"), Some(TypeKind::StringEnum(_))));
        assert!(matches!(kind("Bar"), Some(TypeKind::Class(_))));
        assert!(matches!(kind("Bar.Options"), Some(TypeKind::Interface(_))));
        assert!(matches!(kind("D.Foo"), Some(TypeKind::Class(_))));
        assert!(matches!(kind("D.NS.K"), Some(TypeKind::StringEnum(_))));
        assert!(matches!(
            kind("D.Foo.Options"),
            Some(TypeKind::Interface(_))
        ));
    }

    #[test]
    fn namespace_members_resolve_to_their_populated_declarations() {
        let src = r#"
            declare namespace N { type T = string; interface A { b: string } }
            declare namespace N { interface B {} }
        "#;
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let scope = module.file_scopes[0];
        let ns = module
            .types
            .iter()
            .find_map(|&id| match &gctx.get_type(id).kind {
                TypeKind::Namespace(ns) => Some(ns),
                _ => None,
            })
            .expect("namespace");
        let id = |path: &str| gctx.resolve_path(scope, path).expect(path);

        // Both blocks merge into one namespace holding the member ids.
        assert_eq!(ns.declarations, [id("N.T"), id("N.A"), id("N.B")]);
        match &gctx.get_type(id("N.T")).kind {
            TypeKind::TypeAlias(a) => assert_eq!(a.target, TypeRef::String),
            other => panic!("expected an alias, got {other:?}"),
        }
        match &gctx.get_type(id("N.A")).kind {
            TypeKind::Interface(i) => assert!(!i.members.is_empty()),
            other => panic!("expected an interface, got {other:?}"),
        }
        assert_eq!(gctx.scopes.resolve(ns.child_scope, "A"), Some(id("N.A")));
    }
}
