//! Embed tests for `ts-gen embed`.
//!
//! Each `.wat` file in `tests/fixtures/embed/` is assembled and embedded
//! against a TypeScript declaration file. Generated outputs (bindings.wat,
//! wrapper.wat, builtins.js) are compared against snapshots in
//! `tests/snapshots/embed/`, then the merged module is validated and run
//! in node by `embed.mjs`.
//!
//! The merge needs binaryen's `wasm-merge` (`WASM_MERGE` or `PATH`) and the
//! run needs node 24+ (`NODE` or `PATH`). A missing tool skips its step
//! locally and fails under `CI`.
//!
//! To bless snapshots: `BLESS=1 cargo test --test embed`

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use similar::{ChangeTag, TextDiff};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn bless() -> bool {
    std::env::var("BLESS").is_ok_and(|v| v == "1" || v == "true")
}

fn assert_snapshot(snap_path: &Path, actual: &str) {
    if bless() {
        if let Some(parent) = snap_path.parent() {
            std::fs::create_dir_all(parent).expect("create snapshot dir");
        }
        std::fs::write(snap_path, actual).expect("write snapshot");
        eprintln!("  blessed: {}", snap_path.display());
        return;
    }

    let expected = std::fs::read_to_string(snap_path).unwrap_or_else(|_| {
        panic!(
            "Snapshot not found: {}\nRun `BLESS=1 cargo test --test embed` to create.",
            snap_path.display()
        )
    });

    if actual == expected {
        return;
    }

    let diff = TextDiff::from_lines(expected.as_str(), actual);
    let mut out = format!("\n--- expected: {}\n+++ actual\n", snap_path.display());
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            out.push_str("...\n");
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                out.push_str(&format!("{sign}{change}"));
                if change.missing_newline() {
                    out.push('\n');
                }
            }
        }
    }
    panic!("Snapshot mismatch\n{out}");
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
        Err(_) => std::process::Command::new("which")
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
    let out = std::process::Command::new(&node)
        .arg("--version")
        .output()
        .expect("node --version");
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let major: u32 = version
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    if major < 24 {
        assert!(
            std::env::var_os("CI").is_none(),
            "node {version} is too old for the embed run step; need 24+ (set NODE)"
        );
        eprintln!("  node {version} is too old for the run step (need 24+), skipping");
        return None;
    }
    Some(node)
}

/// The ABI with every declared module selected, as `--export <specifier>`
/// for each would.
fn build_abi(
    ir: &ts_gen::ir::Module,
    gctx: &ts_gen::context::GlobalContext,
    provided: &[ts_gen::provides::ProvidedFn],
) -> ts_gen::abi::Abi {
    let exports = ir
        .types
        .iter()
        .filter_map(|&id| match &gctx.get_type(id).module_context {
            ts_gen::ir::ModuleContext::Module(m) => {
                Some(ts_gen::codegen::ExportSpec::Module(m.to_string()))
            }
            ts_gen::ir::ModuleContext::Global => None,
        })
        .chain([ts_gen::codegen::ExportSpec::File("<input>".into())])
        .collect();
    let options = ts_gen::codegen::GenerateOptions::new().exports(exports);
    ts_gen::abi::build(ir, gctx, &options, provided)
}

/// The worker fixture ABI with `--provides ExportedHandler.fetch`.
fn worker_abi() -> Result<ts_gen::abi::Abi> {
    worker_abi_with("ExportedHandler.fetch")
}

fn worker_abi_with_no_exports() -> Result<ts_gen::abi::Abi> {
    let dts_file = crate_root().join("tests/fixtures/cloudflare-worker.d.ts");
    let dts_content = std::fs::read_to_string(&dts_file).context("reading .d.ts")?;
    let (ir, gctx) =
        ts_gen::parse_source(&dts_content, Some("cloudflare-worker")).context("parsing .d.ts")?;
    Ok(build_abi(&ir, &gctx, &[]))
}

fn worker_abi_with(exports: &str) -> Result<ts_gen::abi::Abi> {
    let dts_file = crate_root().join("tests/fixtures/cloudflare-worker.d.ts");
    let dts_content = std::fs::read_to_string(&dts_file).context("reading .d.ts")?;
    let (ir, mut gctx) =
        ts_gen::parse_source(&dts_content, Some("cloudflare-worker")).context("parsing .d.ts")?;
    let exports = ts_gen::provides::resolve(exports, &ir, &mut gctx)?;
    Ok(build_abi(&ir, &gctx, &exports))
}

fn run_embed_test(fixture_name: &str) -> Result<()> {
    let root = crate_root();
    let fixture = root
        .join("tests/fixtures/embed")
        .join(format!("{fixture_name}.wat"));
    let snap_dir = root.join("tests/snapshots/embed").join(fixture_name);

    // A sibling `<name>.provides.d.ts` is what the fixture provides.
    let exports_file = root
        .join("tests/fixtures/embed")
        .join(format!("{fixture_name}.provides.d.ts"));
    let abi = if exports_file.is_file() {
        worker_abi_with(exports_file.to_str().unwrap())
    } else {
        worker_abi()
    }
    .context("building ABI")?;

    // Read and assemble the fixture
    let wat_source = std::fs::read_to_string(&fixture).context("reading fixture")?;
    let module_bytes = ts_gen::abi::embed::assemble(&wat_source).context("assembling fixture")?;

    // Embed
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./hello.wasm".into(),
    };
    let output =
        ts_gen::abi::embed::embed(&abi, &module_bytes, &options).context("running embed")?;

    // Compare snapshots
    assert_snapshot(&snap_dir.join("bindings.wat"), &output.bindings_wat);
    assert_snapshot(&snap_dir.join("wrapper.wat"), &output.wrapper_wat);
    assert_snapshot(&snap_dir.join("builtins.js"), &output.builtins_js);

    // Assemble and validate the generated modules
    let bindings_bytes =
        ts_gen::abi::embed::assemble(&output.bindings_wat).context("assembling bindings")?;
    let wrapper_bytes =
        ts_gen::abi::embed::assemble(&output.wrapper_wat).context("assembling wrapper")?;

    let Some(wasm_merge) = find_tool("WASM_MERGE", "wasm-merge") else {
        return Ok(());
    };
    let merged = ts_gen::abi::embed::merge(
        &wasm_merge,
        &module_bytes,
        &options.module_specifier,
        &bindings_bytes,
        &wrapper_bytes,
    )
    .context("wasm-merge")?;
    let cleaned = ts_gen::abi::embed::postprocess(&merged, &output.used, &output.handlers)
        .context("postprocess")?;
    let mut validator = wasmparser::Validator::new_with_features(
        wasmparser::WasmFeatures::default() | wasmparser::WasmFeatures::GC,
    );
    validator
        .validate_all(&cleaned)
        .context("validating merged module")?;
    eprintln!(
        "  {fixture_name}: merged module is {} bytes (cleaned from {} bytes)",
        cleaned.len(),
        merged.len()
    );

    let Some(node) = find_node() else {
        return Ok(());
    };
    let dir = root.join("target/embed").join(fixture_name);
    std::fs::create_dir_all(&dir)?;
    let wasm = dir.join("hello.wasm");
    let builtins = dir.join("builtins.js");
    std::fs::write(&wasm, &cleaned)?;
    std::fs::write(&builtins, &output.builtins_js)?;
    let run = std::process::Command::new(node)
        .arg(root.join("tests/embed.mjs"))
        .arg(fixture_name)
        .arg(&wasm)
        .arg(&builtins)
        .output()
        .context("running node")?;
    if !run.status.success() {
        anyhow::bail!(
            "node run failed:\n{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }
    Ok(())
}

#[test]
fn embed_hello_jspi() {
    if let Err(e) = run_embed_test("hello_jspi") {
        panic!("hello_jspi failed: {e:#}");
    }
}

#[test]
fn embed_hello_sync() {
    if let Err(e) = run_embed_test("hello_sync") {
        panic!("hello_sync failed: {e:#}");
    }
}

#[test]
fn embed_variadic() {
    if let Err(e) = run_embed_test("variadic") {
        panic!("variadic failed: {e:#}");
    }
}

#[test]
fn embed_hello_anyref() {
    if let Err(e) = run_embed_test("hello_anyref") {
        panic!("hello_anyref failed: {e:#}");
    }
}

#[test]
fn embed_hello_mixed() {
    if let Err(e) = run_embed_test("hello_mixed") {
        panic!("hello_mixed failed: {e:#}");
    }
}

#[test]
fn embed_booleans() {
    if let Err(e) = run_embed_test("booleans") {
        panic!("booleans failed: {e:#}");
    }
}

#[test]
fn embed_module_imports() {
    if let Err(e) = run_embed_test("module_imports") {
        panic!("module_imports failed: {e:#}");
    }
}

#[test]
fn embed_sys() {
    if let Err(e) = run_embed_test("sys") {
        panic!("sys failed: {e:#}");
    }
}

#[test]
fn embed_nonnull() {
    if let Err(e) = run_embed_test("nonnull") {
        panic!("nonnull failed: {e:#}");
    }
}

#[test]
fn embed_error_nonnull_on_nullable_position() {
    // A JS-produced value (import result) that may be null.
    let err = embed_error(
        r#"(module
        (import "./bindings.wasm" "Headers.get" (func (param externref externref) (result (ref extern))))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("Headers.get") && err.contains("(ref extern)") && err.contains("null"),
        "error should explain the nullable position: {err}"
    );
    // A promise may resolve to null.
    let err = embed_error(
        r#"(module
        (import "./builtins.js" "await" (func (param externref) (result (ref extern))))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("await") && err.contains("(ref extern)"),
        "await result must stay nullable: {err}"
    );
}

#[test]
fn embed_nonnull_on_module_produced_positions() {
    // The module's own import arguments and handler result may be narrowed
    // even where the ABI admits null; only JS-produced positions are guarded.
    let abi = worker_abi().unwrap();
    let module = r#"(module
        (import "./bindings.wasm" "Response.new_with_body" (func $new (param (ref extern)) (result externref)))
        (func (export "fetch") (param externref externref externref) (result (ref extern))
            unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let out = ts_gen::abi::embed::embed(&abi, &bytes, &options)
        .expect("narrowing own positions is sound");
    ts_gen::abi::embed::assemble(&out.bindings_wat).unwrap();
    ts_gen::abi::embed::assemble(&out.wrapper_wat).unwrap();
    assert!(
        out.bindings_wat.contains("(param $body (ref extern))"),
        "{}",
        out.bindings_wat
    );
    assert!(
        out.wrapper_wat.contains("(result (ref extern))"),
        "{}",
        out.wrapper_wat
    );
}

#[test]
fn embed_exports_nested() {
    if let Err(e) = run_embed_test("exports_nested") {
        panic!("exports_nested failed: {e:#}");
    }
}

#[test]
fn embed_anyref_ctor() {
    if let Err(e) = run_embed_test("anyref_ctor") {
        panic!("anyref_ctor failed: {e:#}");
    }
}

/// Embed `wat` against the worker fixture and return the error message.
fn embed_error(wat: &str) -> String {
    let abi = worker_abi().unwrap();
    let bytes = ts_gen::abi::embed::assemble(wat).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./bad.wasm".into(),
    };
    ts_gen::abi::embed::embed(&abi, &bytes, &options)
        .err()
        .expect("embed should fail")
        .to_string()
}

#[test]
fn embed_error_unknown_import() {
    let err = embed_error(
        r#"(module
        (import "./bindings.wasm" "Nonexistent.method" (func (result externref)))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("Nonexistent.method"),
        "error should mention the unknown import: {err}"
    );
}

#[test]
fn embed_error_signature_mismatch() {
    let err = embed_error(
        r#"(module
        (import "./bindings.wasm" "Headers.new" (func (result i32)))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("Headers.new") && (err.contains("type") || err.contains("result")),
        "error should mention signature mismatch: {err}"
    );
}

#[test]
fn embed_error_rest_param_not_anyref_array() {
    let err = embed_error(
        r#"(module
        (type $arr (array (mut externref)))
        (import "./bindings.wasm" "joinAll" (func (param externref (ref null $arr)) (result externref)))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("joinAll")
            && err.contains("(array (mut externref))")
            && err.contains("(array (mut anyref))"),
        "error should show actual and expected array types: {err}"
    );
}

#[test]
fn embed_error_handler_signature() {
    let err = embed_error(
        r#"(module
        (func (export "fetch") (param f32 externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("export \"fetch\"") && err.contains("f32"),
        "error should name the handler and the bad type: {err}"
    );
    let err = embed_error(
        r#"(module
        (func (export "fetch") (param externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("export \"fetch\"") && err.contains("2 params"),
        "error should report the arity mismatch: {err}"
    );
}

#[test]
fn embed_non_null_anyref_follows_the_extern_rule() {
    // `(ref any)` is accepted where `(ref extern)` is: the module's own
    // argument, and a result whose TS type has no null.
    let abi = worker_abi().unwrap();
    let module = r#"(module
        (import "./bindings.wasm" "Request.url" (func (param (ref any)) (result (ref any))))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let out = ts_gen::abi::embed::embed(&abi, &bytes, &options).unwrap();
    ts_gen::abi::embed::assemble(&out.bindings_wat).unwrap();
    assert!(
        out.bindings_wat
            .contains("(param $this (ref any)) (result (ref any))\n    (ref.as_non_null "),
        "{}",
        out.bindings_wat
    );
    // ... and rejected on a nullable JS-produced result.
    let err = embed_error(
        r#"(module
        (import "./bindings.wasm" "Headers.get" (func (param anyref externref) (result (ref any))))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#,
    );
    assert!(
        err.contains("Headers.get") && err.contains("(ref any)") && err.contains("use anyref"),
        "{err}"
    );
}

#[test]
fn embed_error_jspi_without_provides() {
    let abi = worker_abi_with_no_exports().unwrap();
    let module = r#"(module
        (import "./builtins.js" "await" (func (param externref) (result externref)))
        (func (export "fetch") (param externref externref externref) (result externref)
            unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let err = ts_gen::abi::embed::embed(&abi, &bytes, &options)
        .err()
        .expect("nothing would be wrapped with promising")
        .to_string();
    assert!(err.contains("await") && err.contains("--provides"), "{err}");
}

#[test]
fn embed_error_export_collides_with_published_namespace() {
    let exports = crate_root().join("tests/fixtures/embed/exports_nested.provides.d.ts");
    let abi = worker_abi_with(exports.to_str().unwrap()).unwrap();
    // A raw `api` export beside the `api.math.add` the wrapper publishes
    // under `api`; `postprocess` strips only handler functions, so this
    // would survive the merge as a duplicate export.
    let module = r#"(module
        (func (export "api.math.add") (param f64 f64) (result f64) unreachable)
        (func (export "api") (result f64) unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let err = ts_gen::abi::embed::embed(&abi, &bytes, &options)
        .err()
        .expect("collision")
        .to_string();
    assert!(
        err.contains("\"api\"") && err.contains("api.math.add"),
        "{err}"
    );
}

#[test]
fn embed_error_handler_param_message_names_the_export_side() {
    let (ir, mut gctx) = ts_gen::parse_source(
        "interface ExportedHandler { fetch?(request: string, env: object | null): string; }",
        None,
    )
    .unwrap();
    let provided = ts_gen::provides::resolve("ExportedHandler.fetch", &ir, &mut gctx).unwrap();
    let abi = build_abi(&ir, &gctx, &provided);
    let module = r#"(module
        (func (export "fetch") (param externref (ref extern)) (result externref) unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let err = ts_gen::abi::embed::embed(&abi, &bytes, &options)
        .err()
        .expect("nullable handler param")
        .to_string();
    assert_eq!(
        err,
        "export \"fetch\" param 1 is (ref extern) but the ABI type admits null; use externref there"
    );
}

#[test]
fn embed_module_export_globals_take_the_declared_reference_type() {
    let (ir, mut gctx) = ts_gen::parse_source(
        r#"
        declare module "m:lib" { const cfg: Sock; const maybe: Sock | null; class Sock {} }
        interface ExportedHandler { fetch?(request: string): string; }
        "#,
        None,
    )
    .unwrap();
    let provided = ts_gen::provides::resolve("ExportedHandler.fetch", &ir, &mut gctx).unwrap();
    let abi = build_abi(&ir, &gctx, &provided);
    let module = r#"(module
        (import "./bindings.wasm" "cfg" (func (result (ref extern))))
        (import "./bindings.wasm" "maybe" (func (result anyref)))
        (func (export "fetch") (param externref) (result externref) unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let out = ts_gen::abi::embed::embed(&abi, &bytes, &options).unwrap();
    ts_gen::abi::embed::assemble(&out.bindings_wat).unwrap();
    assert!(
        out.bindings_wat
            .contains("(result (ref extern))\n    (ref.as_non_null (global.get $m:cfg))"),
        "{}",
        out.bindings_wat
    );
    assert!(
        out.bindings_wat
            .contains("(result anyref)\n    (any.convert_extern (global.get $m:maybe))"),
        "{}",
        out.bindings_wat
    );
}

#[test]
fn embed_beyond_the_call_family_applies_an_array() {
    let (ir, mut gctx) = ts_gen::parse_source(
        r#"
        declare function nine(a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number): number;
        declare class Big { constructor(a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number); }
        declare class Spread { constructor(...xs: number[]); }
        interface ExportedHandler { fetch?(request: string): string; }
        "#,
        None,
    )
    .unwrap();
    let provided = ts_gen::provides::resolve("ExportedHandler.fetch", &ir, &mut gctx).unwrap();
    let abi = build_abi(&ir, &gctx, &provided);
    let module = r#"(module
        (type $anyrefs (array (mut anyref)))
        (import "./bindings.wasm" "nine" (func (param f64 f64 f64 f64 f64 f64 f64 f64 f64) (result f64)))
        (import "./bindings.wasm" "Big.new" (func (param f64 f64 f64 f64 f64 f64 f64 f64 f64) (result externref)))
        (import "./bindings.wasm" "Spread.new" (func (param (ref null $anyrefs)) (result externref)))
        (func (export "fetch") (param externref) (result externref) unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let out = ts_gen::abi::embed::embed(&abi, &bytes, &options).unwrap();
    ts_gen::abi::embed::assemble(&out.bindings_wat).unwrap();
    assert!(
        out.builtins_js.contains("reflect_apply"),
        "{}",
        out.builtins_js
    );
    assert!(
        !out.builtins_js.contains("call_new9") && !out.builtins_js.contains("call9"),
        "{}",
        out.builtins_js
    );
    assert_eq!(out.bindings_wat.matches("\"construct\"").count(), 1);
}

#[test]
fn embed_error_no_handler() {
    let err = embed_error(
        r#"(module
        (func (export "not_a_handler") (result externref) unreachable))"#,
    );
    assert!(
        err.contains("none of the exports") && err.contains("fetch"),
        "error should list the expected exports: {err}"
    );
}

#[test]
fn embed_without_exports_passes_the_module_through() {
    let abi = worker_abi_with_no_exports().unwrap();
    let module = r#"(module
        (func (export "whatever") (result externref) unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let out = ts_gen::abi::embed::embed(&abi, &bytes, &options).unwrap();
    assert!(out.handlers.is_empty());
    assert!(
        out.wrapper_wat.contains("No --provides"),
        "{}",
        out.wrapper_wat
    );
    ts_gen::abi::embed::assemble(&out.wrapper_wat).unwrap();
}

#[test]
fn embed_binds_one_function_under_every_alias() {
    let (ir, mut gctx) = ts_gen::parse_source(
        r#"
        declare module "m:lib" {
            class _Conn { constructor(); get(): string; }
            export { _Conn as Conn, _Conn as Connection };
        }
        interface ExportedHandler { fetch?(request: string): string; }
        "#,
        None,
    )
    .unwrap();
    let provided = ts_gen::provides::resolve("ExportedHandler.fetch", &ir, &mut gctx).unwrap();
    let abi = build_abi(&ir, &gctx, &provided);
    // The two `new` imports differ in result type, so they need two functions.
    let module = r#"(module
        (import "./bindings.wasm" "Conn.get" (func (param externref) (result externref)))
        (import "./bindings.wasm" "Connection.get" (func (param externref) (result externref)))
        (import "./bindings.wasm" "Conn.new" (func (result externref)))
        (import "./bindings.wasm" "Connection.new" (func (result (ref extern))))
        (func (export "fetch") (param externref) (result externref) unreachable))"#;
    let bytes = ts_gen::abi::embed::assemble(module).unwrap();
    let options = ts_gen::abi::embed::Options {
        module_specifier: "./m.wasm".into(),
    };
    let out = ts_gen::abi::embed::embed(&abi, &bytes, &options).unwrap();
    ts_gen::abi::embed::assemble(&out.bindings_wat).unwrap();
    let b = &out.bindings_wat;
    assert!(
        b.contains(r#"(func (export "Conn.get") (export "Connection.get")"#),
        "{b}"
    );
    assert!(
        b.contains(r#"(func (export "Conn.new") (result externref)"#),
        "{b}"
    );
    assert!(
        b.contains(r#"(func (export "Connection.new") (result (ref extern))"#),
        "{b}"
    );
    assert_eq!(
        out.used,
        ["Conn.get", "Connection.get", "Conn.new", "Connection.new"]
    );
}
