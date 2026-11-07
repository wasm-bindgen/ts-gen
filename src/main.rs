use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use walkdir::WalkDir;

/// Generate wasm-bindgen Rust bindings from TypeScript .d.ts files.
#[derive(Parser, Debug)]
#[command(name = "ts-gen", version, about)]
struct Cli {
    /// One or more .d.ts files or directories to process.
    #[arg(short, long, required = true, num_args = 1..)]
    input: Vec<PathBuf>,

    /// Output file path for the generated .rs file (e.g., `src/bindings.rs`).
    #[arg(short, long)]
    output: PathBuf,

    /// Output surface: files and/or JS module specifiers to emit. The
    /// `--input` set defines the full type universe for resolution;
    /// `--export` is authoritative about what gets emitted as public
    /// Rust code.
    ///
    /// Three forms:
    ///   - File path (e.g. `types/email.d.ts`) — emits the globals that
    ///     file declares.
    ///   - Module specifier (e.g. `cloudflare:email`) — emits that
    ///     module's declarations in a `pub mod` named by the specifier
    ///     (`cloudflare::email`). The `--lib-name` module is emitted at
    ///     the top level instead.
    ///   - Declaration name (e.g. `Request`, `TailStream.TailEvent`) —
    ///     emits that export of any file or module and, transitively,
    ///     the types its bindings reference. A bare identifier is read
    ///     as a declaration name unless it names a `declare module`.
    ///
    /// When omitted entirely, every `--input` file becomes an implicit
    /// file export. NO modules are emitted by the implicit default —
    /// modules must be explicitly listed. As soon as one `--export` is
    /// passed, the implicit default is disabled and only what's listed
    /// contributes to the output.
    #[arg(short = 'E', long = "export", num_args = 1..)]
    export: Vec<String>,

    /// JS module specifier (required when .d.ts has top-level exports).
    /// This module is the library the output binds, so it is emitted at
    /// the top level rather than in a `pub mod`.
    #[arg(long)]
    lib_name: Option<String>,

    /// External type mappings for imported types.
    ///
    /// Format: `LHS=RHS` where:
    ///   - `Blob=::web_sys::Blob` maps a specific type
    ///   - `node:buffer=node_buffer_sys` maps a module
    ///   - `node:*=node_sys::*` maps a module prefix with wildcard
    ///   - `Intl.Locale=my_intl::Locale` maps a namespace member by its path
    ///   - `Intl.*=my_intl::*` maps a namespace's members with wildcard
    ///
    /// Multiple mappings can be comma-separated or specified multiple times.
    #[arg(short = 'e', long = "external", num_args = 1..)]
    external: Vec<String>,

    /// Disable the built-in web platform → `web_sys` defaults.
    ///
    /// By default ts-gen treats names like `Blob`, `Headers`,
    /// `ReadableStream`, `Event`, … as their `::web_sys::*` equivalents
    /// so the generated code compiles against the standard web platform
    /// types out of the box. Pass this flag for environments that don't
    /// link against `web_sys` (custom runtimes, node-only outputs) or
    /// when you want to provide every mapping explicitly via
    /// `--external`. Explicit `--external` mappings always override the
    /// defaults regardless of this flag. The `js_sys` defaults (`URIError`,
    /// `Intl.*`) stay.
    #[arg(long = "no-web-sys")]
    no_web_sys: bool,

    /// Print the parsed IR for debugging.
    #[arg(short, long)]
    verbose: bool,

    /// Rust path of the default error type for fallible bindings,
    /// replacing `JsValue` (e.g. `crate::Error`).
    ///
    /// The type must implement `From<JsValue>`: wasm-bindgen's `catch`
    /// converts the caught exception with `?`. Applies only when a
    /// binding has no `@throws` annotation; `@throws {SomeError}`
    /// still emits the typed error.
    #[arg(
        long = "error-type",
        value_name = "PATH",
        conflicts_with = "errors_as_error"
    )]
    error_type: Option<String>,

    /// Generate an enum (default name `JsException`) with one variant per
    /// type named in any `@throws`, plus `Other(JsValue)`. Every
    /// `@throws`-typed binding returns `Result<T, JsException>`, so a
    /// `@throws {TypeError | RangeError}` keeps both classes instead
    /// of collapsing to `Error`. `From<JsValue>` dispatches with
    /// `instanceof`, most derived class first.
    #[arg(long = "error-enum", value_name = "NAME", num_args = 0..=1, default_missing_value = "JsException")]
    error_enum: Option<String>,

    /// Shorthand for `--error-type ::js_sys::Error`.
    ///
    /// Useful in environments that prefer the typed `Error` API
    /// (`message`, `name`, `cause`, …). Note: JS code that throws a
    /// non-Error value (e.g. `throw "oops"`) is cast unchecked.
    #[arg(long = "errors-as-error")]
    errors_as_error: bool,

    /// Emit one `Result` form per sync method and function instead of a
    /// trapping primary plus a `try_<name>` companion.
    ///
    /// TypeScript carries no throw information, so by default every sync
    /// callable is bound twice. With this flag the primary name catches
    /// and returns `Result<T, E>`; `E` follows the usual rules
    /// (`@throws {T}`, else `JsValue`, or `js_sys::Error` under
    /// `--errors-as-error`). `@throws {never}` keeps a binding
    /// infallible. Async callables, constructors and setters are
    /// unchanged. An async callable's sync `_promise` form catches too,
    /// unless it is a builtin or global one (a specifier with a scheme
    /// such as `node:`, other than a URL or `npm:`, `jsr:`, `data:` or
    /// `blob:`; a bare Node builtin; or no module) or declared `async`,
    /// which reject rather than throw.
    #[arg(long = "fallible")]
    fallible: bool,

    /// Use wasm-bindgen's experimental per-monomorphization codegen for
    /// generic imports instead of erasing their type parameters to JsValue.
    #[arg(long = "experimental-generic-mono")]
    experimental_generic_mono: bool,

    /// Keep the exact carrier for each empty on reads.
    ///
    /// By default a return, awaited value or getter admitting `null`,
    /// `undefined` or both reads as `Option<T>`: a read has no
    /// representation to choose, and `Option<T>` reads either empty as
    /// `None`. With this flag a read of `T | null` is `Nullable<T>` and
    /// one of `T | null | undefined` is `Option<Nullable<T>>`, telling the
    /// two empties apart. Writes are exact under either setting.
    #[arg(long = "strict-nullable")]
    strict_nullable: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolve input files
    let input_files = resolve_input_files(&cli.input)?;
    if input_files.is_empty() {
        bail!("No .d.ts files found in the specified input paths");
    }

    eprintln!("[ts-gen] Processing {} file(s)...", input_files.len());

    // Parse
    let (module, mut gctx) = ts_gen::parse::parse_dts_files(&input_files, cli.lib_name.as_deref())?;

    if cli.no_web_sys {
        gctx.external_map.clear_defaults();
    }

    // Apply external type mappings
    for mapping in &cli.external {
        gctx.external_map.add_mappings(mapping);
    }

    gctx.diagnostics.emit();

    if cli.verbose {
        eprintln!("[ts-gen] Parsed {} declarations", module.types.len());
        for &type_id in &module.types {
            let decl = gctx.get_type(type_id);
            print_declaration(&gctx, decl, 0);
        }
    }

    // Resolve `--export` into a flat set of `ExportSpec`s. A spec is
    // treated as a file path when it exists on disk (same logic as
    // `--input`, which also accepts files or directories); otherwise
    // it's a declaration name when it is spelled like one and names no
    // module, else a JS module specifier.
    let mut exports: std::collections::HashSet<ts_gen::codegen::ExportSpec> =
        std::collections::HashSet::new();
    for spec in &cli.export {
        let as_path = PathBuf::from(spec);
        if as_path.exists() {
            for resolved in resolve_input_files(&[as_path])? {
                let canonical = resolved.canonicalize().unwrap_or(resolved);
                exports.insert(ts_gen::codegen::ExportSpec::File(canonical));
            }
        } else if gctx.find_module(spec).is_none()
            && !gctx.ambient_module_scopes.contains_key(spec)
            && cli.lib_name.as_deref() != Some(spec.as_str())
            && is_declaration_name(spec)
        {
            exports.insert(ts_gen::codegen::ExportSpec::Decl(spec.clone()));
        } else {
            exports.insert(ts_gen::codegen::ExportSpec::Module(spec.clone()));
        }
    }
    // Implicit default: every `--input` file becomes an implicit
    // file export. Modules are never auto-added — emitting a module
    // requires an explicit `--export <spec>`.
    if cli.export.is_empty() {
        for path in &input_files {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            exports.insert(ts_gen::codegen::ExportSpec::File(canonical));
        }
    }

    let mut options = ts_gen::codegen::GenerateOptions::new()
        .errors_as_error(cli.errors_as_error)
        .fallible(cli.fallible)
        .experimental_generic_mono(cli.experimental_generic_mono)
        .strict_nullable(cli.strict_nullable)
        .exports(exports);
    if let Some(path) = &cli.error_type {
        options = options.error_type(path);
    }
    if let Some(name) = &cli.error_enum {
        options = options.error_enum(name);
    }
    let rust_source = ts_gen::codegen::generate_with_options(&module, &gctx, &options)?;

    // Write output file, creating parent directories if needed
    if let Some(parent) = cli.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
    }

    std::fs::write(&cli.output, &rust_source)
        .with_context(|| format!("Failed to write {}", cli.output.display()))?;

    eprintln!("[ts-gen] Wrote {}", cli.output.display());
    eprintln!("[ts-gen] Done");

    Ok(())
}

/// A dotted identifier path (`Request`, `TailStream.TailEvent`) as opposed
/// to a module specifier, which carries `:` / `/` / `*` or a leading `@`.
fn is_declaration_name(spec: &str) -> bool {
    spec.split('.').all(|seg| {
        let mut chars = seg.chars();
        matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    })
}

/// Resolve input paths: expand directories to find all .d.ts files.
fn resolve_input_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in WalkDir::new(path) {
                let entry = entry?;
                if entry.file_type().is_file()
                    && entry.file_name().to_string_lossy().ends_with(".d.ts")
                {
                    files.push(entry.into_path());
                }
            }
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            bail!("Input path does not exist: {}", path.display());
        }
    }
    Ok(files)
}

/// Pretty-print a declaration for debugging.
fn print_declaration(
    gctx: &ts_gen::context::GlobalContext,
    decl: &ts_gen::ir::TypeDeclaration,
    indent: usize,
) {
    let prefix = "  ".repeat(indent);
    let ctx = match &decl.module_context {
        ts_gen::ir::ModuleContext::Global => "global".to_string(),
        ts_gen::ir::ModuleContext::Module(m) => format!("module={m}"),
    };

    match &decl.kind {
        ts_gen::ir::TypeKind::Class(c) => {
            let extends = c
                .extends
                .as_ref()
                .map(|e| format!(" extends {:?}", e.type_ref))
                .unwrap_or_default();
            let abstract_ = if c.is_abstract { " abstract" } else { "" };
            eprintln!(
                "{prefix}[{ctx}]{abstract_} class {} ({} members){extends}",
                c.name,
                c.members.len(),
            );
            for member in &c.members {
                print_member(member, indent + 1);
            }
        }
        ts_gen::ir::TypeKind::Interface(i) => {
            eprintln!(
                "{prefix}[{ctx}] interface {} ({} members, {:?})",
                i.name,
                i.members.len(),
                i.classification,
            );
            for member in &i.members {
                print_member(member, indent + 1);
            }
        }
        ts_gen::ir::TypeKind::DiscriminatedUnion(d) => {
            eprintln!(
                "{prefix}[{ctx}] discriminated_union {} ({} branches, discriminators: {:?})",
                d.name,
                d.branches.len(),
                d.discriminators,
            );
            for member in &d.members {
                print_member(member, indent + 1);
            }
        }
        ts_gen::ir::TypeKind::TypeAlias(t) => {
            eprintln!("{prefix}[{ctx}] type {} = {:?}", t.name, t.target);
        }
        ts_gen::ir::TypeKind::StringEnum(e) => {
            let variants: Vec<_> = e.variants.iter().map(|v| &v.js_value).collect();
            eprintln!("{prefix}[{ctx}] string enum {} = {:?}", e.name, variants);
        }
        ts_gen::ir::TypeKind::TsEnum(e) => {
            eprintln!("{prefix}[{ctx}] enum {} (unevaluated)", e.name);
        }
        ts_gen::ir::TypeKind::NumericEnum(e) => {
            let variants: Vec<_> = e
                .variants
                .iter()
                .map(|v| format!("{} = {}", v.rust_name, v.value))
                .collect();
            eprintln!(
                "{prefix}[{ctx}] numeric enum {} = [{}]",
                e.name,
                variants.join(", ")
            );
        }
        ts_gen::ir::TypeKind::Function(f) => {
            let params: Vec<_> = f.params.iter().map(|p| &p.name).collect();
            eprintln!(
                "{prefix}[{ctx}] function {}({:?}) -> {:?}",
                f.name, params, f.return_type
            );
        }
        ts_gen::ir::TypeKind::Variable(v) => {
            let const_ = if v.is_const { "const" } else { "var" };
            eprintln!("{prefix}[{ctx}] {const_} {}: {:?}", v.name, v.type_ref);
        }
        ts_gen::ir::TypeKind::Namespace(ns) => {
            eprintln!(
                "{prefix}[{ctx}] namespace {} ({} declarations)",
                ns.name,
                ns.declarations.len()
            );
            for &id in &ns.declarations {
                print_declaration(gctx, gctx.get_type(id), indent + 1);
            }
        }
    }
}

/// Pretty-print a member for debugging.
fn print_member(member: &ts_gen::ir::Member, indent: usize) {
    let prefix = "  ".repeat(indent);
    match member {
        ts_gen::ir::Member::Getter(g) => {
            let optional = if g.optional { "?" } else { "" };
            eprintln!("{prefix}getter {}{optional}: {:?}", g.js_name, g.type_ref);
        }
        ts_gen::ir::Member::Setter(s) => {
            eprintln!("{prefix}setter {}: {:?}", s.js_name, s.type_ref);
        }
        ts_gen::ir::Member::Method(m) => {
            let params: Vec<_> = m
                .params
                .iter()
                .map(|p| format!("{}: {:?}", p.name, p.type_ref))
                .collect();
            eprintln!(
                "{prefix}method {}({}): {:?}",
                m.js_name,
                params.join(", "),
                m.return_type
            );
        }
        ts_gen::ir::Member::Constructor(c) => {
            let params: Vec<_> = c
                .params
                .iter()
                .map(|p| format!("{}: {:?}", p.name, p.type_ref))
                .collect();
            eprintln!("{prefix}constructor({})", params.join(", "));
        }
        ts_gen::ir::Member::IndexSignature(i) => {
            eprintln!("{prefix}index [{:?}]: {:?}", i.key_type, i.value_type);
        }
        ts_gen::ir::Member::StaticGetter(g) => {
            eprintln!("{prefix}static getter {}: {:?}", g.js_name, g.type_ref);
        }
        ts_gen::ir::Member::StaticSetter(s) => {
            eprintln!("{prefix}static setter {}: {:?}", s.js_name, s.type_ref);
        }
        ts_gen::ir::Member::StaticMethod(m) => {
            let params: Vec<_> = m
                .params
                .iter()
                .map(|p| format!("{}: {:?}", p.name, p.type_ref))
                .collect();
            eprintln!(
                "{prefix}static method {}({}): {:?}",
                m.js_name,
                params.join(", "),
                m.return_type
            );
        }
    }
}
