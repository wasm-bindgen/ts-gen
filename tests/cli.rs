//! Command-line checks that need no snapshot.

use std::process::Command;

/// `--provides-app` and `--provides-bindings` shape the Rust wrapper, so
/// another target rejects them rather than ignoring them.
#[test]
fn provides_wrapper_flags_are_rust_only() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::temp_dir().join(format!("ts-gen-cli-{}.wat", std::process::id()));
    for flag in ["--provides-app", "--provides-bindings"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ts-gen"))
            .arg("--input")
            .arg(root.join("tests/fixtures/cloudflare-worker.d.ts"))
            .args(["--provides", "ExportedHandler.fetch", "--target", "wat"])
            .args([flag, "super::x", "--output"])
            .arg(&out)
            .output()
            .expect("Failed to execute ts-gen");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success()
                && stderr
                    .contains("--provides-app and --provides-bindings apply to --target rust only"),
            "{flag}: {stderr}"
        );
    }
    assert!(!out.exists());
}
