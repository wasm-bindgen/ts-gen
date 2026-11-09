//! The Kotlin generator (`generators/kotlin`), end to end.
//!
//! Every fixture IR is lowered under Kotlin's profile (`all-host`, with
//! `results = single` and `throws = js`), generated, and
//! compared against `generators/kotlin/tests/snapshots/<fixture>/`. Then each
//! compiles as its own Kotlin/Wasm program (one Gradle build, a project per
//! fixture), and each module's interop surface is checked against the
//! lowered layouts: every
//! import is `(module, key)` or a `js_code` import its `links.tsv` renames
//! to one, every export is named by its key, and each has its core wire
//! type. workers-types and std are compiled and checked, but only their
//! skipped lists are snapshotted.
//!
//! Needs a JDK (Gradle provisions the rest), and node (26+ in CI) for the
//! worker and runtime tests; without them a test is skipped locally and fails
//! under `CI`. Any other failure fails everywhere. `TSGEN_SKIP_KOTLIN`
//! skips them outright, for CI jobs without a JDK. To update snapshots:
//!
//! ```sh
//! BLESS=1 cargo test --test kotlin
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use ts_gen_abi::ir::Ir;
use ts_gen_abi::json::{self, Json};
use ts_gen_abi::profile::Profile;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn kotlin_dir() -> PathBuf {
    root().join("generators/kotlin")
}

fn work() -> PathBuf {
    root().join("target/kotlin-test")
}

fn ci() -> bool {
    std::env::var_os("CI").is_some()
}

/// Whether to skip: `TSGEN_SKIP_KOTLIN` is set, or a tool is missing (a
/// JDK, and node when `node`), which fails under `CI` instead.
fn skipped(node: bool) -> bool {
    if std::env::var_os("TSGEN_SKIP_KOTLIN").is_some() {
        eprintln!("TSGEN_SKIP_KOTLIN is set, skipping");
        return true;
    }
    let java = std::env::var_os("JAVA_HOME")
        .map_or_else(|| "java".into(), |h| PathBuf::from(h).join("bin/java"));
    let missing = if Command::new(java).arg("-version").output().is_err() {
        "a JDK"
    } else if node && Command::new("node").arg("--version").output().is_err() {
        "node"
    } else {
        return false;
    };
    assert!(!ci(), "{missing} is required under CI");
    eprintln!("skipping: {missing} not found");
    true
}

/// `./gradlew` in `dir`.
fn gradle(dir: &Path, args: &[&str]) {
    let out = Command::new(kotlin_dir().join("gradlew"))
        .current_dir(dir)
        .arg("--quiet")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gradlew {} in {}:\n{}{}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The installed generator, built once: the tests share its build
/// directory, so concurrent `installDist`s race.
fn generator() -> PathBuf {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| gradle(&kotlin_dir(), &["installDist"]));
    kotlin_dir().join("build/install/ts-gen-kotlin/bin/ts-gen-kotlin")
}

struct Fixture {
    name: String,
    ir: Ir,
    snapshot: bool,
}

fn parse(path: &Path, libs: &[Ir]) -> Ir {
    let src = std::fs::read_to_string(path).unwrap();
    let ir = ts_gen_abi::text::parse(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let opts = ts_gen_abi::validate::Options {
        libs,
        ..Default::default()
    };
    ts_gen_abi::validate::validate_with(&ir, opts).unwrap();
    ir
}

/// The fixtures, libs first: `std` is generated as its own package, which
/// the fixtures that resolve against it depend on.
fn fixtures() -> Vec<Fixture> {
    let std = parse(&root().join("std/std.ir"), &[]);
    let mut out = vec![
        Fixture {
            name: "std".into(),
            ir: std.clone(),
            snapshot: false,
        },
        Fixture {
            name: "sample".into(),
            ir: parse(&root().join("abi/tests/json/sample.ir"), &[]),
            snapshot: true,
        },
    ];
    let libs = [std];
    let mut lower: Vec<PathBuf> = std::fs::read_dir(root().join("tests/lower"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ir"))
        .collect();
    lower.sort();
    for p in lower {
        out.push(Fixture {
            name: p.file_stem().unwrap().to_string_lossy().into(),
            ir: parse(&p, &libs),
            snapshot: true,
        });
    }
    let files = vec![root().join("tests/fixtures/workers-types.d.ts")];
    let (module, gctx) = ts_gen::parse(&files, None).unwrap();
    out.push(Fixture {
        name: "workers-types".into(),
        ir: ts_gen::lower::lower(&module, &gctx).ir,
        snapshot: false,
    });
    out
}

fn package(name: &str) -> String {
    format!("fixtures.{}", name.replace('-', "_"))
}

fn files(dir: &Path, base: &Path, out: &mut BTreeMap<String, String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, base, out);
        } else {
            let rel = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, std::fs::read_to_string(&p).unwrap());
        }
    }
}

/// What the lowered JSON says the module imports and exports, with core
/// types: members, callback call imports and trampolines.
#[derive(Default)]
struct Surface {
    imports: BTreeMap<(String, String), String>,
    exports: BTreeMap<String, String>,
    /// Imports a module may have but needn't: `take<T>` of the futures it
    /// reads, and the runtime's.
    optional: BTreeMap<(String, String), String>,
}

fn import_key(o: &Json) -> (String, String) {
    let Json::Obj(o) = o else { unreachable!() };
    (s(get(o, "module")), s(get(o, "name")))
}

/// What `--runtime` adds: `tsgen.runtime`'s protocol imports and exports.
/// Also the `tw:ret` spill, which any module may use.
fn runtime(out: &mut Surface) {
    for vt in ts_gen_abi::protocol::SPILL_TYPES {
        out.optional
            .insert(("tw:ret".into(), vt.into()), format!("() -> ({vt})"));
        out.optional.insert(
            ("tw:ret".into(), format!("push:{vt}")),
            format!("({vt}) -> ()"),
        );
    }
    for (m, n, t) in [
        ("tw:future", "poll", "(externref, i32) -> (i32)"),
        ("tw:future", "drop", "(externref) -> ()"),
        ("tw:loop", "work_available", "(i32) -> ()"),
    ] {
        out.optional.insert((m.into(), n.into()), t.into());
    }
    for (n, t) in [
        ("tw:loop/tick", "(i32, i32) -> (i32)"),
        ("tw:loop/next_sleep", "(i32) -> (i64)"),
        ("tw:future/wake", "(i32) -> ()"),
    ] {
        out.exports.insert(n.into(), t.into());
    }
}

/// The profile Kotlin generates for: host carriers, with multi-value
/// results spilled and errors read and set without tags, since Kotlin
/// functions return one value and Kotlin can't declare Wasm tags.
fn kotlin_profile() -> Profile {
    let mut p = Profile::all_host();
    p.results = ts_gen_abi::profile::Results::Single;
    p.throws = ts_gen_abi::profile::Throws::Js;
    p
}

fn get<'a>(o: &'a [(String, Json)], k: &str) -> Option<&'a Json> {
    o.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

fn s(j: Option<&Json>) -> String {
    match j {
        Some(Json::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn core(sig: &Json) -> String {
    let Json::Obj(o) = sig else {
        return String::new();
    };
    let Some(Json::Obj(c)) = get(o, "core") else {
        return String::new();
    };
    let list = |k| strs(get(c, k)).join(", ");
    format!("({}) -> ({})", list("params"), list("results"))
}

fn walk(j: &Json, out: &mut Surface) {
    match j {
        Json::Obj(o) => {
            if let (Some(key), Some(sig)) = (get(o, "key"), get(o, "sig")) {
                let key = s(Some(key));
                if s(get(o, "role")) == "import" {
                    out.imports.insert((s(get(o, "module")), key), core(sig));
                } else {
                    out.exports.insert(key, core(sig));
                }
            }
            // `tw:new` and the `tw:unbox` recovering its instance.
            if let (Some(key), Some(c @ Json::Obj(_)), Some(owner)) =
                (get(o, "key"), get(o, "construct"), get(o, "owner"))
            {
                out.optional
                    .insert(("tw:new".into(), s(Some(key))), core(c));
                out.optional.insert(
                    ("tw:unbox".into(), s(Some(owner))),
                    "(externref) -> (externref)".into(),
                );
            }
            // A box helper's `tw:box` and `tw:unbox`.
            for k in ["box", "unbox"] {
                if let Some(Json::Obj(b)) = get(o, k) {
                    let Some(i) = get(b, "import") else { continue };
                    let list = |k| strs(get(b, k)).join(", ");
                    out.optional.insert(
                        import_key(i),
                        format!("({}) -> ({})", list("params"), list("results")),
                    );
                }
            }
            if let (Some(t), Some(op)) = (get(o, "trampoline"), get(o, "op")) {
                out.exports.insert(s(Some(t)), core(op));
            }
            if let (Some(t), Some(Json::Obj(v))) = (get(o, "take"), get(o, "value")) {
                let wire = strs(get(v, "wire"));
                let slot0 = wire.first().cloned().unwrap_or_default();
                out.optional
                    .insert(import_key(t), format!("(externref) -> ({slot0})"));
                out.optional.insert(
                    import_key(get(o, "resolve").unwrap()),
                    format!(
                        "({}) -> ()",
                        [vec!["externref".into()], wire].concat().join(", ")
                    ),
                );
                out.optional.insert(
                    import_key(get(o, "new").unwrap()),
                    "() -> (externref)".into(),
                );
                let slots = enum_slots(get(o, "rejection"));
                out.optional.insert(
                    import_key(get(o, "errorOp").unwrap()),
                    "(externref) -> (i32)".into(),
                );
                out.optional.insert(
                    import_key(get(o, "reject").unwrap()),
                    format!("(externref, i32{slots}) -> ()"),
                );
            }
            if let (Some(r), Some(st)) = (get(o, "read"), get(o, "set")) {
                let slots = enum_slots(get(o, "enum"));
                out.optional
                    .insert(import_key(r), "(externref) -> (i32)".into());
                out.optional
                    .insert(import_key(st), format!("(i32{slots}) -> ()"));
            }
            if let (Some(Json::Obj(c)), Some(op)) = (get(o, "call"), get(o, "op")) {
                out.imports
                    .insert((s(get(c, "module")), s(get(c, "name"))), core(op));
            }
            for (_, v) in o {
                walk(v, out);
            }
        }
        Json::Arr(xs) => xs.iter().for_each(|x| walk(x, out)),
        _ => {}
    }
}

fn strs(j: Option<&Json>) -> Vec<String> {
    match j {
        Some(Json::Arr(xs)) => xs.iter().map(|x| s(Some(x))).collect(),
        _ => Vec::new(),
    }
}

/// `, s0, s1..`: an enum layout's slot types.
fn enum_slots(e: Option<&Json>) -> String {
    let Some(Json::Obj(e)) = e else {
        return String::new();
    };
    let Some(Json::Obj(shape)) = get(e, "shape") else {
        return String::new();
    };
    strs(get(shape, "slots"))
        .iter()
        .map(|t| format!(", {t}"))
        .collect()
}

fn val(v: &wasmparser::ValType) -> String {
    use wasmparser::ValType::*;
    match v {
        I32 => "i32".into(),
        I64 => "i64".into(),
        F32 => "f32".into(),
        F64 => "f64".into(),
        V128 => "v128".into(),
        Ref(r) if *r == wasmparser::RefType::EXTERNREF => "externref".into(),
        Ref(r) => format!("{r:?}"),
    }
}

/// A compiled module's imports `(module, name, type)` and exports
/// `name -> type`.
type Signatures = (Vec<(String, String, String)>, BTreeMap<String, String>);

fn compiled(wasm: &[u8]) -> Signatures {
    use wasmparser::{CompositeInnerType, ExternalKind, Parser, Payload, TypeRef};
    let mut types = Vec::new();
    let mut funcs = Vec::new();
    let mut imports = Vec::new();
    let mut exports = BTreeMap::new();
    let fmt = |f: &wasmparser::FuncType| {
        let l = |vs: &[wasmparser::ValType]| vs.iter().map(val).collect::<Vec<_>>().join(", ");
        format!("({}) -> ({})", l(f.params()), l(f.results()))
    };
    for p in Parser::new(0).parse_all(wasm) {
        match p.unwrap() {
            Payload::TypeSection(r) => {
                for g in r {
                    for t in g.unwrap().into_types() {
                        types.push(match &t.composite_type.inner {
                            CompositeInnerType::Func(f) => Some(fmt(f)),
                            _ => None,
                        });
                    }
                }
            }
            Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    let i = i.unwrap();
                    if let TypeRef::Func(t) = i.ty {
                        funcs.push(t);
                        let ty = types[t as usize].clone().unwrap();
                        imports.push((i.module.to_string(), i.name.to_string(), ty));
                    }
                }
            }
            Payload::FunctionSection(r) => {
                for f in r {
                    funcs.push(f.unwrap());
                }
            }
            Payload::ExportSection(r) => {
                for e in r {
                    let e = e.unwrap();
                    if e.kind == ExternalKind::Func {
                        let t = funcs[e.index as usize];
                        exports.insert(e.name.to_string(), types[t as usize].clone().unwrap());
                    }
                }
            }
            _ => {}
        }
    }
    (imports, exports)
}

#[test]
fn kotlin() {
    if skipped(false) {
        return;
    }
    let generator = generator();
    // Kotlin reads `undefined` as null, so it can't hold the unit box.
    {
        let dir = work().join("box-none");
        std::fs::create_dir_all(&dir).unwrap();
        let ir = ts_gen_abi::text::parse("ir 1;\nf(v: Str?);").unwrap();
        let mut p = kotlin_profile();
        p.box_none = ts_gen_abi::profile::BoxNone::Unit;
        let lowered = dir.join("lowered.json");
        std::fs::write(&lowered, ts_gen_abi::json::lower(&ir, &p).pretty()).unwrap();
        let out = Command::new(&generator)
            .arg(&lowered)
            .arg(dir.join("out"))
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("box-none = unit"));
    }
    let bless = std::env::var_os("BLESS").is_some();
    let check = work().join("check");
    let gen = check.join("gen");
    let _ = std::fs::remove_dir_all(&gen);
    std::fs::create_dir_all(&gen).unwrap();
    let mut projects = Vec::new();
    let profile = kotlin_profile();
    let mut failures = Vec::new();
    let mut surfaces: BTreeMap<String, (Surface, BTreeMap<String, String>)> = BTreeMap::new();
    let fixtures = fixtures();
    let lib_irs: Vec<&Ir> = fixtures
        .iter()
        .filter(|f| f.ir.lib.is_some())
        .map(|f| &f.ir)
        .collect();
    let mut deps = BTreeMap::new();
    for f in &fixtures {
        let libs: Vec<&Ir> =
            f.ir.with
                .iter()
                .map(|w| *lib_irs.iter().find(|l| l.lib.as_ref() == Some(w)).unwrap())
                .collect();
        let lib_names = f.ir.with.clone();
        deps.insert(f.name.clone(), lib_names.clone());
        let lowered = json::lower_with(&f.ir, &libs, &profile);
        let input = work().join(format!("{}.json", f.name));
        std::fs::write(&input, lowered.pretty()).unwrap();
        let project = gen.join(&f.name);
        let out = project.join("src");
        projects.push(f.name.clone());
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("build.gradle.kts"),
            format!(
                "@file:OptIn(org.jetbrains.kotlin.gradle.ExperimentalWasmDsl::class)\nplugins {{ kotlin(\"multiplatform\") }}\nkotlin {{\n    wasmJs {{ outputModuleName.set(\"{}\"); nodejs(); binaries.executable() }}\n    sourceSets.wasmJsMain {{\n        kotlin.srcDir(\"src\"); kotlin.srcDir(\"main\")\n        dependencies {{ {}}}\n    }}\n}}\n",
                module_name(&f.name),
                lib_names
                    .iter()
                    .map(|l| format!("implementation(project(\":{l}\")) "))
                    .collect::<String>()
            ),
        )
        .unwrap();
        let pkg = package(&f.name);
        std::fs::create_dir_all(project.join("main")).unwrap();
        std::fs::write(
            project.join("main/Main.kt"),
            format!("package {pkg}.main\n\nfun main() {{}}\n"),
        )
        .unwrap();
        let app = format!("{pkg}.app");
        let scaffold = out.join("app/App.kt");
        let status = Command::new(&generator)
            .args([input.to_str().unwrap(), out.to_str().unwrap()])
            .args(["--package", &pkg, "--app-package", &app])
            .args(
                lib_names
                    .iter()
                    .flat_map(|l| ["--lib".to_string(), format!("{l}={}", package(l))]),
            )
            .args(lib_names.is_empty().then_some("--runtime"))
            .args(["--scaffold", scaffold.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}: {}",
            f.name,
            String::from_utf8_lossy(&status.stderr)
        );
        let mut actual = BTreeMap::new();
        files(&out, &out, &mut actual);
        let mut surface = Surface::default();
        walk(&lowered, &mut surface);
        if lib_names.is_empty() {
            runtime(&mut surface);
        }
        surfaces.insert(f.name.clone(), (surface, actual.clone()));
        let snap = kotlin_dir().join("tests/snapshots").join(&f.name);
        // The runtime is the generator's fixed source, not generated.
        actual.retain(|k, _| !k.starts_with("tsgen/runtime/"));
        if !f.snapshot {
            actual.retain(|k, _| k == "skipped.txt");
        }
        if bless {
            let _ = std::fs::remove_dir_all(&snap);
            for (rel, text) in &actual {
                let p = snap.join(rel);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
            }
            continue;
        }
        let mut want = BTreeMap::new();
        if snap.exists() {
            files(&snap, &snap, &mut want);
        }
        if want != actual {
            let names: BTreeSet<&String> = want.keys().chain(actual.keys()).collect();
            for n in names {
                if want.get(n) != actual.get(n) {
                    let diff = similar::TextDiff::from_lines(
                        want.get(n).map_or("", String::as_str),
                        actual.get(n).map_or("", String::as_str),
                    );
                    failures.push(format!(
                        "{}/{n} differs:\n{}",
                        f.name,
                        diff.unified_diff().context_radius(2)
                    ));
                }
            }
        }
    }
    let includes: String = projects
        .iter()
        .map(|p| format!("include(\"{p}\")\nproject(\":{p}\").projectDir = file(\"gen/{p}\")\n"))
        .collect();
    std::fs::write(
        check.join("settings.gradle.kts"),
        format!("rootProject.name = \"check\"\n{includes}"),
    )
    .unwrap();
    std::fs::write(
        check.join("build.gradle.kts"),
        "plugins { kotlin(\"multiplatform\") version \"2.5.0-Beta2\" apply false }\nallprojects { repositories { mavenCentral() } }\n",
    )
    .unwrap();
    std::fs::write(check.join("gradle.properties"), "kotlin.daemon.jvmargs=-Xmx6g\norg.gradle.jvmargs=-Xmx2g\nkotlin.incremental.wasm=false\nkotlin.code.style=official\n").unwrap();
    gradle(
        &check,
        &["--continue", "compileDevelopmentExecutableKotlinWasmJs"],
    );
    for (name, (surface, out)) in &surfaces {
        let libs: Vec<_> = deps[name].iter().map(|l| &surfaces[l]).collect();
        let wasm = std::fs::read(gen.join(name).join(format!(
            "build/compileSync/wasmJs/main/developmentExecutable/kotlin/{}.wasm",
            module_name(name)
        )))
        .unwrap();
        check_surface(name, surface, out, &libs, &wasm, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn module_name(fixture: &str) -> String {
    fixture.replace('-', "_")
}

/// Checks one compiled module against its lowered layouts.
fn check_surface(
    name: &str,
    surface: &Surface,
    out: &BTreeMap<String, String>,
    libs: &[&(Surface, BTreeMap<String, String>)],
    wasm: &[u8],
    failures: &mut Vec<String>,
) {
    let (imports, exports) = compiled(wasm);
    let mut linked = BTreeMap::new();
    let mut want_imports = BTreeMap::new();
    let mut want_exports = BTreeMap::new();
    {
        let skipped: BTreeSet<&str> = out
            .get("skipped.txt")
            .map(|s| s.lines().filter_map(|l| l.split(": ").next()).collect())
            .unwrap_or_default();
        for l in out.get("links.tsv").map_or("", String::as_str).lines() {
            let f: Vec<&str> = l.split('\t').collect();
            linked.insert(f[1].to_string(), (f[2].to_string(), f[3].to_string()));
        }
        for (k, t) in &surface.imports {
            if !skipped.contains(k.1.as_str()) {
                want_imports.insert(k.clone(), (name.to_string(), t.clone()));
            }
        }
        for (k, t) in &surface.exports {
            if !skipped.contains(k.as_str()) {
                want_exports.insert(k.clone(), (name.to_string(), t.clone()));
            }
        }
    }
    // A lib's bindings are linked in, but only what the fixture uses is kept.
    let mut lib_imports = BTreeMap::new();
    for (ls, lout) in libs {
        for l in lout.get("links.tsv").map_or("", String::as_str).lines() {
            let f: Vec<&str> = l.split('\t').collect();
            linked.insert(f[1].to_string(), (f[2].to_string(), f[3].to_string()));
        }
        lib_imports.extend(ls.imports.iter().map(|(k, t)| (k.clone(), t.clone())));
        lib_imports.extend(ls.optional.iter().map(|(k, t)| (k.clone(), t.clone())));
    }
    lib_imports.extend(surface.optional.iter().map(|(k, t)| (k.clone(), t.clone())));
    let mut seen = BTreeSet::new();
    for (module, name, ty) in &imports {
        let key = match linked.get(name) {
            Some(k) if module == "js_code" => k.clone(),
            _ if module == "js_code"
                || !module.starts_with("tw:") && !want_imports.keys().any(|(m, _)| m == module) =>
            {
                continue
            }
            _ => (module.clone(), name.clone()),
        };
        match want_imports.get(&key) {
            None if lib_imports.get(&key) == Some(ty) => continue,
            Some((_, want)) if want == ty => {}
            Some((f, want)) => failures.push(format!(
                "{f}: import {key:?} is {ty}, the layout says {want}"
            )),
            None => failures.push(format!(
                "import {key:?} ({ty}) is no member's or operation's"
            )),
        }
        seen.insert(key);
    }
    for (key, (f, _)) in &want_imports {
        if !seen.contains(key) && !key.0.starts_with("tw:call") {
            failures.push(format!(
                "{f}: import {key:?} is missing from the compiled module"
            ));
        }
    }
    for (key, (f, want)) in &want_exports {
        match exports.get(key) {
            Some(ty) if ty == want => {}
            Some(ty) => failures.push(format!("{f}: export {key} is {ty}, the layout says {want}")),
            None if key.starts_with("tw:call:") => {}
            None => failures.push(format!(
                "{f}: export {key} is missing from the compiled module"
            )),
        }
    }
}

/// `examples/kotlin-worker` end to end in node: the worker's fetch awaits
/// the request body and a timer on the runtime's loop, through the linked
/// module and its bridge.
#[test]
fn kotlin_worker() {
    if skipped(true) {
        return;
    }
    let stdout = run_script(&root().join("examples/kotlin-worker/run.sh"), &["node"]);
    let reply = stdout.split_once("200 ").map(|(_, r)| r).unwrap_or(&stdout);
    assert_eq!(
        reply,
        "text/plain; charset=utf-8\nPOST http://127.0.0.1/echo\nx-kotlin: hi\nGREETING: Hello from node\nbody: Hello!\nform: str v, none\nurl: TypeError\n"
    );
}

/// `generators/kotlin/tests/runtime` in node: Kotlin exports called through
/// the bridge, for what the fixtures only compile. A tuple result pushes
/// its second slot through `tw:ret`, a thrown error goes out through
/// `set:E` (a foreign exception as the fallback arm), and a future rejects
/// through `reject<E>`.
#[test]
fn kotlin_runtime() {
    if skipped(true) {
        return;
    }
    let stdout = run_script(&kotlin_dir().join("tests/runtime/run.sh"), &[]);
    assert_eq!(
        stdout,
        "split(7) = [3,true]\nsplit(8) = [4,false]\ncheck(3) = 30\n\
         check(0) threw RangeError: busy\ncheck(1) threw Error: one\n\
         settle(true) = settled\nsettle(false) rejected RangeError: later\n\
         absent: true,true,false\nimported: true\ncounter: true 7\n\
         modes: [\"off\",5] 0 9\nadder: function 42 2\n\
         box nones: undefined undefined,null\nspill empty: true\n"
    );
}

/// A build-and-run script's stdout, with the generator built.
fn run_script(script: &Path, args: &[&str]) -> String {
    generator();
    let out = Command::new(script)
        .args(args)
        .env("TSGEN_KOTLIN_BUILT", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{}:\n{stdout}{}",
        script.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}
