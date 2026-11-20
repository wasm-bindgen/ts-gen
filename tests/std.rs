//! `std/std.ir`, the JS standard library as a lib IR (IR.md §5), is
//! produced from TypeScript's lib files in `std/typescript-<version>/` and
//! the ABI choices in `std/annotate.d.ts`. It's checked in, pinned to that
//! TypeScript version. To regenerate it:
//!
//! ```sh
//! BLESS=1 cargo test --test std
//! ```

use std::path::{Path, PathBuf};

use ts_gen::parse::Options;

const TYPESCRIPT: &str = "7.0.2";

const LIBS: &[&str] = &[
    "es5",
    "es2015.core",
    "es2015.collection",
    "es2015.iterable",
    "es2015.symbol",
    "es2015.symbol.wellknown",
    "es2016.array.include",
    "es2018.asynciterable",
    "es2020.bigint",
    "es2020.intl",
    "es2022.array",
    "es2022.error",
    "es2023.array",
    "es2025.intl",
    "esnext.disposable",
];

const ROOTS: &[&str] = &[
    "Error",
    "ArrayBuffer",
    "ArrayBufferView",
    "DataView",
    "Date",
    "RegExp",
    "Map",
    "Set",
    "Disposable",
    "Iterator",
    "Iterable",
    "IterableIterator",
    "AsyncIterable",
    "AsyncIterableIterator",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "ErrorOptions",
    "ArrayLike",
    "IteratorObject",
    "IteratorYieldResult",
    "IteratorReturnResult",
    "ArrayIterator",
    "MapIterator",
    "SetIterator",
    "AsyncIterator",
    "AsyncIteratorObject",
    "Intl.DateTimeFormatOptions",
    "Intl.NumberFormatOptions",
    "Intl.Locale",
    "Intl.DurationFormatOptions",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn std_ir() -> ts_gen_abi::ir::Ir {
    let src = std::fs::read_to_string(root().join("std/std.ir")).unwrap();
    ts_gen_abi::text::parse(&src).unwrap()
}

#[test]
fn std_ir_is_current() {
    let dir = root().join(format!("std/typescript-{TYPESCRIPT}"));
    let files: Vec<PathBuf> = LIBS
        .iter()
        .map(|l| dir.join(format!("lib.{l}.d.ts")))
        .collect();
    let (module, mut gctx) = ts_gen::parse_with(&files, None, Options::IR).unwrap();
    let side = [root().join("std/annotate.d.ts")];
    let (annotations, agctx) = ts_gen::parse_with(&side, None, Options::IR).unwrap();
    let errors = ts_gen::parse::annotate::apply(&mut gctx, &module, &annotations, &agctx);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    let opts = ts_gen::lower::Options {
        lib: Some(ts_gen::lower::LibOptions {
            name: "std".into(),
            roots: ROOTS.iter().map(|r| r.to_string()).collect(),
        }),
        ..Default::default()
    };
    let mut lowered = ts_gen::lower::lower_with(&module, &gctx, &opts);
    // TS `object` lowers to `Object`, an interface so the bridge tests for
    // a non-null object; the lib's own `Object` would merge with its
    // constructor into a class. TS `symbol` lowers to `Symbol`, which the
    // bridge tests with `typeof`.
    let object =
        ts_gen_abi::text::parse("ir 1;\nlib std;\ninterface Object {}\nclass Symbol {}\n").unwrap();
    lowered.ir.items.splice(0..0, object.items);
    if let Err(errors) = ts_gen_abi::validate::validate(&lowered.ir) {
        let errors: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("{}", errors.join("\n"));
    }
    let mut text = format!(
        "// Produced by tests/std.rs from TypeScript {TYPESCRIPT}'s lib files and\n// std/annotate.d.ts.\n"
    );
    for d in &lowered.diagnostics {
        text.push_str(&format!("// {d}\n"));
    }
    text.push_str(&ts_gen_abi::text::print(&lowered.ir));
    let path = root().join("std/std.ir");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == text,
        "std/std.ir is stale; regenerate it with BLESS=1 cargo test --test std"
    );
    assert_eq!(std_ir(), lowered.ir);
}

/// workers-types resolves its lib types against `std` instead of
/// stubbing them, and validates with it.
#[test]
fn workers_types_resolve_against_std() {
    let std = std_ir();
    let files = [root().join("tests/fixtures/workers-types.d.ts")];
    let (module, gctx) = ts_gen::parse_with(&files, None, Options::IR).unwrap();
    let opts = ts_gen::lower::Options {
        with: vec![std.clone()],
        ..Default::default()
    };
    let lowered = ts_gen::lower::lower_with(&module, &gctx, &opts);
    assert_eq!(lowered.ir.with, ["std"]);
    audit(&gctx, &lowered);
    let libs = [std];
    let checks = ts_gen_abi::validate::Options {
        libs: &libs,
        ..Default::default()
    };
    if let Err(errors) = ts_gen_abi::validate::validate_with(&lowered.ir, checks) {
        let errors: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("{}", errors.join("\n"));
    }
    let declared: Vec<&str> = lowered
        .ir
        .items
        .iter()
        .filter_map(|d| match d {
            ts_gen_abi::ir::Decl::Class(c) => Some(c.name.as_str()),
            _ => None,
        })
        .collect();
    // workers-types declares its own empty `Disposable` for older TypeScript.
    for r in ROOTS.iter().filter(|r| **r != "Disposable") {
        assert!(!declared.contains(r), "workers-types still stubs `{r}`");
    }
}

/// Every construct of workers-types the IR can't hold is a diagnostic, an
/// error or a skip with its reason, and never dropped silently: the full
/// list is a snapshot.
fn audit(gctx: &ts_gen::context::GlobalContext, lowered: &ts_gen::lower::Lowered) {
    use ts_gen::util::diagnostics::DiagnosticLevel;
    let mut text = String::from(
        "// Produced by tests/std.rs: the parse, then the lowering, diagnostics of\n// tests/fixtures/workers-types.d.ts against std.\n",
    );
    // The parse reports each occurrence, without a location.
    let mut parse: Vec<(String, usize)> = Vec::new();
    for d in &gctx.diagnostics.diagnostics {
        let level = match d.level {
            DiagnosticLevel::Error => "error",
            DiagnosticLevel::Warning => "warning",
            DiagnosticLevel::Info => continue,
        };
        let line = format!("{level}: {}", d.message);
        match parse.iter_mut().find(|(l, _)| *l == line) {
            Some((_, n)) => *n += 1,
            None => parse.push((line, 1)),
        }
    }
    for (line, n) in parse {
        text.push_str(&format!("parse (x{n}) {line}\n"));
    }
    for d in &lowered.diagnostics {
        text.push_str(&format!("ir: {d}\n"));
    }
    let path = root().join("tests/workers-types.diagnostics");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == text,
        "tests/workers-types.diagnostics is stale; regenerate it with BLESS=1 cargo test --test std"
    );
}
