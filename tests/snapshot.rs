//! Snapshot tests for ts-gen code generation.
//!
//! Every `.d.ts` file in `tests/fixtures/` is automatically discovered and run
//! through the `ts-gen` CLI binary once per entry in [`CONFIGS`], then each
//! generated Rust output is compared against a blessed snapshot in
//! `tests/snapshots/<name>/<config>.rs`. Keeping the configurations side by
//! side in one directory makes the effect of a flag on a fixture a plain
//! `diff` between siblings.
//!
//! Fixtures can include `//! @ts-gen <args>` directive comments at the top.
//! The args after `//! @ts-gen` are passed **verbatim** to the CLI, so any
//! flags the CLI supports (e.g. `--lib-name`, `--external`) just work. Flags
//! owned by a configuration can't appear there.
//!
//! To add a new snapshot test, just drop a `.d.ts` file into `tests/fixtures/`.
//! To update snapshots after intentional changes:
//!
//! ```sh
//! BLESS=1 cargo test -p ts-gen
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

/// The CLI configurations every fixture is rendered under, as
/// `(snapshot file stem, extra CLI args)`.
const CONFIGS: &[(&str, &[&str])] = &[("default", &[]), ("mono", &["--experimental-generic-mono"])];

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

/// Run the `ts-gen` CLI on a `.d.ts` fixture under every configuration and
/// compare each generated Rust output against its blessed snapshot.
fn snapshot_test(fixture_name: &str) {
    let root = crate_root();
    let fixture = root.join("tests/fixtures").join(fixture_name);
    let base_name = fixture_name.replace(".d.ts", "");
    let snap_dir = root.join("tests").join("snapshots").join(&base_name);

    // Collect any extra CLI flags from directive comments
    let extra_args = collect_directive_args(&fixture);
    for (_, config_args) in CONFIGS {
        for flag in *config_args {
            assert!(
                !extra_args.iter().any(|a| a == flag),
                "{fixture_name}: `{flag}` is applied by the snapshot harness, \
                 remove it from the `//! @ts-gen` directive"
            );
        }
    }

    let out_dir = TempDir::new(&format!("snapshot-{base_name}"));

    for (config_name, config_args) in CONFIGS {
        let out_file = out_dir.0.join(format!("{config_name}.rs"));

        // Invoke the CLI binary: ts-gen --input <fixture> --output <out_file> [extra_args...]
        let output = Command::new(ts_gen_bin())
            .arg("--input")
            .arg(&fixture)
            .arg("--output")
            .arg(&out_file)
            .args(&extra_args)
            .args(*config_args)
            .output()
            .expect("Failed to execute ts-gen binary");

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!(
                "ts-gen CLI failed for {fixture_name} ({config_name}):\n\
                 exit code: {:?}\n\
                 stderr:\n{stderr}\n\
                 stdout:\n{stdout}",
                output.status.code()
            );
        }

        let rust_code = std::fs::read_to_string(&out_file).unwrap_or_else(|e| {
            panic!(
                "Failed to read generated output {}: {e}",
                out_file.display()
            )
        });

        assert_snapshot(&snap_dir.join(format!("{config_name}.rs")), &rust_code);
    }
}

/// Every entry under `tests/snapshots/` must be the directory of a current
/// fixture holding exactly one file per configuration, so a renamed fixture or
/// a dropped configuration can't leave stale snapshots behind.
fn check_stale_snapshots(fixtures: &[String]) -> Vec<String> {
    let snapshots_dir = crate_root().join("tests/snapshots");
    let expected_files: Vec<String> = CONFIGS
        .iter()
        .map(|(name, _)| format!("{name}.rs"))
        .collect();
    let mut stale = Vec::new();
    for entry in std::fs::read_dir(&snapshots_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !path.is_dir() || !fixtures.contains(&format!("{name}.d.ts")) {
            stale.push(path.display().to_string());
            continue;
        }
        for file in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            if !expected_files.contains(&file.file_name().to_string_lossy().to_string()) {
                stale.push(file.path().display().to_string());
            }
        }
    }
    stale.sort();
    stale
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
            if path.extension().and_then(|e| e.to_str()) == Some("ts")
                && path.to_string_lossy().ends_with(".d.ts")
            {
                Some(path.file_name()?.to_string_lossy().to_string())
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

    let stale = check_stale_snapshots(&fixtures);
    assert!(
        stale.is_empty(),
        "Snapshots without a fixture or configuration, delete them:\n  {}",
        stale.join("\n  ")
    );

    eprintln!(
        "All {} snapshot tests passed under {} configurations.",
        fixtures.len(),
        CONFIGS.len()
    );
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

/// The snapshot run is from the crate root; run from elsewhere, imports in
/// `declare module` blocks still resolve from their own file, and the
/// imports TypeScript rejects are reported.
#[test]
fn ambient_module_imports_resolve_independently_of_cwd() {
    let dir = crate_root().join("tests/fixtures/ambient-module-import-base-dir");
    let out_dir = TempDir::new("ambient-module-import-base-dir");
    let out_file = out_dir.0.join("bindings.rs");
    let output = Command::new(ts_gen_bin())
        .current_dir(&out_dir.0)
        .arg("--input")
        .arg(dir.with_extension("d.ts"))
        .arg("--input")
        .arg(dir.join("script.d.ts"))
        .arg("--input")
        .arg(dir.join("aug.d.ts"))
        .args(["--lib-name", "ambient-module-import-base-dir-aug"])
        .args(["--export", "lib"])
        .arg("--output")
        .arg(&out_file)
        .output()
        .expect("Failed to execute ts-gen binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for expected in [
        "script.d.ts:3:23: Import or export declaration in an ambient module declaration \
         cannot reference module through relative module name.",
        "aug.d.ts:4:23: Imports are not permitted in module augmentations.",
    ] {
        assert!(stderr.contains(expected), "{stderr}");
    }
    assert!(!stderr.contains("Could not resolve import"), "{stderr}");

    let actual = std::fs::read_to_string(&out_file).unwrap();
    let expected = std::fs::read_to_string(
        crate_root().join("tests/snapshots/ambient-module-import-base-dir/default.rs"),
    )
    .unwrap();
    assert_eq!(actual, expected);
}
