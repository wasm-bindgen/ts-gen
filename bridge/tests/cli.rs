//! The `ts-gen-bridge` binary: `link`, then `generate`, on files.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ts-gen-bridge"))
}

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(c: &mut Command) -> Result<(), String> {
    let out = c.output().unwrap();
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

#[test]
fn links_then_generates() {
    let d = scratch("cli");
    std::fs::write(d.join("api.ir"), "ir 1;\nclass C { f(): (u32, str); }\n").unwrap();
    // A language that can only import from `js_code` under its own names.
    std::fs::write(
        d.join("lang.wat"),
        r#"(module
  (import "js_code" "app.f" (func (param externref) (result i32)))
  (import "tw:ret" "externref" (func (result externref))))"#,
    )
    .unwrap();
    std::fs::write(d.join("links.tsv"), "js_code\tapp.f\ttw:js/global\tC#f()\n").unwrap();
    let p = |f: &str| d.join(f).to_str().unwrap().to_string();
    run(bin().args([
        "link",
        &p("lang.wat"),
        &p("links.tsv"),
        "-o",
        &p("linked.wasm"),
    ]))
    .unwrap();
    run(bin().args([
        "generate",
        &p("api.ir"),
        &p("linked.wasm"),
        "-o",
        &p("out"),
        "--profile",
        "all-host, results = single",
        "--wat",
    ]))
    .unwrap();
    for f in ["bridge.js", "bridge.wasm", "bridge.wat"] {
        assert!(d.join("out").join(f).exists(), "{f}");
    }
    let js = std::fs::read_to_string(d.join("out/bridge.js")).unwrap();
    assert!(js.contains("\"C#f()\""), "{js}");

    // Unlinked, the profile mismatch is reported with the import.
    let e = run(bin().args([
        "generate",
        &p("api.ir"),
        &p("lang.wat"),
        "-o",
        &p("bad"),
        "--profile",
        "all-host",
    ]))
    .unwrap_err();
    assert!(e.contains("the spill needs `results = single`"), "{e}");
    let e = run(bin().args(["generate", &p("api.ir")])).unwrap_err();
    assert!(e.contains("usage:"), "{e}");
}
