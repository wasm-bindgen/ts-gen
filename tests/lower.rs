//! Producer tests: `.d.ts` → IR.
//!
//! Every `.d.ts` in `tests/lower/` is lowered and compared against the
//! IR snapshot beside it (`<name>.ir`), headed by the lowering diagnostics
//! as `//` comments. Every produced IR, including one per fixture of the
//! Rust snapshot suite, must print, read back equal, and pass strict
//! validation.
//!
//! Fixtures take `//! @ts-gen` directives: `--lib-name`, `--input`,
//! `--annotate`, `--provides`, `--once`, `--no-cancel`, `--strict-nullable`,
//! `--bound` and `--with`. To update snapshots after intentional changes:
//!
//! ```sh
//! BLESS=1 cargo test --test lower
//! ```

use std::path::{Path, PathBuf};

use ts_gen::parse::Options;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[derive(Default)]
struct Directives {
    lib_name: Option<String>,
    inputs: Vec<PathBuf>,
    annotate: Vec<PathBuf>,
    provides: Vec<String>,
    opts: ts_gen::lower::Options,
}

fn directives(fixture: &Path) -> Directives {
    let src = std::fs::read_to_string(fixture).unwrap();
    let mut d = Directives::default();
    for line in src.lines() {
        let Some(rest) = line.trim().strip_prefix("//! @ts-gen ") else {
            continue;
        };
        let mut args = rest.split_whitespace();
        while let Some(flag) = args.next() {
            match flag {
                "--lib-name" => d.lib_name = args.next().map(Into::into),
                "--input" => d.inputs.extend(args.next().map(|p| root().join(p))),
                "--annotate" => d.annotate.extend(args.next().map(|p| root().join(p))),
                "--provides" => d.provides.extend(args.next().map(|s| {
                    let path = root().join(s);
                    if path.is_file() {
                        path.to_string_lossy().into_owned()
                    } else {
                        s.to_string()
                    }
                })),
                "--once" => d.opts.once.extend(args.next().map(Into::into)),
                "--no-cancel" => d.opts.cancel = false,
                "--strict-nullable" => d.opts.strict_nullable = true,
                "--with" => d.opts.with.extend(args.next().map(|p| {
                    let src = std::fs::read_to_string(root().join(p)).unwrap();
                    ts_gen_abi::text::parse(&src).unwrap()
                })),
                "--bound" => d
                    .opts
                    .bounds
                    .extend(args.next().map(|b| ts_gen::lower::parse_bound(b).unwrap())),
                _ => {}
            }
        }
    }
    d
}

/// The IR text of `fixture`, headed by its diagnostics, or what's wrong
/// with it.
fn produce(fixture: &Path) -> Result<String, String> {
    let mut d = directives(fixture);
    let mut files = vec![fixture.to_path_buf()];
    files.extend(d.inputs);
    let (module, mut gctx) = ts_gen::parse_with(&files, d.lib_name.as_deref(), Options::IR)
        .map_err(|e| e.to_string())?;
    if !d.annotate.is_empty() {
        let (side, sgctx) = ts_gen::parse_with(&d.annotate, d.lib_name.as_deref(), Options::IR)
            .map_err(|e| e.to_string())?;
        let errors = ts_gen::parse::annotate::apply(&mut gctx, &module, &side, &sgctx);
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
    }
    for spec in &d.provides {
        let p = ts_gen::provides::resolve(spec, &module, &mut gctx).map_err(|e| e.to_string())?;
        d.opts.provides.fns.extend(p.fns);
        d.opts.provides.classes.extend(p.classes);
    }
    let lowered = ts_gen::lower::lower_with(&module, &gctx, &d.opts);
    let text = ts_gen_abi::text::print(&lowered.ir);
    let back = ts_gen_abi::text::parse(&text).map_err(|e| format!("{e}\n{text}"))?;
    if back != lowered.ir {
        let first = first_difference(&lowered.ir.items, &back.items);
        return Err(format!("printed IR reads back differently: {first}"));
    }
    let checks = ts_gen_abi::validate::Options {
        libs: &d.opts.with,
        ..Default::default()
    };
    if let Err(errors) = ts_gen_abi::validate::validate_with(&lowered.ir, checks) {
        let errors: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        return Err(format!("{}\n{text}", errors.join("\n")));
    }
    let mut out = String::new();
    for diag in &lowered.diagnostics {
        out.push_str(&format!("// {diag}\n"));
    }
    out.push_str(&text);
    Ok(out)
}

/// The first item that reads back differently, in its debug form.
fn first_difference(a: &[ts_gen_abi::ir::Decl], b: &[ts_gen_abi::ir::Decl]) -> String {
    use ts_gen_abi::ir::Decl;
    for (x, y) in a.iter().zip(b) {
        if x == y {
            continue;
        }
        match (x, y) {
            (Decl::Namespace(x), Decl::Namespace(y)) => {
                return first_difference(&x.items, &y.items)
            }
            (Decl::Module(x), Decl::Module(y)) if x.items != y.items => {
                return first_difference(&x.items, &y.items)
            }
            (Decl::Class(x), Decl::Class(y)) => {
                for (m, n) in x.members.iter().zip(&y.members) {
                    if m != n {
                        return format!("{}: {m:?}\n  reads back as {n:?}", x.name);
                    }
                }
            }
            _ => {}
        }
        return format!("{x:?}\n  reads back as {y:?}");
    }
    format!("{} items read back as {}", a.len(), b.len())
}

fn dts_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".d.ts"))
        .collect();
    files.sort();
    files
}

#[test]
fn snapshots() {
    let bless = std::env::var_os("BLESS").is_some();
    let mut failures = Vec::new();
    for fixture in dts_files(&root().join("tests/lower")) {
        let name = fixture
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(".d.ts", "");
        let snapshot = fixture.with_file_name(format!("{name}.ir"));
        let actual = match produce(&fixture) {
            Ok(text) => text,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        if bless {
            std::fs::write(&snapshot, &actual).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&snapshot).unwrap_or_default();
        if expected != actual {
            let diff = similar::TextDiff::from_lines(&expected, &actual);
            failures.push(format!(
                "{name}: snapshot differs (BLESS=1 to update)\n{}",
                diff.unified_diff().context_radius(2)
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

/// Every Rust snapshot fixture lowers to a valid IR.
#[test]
fn every_fixture_lowers() {
    let mut failures = Vec::new();
    for fixture in dts_files(&root().join("tests/fixtures")) {
        if let Err(e) = produce(&fixture) {
            let name = fixture.file_name().unwrap().to_string_lossy().to_string();
            let head: Vec<&str> = e.lines().take(12).collect();
            failures.push(format!("{name}: {}", head.join("\n")));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}
