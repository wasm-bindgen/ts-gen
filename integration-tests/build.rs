use std::path::{Path, PathBuf};

/// For every `integration-tests/tests/<name>.rs` test file, generate Rust
/// bindings from the matching fixture at
/// `<workspace>/tests/fixtures/<name-with-dashes>.d.ts` and `include!` the
/// result into the test crate's lib under `pub mod <name>`. Fixtures that
/// don't have a matching test file are simply not touched.
///
/// Per-fixture configuration may optionally be embedded as a comment
/// header in the `.d.ts` file using the same `//! @ts-gen ...` directive
/// recognized by the snapshot tests:
///   - `--external <mapping>` configures external type mappings.
///   - `--lib-name <name>` overrides the wasm-bindgen `module = "..."`
///     string. When absent, the kebab-case fixture stem is used.
///     The Rust `pub mod` name is always derived (snake_case) from the
///     `--lib-name` value (after stripping protocol prefixes like `node:`).
fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_dir = crate_dir
        .parent()
        .expect("integration-tests is expected to live one level below the workspace root");
    let fixtures_dir = workspace_dir.join("tests/fixtures");
    let tests_dir = crate_dir.join("tests");

    let entries = discover_entries(&tests_dir, &fixtures_dir);

    let mut lib_code = String::new();

    for entry in &entries {
        generate_bindings(entry, &out_dir);
        lib_code.push_str(&format!(
            "include!(concat!(env!(\"OUT_DIR\"), \"/{}.rs\"));\n\n",
            entry.mod_name
        ));
    }

    let lib_file = out_dir.join("_lib.rs");
    std::fs::write(&lib_file, &lib_code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", lib_file.display()));

    // Re-run if test files are added/removed or fixtures change.
    println!("cargo:rerun-if-changed={}", tests_dir.display());
    println!("cargo:rerun-if-changed={}", fixtures_dir.display());
}

struct Entry {
    /// snake_case test stem — used as the Rust `pub mod` name (after
    /// codegen snake-cases the `lib_name`).
    mod_name: String,
    /// kebab-case fixture stem — passed as `--lib-name` so codegen emits
    /// `#[wasm_bindgen(module = "<kebab>")]`. Node resolves this against
    /// `node_modules` at runtime.
    lib_name: String,
    /// Absolute path to the matching `<workspace>/tests/fixtures/<kebab>.d.ts`.
    dts_path: PathBuf,
    /// External type mappings parsed from `//! @ts-gen --external ...`
    /// directives in the fixture, if any.
    externals: Vec<String>,
}

/// Pair every `integration-tests/tests/<name>.rs` with the fixture
/// `<workspace>/tests/fixtures/<kebab>.d.ts`. Panic if a test file has
/// no matching fixture.
fn discover_entries(tests_dir: &Path, fixtures_dir: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();

    let read_dir = std::fs::read_dir(tests_dir)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", tests_dir.display()));

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();

        // Conventionally test stems are snake_case; the matching fixture is
        // the kebab-case form. (Stems without underscores are unchanged.)
        let kebab = stem.replace('_', "-");
        let dts_path = fixtures_dir.join(format!("{kebab}.d.ts"));
        if !dts_path.is_file() {
            panic!(
                "test file {} has no matching fixture at {}",
                path.display(),
                dts_path.display()
            );
        }

        let directive = parse_directive(&dts_path);

        entries.push(Entry {
            mod_name: stem.to_string(),
            // Directive override takes precedence (e.g. `node:console`),
            // otherwise default to the kebab-case fixture stem.
            lib_name: directive.lib_name.unwrap_or(kebab),
            dts_path,
            externals: directive.externals,
        });
    }

    entries.sort_by(|a, b| a.mod_name.cmp(&b.mod_name));
    entries
}

struct Directive {
    /// Value of `--lib-name <name>` if specified.
    lib_name: Option<String>,
    /// All `--external <mapping>` (or `-e <mapping>`) values.
    externals: Vec<String>,
}

/// Parse the `//! @ts-gen ...` directive lines at the top of the fixture
/// file. Stops scanning at the first non-comment line.
fn parse_directive(dts_path: &Path) -> Directive {
    let content = std::fs::read_to_string(dts_path).unwrap_or_default();
    let mut lib_name = None;
    let mut externals = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("//! @ts-gen ") {
            let args = shell_split(rest);
            let mut i = 0;
            while i < args.len() {
                match args[i].as_str() {
                    "--lib-name" | "-l" if i + 1 < args.len() => {
                        lib_name = Some(args[i + 1].clone());
                        i += 2;
                    }
                    "--external" | "-e" if i + 1 < args.len() => {
                        externals.push(args[i + 1].clone());
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
        } else if !trimmed.starts_with("//") && !trimmed.is_empty() {
            break;
        }
    }

    Directive {
        lib_name,
        externals,
    }
}

/// Simple shell-style splitting that respects double quotes.
fn shell_split(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in s.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn generate_bindings(entry: &Entry, out_dir: &Path) {
    let path_refs: Vec<&PathBuf> = vec![&entry.dts_path];

    let (module, mut gctx) = ts_gen::parse(&path_refs, Some(&entry.lib_name))
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", entry.dts_path.display()));

    for mapping in &entry.externals {
        // `--external` accepts a single mapping or a comma-separated list.
        gctx.external_map.add_mappings(mapping);
    }

    for diag in &gctx.diagnostics.diagnostics {
        let level = match diag.level {
            ts_gen::util::diagnostics::DiagnosticLevel::Error => "ERROR",
            ts_gen::util::diagnostics::DiagnosticLevel::Warning => "warning",
            ts_gen::util::diagnostics::DiagnosticLevel::Info => "info",
        };
        println!(
            "cargo:warning=[ts-gen {level}] {}: {}",
            entry.mod_name, diag.message
        );
    }

    let rust_code = ts_gen::codegen::generate(&module, &gctx)
        .unwrap_or_else(|e| panic!("codegen failed for {}: {e}", entry.mod_name));

    // Generated code uses inner attributes (`#![...]`, `//!`) which are only
    // valid at the crate root. Since we `include!` the file into a module,
    // strip them.
    let rust_code = strip_inner_attributes(&rust_code);

    let out_file = out_dir.join(format!("{}.rs", entry.mod_name));
    std::fs::write(&out_file, &rust_code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", out_file.display()));

    println!("cargo:rerun-if-changed={}", entry.dts_path.display());
}

fn strip_inner_attributes(code: &str) -> String {
    code.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("#![") && !trimmed.starts_with("//!")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
