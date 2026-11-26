//! The `ts-gen` binary: the IR, the lowered JSON and the bridge on files,
//! and a language generator found, installed and run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// `ts-gen`, with no generator of the environment's in reach.
fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ts-gen"));
    let none = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli/none");
    c.env_remove("TS_GEN_WASM_BINDGEN")
        .env("CARGO", none.join("cargo"))
        .env("XDG_CACHE_HOME", &none)
        .env("HOME", &none);
    c
}

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("cli")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn error(c: &mut Command) -> String {
    let out = c.output().unwrap();
    assert!(!out.status.success());
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const DTS: &str = "declare function add(a: number, b: number): number;\n";

#[test]
fn ir() {
    let d = scratch("ir");
    std::fs::write(d.join("api.d.ts"), DTS).unwrap();
    let text = stdout(
        &bin()
            .args(["--ir", "--input"])
            .arg(d.join("api.d.ts"))
            .output()
            .unwrap(),
    );
    let ir = ts_gen_abi::text::parse(&text).unwrap();
    assert!(text.contains("add(a: f64, b: f64): f64"), "{text}");

    // To a file, and from an IR: validated and printed again.
    let out = bin()
        .args(["--ir", "-i"])
        .arg(d.join("api.d.ts"))
        .arg("-o")
        .arg(d.join("out/api.tw"))
        .output()
        .unwrap();
    assert_eq!(stdout(&out), "");
    assert_eq!(std::fs::read_to_string(d.join("out/api.tw")).unwrap(), text);
    let again = bin()
        .args(["--ir", "-o", "-", "-i"])
        .arg(d.join("out/api.tw"))
        .output()
        .unwrap();
    assert_eq!(stdout(&again), text);
    assert_eq!(ts_gen_abi::text::parse(&stdout(&again)).unwrap(), ir);

    std::fs::write(d.join("bad.tw"), "ir 1;\nclass C { f(): Missing; }\n").unwrap();
    let e = error(bin().arg("--ir").arg("-i").arg(d.join("bad.tw")));
    assert!(e.contains("Missing"), "{e}");
}

#[test]
fn json() {
    let d = scratch("json");
    let sample = root().join("abi/tests/json/sample.tw");
    let ir = ts_gen_abi::text::parse(&std::fs::read_to_string(&sample).unwrap()).unwrap();
    let profile = ts_gen_abi::profile::Profile::parse("all-host").unwrap();
    let want = ts_gen_abi::json::lower(&ir, &profile).pretty();
    let out = bin()
        .args(["--json", "--profile", "all-host", "-i"])
        .arg(&sample)
        .output()
        .unwrap();
    assert_eq!(stdout(&out), want);

    // The profile as a file, the output as a file, and `.d.ts` input
    // lowered on the way.
    std::fs::write(d.join("profile"), "all-host").unwrap();
    std::fs::write(d.join("api.d.ts"), DTS).unwrap();
    let out = bin()
        .arg("--json")
        .arg("--profile")
        .arg(d.join("profile"))
        .arg("-i")
        .arg(d.join("api.d.ts"))
        .arg("-o")
        .arg(d.join("api.json"))
        .output()
        .unwrap();
    assert_eq!(stdout(&out), "");
    let json = std::fs::read_to_string(d.join("api.json")).unwrap();
    assert!(json.contains("\"add(f64,f64)\""), "{json}");

    let e = error(
        bin()
            .args(["--json", "--profile", "all-nothing", "-i"])
            .arg(&sample),
    );
    assert!(e.contains("--profile all-nothing"), "{e}");
}

#[test]
fn bridge() {
    let d = scratch("bridge");
    std::fs::write(d.join("api.tw"), "ir 1;\nclass C { f(): (u32, str); }\n").unwrap();
    std::fs::write(
        d.join("lang.wat"),
        r#"(module
  (import "tw:js/global" "C#f()" (func (param externref) (result i32)))
  (import "tw:ret" "externref" (func (result externref))))"#,
    )
    .unwrap();
    let run = |out: &str, extra: &[&str]| {
        let mut c = bin();
        c.arg("-i")
            .arg(d.join("api.tw"))
            .arg("--bridge")
            .arg(d.join("lang.wat"))
            .arg("-o")
            .arg(d.join(out))
            .args(extra);
        c
    };
    stdout(
        &run("out", &["--profile", "all-host, results = single", "--wat"])
            .output()
            .unwrap(),
    );
    for f in ["bridge.js", "bridge.wasm", "bridge.wat"] {
        assert!(d.join("out").join(f).exists(), "{f}");
    }
    let js = std::fs::read_to_string(d.join("out/bridge.js")).unwrap();
    assert!(js.contains("\"C#f()\""), "{js}");
    assert!(!js.contains("createBridge"), "{js}");

    stdout(
        &run(
            "factory",
            &["--profile", "all-host, results = single", "--factory"],
        )
        .output()
        .unwrap(),
    );
    let js = std::fs::read_to_string(d.join("factory/bridge.js")).unwrap();
    assert!(js.contains("createBridge"), "{js}");
    assert!(!d.join("factory/bridge.wat").exists());

    // A profile mismatch is reported with the import.
    let e = error(&mut run("bad", &["--profile", "all-host"]));
    assert!(
        e.contains("(\"tw:js/global\", \"C#f()\"): the language declares (i32)"),
        "{e}"
    );
    let e = error(
        bin()
            .arg("-i")
            .arg(d.join("api.tw"))
            .arg("--bridge")
            .arg(d.join("lang.wat")),
    );
    assert!(e.contains("--output <dir>"), "{e}");

    // `.d.ts` input is lowered on the way to the bridge.
    std::fs::write(d.join("api.d.ts"), DTS).unwrap();
    std::fs::write(d.join("empty.wat"), "(module)").unwrap();
    stdout(
        &bin()
            .arg("-i")
            .arg(d.join("api.d.ts"))
            .arg("--bridge")
            .arg(d.join("empty.wat"))
            .arg("-o")
            .arg(d.join("dts"))
            .output()
            .unwrap(),
    );
    assert!(d.join("dts/bridge.js").exists());
}

#[test]
fn rejects() {
    let d = scratch("rejects");
    std::fs::write(d.join("api.tw"), "ir 1;\n").unwrap();
    std::fs::write(d.join("api.d.ts"), DTS).unwrap();
    let tw = d.join("api.tw");
    let dts = d.join("api.d.ts");

    // One output per run.
    let e = error(bin().args(["--ir", "--json", "-i"]).arg(&tw));
    assert!(e.contains("cannot be used with"), "{e}");
    let e = error(bin().args(["--ir", "--lang", "kotlin", "-i"]).arg(&tw));
    assert!(e.contains("cannot be used with"), "{e}");
    let e = error(bin().args(["--ir", "--wat", "-i"]).arg(&tw));
    assert!(e.contains("--wat applies to --bridge"), "{e}");
    let e = error(bin().args(["--factory", "-i"]).arg(&dts));
    assert!(e.contains("--factory applies to --bridge"), "{e}");

    let e = error(bin().args(["--ir", "-i"]).arg(&tw).arg(&dts));
    assert!(e.contains("a .tw input is the only input"), "{e}");
    let e = error(bin().args(["--ir", "--no-cancel", "-i"]).arg(&tw));
    assert!(e.contains("--no-cancel applies to .d.ts input"), "{e}");
    let e = error(bin().args(["--ir", "--profile", "all-host", "-i"]).arg(&tw));
    assert!(
        e.contains("--profile applies to --json and --bridge"),
        "{e}"
    );
    let e = error(bin().args(["--profile", "all-host", "-i"]).arg(&dts));
    assert!(
        e.contains("--profile applies to --json and --bridge"),
        "{e}"
    );
    let e = error(
        bin()
            .args(["--ir", "-i"])
            .arg(&tw)
            .args(["--", "--fallible"]),
    );
    assert!(e.contains("after `--`"), "{e}");
    let e = error(bin().args(["--lang", "../evil", "-i"]).arg(&dts));
    assert!(e.contains("a language name"), "{e}");

    // No generator in reach: the failure names the override.
    let e = error(bin().arg("-i").arg(&dts));
    assert!(e.contains("set TS_GEN_WASM_BINDGEN to its binary"), "{e}");
}

#[cfg(unix)]
mod generators {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn lines(path: &Path) -> Vec<String> {
        let text = std::fs::read_to_string(path).unwrap();
        text.lines().map(str::to_string).collect()
    }

    /// The generator `TS_GEN_<LANG>` names gets the input, the output, the
    /// `.d.ts` options and the arguments after `--`, and its exit code is
    /// `ts-gen`'s.
    #[test]
    fn runs() {
        let d = scratch("runs");
        let log = d.join("args");
        script(
            &d.join("generator"),
            "printf '%s\\n' \"$@\" > \"$LOG\"\necho generated\nexit 7\n",
        );
        let out = bin()
            .env("TS_GEN_FAKE_LANG", d.join("generator"))
            .env("LOG", &log)
            .args(["--lang", "fake-lang", "-i", "a.d.ts", "types", "-o", "out"])
            .args(["--lib-name", "lib", "--annotate", "tags.d.ts"])
            .args(["--strict-nullable", "--provides", "Handler.fetch"])
            .args(["--", "--fallible", "--export", "a b"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(7));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "generated\n");
        assert_eq!(
            lines(&log),
            [
                "--input",
                "a.d.ts",
                "types",
                "--output",
                "out",
                "--lib-name",
                "lib",
                "--annotate",
                "tags.d.ts",
                "--provides",
                "Handler.fetch",
                "--strict-nullable",
                "--fallible",
                "--export",
                "a b",
            ]
        );

        // `wasm-bindgen` is the default language.
        script(&d.join("default"), "printf '%s\\n' \"$@\" > \"$LOG\"\n");
        let out = bin()
            .env("TS_GEN_WASM_BINDGEN", d.join("default"))
            .env("LOG", &log)
            .args(["-i", "a.d.ts"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(lines(&log), ["--input", "a.d.ts"]);

        let e = error(bin().env("TS_GEN_FAKE_LANG", d.join("missing")).args([
            "--lang",
            "fake-lang",
            "-i",
            "a.d.ts",
        ]));
        assert!(e.contains("couldn't run") && e.contains("missing"), "{e}");
    }

    /// A generator not yet in the cache is installed there, pinned to
    /// this version, and found there from then on.
    #[test]
    fn installs() {
        let d = scratch("installs");
        let log = d.join("cargo-args");
        // Installs a generator that prints its arguments.
        script(
            &d.join("cargo"),
            r#"echo "$*" >> "$LOG"
echo building
while [ "$1" != --root ]; do shift; done
mkdir -p "$2/bin"
printf '#!/bin/sh\necho "ran $*"\n' > "$2/bin/ts-gen-fake"
chmod +x "$2/bin/ts-gen-fake"
"#,
        );
        let run = || {
            let mut c = bin();
            c.env("CARGO", d.join("cargo"))
                .env("XDG_CACHE_HOME", d.join("cache"))
                .env("HOME", d.join("home"))
                .env("LOG", &log)
                .args(["--lang", "fake", "-i", "a.d.ts"]);
            c
        };
        let cache = match cfg!(target_os = "macos") {
            true => d.join("home/Library/Caches"),
            false => d.join("cache"),
        };
        let install_root = cache.join("ts-gen").join(env!("CARGO_PKG_VERSION"));
        // The install's own output stays off stdout.
        assert_eq!(stdout(&run().output().unwrap()), "ran --input a.d.ts\n");
        assert_eq!(
            lines(&log),
            [format!(
                "install ts-gen-fake --version ={} --locked --root {}",
                env!("CARGO_PKG_VERSION"),
                install_root.display()
            )]
        );
        assert_eq!(stdout(&run().output().unwrap()), "ran --input a.d.ts\n");
        assert_eq!(lines(&log).len(), 1);

        // `TS_GEN_<LANG>` wins over the cache.
        script(&d.join("other"), "echo other\n");
        let out = run().env("TS_GEN_FAKE", d.join("other")).output().unwrap();
        assert_eq!(stdout(&out), "other\n");

        script(&d.join("failing"), "exit 1\n");
        let e = error(
            run()
                .env("CARGO", d.join("failing"))
                .env("XDG_CACHE_HOME", d.join("empty"))
                .env("HOME", d.join("empty")),
        );
        assert!(
            e.contains("couldn't install ts-gen-fake") && e.contains("set TS_GEN_FAKE"),
            "{e}"
        );
    }
}
