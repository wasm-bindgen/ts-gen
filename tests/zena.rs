//! Tests for `--target zena`.
//!
//! Every `.d.ts` file in `tests/fixtures/` generates Zena bindings compared
//! against `tests/snapshots/<name>/default.zena` (plus `default.main.zena`
//! and `default.app.zena` with `--provides`), beside the snapshot harness's
//! outputs. The ABI is the same under every snapshot configuration, so the
//! Zena target is rendered under the default one. The `@external` declarations of
//! each output must be exactly the imports `--target wat` lists for the
//! same input, with the same Wasm types.
//!
//! With `ZENA` set to a `zena` CLI binary, every output is also compiled
//! with `-t js`, and the compiled module is embedded with `ts-gen embed`,
//! which validates its imports against the ABI and merges it. The worker
//! in `examples/zena-worker/` is compiled, embedded and run in node. The
//! merge needs `wasm-merge` (`WASM_MERGE` or `PATH`) and the run node 24+
//! (`NODE` or `PATH`); a missing tool skips its step locally and fails
//! under `CI`. Without `ZENA` these steps are skipped with a note.
//!
//! To update snapshots:
//!
//! ```sh
//! BLESS=1 cargo test -p ts-gen --test zena
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn ts_gen_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ts-gen"))
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

fn collect_directive_args(fixture: &Path) -> Vec<String> {
    let content = std::fs::read_to_string(fixture).unwrap_or_default();
    let mut args = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("//! @ts-gen ") {
            args.extend(rest.split_whitespace().map(String::from));
        } else if !trimmed.starts_with("//") && !trimmed.is_empty() {
            break;
        }
    }
    args
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
             Run `BLESS=1 cargo test -p ts-gen --test zena` to create it.\n\
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
         Run `BLESS=1 cargo test -p ts-gen --test zena` to update.\n\
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

/// `ts-gen embed` the compiled `module` against `fixture` with the
/// directive's selection flags, and validate the merged module.
fn embed(fixture: &Path, args: &[String], module: &Path, out_dir: &Path, wasm_merge: &Path) {
    let mut selection = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" | "--lib-name" | "--export" | "--external" | "--provides" => {
                selection.push(arg.clone());
                selection.push(args.next().expect("flag value").clone());
            }
            _ => {}
        }
    }
    run(
        Command::new(ts_gen_bin())
            .current_dir(crate_root())
            .arg("embed")
            .arg("--input")
            .arg(fixture)
            .args(&selection)
            .arg("--module")
            .arg(module)
            .arg("--out-dir")
            .arg(out_dir)
            .arg("--wasm-merge")
            .arg(wasm_merge),
        &format!("ts-gen embed {}", module.display()),
    );
    let merged = std::fs::read(out_dir.join(module.file_name().unwrap())).unwrap();
    let mut validator = wasmparser::Validator::new_with_features(
        wasmparser::WasmFeatures::default() | wasmparser::WasmFeatures::GC,
    );
    validator
        .validate_all(&merged)
        .unwrap_or_else(|e| panic!("merged {} does not validate: {e}", module.display()));
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

fn snapshot_test(fixture_name: &str) {
    let root = crate_root();
    let fixture = root.join("tests/fixtures").join(fixture_name);
    let base_name = fixture_name.replace(".d.ts", "");
    let snapshots = root.join("tests/snapshots").join(&base_name);
    let extra_args = collect_directive_args(&fixture);

    let out_dir = TempDir::new(&format!("zena-{base_name}"));
    let out_file = out_dir.0.join("bindings.zena");
    let output = Command::new(ts_gen_bin())
        .current_dir(&root)
        .arg("--input")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_file)
        .args(["--target", "zena"])
        .args(&extra_args)
        .output()
        .expect("Failed to execute ts-gen binary");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // A fixture the Zena target rejects by design (nested `--provides`
        // paths) pins the error instead of an output.
        let error_snap = snapshots.join("default.zena.error");
        if error_snap.is_file() || (bless_enabled() && stderr.contains("--target zena cannot")) {
            let message = stderr
                .lines()
                .find(|l| l.starts_with("Error:"))
                .unwrap_or_default()
                .to_string();
            assert_snapshot(&error_snap, &format!("{message}\n"));
            return;
        }
        panic!("ts-gen --target zena failed for {fixture_name}:\n{stderr}");
    }

    let zena_code = std::fs::read_to_string(&out_file).unwrap();
    assert_snapshot(&snapshots.join("default.zena"), &zena_code);

    let wat_file = out_dir.0.join("bindings.wat");
    run(
        Command::new(ts_gen_bin())
            .current_dir(&root)
            .arg("--input")
            .arg(&fixture)
            .arg("--output")
            .arg(&wat_file)
            .args(["--target", "wat"])
            .args(&extra_args),
        "ts-gen --target wat",
    );
    let wat = wat::parse_file(&wat_file).expect("template assembles");
    let template = wasm_imports(&wat);
    let declared = zena_imports(&zena_code);
    let missing: Vec<_> = template.difference(&declared).collect();
    let extra: Vec<_> = declared.difference(&template).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{fixture_name}: the Zena imports differ from the template\n\
         only in the template: {missing:#?}\nonly in the Zena bindings: {extra:#?}"
    );

    // `--provides` adds the compile entry and an app scaffold; compiling
    // `main.zena` type-checks all three together.
    let entry = if out_dir.0.join("main.zena").is_file() {
        for file in ["main.zena", "app.zena"] {
            let code = std::fs::read_to_string(out_dir.0.join(file)).unwrap();
            assert_snapshot(&snapshots.join(format!("default.{file}")), &code);
        }
        "main.zena"
    } else {
        "bindings.zena"
    };

    let Some(zena) = zena() else {
        return;
    };
    if entry == "bindings.zena" {
        // Compiled as the entry, every export is kept, `await_` included,
        // and `embed` rejects JSPI without a handler to wrap.
        let decl = "@external(\"./builtins.js\", \"await\")\nexport declare function await_(p: externref): externref | null;\n";
        assert!(
            zena_code.contains(decl),
            "{fixture_name}: no await_ declaration"
        );
        std::fs::write(&out_file, zena_code.replace(decl, "")).unwrap();
    }
    let module = out_dir.0.join("module.wasm");
    compile(&zena, &out_dir.0, entry, &module);
    // Imports from other modules are the language's own (`wasm:js-string`,
    // Zena's `env` hooks), which `embed` leaves to the host.
    let abi_modules: BTreeSet<&str> = template.iter().map(|(m, ..)| m.as_str()).collect();
    let compiled = wasm_imports(&std::fs::read(&module).unwrap());
    let stray: Vec<_> = compiled
        .iter()
        .filter(|(m, ..)| abi_modules.contains(m.as_str()))
        .filter(|i| !template.contains(i))
        .collect();
    assert!(
        stray.is_empty(),
        "{fixture_name}: compiled imports not in the template: {stray:#?}"
    );
    let Some(wasm_merge) = find_tool("WASM_MERGE", "wasm-merge") else {
        return;
    };
    embed(
        &fixture,
        &extra_args,
        &module,
        &out_dir.0.join("out"),
        &wasm_merge,
    );
}

#[test]
fn zena_snapshots() {
    let fixtures_dir = crate_root().join("tests/fixtures");
    let mut fixtures: Vec<_> = std::fs::read_dir(&fixtures_dir)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", fixtures_dir.display()))
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().to_string();
            // `<fixture>.provides.d.ts` is a fixture's `--provides` surface,
            // not an input of its own.
            (name.ends_with(".d.ts") && !name.ends_with(".provides.d.ts")).then_some(name)
        })
        .collect();
    fixtures.sort();
    assert!(!fixtures.is_empty(), "No .d.ts fixtures found");

    let mut failures = Vec::new();
    for fixture in &fixtures {
        if let Err(e) = std::panic::catch_unwind(|| snapshot_test(fixture)) {
            let msg = if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "unknown panic".to_string()
            };
            failures.push((fixture.clone(), msg));
        }
    }
    if !failures.is_empty() {
        let mut report = format!("\n{} zena snapshot(s) failed:\n", failures.len());
        for (name, msg) in &failures {
            report.push_str(&format!("\n--- {name} ---\n{msg}\n"));
        }
        panic!("{report}");
    }
    eprintln!("All {} zena snapshot tests passed.", fixtures.len());
}

/// `examples/zena-worker/`, built as its README does and run in node.
#[test]
fn zena_worker() {
    let Some(zena) = zena() else {
        return;
    };
    let root = crate_root();
    let example = root.join("examples/zena-worker");
    let fixture = root.join("tests/fixtures/cloudflare-worker.d.ts");
    let args: Vec<String> = [
        "--input",
        example.join("env.d.ts").to_str().unwrap(),
        "--provides",
        "ExportedHandler.fetch",
    ]
    .map(String::from)
    .into();
    let dir = TempDir::new("zena-worker");
    std::fs::copy(example.join("app.zena"), dir.0.join("app.zena")).unwrap();
    run(
        Command::new(ts_gen_bin())
            .arg("--input")
            .arg(&fixture)
            .args(&args)
            .args(["--target", "zena", "--output"])
            .arg(dir.0.join("bindings.zena")),
        "ts-gen --target zena",
    );
    let module = dir.0.join("main.wasm");
    compile(&zena, &dir.0, "main.zena", &module);
    let Some(wasm_merge) = find_tool("WASM_MERGE", "wasm-merge") else {
        return;
    };
    let out = dir.0.join("out");
    embed(&fixture, &args, &module, &out, &wasm_merge);
    let Some(node) = find_node() else {
        return;
    };
    run(
        Command::new(node)
            .arg(root.join("tests/zena-worker.mjs"))
            .arg(out.join("main.wasm"))
            .arg(out.join("builtins.js")),
        "node zena-worker.mjs",
    );
}

/// `main.zena` and `app.zena` import the bindings by the output's file
/// name, which therefore cannot be either of theirs.
#[test]
fn provides_entry_imports_the_output_file() {
    let root = crate_root();
    let fixture = root.join("tests/fixtures/cloudflare-worker.d.ts");
    let dir = TempDir::new("zena-output-name");
    let ts_gen = |output: &str, target: &str, extra: &[&str]| {
        Command::new(ts_gen_bin())
            .arg("--input")
            .arg(&fixture)
            .args(["--provides", "ExportedHandler.fetch", "--target", target])
            .args(extra)
            .arg("--output")
            .arg(dir.0.join(output))
            .output()
            .expect("Failed to execute ts-gen binary")
    };

    let out = ts_gen("types.zena", "zena", &[]);
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

    for (output, target, extra, message) in [
        (
            "main.zena",
            "zena",
            &[][..],
            "-o main.zena: --provides writes main.zena and app.zena",
        ),
        (
            "app.zena",
            "zena",
            &[][..],
            "-o app.zena: --provides writes main.zena and app.zena",
        ),
        (
            "types.zena",
            "zena",
            &["--provides-app", "super::app"][..],
            "--provides-app and --provides-bindings apply to --target rust only",
        ),
        (
            "types.wat",
            "wat",
            &["--provides-bindings", "super::b"][..],
            "--provides-app and --provides-bindings apply to --target rust only",
        ),
    ] {
        let out = ts_gen(output, target, extra);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && stderr.contains(message),
            "-o {output} --target {target} {extra:?}: {stderr}"
        );
    }
}
