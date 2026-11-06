//! Tests for `ts-gen-zena`, which reads only the ts-gen IR.
//!
//! Every fixture in ts-gen's `tests/fixtures/` is built into an IR with its
//! `//! @ts-gen` options and the `zena` naming preset, as `ts-gen --target
//! ir --naming zena` does, written as text and read back, and generates
//! Zena bindings compared against `tests/snapshots/<name>.zena` here (plus
//! `.main.zena` and `.app.zena` for an IR with provides). The `@external`
//! declarations of each output must be exactly the IR's imports, with the
//! same Wasm types.
//!
//! With `ZENA` set to a `zena` CLI binary, every output is also compiled
//! with `-t js`, and the compiled module is embedded against the IR with
//! `ts-gen embed --ir`, which validates its imports and merges it. The
//! worker in `examples/zena-worker/` is built through ts-gen, this
//! generator, Zena and `ts-gen embed --ir`, and run in node. The merge
//! needs `wasm-merge` (`WASM_MERGE` or `PATH`) and the run node 24+
//! (`NODE` or `PATH`); a missing tool skips its step locally and fails
//! under `CI`. Without `ZENA` these steps are skipped with a note.
//!
//! To update snapshots:
//!
//! ```sh
//! BLESS=1 cargo test -p ts-gen-zena
//! ```

use std::collections::{BTreeSet, HashSet};
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
/// indices replaced by `$anyrefs`, the only one the IR uses.
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

/// Every import a language module may take from the bindings of `ir`, by
/// IR.md: one per member, the three `bindings.wasm` functions every IR
/// has, and the `builtins.js` `undefined` global and `await`.
fn ir_imports(ir: &ts_gen_abi::ir::Ir) -> BTreeSet<ImportSig> {
    use ts_gen_abi::ir::{Member, Type};
    let val = |t: &Type| match t {
        Type::Void => None,
        Type::F64 => Some("f64"),
        Type::I64 => Some("i64"),
        Type::Bool => Some("i32"),
        Type::Js(_) => Some("externref"),
    };
    let func = |m: &Member| {
        let mut sig = String::from("(func");
        if m.kind.has_receiver() {
            sig.push_str(" (param externref)");
        }
        for (n, p) in m.params.iter().enumerate() {
            let ty = if m.variadic && n + 1 == m.params.len() {
                "(ref null $anyrefs)"
            } else {
                val(&p.ty).expect("a parameter has a value")
            };
            sig.push_str(&format!(" (param {ty})"));
        }
        if let Some(r) = val(&m.result) {
            sig.push_str(&format!(" (result {r})"));
        }
        sig.push(')');
        sig
    };
    let module = |owner: &Option<String>, m: &Member| {
        m.module
            .clone()
            .or_else(|| owner.clone())
            .unwrap_or_else(|| ts_gen_abi::BINDINGS_SPECIFIER.to_string())
    };
    let mut imports = BTreeSet::new();
    for c in &ir.classes {
        for m in &c.members {
            imports.insert((
                module(&c.module, m),
                format!("{}.{}", c.name, m.name),
                func(m),
            ));
        }
    }
    for f in &ir.functions {
        let m = &f.member;
        imports.insert((module(&f.module, m), m.name.clone(), func(m)));
    }
    let bindings = ts_gen_abi::BINDINGS_SPECIFIER;
    let builtins = ts_gen_abi::BUILTINS_SPECIFIER;
    for (module, name, sig) in [
        (
            bindings,
            "JsValue.is_undefined",
            "(func (param externref) (result i32))",
        ),
        (bindings, "JsValue.undefined", "(func (result externref))"),
        (
            bindings,
            "Number.value",
            "(func (param externref) (result f64))",
        ),
        (builtins, ts_gen_abi::UNDEFINED_GLOBAL, "(global externref)"),
        (
            builtins,
            "await",
            "(func (param externref) (result externref))",
        ),
    ] {
        imports.insert((module.to_string(), name.to_string(), sig.to_string()));
    }
    imports
}

/// A fixture's `//! @ts-gen` options, as `tests/ir_rust.rs` reads them.
#[derive(Default)]
struct Args {
    inputs: Vec<String>,
    lib_name: Option<String>,
    exports: Vec<String>,
    externals: Vec<String>,
    error_type: Option<String>,
    error_enum: Option<String>,
    errors_as_error: bool,
    fallible: bool,
    provides: Option<String>,
    no_web_sys: bool,
}

fn directive_args(fixture: &Path) -> Vec<String> {
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

fn parse_args(tokens: &[String]) -> Args {
    let mut args = Args::default();
    let mut i = 0;
    let values = |i: &mut usize| {
        let mut out = Vec::new();
        while *i + 1 < tokens.len() && !tokens[*i + 1].starts_with('-') {
            *i += 1;
            out.push(tokens[*i].clone());
        }
        out
    };
    while i < tokens.len() {
        let flag = tokens[i].as_str();
        let vals = values(&mut i);
        let one = || vals.first().cloned();
        match flag {
            "--input" | "-i" => args.inputs.extend(vals.iter().cloned()),
            "--export" | "-E" => args.exports.extend(vals.iter().cloned()),
            "--external" | "-e" => args.externals.extend(vals.iter().cloned()),
            "--lib-name" => args.lib_name = one(),
            "--error-type" => args.error_type = one(),
            "--error-enum" => args.error_enum = Some(one().unwrap_or("JsException".into())),
            "--errors-as-error" => args.errors_as_error = true,
            "--fallible" => args.fallible = true,
            "--provides" => args.provides = one(),
            "--no-web-sys" => args.no_web_sys = true,
            other => panic!("unsupported fixture flag {other}"),
        }
        i += 1;
    }
    args
}

fn dts_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for e in entries {
            dts_files(&e, out);
        }
    } else if path.to_string_lossy().ends_with(".d.ts") {
        out.push(path.to_path_buf());
    }
}

/// The CLI's `resolve_exports`, with paths read from the repository root.
fn resolve_exports(
    args: &Args,
    inputs: &[PathBuf],
    gctx: &ts_gen::context::GlobalContext,
) -> HashSet<ts_gen::codegen::ExportSpec> {
    use ts_gen::codegen::ExportSpec;
    let is_decl_name = |spec: &str| {
        spec.split('.').all(|seg| {
            let mut chars = seg.chars();
            matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
    };
    let mut exports = HashSet::new();
    for spec in &args.exports {
        let path = repo_root().join(spec);
        if path.exists() {
            let mut files = Vec::new();
            dts_files(&path, &mut files);
            for f in files {
                exports.insert(ExportSpec::File(f.canonicalize().unwrap_or(f)));
            }
        } else if gctx.find_module(spec).is_none()
            && !gctx.ambient_module_scopes.contains_key(spec)
            && args.lib_name.as_deref() != Some(spec.as_str())
            && is_decl_name(spec)
        {
            exports.insert(ExportSpec::Decl(spec.clone()));
        } else {
            exports.insert(ExportSpec::Module(spec.clone()));
        }
    }
    if args.exports.is_empty() {
        for p in inputs {
            exports.insert(ExportSpec::File(p.canonicalize().unwrap_or(p.clone())));
        }
    }
    exports
}

/// The IR of a fixture under its options, as `ts-gen --target ir --naming
/// zena` writes it.
fn fixture_ir(fixture: &Path) -> String {
    let root = repo_root();
    let args = parse_args(&directive_args(fixture));
    let mut inputs = vec![fixture.to_path_buf()];
    inputs.extend(args.inputs.iter().map(|p| root.join(p)));
    let (module, mut gctx) = ts_gen::parse::parse_dts_files(&inputs, args.lib_name.as_deref())
        .unwrap_or_else(|e| panic!("parse {}: {e:#}", fixture.display()));
    if args.no_web_sys {
        gctx.external_map.clear_defaults();
    }
    for m in &args.externals {
        gctx.external_map.add_mappings(m);
    }
    let exports = resolve_exports(&args, &inputs, &gctx);
    let provided = match &args.provides {
        Some(spec) => {
            let path = root.join(spec);
            let spec = if path.is_file() {
                path.to_string_lossy().into_owned()
            } else {
                spec.clone()
            };
            ts_gen::provides::resolve(&spec, &module, &mut gctx)
                .unwrap_or_else(|e| panic!("--provides {spec}: {e:#}"))
        }
        None => Vec::new(),
    };
    let mut options = ts_gen::codegen::GenerateOptions::new()
        .errors_as_error(args.errors_as_error)
        .fallible(args.fallible)
        .exports(exports)
        .naming(ts_gen::abi::Naming::preset(ts_gen::abi::Preset::Zena));
    if let Some(path) = &args.error_type {
        options = options.error_type(path);
    }
    if let Some(name) = &args.error_enum {
        options = options.error_enum(name);
    }
    let ir = ts_gen::abi::build_ir(&module, &gctx, &options, &provided);
    ts_gen_abi::text::emit(&ir)
}

/// `ts-gen`, run from the repository root.
fn ts_gen() -> Command {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.current_dir(repo_root())
        .args(["run", "-q", "-p", "ts-gen", "--"]);
    cmd
}

/// `ts-gen embed --ir` the compiled `module` against `ir_file` into
/// `out_dir`, and validate the merged module.
fn embed(ir_file: &Path, module: &Path, out_dir: &Path, wasm_merge: &Path) {
    run(
        ts_gen()
            .arg("embed")
            .arg("--ir")
            .arg(ir_file)
            .arg("-m")
            .arg(module)
            .arg("-o")
            .arg(out_dir)
            .arg("--wasm-merge")
            .arg(wasm_merge),
        &format!("ts-gen embed --ir {}", ir_file.display()),
    );
    let merged = std::fs::read(out_dir.join(module.file_name().unwrap())).unwrap();
    let mut validator = wasmparser::Validator::new_with_features(
        wasmparser::WasmFeatures::default() | wasmparser::WasmFeatures::GC,
    );
    validator
        .validate_all(&merged)
        .unwrap_or_else(|e| panic!("merged {} does not validate: {e}", module.display()));
}

/// Generate into `dir` as the CLI does, returning the bindings.
fn generate(ir: &ts_gen_abi::ir::Ir, dir: &Path) -> anyhow::Result<String> {
    let zena = ts_gen_zena::generate(ir, "bindings.zena")?;
    std::fs::write(dir.join("bindings.zena"), &zena.bindings).unwrap();
    if let Some((main, app)) = &zena.provides {
        std::fs::write(dir.join("main.zena"), main).unwrap();
        std::fs::write(dir.join("app.zena"), app).unwrap();
    }
    Ok(zena.bindings)
}

fn snapshot_test(name: &str, fixture: &Path) {
    let snapshots = crate_root().join("tests/snapshots");
    let text = fixture_ir(fixture);
    let ir = ts_gen_abi::text::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));

    let out_dir = TempDir::new(&format!("zena-{name}"));
    let ir_file = out_dir.0.join("bindings.ir");
    std::fs::write(&ir_file, &text).unwrap();
    let zena_code = match generate(&ir, &out_dir.0) {
        Ok(code) => code,
        // An IR the generator rejects by design (nested provided paths)
        // pins the error instead of an output.
        Err(e) => {
            assert_snapshot(
                &snapshots.join(format!("{name}.zena.error")),
                &format!("Error: {e}\n"),
            );
            return;
        }
    };
    assert_snapshot(&snapshots.join(format!("{name}.zena")), &zena_code);

    let ir_imports = ir_imports(&ir);
    let declared = zena_imports(&zena_code);
    let missing: Vec<_> = ir_imports.difference(&declared).collect();
    let extra: Vec<_> = declared.difference(&ir_imports).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{name}: the Zena imports differ from the IR's\n\
         only in the IR: {missing:#?}\nonly in the Zena bindings: {extra:#?}"
    );

    // With provides, the compile entry and an app scaffold; compiling
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
        // and `embed` rejects JSPI without a provide to wrap.
        let decl = "@external(\"./builtins.js\", \"await\")\nexport declare function await_(p: externref): externref | null;\n";
        assert!(zena_code.contains(decl), "{name}: no await_ declaration");
        std::fs::write(&out_file, zena_code.replace(decl, "")).unwrap();
    }
    let module = out_dir.0.join("module.wasm");
    compile(&zena, &out_dir.0, entry, &module);
    // Imports from other modules are the language's own (`wasm:js-string`,
    // Zena's `env` hooks), which `embed` leaves to the host.
    let ir_modules: BTreeSet<&str> = ir_imports.iter().map(|(m, ..)| m.as_str()).collect();
    let compiled = wasm_imports(&std::fs::read(&module).unwrap());
    let stray: Vec<_> = compiled
        .iter()
        .filter(|(m, ..)| ir_modules.contains(m.as_str()))
        .filter(|i| !ir_imports.contains(i))
        .collect();
    assert!(
        stray.is_empty(),
        "{name}: compiled imports not in the IR: {stray:#?}"
    );
    let Some(wasm_merge) = find_tool("WASM_MERGE", "wasm-merge") else {
        return;
    };
    embed(&ir_file, &module, &out_dir.0.join("out"), &wasm_merge);
}

/// Every fixture, by name, as ts-gen's snapshot harness finds them.
fn fixtures() -> Vec<(String, PathBuf)> {
    let dir = repo_root().join("tests/fixtures");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let file = path.file_name()?.to_string_lossy().into_owned();
            // `<fixture>.provides.d.ts` is a fixture's `--provides` surface.
            let name = file
                .strip_suffix(".d.ts")
                .filter(|_| !file.ends_with(".provides.d.ts"))?;
            Some((name.to_string(), path.clone()))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn zena_snapshots() {
    let fixtures = fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let mut failures = Vec::new();
    for (name, fixture) in &fixtures {
        if let Err(e) = std::panic::catch_unwind(|| snapshot_test(name, fixture)) {
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
    // A snapshot whose fixture is gone is stale.
    let names: BTreeSet<&str> = fixtures.iter().map(|(n, _)| n.as_str()).collect();
    for entry in std::fs::read_dir(crate_root().join("tests/snapshots")).unwrap() {
        let file = entry.unwrap().file_name().to_string_lossy().into_owned();
        let stem = file.split('.').next().unwrap_or_default();
        assert!(
            names.contains(stem),
            "stale snapshot {file}: no fixture {stem}"
        );
    }
    eprintln!("All {} zena snapshot tests passed.", fixtures.len());
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
    let ir_file = dir.0.join("worker.ir");
    run(
        ts_gen()
            .arg("--input")
            .arg(root.join("tests/fixtures/cloudflare-worker.d.ts"))
            .arg("--input")
            .arg(example.join("env.d.ts"))
            .args([
                "--provides",
                "ExportedHandler.fetch",
                "--target",
                "ir",
                "--naming",
                "zena",
                "--output",
            ])
            .arg(&ir_file),
        "ts-gen --target ir --naming zena",
    );
    std::fs::copy(example.join("app.zena"), dir.0.join("app.zena")).unwrap();
    run(
        Command::new(env!("CARGO_BIN_EXE_ts-gen-zena"))
            .arg(&ir_file)
            .arg("-o")
            .arg(dir.0.join("bindings.zena")),
        "ts-gen-zena",
    );
    let module = dir.0.join("main.wasm");
    compile(&zena, &dir.0, "main.zena", &module);
    let Some(wasm_merge) = find_tool("WASM_MERGE", "wasm-merge") else {
        return;
    };
    let out = dir.0.join("out");
    embed(&ir_file, &module, &out, &wasm_merge);
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
    let ir = repo_root().join("tests/snapshots/cloudflare-worker/zena.ir");
    let dir = TempDir::new("zena-output-name");
    let generate = |output: &str| {
        Command::new(env!("CARGO_BIN_EXE_ts-gen-zena"))
            .arg(&ir)
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
