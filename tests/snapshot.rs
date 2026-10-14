//! Snapshot tests for ts-gen code generation.
//!
//! Every `.d.ts` file in `tests/fixtures/` is automatically discovered and run
//! through the `ts-gen` CLI binary, then the generated Rust output is compared
//! against a blessed snapshot in `tests/snapshots/<name>.rs`.
//!
//! The harness also runs `--target wat` for each fixture, producing a
//! `tests/snapshots/<name>.wat` alongside the `.rs`. The WAT output is
//! validated with `wat::parse_str` and `wasmparser` to ensure it assembles
//! and passes validation. A fixture the ABI rejects pins the error instead,
//! as `tests/snapshots/<name>.wat.err`; exactly one of the two exists.
//!
//! Fixtures can include `//! @ts-gen <args>` directive comments at the top.
//! The args after `//! @ts-gen` are passed **verbatim** to the CLI, so any
//! flags the CLI supports (e.g. `--lib-name`, `--external`) just work.
//!
//! To add a new snapshot test, just drop a `.d.ts` file into `tests/fixtures/`.
//! To update snapshots after intentional changes:
//!
//! ```sh
//! BLESS=1 cargo test -p ts-gen
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

/// Root of the ts-gen crate (where Cargo.toml lives).
fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Path to the compiled `ts-gen` binary.
///
/// `cargo test` compiles the binary into the same target directory.
fn ts_gen_bin() -> PathBuf {
    // env!("CARGO_BIN_EXE_ts-gen") is set by cargo for integration tests
    // when the crate defines a [[bin]] target.
    PathBuf::from(env!("CARGO_BIN_EXE_ts-gen"))
}

/// A fresh output directory for one CLI run. The name includes the process
/// id, so concurrent test runs (e.g. from separate worktrees) don't share or
/// delete each other's output. It's removed on drop, including when the test
/// panics.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ts-gen-{name}-{}", std::process::id()));
        // Left over from an earlier process with the same id.
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

/// Collect extra CLI args from `//! @ts-gen <args>` directive lines at the top
/// of a `.d.ts` file. The text after the prefix is split on whitespace and
/// returned as a flat list of arguments — no interpretation, just passthrough.
fn collect_directive_args(fixture: &Path) -> Vec<String> {
    let content = std::fs::read_to_string(fixture).unwrap_or_default();
    let mut args = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("//! @ts-gen ") {
            args.extend(rest.split_whitespace().map(String::from));
        } else if !trimmed.starts_with("//") && !trimmed.is_empty() {
            // Stop scanning once we hit non-comment content
            break;
        }
    }

    args
}

/// Compare `actual` against the snapshot file at `snap_path`.
///
/// - If `BLESS=1` is set, writes `actual` to `snap_path` and returns Ok.
/// - Otherwise, reads the snapshot and asserts equality, printing a diff on mismatch.
fn assert_snapshot(snap_path: &Path, actual: &str) {
    let bless = std::env::var("BLESS").is_ok_and(|v| v == "1" || v == "true");

    if bless {
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
             Run `BLESS=1 cargo test -p ts-gen` to create it.\n\
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
         Run `BLESS=1 cargo test -p ts-gen` to update.\n\
         {diff_output}",
        snap_path.display()
    );
}

/// Run the `ts-gen` CLI on a `.d.ts` fixture and compare the generated Rust
/// output against the blessed snapshot. Also runs `--target wat` and validates
/// the generated WAT assembles and passes Wasm validation.
fn snapshot_test(fixture_name: &str) {
    let root = crate_root();
    let fixture = root.join("tests/fixtures").join(fixture_name);
    let base_name = fixture_name.replace(".d.ts", "");
    let snap_path_rs = root
        .join("tests")
        .join("snapshots")
        .join(format!("{base_name}.rs"));
    let snap_path_wat = root
        .join("tests")
        .join("snapshots")
        .join(format!("{base_name}.wat"));
    let snap_path_wat_err = root
        .join("tests")
        .join("snapshots")
        .join(format!("{base_name}.wat.err"));

    // Collect any extra CLI flags from directive comments
    let extra_args = collect_directive_args(&fixture);

    let out_dir = TempDir::new(&format!("snapshot-{base_name}"));
    let out_file_rs = out_dir.0.join("bindings.rs");
    let out_file_wat = out_dir.0.join("bindings.wat");

    // Invoke the CLI binary: ts-gen --input <fixture> --output <out_file> [extra_args...]
    let output = Command::new(ts_gen_bin())
        .arg("--input")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_file_rs)
        .args(&extra_args)
        .output()
        .expect("Failed to execute ts-gen binary");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!(
            "ts-gen CLI failed for {fixture_name}:\n\
             exit code: {:?}\n\
             stderr:\n{stderr}\n\
             stdout:\n{stdout}",
            output.status.code()
        );
    }

    let rust_code = std::fs::read_to_string(&out_file_rs).unwrap_or_else(|e| {
        panic!(
            "Failed to read generated output {}: {e}",
            out_file_rs.display()
        )
    });

    assert_snapshot(&snap_path_rs, &rust_code);

    // `--provides` writes a wrapper and an app scaffold next to the output.
    for file in ["provides.rs", "app.rs"] {
        let path = out_dir.0.join(file);
        if path.is_file() {
            let code = std::fs::read_to_string(&path).unwrap();
            assert_snapshot(
                &root
                    .join("tests/snapshots")
                    .join(format!("{base_name}.{file}")),
                &code,
            );
        }
    }

    // Run --target wat
    let output_wat = Command::new(ts_gen_bin())
        .arg("--input")
        .arg(&fixture)
        .arg("--output")
        .arg(&out_file_wat)
        .arg("--target")
        .arg("wat")
        .args(&extra_args)
        // An error's text is snapshotted; a backtrace would follow it.
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .output()
        .expect("Failed to execute ts-gen binary for WAT");

    if !output_wat.status.success() {
        let stderr = String::from_utf8_lossy(&output_wat.stderr);
        let stdout = String::from_utf8_lossy(&output_wat.stdout);
        // Earlier stderr lines are diagnostics carrying absolute paths; the
        // error itself is what the ABI rejected.
        let Some(start) = stderr.find("Error: ") else {
            panic!(
                "ts-gen --target wat failed for {fixture_name} without an error:\n\
                 exit code: {:?}\n\
                 stderr:\n{stderr}\n\
                 stdout:\n{stdout}",
                output_wat.status.code()
            );
        };
        assert_sole_snapshot(&snap_path_wat_err, &snap_path_wat);
        assert_snapshot(&snap_path_wat_err, &stderr[start..]);
        return;
    }

    let wat_code = std::fs::read_to_string(&out_file_wat).unwrap_or_else(|e| {
        panic!(
            "Failed to read generated WAT output {}: {e}",
            out_file_wat.display()
        )
    });

    // Validate the WAT assembles and passes Wasm validation
    validate_wat(&wat_code, fixture_name);

    assert_sole_snapshot(&snap_path_wat, &snap_path_wat_err);
    assert_snapshot(&snap_path_wat, &wat_code);
}

/// `actual` is the snapshot this run produces and `other` the alternative
/// outcome's, which must not linger: blessing removes it, otherwise its
/// presence means the outcome changed.
fn assert_sole_snapshot(actual: &Path, other: &Path) {
    if !other.exists() {
        return;
    }
    if std::env::var("BLESS").is_ok_and(|v| v == "1" || v == "true") {
        std::fs::remove_file(other)
            .unwrap_or_else(|e| panic!("Failed to remove {}: {e}", other.display()));
        eprintln!("  removed: {}", other.display());
        return;
    }
    panic!(
        "{} exists, but this run produced {}.\n\
         Run `BLESS=1 cargo test -p ts-gen` to update.",
        other.display(),
        actual.display()
    );
}

/// Validate that WAT text assembles and passes Wasm validation.
fn validate_wat(wat_text: &str, fixture_name: &str) {
    let bytes = match wat::parse_str(wat_text) {
        Ok(b) => b,
        Err(e) => panic!("WAT parse failed for {fixture_name}:\n{e}\n\nSource:\n{wat_text}"),
    };
    let mut validator = wasmparser::Validator::new_with_features(
        wasmparser::WasmFeatures::default() | wasmparser::WasmFeatures::GC,
    );
    if let Err(e) = validator.validate_all(&bytes) {
        panic!("Wasm validation failed for {fixture_name}:\n{e}\n\nSource:\n{wat_text}");
    }
}

/// Discover all `.d.ts` files in `tests/fixtures/` and run a snapshot test
/// for each one. Test names are derived from the file stem.
#[test]
fn snapshots() {
    let fixtures_dir = crate_root().join("tests/fixtures");
    let mut fixtures: Vec<_> = std::fs::read_dir(&fixtures_dir)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", fixtures_dir.display()))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().to_string();
            // `<fixture>.provides.d.ts` is a fixture's `--provides` surface,
            // not an input of its own.
            if name.ends_with(".d.ts") && !name.ends_with(".provides.d.ts") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    fixtures.sort();

    assert!(
        !fixtures.is_empty(),
        "No .d.ts fixtures found in {}",
        fixtures_dir.display()
    );

    let mut failures = Vec::new();

    for fixture in &fixtures {
        let result = std::panic::catch_unwind(|| {
            snapshot_test(fixture);
        });
        if let Err(e) = result {
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
        let mut report = format!("\n{} snapshot(s) failed:\n", failures.len());
        for (name, msg) in &failures {
            report.push_str(&format!("\n--- {name} ---\n{msg}\n"));
        }
        panic!("{report}");
    }

    eprintln!("All {} snapshot tests passed.", fixtures.len());
}

/// Diagnostics aren't part of a snapshot, so the absence of the dropped
/// generic argument warning is checked separately.
#[test]
fn typed_array_buffer_arguments_drop_without_a_warning() {
    let fixture = crate_root().join("tests/fixtures/typed-array-buffers.d.ts");
    let out_dir = TempDir::new("typed-array-buffers");
    let output = Command::new(ts_gen_bin())
        .arg("--input")
        .arg(&fixture)
        .arg("--output")
        .arg(out_dir.0.join("bindings.rs"))
        .output()
        .expect("Failed to execute ts-gen binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
}
