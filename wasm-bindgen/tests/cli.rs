//! The `ts-gen-wasm-bindgen` binary on what `ts-gen` can pass it that
//! Rust output doesn't take.

use std::path::Path;
use std::process::Command;

fn error(args: &[&str]) -> String {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("generator-cli");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("api.d.ts"), "declare function f(): void;\n").unwrap();
    std::fs::write(dir.join("api.tw"), "ir 1;\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ts-gen-wasm-bindgen"))
        .current_dir(&dir)
        .args(args)
        .output()
        .unwrap();
    assert!(!out.status.success(), "{args:?}");
    assert!(!dir.join("out.rs").exists(), "{args:?}");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn ir_options_do_not_apply() {
    for (flag, args) in [
        ("--provides", &["--provides", "Handler.fetch"][..]),
        ("--once", &["--once", "f.cb"]),
        ("--no-cancel", &["--no-cancel"]),
        ("--bound", &["--bound", "Map.K=Str"]),
        ("--with", &["--with", "std.tw"]),
        ("--emit-lib", &["--emit-lib", "std", "--root", "f"]),
        ("--root", &["--root", "f"]),
    ] {
        let mut all = vec!["--input", "api.d.ts", "--output", "out.rs"];
        all.extend(args);
        let e = error(&all);
        assert!(
            e.contains(&format!("{flag} shapes the IR")) && !e.contains("Usage:"),
            "{e}"
        );
    }
}

#[test]
fn an_ir_input_is_rejected() {
    let e = error(&["--input", "api.tw", "--output", "out.rs"]);
    assert!(e.contains("reads .d.ts files, not the IR (api.tw)"), "{e}");
}
