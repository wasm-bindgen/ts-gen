//! Tests for `ts-gen-zena`.
//!
//! Every `.ir` in `tests/fixtures/` here and in ts-gen's `tests/lower/`
//! (the producer's snapshots) generates bindings compared against
//! `tests/snapshots/<name>.zena`.
//!
//! The import check compiles each fixture's bindings with `--probe`, or its
//! `<name>.main.zena` against them, and requires the `@external`
//! declarations, the compiled module's imports and the IR's imports to
//! agree: every non-generic member not skipped is declared and imported
//! under its key with exactly its layout's core type, and every generic
//! instantiation the module imports has the type its key's type arguments
//! give. `workers_types` does the same for the producer's IR of
//! `tests/fixtures/workers-types.d.ts`.
//!
//! Compiling needs the Zena compiler with the `tw` option on `@external`:
//! `ZENA` is its `zena-cli` binary and `ZENA_REPO_ROOT` its checkout. Node
//! 26 must be the `node` on `PATH`. Without `ZENA` the compile steps are
//! skipped locally, and fail under `CI` unless `TSGEN_SKIP_ZENA` is set.
//!
//! To update snapshots:
//!
//! ```sh
//! BLESS=1 cargo test -p ts-gen-zena
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use ts_gen_abi::ir::{FnType, Ir, Type};
use ts_gen_abi::key::{Keyed, Role};
use ts_gen_abi::layout::{Def, FuncType, Heap, Layouts, RefType, Storage, ValType};
use ts_gen_abi::resolve::Context;
use wasmparser::{
    AbstractHeapType, CompositeInnerType, HeapType, Parser, Payload, StorageType, SubType, TypeRef,
};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    crate_root().join("../..")
}

fn fixtures() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for dir in [
        crate_root().join("tests/fixtures"),
        repo_root().join("tests/lower"),
    ] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ir"))
            .collect();
        paths.sort();
        for p in paths {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            out.push((name, p));
        }
    }
    out
}

/// The libs an IR resolves against: `std`, as `./std.zena`.
fn libs(ir: &Ir) -> Vec<Ir> {
    ir.with
        .iter()
        .map(|name| {
            assert_eq!(name, "std", "no lib `{name}`");
            read_ir(&repo_root().join("std/std.ir"))
        })
        .collect()
}

fn generate(ir: &Ir, libs: &[Ir], opts: &ts_gen_zena::Options) -> ts_gen_zena::Output {
    let libs: Vec<ts_gen_zena::Lib> = libs
        .iter()
        .map(|l| ts_gen_zena::Lib {
            ir: l,
            module: format!("./{}.zena", l.lib.as_deref().unwrap()),
        })
        .collect();
    ts_gen_zena::generate_with(ir, &libs, opts).unwrap()
}

/// Writes each lib's bindings into `dir`, where the IR's import them.
fn write_libs(libs: &[Ir], dir: &Path) {
    for l in libs {
        let out = ts_gen_zena::generate(l, &ts_gen_zena::Options::default()).unwrap();
        let name = l.lib.as_deref().unwrap();
        std::fs::write(dir.join(format!("{name}.zena")), out.zena).unwrap();
    }
}

fn read_ir(path: &Path) -> Ir {
    let src = std::fs::read_to_string(path).unwrap();
    ts_gen_abi::text::parse(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn bless() -> bool {
    std::env::var("BLESS").is_ok_and(|v| v == "1" || v == "true")
}

fn assert_snapshot(path: &Path, actual: &str) {
    if bless() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(path).unwrap_or_else(|_| {
        panic!(
            "{} is missing; run `BLESS=1 cargo test -p ts-gen-zena`",
            path.display()
        )
    });
    if expected == actual {
        return;
    }
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(expected.as_str(), actual);
    let mut out = String::new();
    for group in diff.grouped_ops(3) {
        for op in group {
            for c in diff.iter_changes(&op) {
                let sign = match c.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                out.push_str(&format!("{sign}{c}"));
            }
        }
        out.push_str("...\n");
    }
    panic!(
        "{} differs; run `BLESS=1 cargo test -p ts-gen-zena`\n{out}",
        path.display()
    );
}

#[test]
fn snapshots() {
    for (name, path) in fixtures() {
        let ir = read_ir(&path);
        let opts = ts_gen_zena::Options {
            entry: Some(ts_gen_zena::Entry {
                bindings: format!("./{name}.zena"),
                app: "./app.zena".into(),
            }),
            ..Default::default()
        };
        let out = generate(&ir, &libs(&ir), &opts);
        assert_snapshot(
            &crate_root().join(format!("tests/snapshots/{name}.zena")),
            &out.zena,
        );
        let entry = crate_root().join(format!("tests/snapshots/{name}.entry.zena"));
        match &out.entry {
            Some(e) if e.contains("@export") => assert_snapshot(&entry, e),
            _ => assert!(!entry.exists(), "{} has no exports", entry.display()),
        }
    }
}

/// The Zena compiler and its checkout, or `None` to skip.
fn zena() -> Option<(PathBuf, PathBuf)> {
    if std::env::var_os("TSGEN_SKIP_ZENA").is_some() {
        eprintln!("TSGEN_SKIP_ZENA is set, skipping the Zena compile");
        return None;
    }
    let Ok(cli) = std::env::var("ZENA") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "ZENA must name a zena-cli under CI"
        );
        eprintln!("ZENA not set, skipping the Zena compile");
        return None;
    };
    let cli = PathBuf::from(cli);
    let root = std::env::var("ZENA_REPO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            cli.parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .unwrap()
                .to_path_buf()
        });
    Some((cli, root))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("ts-gen-zena-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether an `@external` name has placeholders, beside the `{{` and `}}`
/// that stand for braces.
fn is_template(name: &str) -> bool {
    name.replace("{{", "").replace("}}", "").contains('{')
}

fn compile(zena: &(PathBuf, PathBuf), entry: &Path, out: &Path) -> Vec<u8> {
    let o = Command::new(&zena.0)
        .arg("build")
        .arg(entry)
        .args(["-t", "js", "-o"])
        .arg(out)
        .env("ZENA_REPO_ROOT", &zena.1)
        .output()
        .expect("zena-cli runs");
    assert!(
        o.status.success() && out.exists(),
        "{} did not compile:\n{}{}",
        entry.display(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    std::fs::read(out).unwrap()
}

/// What one fixture's check covered.
#[derive(Default, Debug)]
struct Coverage {
    imports: usize,
    generic: usize,
    skipped: usize,
}

/// Generates, compiles and checks the imports of one IR. `main` is a Zena
/// entry importing the bindings as `./<name>.zena`; without one the
/// bindings are compiled with their probes.
fn check_imports(zena: &(PathBuf, PathBuf), name: &str, ir: &Ir, main: Option<&Path>) -> Coverage {
    let dir = TempDir::new(&format!(
        "{name}{}",
        if main.is_some() { "-main" } else { "" }
    ));
    let libs = libs(ir);
    write_libs(&libs, &dir.0);
    let out = generate(
        ir,
        &libs,
        &ts_gen_zena::Options {
            probe: main.is_none(),
            ..Default::default()
        },
    );
    let bindings = dir.0.join(format!("{name}.zena"));
    std::fs::write(&bindings, &out.zena).unwrap();
    let entry = match main {
        Some(m) => {
            let e = dir.0.join(format!("{name}.main.zena"));
            std::fs::copy(m, &e).unwrap();
            e
        }
        None => bindings.clone(),
    };
    let wasm = compile(zena, &entry, &dir.0.join(format!("{name}.wasm")));

    let profile = ts_gen_zena::profile();
    let lib_irs: Vec<&Ir> = libs.iter().collect();
    let layouts = Layouts::with_libs(ir, &lib_irs, &profile);
    let members = layouts.keys.members();
    let skipped: BTreeSet<&str> = out.skipped.iter().map(|s| s.key.as_str()).collect();
    let mut expected: BTreeMap<(String, String), FuncType> = BTreeMap::new();
    let mut generic: Vec<(&Keyed, String, String)> = Vec::new();
    // The arm variants of `union` parameters, which bindings may import.
    let mut arms: BTreeMap<(String, String), FuncType> = BTreeMap::new();
    let mut modules = BTreeSet::new();
    for k in &members {
        let Role::Import(module) = &k.role else {
            continue;
        };
        modules.insert(module.clone());
        for (key, tys) in ts_gen_abi::union::variants(&layouts.keys, k) {
            if let Ok(sig) = layouts.member_as(k, &[], Some(&tys)) {
                arms.insert((module.clone(), key), sig.core());
            }
        }
        if skipped.contains(k.key.as_str()) {
            continue;
        }
        let tparams: Vec<&str> = k
            .owner
            .iter()
            .flat_map(|c| &c.tparams)
            .chain(&k.member.tparams)
            .map(|p| p.name.as_str())
            .collect();
        if tparams.is_empty() {
            let sig = layouts.member(k, &[]).unwrap();
            expected.insert((module.clone(), k.key.clone()), sig.core());
        } else {
            let (prefix, suffix) = split_key(&k.key);
            generic.push((k, prefix, suffix));
        }
    }

    let declared: BTreeSet<(String, String)> = externals(&out.zena)
        .into_iter()
        // The runtime's protocol imports aren't members'.
        .filter(|(m, n)| modules.contains(m) && !is_template(n))
        .map(|(m, n)| (m, n.replace("{{", "{").replace("}}", "}")))
        // Nor are a generic member's instantiations, checked below.
        .filter(|(_, n)| !generic.iter().any(|(_, p, s)| instantiates(n, p, s)))
        .filter(|k| !arms.contains_key(k))
        .collect();
    let want: BTreeSet<(String, String)> = expected.keys().cloned().collect();
    assert_eq!(
        declared, want,
        "{name}: the declared externals aren't the IR's imports less those skipped"
    );

    let mut cov = Coverage {
        skipped: out.skipped.len(),
        ..Coverage::default()
    };
    let mut seen = BTreeSet::new();
    for (module, field, ty) in imports(&wasm) {
        if module == "tw:call" {
            let sig = layouts
                .call_import(&fn_type(&field), &Context::default())
                .unwrap_or_else(|e| panic!("{name}: `tw:call` `{field}`: {e}"));
            assert_eq!(
                ty,
                sig.core(),
                "{name}: `tw:call` `{field}` has the wrong type"
            );
            continue;
        }
        if module == "tw:future" {
            let arg = |p: &str| field.strip_prefix(p)?.strip_suffix('>');
            let cx = Context::default();
            let sig = if let Some(t) = arg("take<") {
                layouts.future_take(&ts_gen_abi::text::parse_type(t).unwrap(), &cx)
            } else if let Some(t) = arg("resolve<") {
                layouts.future_resolve(&ts_gen_abi::text::parse_type(t).unwrap(), &cx)
            } else if let Some(e) = arg("error<") {
                layouts.future_error(&ts_gen_abi::ir::Path::new(vec![e.to_string()]), &cx)
            } else if let Some(e) = arg("reject<") {
                layouts.future_reject(&ts_gen_abi::ir::Path::new(vec![e.to_string()]), &cx)
            } else {
                continue;
            };
            let sig = sig.unwrap_or_else(|e| panic!("{name}: `tw:future` `{field}`: {e}"));
            assert_eq!(
                ty,
                sig.core(),
                "{name}: `tw:future` `{field}` has the wrong type"
            );
            continue;
        }
        if module == "tw:loop" {
            continue;
        }
        if module == "tw:isNullish" {
            assert_eq!(Some(ty), layouts.fixed_op(Some(&module), &field));
            continue;
        }
        if !modules.contains(&module) {
            assert!(
                !module.starts_with("tw:"),
                "{name}: `{module}` `{field}` is no IR import"
            );
            continue;
        }
        let key = (module.clone(), field.clone());
        if let Some(want) = arms.get(&key) {
            assert_eq!(&ty, want, "{name}: `{field}` has the wrong type");
            continue;
        }
        if let Some(want) = expected.get(&key) {
            assert_eq!(&ty, want, "{name}: `{field}` has the wrong type");
            seen.insert(key);
            cov.imports += 1;
            continue;
        }
        let found = generic.iter().find_map(|(k, prefix, suffix)| {
            let args = field
                .strip_prefix(prefix.as_str())?
                .strip_suffix(suffix.as_str())?;
            let args = args.strip_prefix('<')?.strip_suffix('>')?;
            Some((k, args))
        });
        let Some((k, args)) = found else {
            panic!("{name}: `{field}` from `{module}` is no member's import");
        };
        let targs: Vec<Type> = split_args(args)
            .iter()
            .map(|a| ts_gen_abi::text::parse_type(a).unwrap())
            .collect();
        let sig = layouts
            .member(k, &targs)
            .unwrap_or_else(|e| panic!("{name}: `{field}`: {e}"));
        assert_eq!(ty, sig.core(), "{name}: `{field}` has the wrong type");
        cov.generic += 1;
    }
    for (export, ty) in exports(&wasm, |e| e.starts_with("tw:call:")) {
        let Some(f) = export.strip_prefix("tw:call:") else {
            continue;
        };
        let sig = layouts
            .trampoline(&fn_type(f), &Context::default())
            .unwrap_or_else(|e| panic!("{name}: `{export}`: {e}"));
        assert_eq!(ty, sig.core(), "{name}: `{export}` has the wrong type");
    }
    if main.is_none() {
        let missing: Vec<&(String, String)> =
            expected.keys().filter(|k| !seen.contains(*k)).collect();
        assert!(missing.is_empty(), "{name}: not imported: {missing:?}");
    }
    cov
}

fn instantiates(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|n| n.strip_suffix(suffix))
        .is_some_and(|args| args.starts_with('<') && args.ends_with('>'))
}

/// A key's text before and after its type argument list.
fn split_key(key: &str) -> (String, String) {
    let open = key.find('(').unwrap();
    (key[..open].to_string(), key[open..].to_string())
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// The `@external(module, name, "tw")` pairs of generated bindings.
fn externals(zena: &str) -> Vec<(String, String)> {
    zena.lines()
        .filter_map(|l| l.strip_prefix("@external("))
        .map(|l| {
            let strings = zena_strings(l);
            (strings[0].clone(), strings[1].clone())
        })
        .collect()
}

fn zena_strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut v = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => v.extend(chars.next()),
                '"' => break,
                c => v.push(c),
            }
        }
        out.push(v);
    }
    out
}

/// A compiled module's function imports, with GC types resolved
/// structurally. A GC definition must be a final singleton rec group, as
/// the bridge's are, for the types to be the same across the link.
fn imports(wasm: &[u8]) -> Vec<(String, String, FuncType)> {
    let mut types: Vec<SubType> = Vec::new();
    let mut alone: Vec<bool> = Vec::new();
    let mut raw = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.unwrap() {
            Payload::TypeSection(r) => {
                for group in r {
                    let group = group.unwrap();
                    let n = group.types().len();
                    alone.extend(std::iter::repeat_n(n == 1, n));
                    types.extend(group.into_types());
                }
            }
            Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    let i = i.unwrap();
                    if let TypeRef::Func(t) | TypeRef::FuncExact(t) = i.ty {
                        raw.push((i.module.to_string(), i.name.to_string(), t));
                    }
                }
            }
            _ => {}
        }
    }
    let t = Types {
        types: &types,
        alone: &alone,
    };
    raw.into_iter().map(|(m, n, i)| (m, n, t.func(i))).collect()
}

/// A function type's spelling, parsed.
fn fn_type(f: &str) -> FnType {
    match ts_gen_abi::text::parse_type(f) {
        Ok(Type::Fn(f)) => *f,
        other => panic!("`{f}` is no function type: {other:?}"),
    }
}

/// A module's trampoline exports, `tw:call:F`, with their types.
fn exports(wasm: &[u8], keep: impl Fn(&str) -> bool) -> Vec<(String, FuncType)> {
    let mut types: Vec<SubType> = Vec::new();
    let mut alone: Vec<bool> = Vec::new();
    let mut funcs: Vec<u32> = Vec::new();
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.unwrap() {
            Payload::TypeSection(r) => {
                for group in r {
                    let group = group.unwrap();
                    let n = group.types().len();
                    alone.extend(std::iter::repeat_n(n == 1, n));
                    types.extend(group.into_types());
                }
            }
            Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    if let TypeRef::Func(t) | TypeRef::FuncExact(t) = i.unwrap().ty {
                        funcs.push(t);
                    }
                }
            }
            Payload::FunctionSection(r) => {
                for t in r {
                    funcs.push(t.unwrap());
                }
            }
            Payload::ExportSection(r) => {
                for e in r {
                    let e = e.unwrap();
                    if e.kind == wasmparser::ExternalKind::Func && keep(e.name) {
                        out.push((e.name.to_string(), e.index));
                    }
                }
            }
            _ => {}
        }
    }
    let t = Types {
        types: &types,
        alone: &alone,
    };
    out.into_iter()
        .map(|(n, i)| (n, t.func(funcs[i as usize])))
        .collect()
}

struct Types<'a> {
    types: &'a [SubType],
    alone: &'a [bool],
}

impl Types<'_> {
    fn func(&self, i: u32) -> FuncType {
        let CompositeInnerType::Func(f) = &self.types[i as usize].composite_type.inner else {
            panic!("type {i} is not a function type");
        };
        FuncType {
            params: f.params().iter().map(|v| self.val(*v)).collect(),
            results: f.results().iter().map(|v| self.val(*v)).collect(),
        }
    }

    fn val(&self, v: wasmparser::ValType) -> ValType {
        match v {
            wasmparser::ValType::I32 => ValType::I32,
            wasmparser::ValType::I64 => ValType::I64,
            wasmparser::ValType::F32 => ValType::F32,
            wasmparser::ValType::F64 => ValType::F64,
            wasmparser::ValType::V128 => panic!("v128 is not in the ABI"),
            wasmparser::ValType::Ref(r) => {
                let heap = match r.heap_type() {
                    HeapType::Abstract {
                        ty: AbstractHeapType::Extern,
                        shared: false,
                    } => Heap::Extern,
                    HeapType::Abstract {
                        ty: AbstractHeapType::Any,
                        shared: false,
                    } => Heap::Any,
                    HeapType::Concrete(i) => {
                        Heap::Def(Box::new(self.def(i.as_module_index().unwrap())))
                    }
                    h => panic!("the heap type {h:?} is not in the ABI"),
                };
                ValType::Ref(RefType {
                    nullable: r.is_nullable(),
                    heap,
                })
            }
        }
    }

    fn def(&self, i: u32) -> Def {
        let t = &self.types[i as usize];
        assert!(
            self.alone[i as usize] && t.is_final && t.supertype_idxs.is_empty(),
            "type {i} isn't a final singleton rec group"
        );
        let storage = |f: &wasmparser::FieldType| match f.element_type {
            StorageType::I8 => Storage::I8,
            StorageType::I16 => Storage::I16,
            StorageType::Val(v) => Storage::Val(self.val(v)),
        };
        match &t.composite_type.inner {
            CompositeInnerType::Array(a) if a.0.mutable => Def::Array(storage(&a.0)),
            CompositeInnerType::Struct(s) if s.fields.iter().all(|f| !f.mutable) => {
                Def::Struct(s.fields.iter().map(storage).collect())
            }
            _ => panic!("type {i} is not an ABI type"),
        }
    }
}

#[test]
fn imports_match() {
    let Some(zena) = zena() else {
        return;
    };
    let mut report = HashMap::new();
    for (name, path) in fixtures() {
        let ir = read_ir(&path);
        let main = path.with_extension("main.zena");
        let cov = check_imports(&zena, &name, &ir, None);
        report.insert(name.clone(), cov);
        if main.exists() {
            let cov = check_imports(&zena, &name, &ir, Some(&main));
            assert!(cov.generic > 0, "{name}.main.zena instantiates nothing");
            report.insert(format!("{name}.main"), cov);
        }
    }
    let mut names: Vec<_> = report.keys().cloned().collect();
    names.sort();
    for n in names {
        let c = &report[&n];
        eprintln!(
            "{n}: {} imports, {} generic instantiations, {} skipped",
            c.imports, c.generic, c.skipped
        );
    }
}

/// Compiles each IR with a `tests/fixtures/<name>.app.zena` app through
/// its entry module, and checks the exports' types are the IR's.
#[test]
fn exports_match() {
    let Some(zena) = zena() else {
        return;
    };
    for (name, path) in fixtures() {
        let app = crate_root().join(format!("tests/fixtures/{name}.app.zena"));
        if !app.exists() {
            continue;
        }
        let ir = read_ir(&path);
        let dir = TempDir::new(&format!("{name}-app"));
        let libs = libs(&ir);
        write_libs(&libs, &dir.0);
        let out = generate(
            &ir,
            &libs,
            &ts_gen_zena::Options {
                entry: Some(ts_gen_zena::Entry {
                    bindings: format!("./{name}.zena"),
                    app: "./app.zena".into(),
                }),
                ..Default::default()
            },
        );
        std::fs::write(dir.0.join(format!("{name}.zena")), &out.zena).unwrap();
        std::fs::copy(&app, dir.0.join("app.zena")).unwrap();
        let entry = dir.0.join("main.zena");
        std::fs::write(&entry, out.entry.unwrap()).unwrap();
        let wasm = compile(&zena, &entry, &dir.0.join(format!("{name}.wasm")));

        let profile = ts_gen_zena::profile();
        let lib_irs: Vec<&Ir> = libs.iter().collect();
        let layouts = Layouts::with_libs(&ir, &lib_irs, &profile);
        let skipped: BTreeSet<&str> = out.skipped.iter().map(|s| s.key.as_str()).collect();
        let mut want: BTreeMap<String, FuncType> = BTreeMap::new();
        for k in layouts.keys.members() {
            if k.role == Role::Export && !skipped.contains(k.key.as_str()) {
                want.insert(k.key.clone(), layouts.member(&k, &[]).unwrap().core());
            }
        }
        assert!(!want.is_empty(), "{name} has no exports");
        let got: BTreeMap<String, FuncType> = exports(&wasm, |e| want.contains_key(e))
            .into_iter()
            .collect();
        assert_eq!(got, want, "{name}: the exports aren't the IR's");
    }
}

/// `std`, the lib other IRs resolve against, binds whole but for the
/// comparators whose `number | bigint` result Zena has no type for, and
/// compiles, with its imports' types the IR's.
#[test]
fn std_lib() {
    let ir = read_ir(&repo_root().join("std/std.ir"));
    let out = ts_gen_zena::generate(&ir, &ts_gen_zena::Options::default()).unwrap();
    let skipped: Vec<&str> = out.skipped.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(
        skipped,
        [
            "BigInt64Array#sort(fn(i64,i64)->F64OrBigInt)",
            "BigUint64Array#sort(fn(u64,u64)->F64OrBigInt)",
        ],
        "std skips members"
    );
    let Some(zena) = zena() else {
        return;
    };
    let cov = check_imports(&zena, "std", &ir, None);
    assert!(cov.imports > 1000, "{cov:?}");
}

#[test]
fn workers_types() {
    let Some(zena) = zena() else {
        return;
    };
    let fixture = repo_root().join("tests/fixtures/workers-types.d.ts");
    let (module, gctx) = ts_gen::parse(&[fixture], None).unwrap();
    let lowered = ts_gen::lower::lower(&module, &gctx);
    let text = ts_gen_abi::text::print(&lowered.ir);
    let ir = ts_gen_abi::text::parse(&text).unwrap();
    let cov = check_imports(&zena, "workers-types", &ir, None);
    eprintln!(
        "workers-types: {} imports, {} skipped",
        cov.imports, cov.skipped
    );
    assert!(cov.imports > 1000, "{cov:?}");
}

/// `examples/zena-worker` end to end in node: the IR, the bindings and
/// entry module, the Zena module and its bridge.
#[test]
fn zena_worker() {
    let Some(zena) = zena() else {
        return;
    };
    let run = repo_root().join("examples/zena-worker/run.sh");
    let out = Command::new(&run)
        .arg("node")
        .env("ZENA", &zena.0)
        .env("ZENA_REPO_ROOT", &zena.1)
        .output()
        .expect("run.sh runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "run.sh node:\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let reply = stdout.split_once("200 ").map(|(_, r)| r).unwrap_or(&stdout);
    assert_eq!(
        reply,
        "text/plain; charset=utf-8\n\
         POST http://127.0.0.1/echo\n\
         x-zena: hi\n\
         body: Hello!\n\
         headers: x-hang x-zena\n\
         invalid URL: TypeError\n\
         caught: Failed to parse URL from not a url\n\
         race: timed out\n\
         GREETING: Hello from node\n\
         aborted: true\n\
         count: 1\n\
         count: 2\n\
         rejected: TypeError: zena says no\n\
         constructor threw: Error: no greeting\n"
    );
}
