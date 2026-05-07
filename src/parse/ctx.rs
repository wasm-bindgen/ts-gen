//! Shared parse-time context.
//!
//! Bundles the ambient state every type / member / declaration
//! converter needs to thread along: the scope arena (for lexical
//! resolution + child-scope creation), the diagnostic collector, doc
//! comments, the used-type-name set (for synthesized-interface name
//! dedup), and the synthesized-interface sink.
//!
//! Threading this as `&mut ParseCtx` instead of five-plus individual
//! parameters keeps converter signatures small and makes adding new
//! ambient state a one-place change.

use std::collections::HashSet;

use crate::ir::InterfaceDecl;
use crate::parse::docs::DocComments;
use crate::parse::scope::ScopeArena;
use crate::util::diagnostics::DiagnosticCollector;

/// Ambient parse-time state.
///
/// Held by `&mut` for the duration of a single file's
/// `populate_declarations` call. Lifetimes:
///
/// * `'arena` — borrow of the scope arena and diagnostic collector
///   (live for the whole parse).
/// * `'docs` — the `DocComments<'docs>` borrows source-allocated
///   strings; ties to the AST allocator's lifetime.
pub struct ParseCtx<'arena, 'docs> {
    /// Scope arena. Resolution walks this for declared-type lookup;
    /// new child scopes get appended here as type-parameter-bearing
    /// declarations are encountered.
    pub scopes: &'arena mut ScopeArena,
    /// Diagnostic sink for warnings / errors raised during parsing.
    pub diag: &'arena mut DiagnosticCollector,
    /// JSDoc comment table for the current file.
    pub docs: &'arena DocComments<'docs>,
    /// Names already in use across this run — seeded from the type
    /// registry and grown as anonymous interfaces get hoisted. Used
    /// to dedup candidate names for synthesized parameter / iterable
    /// wrapper types.
    pub used_type_names: &'arena mut HashSet<String>,
    /// Sink for interfaces synthesized during member conversion
    /// (anonymous parameter types, iterable wrappers). The caller
    /// drains this and emits the synthesized interfaces alongside
    /// the parent declaration.
    pub synth: &'arena mut Vec<InterfaceDecl>,
}
