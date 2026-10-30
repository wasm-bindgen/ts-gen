//! Tests for `ts-gen-zena`, which reads only ABI templates.
//!
//! Its inputs are the templates ts-gen's snapshot harness writes for every
//! fixture, `tests/snapshots/<name>/default.wat` at the repository root,
//! so this crate tests the generator the way any consumer of the format
//! runs it. Each generates Zena bindings compared against
//! `tests/snapshots/<name>.zena` here (plus `.main.zena` and `.app.zena`
//! for a template with handlers). The `@external` declarations of each
//! output must be exactly the template's imports, with the same Wasm types.
//!
//! With `ZENA` set to a `zena` CLI binary, every output is also compiled
//! with `-t js`, and the compiled module is embedded against the template,
//! which validates its imports against the ABI and merges it. The worker
//! in `examples/zena-worker/` is built through ts-gen, this generator,
//! Zena and `ts-gen embed --abi`, and run in node. The merge needs
//! `wasm-merge` (`WASM_MERGE` or `PATH`) and the run node 24+ (`NODE` or
//! `PATH`); a missing tool skips its step locally and fails under `CI`.
//! Without `ZENA` these steps are skipped with a note.
//!
//! To update snapshots:
//!
//! ```sh
//! BLESS=1 cargo test -p ts-gen-zena
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn repo_root() -> PathBuf {
    crate_root().join("../..").canonicalize().unwrap()
}

/// A fresh output directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ts-gen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("Failed to create {}: {e}", dir.display()));
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bless_enabled() -> bool {
    std::env::var("BLESS").is_ok_and(|v| v == "1" || v == "true")
}

fn assert_snapshot(snap_path: &Path, actual: &str) {
    if bless_enabled() {
        if let Some(parent) = snap_path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create snapshot directory");
        }
        std::fs::write(snap_path, actual)
            .unwrap_or_else(|e| panic!("Failed to write snapshot {}: {e}", snap_path.display()));
        eprintln!("  blessed: {}", snap_path.display());
        return;
    }

    let expected = std::fs::read_to_string(snap_path).unwrap_or_else(|e| {
        panic!(
            "Snapshot file not found: {}\n\
             Run `BLESS=1 cargo test -p ts-gen-zena` to create it.\n\
             Error: {e}",
            snap_path.display()
        )
    });
    if actual == expected {
        return;
    }

    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(expected.as_str(), actual);
    let mut diff_output = String::new();
    let snap_name = snap_path.file_name().unwrap().to_string_lossy();
    diff_output.push_str(&format!("\n--- expected: {snap_name}\n+++ actual\n"));
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            diff_output.push_str("...\n");
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                diff_output.push_str(&format!("{sign}{change}"));
                if change.missing_newline() {
                    diff_output.push('\n');
                }
            }
        }
    }
    panic!(
        "Snapshot mismatch for {}\n\
         Run `BLESS=1 cargo test -p ts-gen-zena` to update.\n\
         {diff_output}",
        snap_path.display()
    );
}

/// A tool from `$VAR`, else `PATH`. Absent tools fail under `CI` so the
/// steps they drive are never silently skipped there.
fn find_tool(var: &str, name: &str) -> Option<PathBuf> {
    let found = match std::env::var(var) {
        Ok(path) => {
            let p = PathBuf::from(&path);
            assert!(p.exists(), "{var}={path} does not exist");
            Some(p)
        }
        Err(_) => Command::new("which")
            .arg(name)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
            .filter(|p| !p.as_os_str().is_empty()),
    };
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "{name} not found; CI must provide it (set {var})"
        );
        eprintln!("  {name} not found, skipping");
    }
    found
}

/// Node 24 is the first with `WebAssembly.promising` and
/// `importedStringConstants` unflagged.
fn find_node() -> Option<PathBuf> {
    let node = find_tool("NODE", "node")?;
    let version = run(Command::new(&node).arg("--version"), "node --version");
    let major: u32 = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    if major < 24 {
        assert!(
            std::env::var_os("CI").is_none(),
            "node {} is too old for the run step; need 24+ (set NODE)",
            version.trim()
        );
        eprintln!(
            "  node {} is too old for the run step, skipping",
            version.trim()
        );
        return None;
    }
    Some(node)
}

fn zena() -> Option<PathBuf> {
    let zena = std::env::var_os("ZENA").map(PathBuf::from);
    if zena.is_none() {
        eprintln!("  ZENA not set, skipping the Zena compile and embed steps");
    }
    zena
}

fn run(cmd: &mut Command, what: &str) -> String {
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("Failed to execute {what}: {e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{what} failed ({:?}):\n{stdout}\n{stderr}",
        output.status.code()
    );
    stdout.into_owned()
}

/// Compile `entry` in `dir` to `out` with the Zena CLI at `zena`, as the
/// bindings are meant to be compiled for `embed`.
fn compile(zena: &Path, dir: &Path, entry: &str, out: &Path) {
    run(
        Command::new(zena)
            .current_dir(dir)
            // A development build of the CLI would otherwise read the
            // standard library from its own checkout.
            .env("ZENA_REPO_ROOT", "")
            .arg("build")
            .arg(entry)
            .args(["-t", "js"])
            .args(["--imported-string-constants", "wasm:js/string-constants"])
            .arg("-o")
            .arg(out),
        &format!("zena build {}", dir.join(entry).display()),
    );
}

/// A Wasm import as `module`, `name` and its type in WAT, ref type
/// indices replaced by `$anyrefs`, the only one the template uses.
type ImportSig = (String, String, String);

fn wasm_imports(bytes: &[u8]) -> BTreeSet<ImportSig> {
    use wasmparser::{CompositeInnerType, Parser, Payload, TypeRef, ValType};
    let val = |t: ValType| match t {
        ValType::Ref(r) if r.is_concrete_type_ref() => {
            if r.is_nullable() {
                "(ref null $anyrefs)".to_string()
            } else {
                "(ref $anyrefs)".to_string()
            }
        }
        ValType::Ref(r) if r == wasmparser::RefType::EXTERNREF => "externref".to_string(),
        ValType::Ref(r) if r == wasmparser::RefType::ANYREF => "anyref".to_string(),
        other => other.to_string(),
    };
    let mut funcs = Vec::new();
    let mut imports = BTreeSet::new();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.expect("parse wasm") {
            Payload::TypeSection(types) => {
                for group in types {
                    for ty in group.expect("type").into_types() {
                        funcs.push(match &ty.composite_type.inner {
                            CompositeInnerType::Func(f) => {
                                let params: String = f
                                    .params()
                                    .iter()
                                    .map(|&p| format!(" (param {})", val(p)))
                                    .collect();
                                let results: String = f
                                    .results()
                                    .iter()
                                    .map(|&r| format!(" (result {})", val(r)))
                                    .collect();
                                Some(format!("(func{params}{results})"))
                            }
                            _ => None,
                        });
                    }
                }
            }
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import.expect("import");
                    let ty = match import.ty {
                        TypeRef::Func(i) => funcs[i as usize].clone().expect("func type"),
                        TypeRef::Global(g) => format!("(global {})", val(g.content_type)),
                        other => format!("{other:?}"),
                    };
                    imports.insert((import.module.to_string(), import.name.to_string(), ty));
                }
            }
            _ => {}
        }
    }
    imports
}

/// The `@external` declarations of a generated file, in the same form.
fn zena_imports(code: &str) -> BTreeSet<ImportSig> {
    let ty = |t: &str| match t.trim() {
        "f64" => "f64",
        "i64" => "i64",
        "boolean" => "i32",
        "externref" | "externref | null" => "externref",
        "FixedArray<anyref> | null" => "(ref null $anyrefs)",
        other => panic!("unexpected Zena type in an @external declaration: {other}"),
    };
    let mut imports = BTreeSet::new();
    let mut lines = code.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.strip_prefix("@external(") else {
            continue;
        };
        let (module, rest) = quoted(rest);
        let (name, _) = quoted(rest.trim_start_matches(", "));
        let decl = lines.next().expect("declaration after @external");
        let decl = decl.trim_start_matches("export ");
        let sig = if let Some(rest) = decl.strip_prefix("declare let ") {
            let (_, t) = rest.trim_end_matches(';').split_once(": ").unwrap();
            format!("(global {})", ty(t))
        } else {
            let rest = decl.strip_prefix("declare function ").expect(decl);
            let (_, rest) = rest.split_once('(').unwrap();
            let (params, result) = rest.rsplit_once("): ").unwrap();
            let params: String = params
                .split(", ")
                .filter(|p| !p.is_empty())
                .map(|p| format!(" (param {})", ty(p.split_once(": ").unwrap().1)))
                .collect();
            let result = match result.trim_end_matches(';') {
                "void" => String::new(),
                r => format!(" (result {})", ty(r)),
            };
            format!("(func{params}{result})")
        };
        imports.insert((module, name, sig));
    }
    imports
}

/// A `"..."` string literal at the start of `s` and what follows it.
fn quoted(s: &str) -> (String, &str) {
    let mut out = String::new();
    let mut chars = s.strip_prefix('"').expect("string literal").char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return (out, &s[i + 2..]),
            '\\' => out.push(chars.next().expect("escape").1),
            c => out.push(c),
        }
    }
    panic!("unterminated string literal: {s}");
}

/// `ts-gen embed` the compiled `module` against `abi`, as `embed --abi`
/// does, and validate the merged module.
fn embed(abi: &ts_gen_abi::Abi, module: &Path, out_dir: &Path, wasm_merge: &Path) {
    use ts_gen::abi::embed;
    let bytes = std::fs::read(module).unwrap();
    let specifier = format!("./{}", module.file_name().unwrap().to_string_lossy());
    let options = embed::Options::new(&specifier);
    let out = embed::embed(abi, &bytes, &options)
        .unwrap_or_else(|e| panic!("embed {}: {e:#}", module.display()));
    let bindings = embed::assemble(&out.bindings_wat).expect("bindings.wasm");
    let wrapper = embed::assemble(&out.wrapper_wat).expect("wrapper.wasm");
    let merged = embed::merge(wasm_merge, &out.module, &specifier, &bindings, &wrapper)
        .unwrap_or_else(|e| panic!("merge {}: {e:#}", module.display()));
    let merged = embed::postprocess(&merged, &out.handlers, &specifier).expect("postprocess");
    let mut validator = wasmparser::Validator::new_with_features(
        wasmparser::WasmFeatures::default() | wasmparser::WasmFeatures::GC,
    );
    validator
        .validate_all(&merged)
        .unwrap_or_else(|e| panic!("merged {} does not validate: {e}", module.display()));
    std::fs::create_dir_all(out_dir).unwrap();
    std::fs::write(out_dir.join(module.file_name().unwrap()), &merged).unwrap();
    std::fs::write(out_dir.join("builtins.js"), &out.builtins_js).unwrap();
}

/// Generate into `dir` as the CLI does, returning the bindings.
fn generate(abi: &ts_gen_abi::Abi, dir: &Path) -> anyhow::Result<String> {
    let zena = ts_gen_zena::generate(abi, "bindings.zena")?;
    std::fs::write(dir.join("bindings.zena"), &zena.bindings).unwrap();
    if let Some((main, app)) = &zena.provides {
        std::fs::write(dir.join("main.zena"), main).unwrap();
        std::fs::write(dir.join("app.zena"), app).unwrap();
    }
    Ok(zena.bindings)
}

fn snapshot_test(name: &str, template: &Path) {
    let snapshots = crate_root().join("tests/snapshots");
    let text = std::fs::read_to_string(template).unwrap();
    let abi = ts_gen_abi::format::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));

    let out_dir = TempDir::new(&format!("zena-{name}"));
    let zena_code = match generate(&abi, &out_dir.0) {
        Ok(code) => code,
        // A template the generator rejects by design (nested provided
        // paths) pins the error instead of an output.
        Err(e) => {
            assert_snapshot(
                &snapshots.join(format!("{name}.zena.error")),
                &format!("Error: {e}\n"),
            );
            return;
        }
    };
    assert_snapshot(&snapshots.join(format!("{name}.zena")), &zena_code);

    let template_imports = wasm_imports(&wat::parse_str(&text).expect("template assembles"));
    let declared = zena_imports(&zena_code);
    let missing: Vec<_> = template_imports.difference(&declared).collect();
    let extra: Vec<_> = declared.difference(&template_imports).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{name}: the Zena imports differ from the template\n\
         only in the template: {missing:#?}\nonly in the Zena bindings: {extra:#?}"
    );

    // With handlers, the compile entry and an app scaffold; compiling
    // `main.zena` type-checks all three together.
    let entry = if out_dir.0.join("main.zena").is_file() {
        for file in ["main.zena", "app.zena"] {
            let code = std::fs::read_to_string(out_dir.0.join(file)).unwrap();
            let suffix = file.trim_end_matches(".zena");
            assert_snapshot(&snapshots.join(format!("{name}.{suffix}.zena")), &code);
        }
        "main.zena"
    } else {
        "bindings.zena"
    };

    let Some(zena) = zena() else {
        return;
    };
    let out_file = out_dir.0.join("bindings.zena");
    if entry == "bindings.zena" {
        // Compiled as the entry, every export is kept, `await_` included,
        // and `embed` rejects JSPI without a handler to wrap.
        let decl = "@external(\"./builtins.js\", \"await\")\nexport declare function await_(p: externref): externref | null;\n";
        assert!(zena_code.contains(decl), "{name}: no await_ declaration");
        std::fs::write(&out_file, zena_code.replace(decl, "")).unwrap();
    }
    let module = out_dir.0.join("module.wasm");
    compile(&zena, &out_dir.0, entry, &module);
    // Imports from other modules are the language's own (`wasm:js-string`,
    // Zena's `env` hooks), which `embed` leaves to the host.
    let abi_modules: BTreeSet<&str> = template_imports.iter().map(|(m, ..)| m.as_str()).collect();
    let compiled = wasm_imports(&std::fs::read(&module).unwrap());
    let stray: Vec<_> = compiled
        .iter()
        .filter(|(m, ..)| abi_modules.contains(m.as_str()))
        .filter(|i| !template_imports.contains(i))
        .collect();
    assert!(
        stray.is_empty(),
        "{name}: compiled imports not in the template: {stray:#?}"
    );
    let Some(wasm_merge) = find_tool("WASM_MERGE", "wasm-merge") else {
        return;
    };
    embed(&abi, &module, &out_dir.0.join("out"), &wasm_merge);
}

/// Every fixture's template, by fixture name.
fn templates() -> Vec<(String, PathBuf)> {
    let dir = repo_root().join("tests/snapshots");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let template = entry.path().join("default.wat");
            template
                .is_file()
                .then(|| (entry.file_name().to_string_lossy().into_owned(), template))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn zena_snapshots() {
    let templates = templates();
    assert!(!templates.is_empty(), "No templates found");

    let mut failures = Vec::new();
    for (name, template) in &templates {
        if let Err(e) = std::panic::catch_unwind(|| snapshot_test(name, template)) {
            let msg = if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "unknown panic".to_string()
            };
            failures.push((name.clone(), msg));
        }
    }
    if !failures.is_empty() {
        let mut report = format!("\n{} zena snapshot(s) failed:\n", failures.len());
        for (name, msg) in &failures {
            report.push_str(&format!("\n--- {name} ---\n{msg}\n"));
        }
        panic!("{report}");
    }
    // A snapshot whose template is gone is stale.
    let names: BTreeSet<&str> = templates.iter().map(|(n, _)| n.as_str()).collect();
    for entry in std::fs::read_dir(crate_root().join("tests/snapshots")).unwrap() {
        let file = entry.unwrap().file_name().to_string_lossy().into_owned();
        let stem = file.split('.').next().unwrap_or_default();
        assert!(
            names.contains(stem),
            "stale snapshot {file}: no template {stem}"
        );
    }
    eprintln!("All {} zena snapshot tests passed.", templates.len());
}

/// `examples/zena-worker/`, built as its README does and run in node.
#[test]
fn zena_worker() {
    let Some(zena) = zena() else {
        return;
    };
    let root = repo_root();
    let example = root.join("examples/zena-worker");
    let dir = TempDir::new("zena-worker");
    let template = dir.0.join("worker.abi.wat");
    run(
        Command::new(env!("CARGO"))
            .current_dir(&root)
            .args(["run", "-q", "-p", "ts-gen", "--"])
            .arg("--input")
            .arg(root.join("tests/fixtures/cloudflare-worker.d.ts"))
            .arg("--input")
            .arg(example.join("env.d.ts"))
            .args([
                "--provides",
                "ExportedHandler.fetch",
                "--target",
                "wat",
                "--output",
            ])
            .arg(&template),
        "ts-gen --target wat",
    );
    std::fs::copy(example.join("app.zena"), dir.0.join("app.zena")).unwrap();
    run(
        Command::new(env!("CARGO_BIN_EXE_ts-gen-zena"))
            .arg(&template)
            .arg("-o")
            .arg(dir.0.join("bindings.zena")),
        "ts-gen-zena",
    );
    let module = dir.0.join("main.wasm");
    compile(&zena, &dir.0, "main.zena", &module);
    let Some(wasm_merge) = find_tool("WASM_MERGE", "wasm-merge") else {
        return;
    };
    let abi = ts_gen_abi::format::parse(&std::fs::read_to_string(&template).unwrap()).unwrap();
    let out = dir.0.join("out");
    embed(&abi, &module, &out, &wasm_merge);
    let Some(node) = find_node() else {
        return;
    };
    run(
        Command::new(node)
            .arg(crate_root().join("tests/worker.mjs"))
            .arg(out.join("main.wasm"))
            .arg(out.join("builtins.js")),
        "node worker.mjs",
    );
}

/// `main.zena` and `app.zena` import the bindings by the output's file
/// name, which therefore cannot be either of theirs.
#[test]
fn provides_entry_imports_the_output_file() {
    let template = repo_root().join("tests/snapshots/cloudflare-worker/default.wat");
    let dir = TempDir::new("zena-output-name");
    let generate = |output: &str| {
        Command::new(env!("CARGO_BIN_EXE_ts-gen-zena"))
            .arg(&template)
            .arg("-o")
            .arg(dir.0.join(output))
            .output()
            .expect("Failed to execute ts-gen-zena")
    };

    let out = generate("types.zena");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for file in ["main.zena", "app.zena"] {
        let code = std::fs::read_to_string(dir.0.join(file)).unwrap();
        assert!(code.contains("from './types.zena';"), "{file}:\n{code}");
    }
    if let Some(zena) = zena() {
        compile(&zena, &dir.0, "main.zena", &dir.0.join("main.wasm"));
    }
    for output in ["main.zena", "app.zena"] {
        let out = generate(output);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && stderr.contains("writes main.zena and app.zena"),
            "-o {output}: {stderr}"
        );
    }
}
