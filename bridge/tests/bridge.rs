//! Bridge fixtures: each `tests/fixtures/<name>/` has an `ir.ts`, an
//! optional `profile`, and a `test.mjs`. The language module is the
//! fixture's `lang.wat` (with `{RUNTIME}` standing for the runtime below), or by default a pass-through module that imports
//! every IR member (and each `protocol` line) and re-exports it as
//! `call:<key>` (`call:<group>:<name>` for protocol imports), over the counting runtime in `tests/runtime.wat`.
//!
//! The generated glue and bridge module are snapshots under
//! `tests/snapshots/<name>.{js,wat}` (`BLESS=1` rewrites them). Then Node
//! 26+ runs `test.mjs` next to the generated files. Without Node the
//! runtime half is skipped locally, and fails under `CI`.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use std::cell::RefCell;
use ts_gen_abi::ir::Ir;
use ts_gen_abi::key::{Keys, Role};

use ts_gen_abi::layout::{spill_name, Def, FuncType, Heap, Layouts, Storage, ValType};
use ts_gen_abi::profile::{MemoryWidth, Profile, Results};
use ts_gen_abi::resolve::Context;
use ts_gen_abi::{protocol, text};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// GC type definitions in first-use order, each a singleton rec group, as
/// the bridge declares them.
#[derive(Default)]
struct Types(Vec<Def>);

impl Types {
    fn ty(&mut self, v: &ValType) -> String {
        let ValType::Ref(r) = v else {
            return v.to_string();
        };
        let heap = match &r.heap {
            Heap::Extern => {
                return if r.nullable {
                    "externref"
                } else {
                    "(ref extern)"
                }
                .into()
            }
            Heap::Any => return if r.nullable { "anyref" } else { "(ref any)" }.into(),
            Heap::Def(_) => {
                let mut ds = Vec::new();
                v.defs(&mut ds);
                for d in &ds {
                    if !self.0.contains(d) {
                        self.0.push(d.clone());
                    }
                }
                let d = ds.last().unwrap();
                format!("$d{}", self.0.iter().position(|x| x == d).unwrap())
            }
        };
        if r.nullable {
            format!("(ref null {heap})")
        } else {
            format!("(ref {heap})")
        }
    }

    fn storage(&mut self, s: &Storage) -> String {
        match s {
            Storage::I8 => "i8".into(),
            Storage::I16 => "i16".into(),
            Storage::Val(v) => self.ty(v),
        }
    }

    fn sig(&mut self, f: &FuncType) -> String {
        let mut s = String::new();
        if !f.params.is_empty() {
            let ps: Vec<String> = f.params.iter().map(|v| self.ty(v)).collect();
            let _ = write!(s, " (param {})", ps.join(" "));
        }
        if !f.results.is_empty() {
            let rs: Vec<String> = f.results.iter().map(|v| self.ty(v)).collect();
            let _ = write!(s, " (result {})", rs.join(" "));
        }
        s
    }

    fn decls(&mut self) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < self.0.len() {
            let body = match self.0[i].clone() {
                Def::Array(e) => format!("(array (mut {}))", self.storage(&e)),
                Def::Struct(fs) => {
                    let fs: Vec<String> = fs
                        .iter()
                        .map(|f| format!(" (field {})", self.storage(f)))
                        .collect();
                    format!("(struct{})", fs.concat())
                }
            };
            let _ = writeln!(out, "  (type $d{i} {body})");
            i += 1;
        }
        out
    }
}

fn wat_str(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            b => {
                let _ = write!(out, "\\{b:02x}");
            }
        }
    }
    out.push('"');
    out
}

thread_local! {
    /// The runtime the pass-through module embeds, when not `runtime.wat`.
    static RUNTIME: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The pass-through language module.
fn passthrough(ir: &Ir, libs: &[Ir], profile: &Profile, protocol: &str) -> String {
    let libs = ts_gen_abi::resolve::libs_for(ir, libs).expect("libs");
    let l = Layouts::with_libs(ir, &libs, profile);
    let keys = Keys::with_libs(ir, &libs);
    let mut imports = String::new();
    let mut funcs = String::new();
    let mut n = 0;
    let mut tags = Vec::new();
    let mut tag_imports = String::new();
    let mut tag_exports = String::new();
    let types = RefCell::new(Types::default());
    // Under `results = single` the forwarders keep the full signature for
    // the JS test, popping the spilled slots after an import's slot 0 and
    // pushing them before an export returns slot 0.
    if profile.results == Results::Single {
        for vt in protocol::SPILL_TYPES {
            let _ = writeln!(
                imports,
                "  (import \"tw:ret\" \"{vt}\" (func $ret_{vt} (result {vt})))"
            );
            let _ = writeln!(
                imports,
                "  (import \"tw:ret\" \"push:{vt}\" (func $push_{vt} (param {vt})))"
            );
        }
    }
    let lang = |f: &FuncType| l.lang_core(f).unwrap_or_else(|e| panic!("{f}: {e}"));
    // `loops` (benchmarks): each import also gets `loop:<export>(n, args…)`,
    // calling it `n` times from Wasm so a measurement has no JS → Wasm hop.
    let loops = protocol.lines().any(|l| l.trim() == "loops");
    let mut add = |module: &str,
                   name: &str,
                   export: &str,
                   f: &FuncType,
                   imports: &mut String,
                   funcs: &mut String| {
        let lc = lang(f);
        let isig = types.borrow_mut().sig(&lc.ty);
        let sig = types.borrow_mut().sig(f);
        let _ = writeln!(
            imports,
            "  (import {} {} (func $i{n}{isig}))",
            wat_str(module),
            wat_str(name)
        );
        let mut body = String::new();
        for i in 0..f.params.len() {
            let _ = write!(body, " (local.get {i})");
        }
        let _ = write!(body, " (call $i{n})");
        for v in &lc.spill {
            let vt = spill_name(v).unwrap();
            let _ = write!(body, " (call $ret_{vt})");
            if matches!(v, ValType::Ref(r) if !r.nullable) {
                body.push_str(" ref.as_non_null");
            }
        }
        let _ = writeln!(
            funcs,
            "  (func (export {}){sig}{body})",
            wat_str(&format!("call:{export}"))
        );
        if loops {
            let mut ps = String::from(" (param i32)");
            let mut args = String::new();
            for (i, v) in lc.ty.params.iter().enumerate() {
                let _ = write!(ps, " (param {})", types.borrow_mut().ty(v));
                let _ = write!(args, " (local.get {})", i + 1);
            }
            let mut drops = " drop".repeat(lc.ty.results.len());
            for v in &lc.spill {
                let _ = write!(drops, " (call $ret_{}) drop", spill_name(v).unwrap());
            }
            let _ = writeln!(
                funcs,
                "  (func (export {}){ps} (local $i i32) (block $b (loop $l (br_if $b (i32.ge_u (local.get $i) (local.get 0))){args} (call $i{n}){drops} (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))))",
                wat_str(&format!("loop:{export}"))
            );
        }
        n += 1;
    };
    // An export forwards to the JS test's `("test:impl", key)`.
    let mut x = 0;
    let mut export = |key: &str, f: &FuncType, imports: &mut String, funcs: &mut String| {
        let lc = lang(f);
        let sig = types.borrow_mut().sig(f);
        let esig = types.borrow_mut().sig(&lc.ty);
        let _ = writeln!(
            imports,
            "  (import \"test:impl\" {} (func $x{x}{sig}))",
            wat_str(key)
        );
        let mut body = String::new();
        for i in 0..f.params.len() {
            let _ = write!(body, " (local.get {i})");
        }
        let _ = write!(body, " (call $x{x})");
        if !lc.spill.is_empty() {
            let mut locals = String::new();
            for (j, v) in f.results.iter().enumerate() {
                let t = types.borrow_mut().ty(v);
                let _ = write!(locals, " (local $r{j} {t})");
            }
            for j in (0..f.results.len()).rev() {
                let _ = write!(body, " (local.set $r{j})");
            }
            for (j, v) in lc.spill.iter().enumerate() {
                let vt = spill_name(v).unwrap();
                let _ = write!(body, " (call $push_{vt} (local.get $r{}))", j + 1);
            }
            body.push_str(" (local.get $r0)");
            body = format!("{locals}{body}");
        }
        let _ = writeln!(funcs, "  (func (export {}){esig}{body})", wat_str(key));
        x += 1;
    };
    // `export mono <key>`: a mono variant exported in place of the erased
    // generic member.
    let monos: Vec<&str> = protocol
        .lines()
        .filter_map(|l| l.trim().strip_prefix("export mono "))
        .collect();
    let split = |key: &str| {
        let open = key.find('<').expect("type arguments");
        let close = key.find(">(").expect("type arguments");
        let base = format!("{}{}", &key[..open], &key[close + 1..]);
        let targs = match text::parse_type(&format!("T<{}>", &key[open + 1..close])).unwrap() {
            ts_gen_abi::ir::Type::Named(r) => r.args,
            _ => unreachable!(),
        };
        (base, targs)
    };
    for k in keys.members() {
        if k.role != Role::Export {
            continue;
        }
        let variants: Vec<&str> = monos
            .iter()
            .copied()
            .filter(|m| split(m).0 == k.key)
            .collect();
        if variants.is_empty() {
            let sig = l
                .member(&k, &[])
                .unwrap_or_else(|e| panic!("{}: {e}", k.key));
            export(&k.key, &sig.core(), &mut imports, &mut funcs);
        }
        for v in variants {
            let sig = l
                .member(&k, &split(v).1)
                .unwrap_or_else(|e| panic!("{v}: {e}"));
            export(v, &sig.core(), &mut imports, &mut funcs);
        }
    }
    for line in protocol.lines().map(str::trim) {
        let Some(name) = line.strip_prefix("export ") else {
            continue;
        };
        if name.starts_with("mono ") {
            continue;
        }
        let cx = Context::default();
        let f = if let Some(f) = l.fixed_op(None, name) {
            f
        } else if let Some(t) = name.strip_prefix("tw:call:") {
            match text::parse_type(t).unwrap_or_else(|e| panic!("{t}: {e}")) {
                ts_gen_abi::ir::Type::Fn(f) => l.trampoline(&f, &cx).unwrap().core(),
                _ => panic!("{t}: not a function type"),
            }
        } else {
            l.handle_op().unwrap()
        };
        export(name, &f, &mut imports, &mut funcs);
    }
    for k in keys.members() {
        let Role::Import(module) = &k.role else {
            continue;
        };
        let sig = l
            .member(&k, &[])
            .unwrap_or_else(|e| panic!("{}: {e}", k.key));
        add(
            module,
            &k.key,
            &k.key,
            &sig.core(),
            &mut imports,
            &mut funcs,
        );
        // Every arm variant a `union` parameter derives.
        for (key, tys) in ts_gen_abi::union::variants(&keys, &k) {
            let sig = l
                .member_as(&k, &[], Some(&tys))
                .unwrap_or_else(|e| panic!("{key}: {e}"));
            add(module, &key, &key, &sig.core(), &mut imports, &mut funcs);
        }
    }
    for line in protocol
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("export "))
        .filter(|l| *l != "probes" && *l != "loops")
    {
        let (module, name) = line.split_once(' ').unwrap_or((line, ""));
        let cx = Context::default();
        // `mono <module> <key>`: a mono variant of a generic member.
        if module == "mono" {
            let (module, key) = name.split_once(' ').expect("`mono module key`");
            let open = key.find('<').expect("type arguments");
            let close = key.find(">(").expect("type arguments");
            let base = format!("{}{}", &key[..open], &key[close + 1..]);
            let targs = match text::parse_type(&format!("T<{}>", &key[open + 1..close])).unwrap() {
                ts_gen_abi::ir::Type::Named(r) => r.args,
                _ => unreachable!(),
            };
            let k = keys
                .members()
                .into_iter()
                .find(|k| k.key == base)
                .unwrap_or_else(|| panic!("no member {base}"));
            let f = l.member(&k, &targs).unwrap().core();
            add(module, key, key, &f, &mut imports, &mut funcs);
            continue;
        }
        let ty = |s: &str| text::parse_type(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        let f = match module {
            "tw:box" => l.box_(&ty(name), &cx).unwrap().core(),
            "tw:unbox" | "tw:unboxUnchecked" => l.unbox(&ty(name), &cx).unwrap().core(),
            "tw:isNullish" | "tw:isUndefined" | "tw:unit" | "tw:true" | "tw:false" => {
                l.fixed_op(Some(module), "").unwrap()
            }
            "tw:arr" if name == "len" => FuncType {
                params: vec![ValType::EXTERNREF],
                results: vec![ValType::I32],
            },
            "tw:tryUnbox" => l.try_unbox(&ty(name), &cx).unwrap().core(),
            "tw:new" => {
                let k = l
                    .keys
                    .members()
                    .into_iter()
                    .find(|k| k.key == name && k.role == ts_gen_abi::key::Role::Export)
                    .unwrap_or_else(|| panic!("{name}: no such constructor"));
                l.construct(&k).unwrap().core()
            }
            "tw:arr" => l
                .arr_get(&ty(&name["get<".len()..name.len() - 1]), &cx)
                .unwrap()
                .core(),
            "tw:destroy" => l.handle_op().unwrap(),
            "tw:future" | "tw:loop" => {
                let arg = |p: &str| name.strip_prefix(p).and_then(|n| n.strip_suffix('>'));
                let path = |e: &str| match ty(e) {
                    ts_gen_abi::ir::Type::Named(r) => r.path,
                    _ => panic!("{e}"),
                };
                if let Some(f) = l.fixed_op(Some(module), name) {
                    f
                } else if let Some(t) = arg("take<") {
                    l.future_take(&ty(t), &cx).unwrap().core()
                } else if let Some(t) = arg("resolve<") {
                    l.future_resolve(&ty(t), &cx).unwrap().core()
                } else if let Some(e) = arg("error<") {
                    l.future_error(&path(e), &cx).unwrap().core()
                } else if let Some(e) = arg("reject<") {
                    l.future_reject(&path(e), &cx).unwrap().core()
                } else {
                    panic!("{module} {name}")
                }
            }
            "tw:call" => match ty(name) {
                ts_gen_abi::ir::Type::Fn(f) => l.call_import(&f, &cx).unwrap().core(),
                _ => panic!("{name}: not a function type"),
            },
            "tw:instanceof" => FuncType {
                params: vec![ValType::EXTERNREF],
                results: vec![ValType::I32],
            },
            "tw:tag" => {
                let p = match ty(name) {
                    ts_gen_abi::ir::Type::Named(r) => r.path,
                    _ => panic!("{name}"),
                };
                let params = l.tag(&p, &cx).unwrap();
                let sig = types.borrow_mut().sig(&FuncType {
                    params,
                    results: Vec::new(),
                });
                let t = tags.len();
                tags.push(());
                let _ = writeln!(
                    tag_imports,
                    "  (import \"tw:tag\" {} (tag $tag{t}{sig}))",
                    wat_str(name)
                );
                let _ = writeln!(
                    tag_exports,
                    "  (export {} (tag $tag{t}))",
                    wat_str(&format!("tag:{name}"))
                );
                continue;
            }
            "tw:err" => {
                let (set, e) = match name.strip_prefix("set:") {
                    Some(e) => (true, e),
                    None => (false, name),
                };
                let p = match ty(e) {
                    ts_gen_abi::ir::Type::Named(r) => r.path,
                    _ => panic!("{e}"),
                };
                let wire = l.tag(&p, &cx).unwrap();
                if set {
                    FuncType {
                        params: wire,
                        results: Vec::new(),
                    }
                } else {
                    FuncType {
                        params: vec![ValType::EXTERNREF],
                        results: wire,
                    }
                }
            }
            m => panic!("unknown protocol group {m}"),
        };
        let group = module.trim_start_matches("tw:");
        add(
            module,
            name,
            &format!("{group}:{name}"),
            &f,
            &mut imports,
            &mut funcs,
        );
    }
    let (p, mem) = match profile.memory {
        Some(MemoryWidth::M64) => ("i64", "i64"),
        _ => ("i32", ""),
    };
    let mut runtime = RUNTIME
        .with(|r| r.borrow().clone())
        .unwrap_or_else(|| std::fs::read_to_string(root().join("runtime.wat")).unwrap())
        .replace("{P}", p)
        .replace("{MEM}", mem);
    for name in [
        protocol::MEMORY,
        protocol::TABLE,
        protocol::REALLOC,
        protocol::FREE,
        protocol::TABLE_ALLOC,
        protocol::TABLE_FREE,
        protocol::TABLE_STACK_BASE,
        protocol::TABLE_CONST_BASE,
        protocol::TABLE_HEAP_BASE,
    ] {
        runtime = runtime.replace(
            &format!("(export {})", wat_str(name)),
            &format!("(export {})", wat_str(profile.name(name))),
        );
    }
    if profile.results == Results::Single {
        // The bridge defines the table for its spill.
        runtime = runtime.replace("(table $tab (export \"tw:table\")", "(table $tab");
        for g in [
            protocol::TABLE_STACK_BASE,
            protocol::TABLE_CONST_BASE,
            protocol::TABLE_HEAP_BASE,
        ] {
            runtime = runtime.replace(&format!("(global (export {})", wat_str(g)), "(global");
        }
    }
    if profile.termination_flag {
        // With a memory, the address of a state byte below the allocator's
        // first block; without one, the state itself.
        let g = if profile.memory.is_some() {
            format!("{p} ({p}.const 8)")
        } else {
            "(mut i32) (i32.const 0)".into()
        };
        let _ = writeln!(
            runtime,
            "  (global (export {}) {g})",
            wat_str(profile.name(protocol::TERMINATED))
        );
    }
    // `probes`: GC helpers for the JS test, which can't make or test an
    // `i31ref` or a struct itself.
    if protocol.lines().any(|l| l.trim() == "probes") {
        funcs.push_str(concat!(
            "  (type $probe (struct (field i32)))\n",
            "  (func (export \"test:isI31\") (param externref) (result i32)\n",
            "    (ref.test (ref i31) (any.convert_extern (local.get 0))))\n",
            "  (func (export \"test:i31\") (param i32) (result externref)\n",
            "    (extern.convert_any (ref.i31 (local.get 0))))\n",
            "  (func (export \"test:struct\") (result anyref) (struct.new $probe (i32.const 0)))\n",
        ));
    }
    let decls = types.borrow_mut().decls();
    format!("(module\n{decls}{tag_imports}{imports}{runtime}{funcs}{tag_exports})\n")
}

fn node() -> Option<PathBuf> {
    if let Ok(n) = std::env::var("NODE") {
        return Some(n.into());
    }
    let nvm = dirs_home().join(".nvm/versions/node");
    let mut best: Option<(u32, PathBuf)> = None;
    if let Ok(rd) = std::fs::read_dir(&nvm) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let major: u32 = name
                .trim_start_matches('v')
                .split('.')
                .next()
                .and_then(|m| m.parse().ok())
                .unwrap_or(0);
            let bin = e.path().join("bin/node");
            if major >= 26 && bin.exists() && best.as_ref().is_none_or(|(m, _)| major > *m) {
                best = Some((major, bin));
            }
        }
    }
    if let Some((_, b)) = best {
        return Some(b);
    }
    let out = Command::new("node").arg("--version").output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout);
    let major: u32 = v
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()?;
    (major >= 26).then(|| "node".into())
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

fn snapshot(name: &str, ext: &str, got: &str) {
    let path = root().join("snapshots").join(format!("{name}.{ext}"));
    if std::env::var("BLESS").is_ok() {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    if want != got {
        let diff = similar::TextDiff::from_lines(want.as_str(), got);
        let mut s = String::new();
        for c in diff.iter_all_changes() {
            let sign = match c.tag() {
                similar::ChangeTag::Delete => "-",
                similar::ChangeTag::Insert => "+",
                similar::ChangeTag::Equal => continue,
            };
            let _ = write!(s, "{sign}{c}");
        }
        panic!(
            "snapshot {} differs (BLESS=1 to update):\n{s}",
            path.display()
        );
    }
}

fn fixture(name: &str) {
    let dir = root().join("fixtures").join(name);
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    let ir = text::parse(&read("ir.ts").expect("ir.ts")).unwrap_or_else(|e| panic!("ir.ts: {e}"));
    let profile = match read("profile") {
        Some(p) => Profile::parse(&p).unwrap_or_else(|e| panic!("profile: {e}")),
        None => Profile::all_linear(),
    };
    let libs: Vec<Ir> = read("lib.ts")
        .map(|l| text::parse(&l).unwrap_or_else(|e| panic!("lib.ts: {e}")))
        .into_iter()
        .collect();
    let lang_src = match read("lang.wat") {
        // A hand-written language over the counting runtime.
        Some(w) => w.replace(
            "{RUNTIME}",
            &std::fs::read_to_string(root().join("runtime.wat"))
                .unwrap()
                .replace("{P}", "i32")
                .replace("{MEM}", ""),
        ),
        None => passthrough(&ir, &libs, &profile, &read("protocol").unwrap_or_default()),
    };
    let lang_src = match read("shared") {
        Some(_) => lang_src.replace(
            "(memory (export \"memory\")  1)",
            "(memory (export \"memory\") 1 256 shared)",
        ),
        None => lang_src,
    };
    let lang = wat::parse_str(&lang_src).unwrap_or_else(|e| panic!("lang.wat: {e}\n{lang_src}"));
    let bridge = ts_gen_bridge::generate_with(&ir, &libs, &profile, &lang)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    snapshot(name, "js", &bridge.js);
    snapshot(name, "wat", &bridge.wat);
    if !bridge.warnings.is_empty() {
        snapshot(name, "warnings", &(bridge.warnings.join("\n") + "\n"));
    }

    let Some(node) = node() else {
        if std::env::var("CI").is_ok() {
            panic!("Node 26+ is required under CI");
        }
        eprintln!("skipping the {name} runtime test: no Node 26+");
        return;
    };
    let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("bridge")
        .join(name);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "mjs") {
            std::fs::copy(&p, out.join(e.file_name())).unwrap();
        }
    }
    std::fs::copy(root().join("harness.mjs"), out.join("harness.mjs")).unwrap();
    std::fs::write(out.join("bridge.js"), &bridge.js).unwrap();
    std::fs::write(out.join("bridge.wasm"), &bridge.wasm).unwrap();
    std::fs::write(out.join("lang.wasm"), &lang).unwrap();
    let run = Command::new(&node)
        .arg("--expose-gc")
        .arg("test.mjs")
        .current_dir(&out)
        .output()
        .unwrap();
    if !run.status.success() {
        panic!(
            "{name}: test.mjs failed\n--- stdout\n{}\n--- stderr\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }
}

macro_rules! fixtures {
    ($($name:ident => $dir:literal,)*) => {
        $(
            #[test]
            fn $name() {
                fixture($dir);
            }
        )*
    };
}

fixtures! {
    scalars => "scalars",
    sequences => "sequences",
    enums => "enums",
    classes => "classes",
    errors => "errors",
    eh_modern => "eh-modern",
    eh_legacy => "eh-legacy",
    boxes => "boxes",
    boxes_null => "boxes-null",
    unchecked => "unchecked",
    memory64 => "memory64",
    all_gc => "all-gc",
    gc_wtf16 => "gc-wtf16",
    renames => "renames",
    host_str => "host-str",
    handles => "handles",
    handles_gc => "handles-gc",
    all_host => "all-host",
    mixed_host => "mixed-host",
    libs => "libs",
    async_ => "async",
    async_gc => "async-gc",
    generics => "generics",
    generics_gc => "generics-gc",
    mixed_gc => "mixed-gc",
    linear_wat => "linear-wat",
    handles64 => "handles64",
    single => "single",
    single_gc => "single-gc",
    mono_exports => "mono-exports",
    single_faults => "single-faults",
    linear_wtf16 => "linear-wtf16",
    nested_enums => "nested-enums",
    absent_classes => "absent-classes",
    rest_callbacks => "rest-callbacks",
    never => "never",
    constructor_values => "constructor-values",
    unit => "unit",
    shared_views => "shared-views",
    nullability => "nullability",
    strict_null => "strict-null",
    symbols => "symbols",
    recursive_callbacks => "recursive-callbacks",
    unions => "unions",
}

#[test]
fn termination_flag_needs_its_global() {
    let ir = text::parse("ir 1;\nclass C { f(): u32; }").unwrap();
    let lang = |global: &str| {
        wat::parse_str(format!(
            "(module (import \"tw:js/global\" \"C#f()\" (func (param externref) (result i32))) {global})"
        ))
        .unwrap()
    };
    let p = Profile::parse("termination = on").unwrap();
    let err = |g: &str| match ts_gen_bridge::generate(&ir, &p, &lang(g)) {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    };
    let want = "the termination flag needs an exported immutable `i32` global `tw:terminated`";
    assert!(err("").contains(want), "{}", err(""));
    assert!(err("(global (export \"tw:terminated\") (mut i32) (i32.const 8))").contains(want));
    assert_eq!(
        err("(global (export \"tw:terminated\") i32 (i32.const 8))"),
        ""
    );
    let gc = Profile::parse("all-gc\ntermination = on").unwrap();
    let r = ts_gen_bridge::generate(
        &ir,
        &gc,
        &lang("(global (export \"tw:terminated\") (mut i32) (i32.const 0))"),
    );
    assert!(r.is_ok(), "{:?}", r.err().map(|e| e.to_string()));
}

#[test]
fn table_needs_its_bases() {
    let ir = text::parse("ir 1;\nclass C { f(): u32; }").unwrap();
    let lang = |globals: &str| {
        wat::parse_str(format!(
            "(module (import \"tw:js/global\" \"C#f()\" (func (param externref) (result i32))) (table (export \"tw:table\") 132 externref) {globals})"
        ))
        .unwrap()
    };
    let p = Profile::all_linear();
    let err = |g: &str| match ts_gen_bridge::generate(&ir, &p, &lang(g)) {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    };
    let bases = |heap: &str| {
        format!(
            "(global (export \"tw:table/stack_base\") i32 (i32.const 1)) (global (export \"tw:table/const_base\") i32 (i32.const 128)) {heap}"
        )
    };
    assert!(err("").contains("an exported immutable `i32` global `tw:table/stack_base`"));
    assert!(err(&bases(
        "(global (export \"tw:table/heap_base\") (mut i32) (i32.const 132))"
    ))
    .contains("an exported immutable `i32` global `tw:table/heap_base`"));
    assert_eq!(
        err(&bases(
            "(global (export \"tw:table/heap_base\") i32 (i32.const 132))"
        )),
        ""
    );
}

#[test]
fn recursive_types_are_errors() {
    let ir = text::parse("ir 1;\nclass C { f(): u32; }\nexport namespace u { g(): u32; }").unwrap();
    let err = |module: &str| {
        let lang = wat::parse_str(format!("(module {module})")).unwrap();
        match ts_gen_bridge::generate(&ir, &Profile::all_gc(), &lang) {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    let l = "(type $l (struct (field i32) (field (ref null $l))))";
    let a = "(type $a (array (mut (ref null $a))))";
    let g = "(func (export \"u.g()\") (result i32) i32.const 0)";
    let import = format!(
        "{l} (import \"tw:js/global\" \"C#f()\" (func (param (ref null $l)) (result i32))) {g}"
    );
    assert!(err(&import).contains("type 0 is recursive, which the ABI can't name"));
    let export = format!("{a} (func (export \"u.g()\") (result (ref null $a)) ref.null $a)");
    assert!(err(&export).contains("type 0 is recursive, which the ABI can't name"));
    // The embedder's imports may use any type.
    assert_eq!(
        err(&format!(
            "{l} (import \"env\" \"f\" (func (param (ref null $l)))) {g}"
        )),
        ""
    );
}

#[test]
fn exports_need_their_conversions() {
    let err = |ir: &str, module: &str| {
        let ir = text::parse(&format!(
            "ir 1;\nclass C {{ }}\nexport namespace util {{ {ir} }}"
        ))
        .unwrap();
        let lang = wat::parse_str(format!("(module {module})")).unwrap();
        match ts_gen_bridge::generate(&ir, &Profile::all_linear(), &lang) {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    let memory = "(memory (export \"memory\") 1) (func (export \"tw:realloc\") (param i32 i32 i32 i32) (result i32) i32.const 0) (func (export \"tw:free\") (param i32 i32 i32))";
    let table = "(table (export \"tw:table\") 132 externref) (func (export \"tw:table/alloc\") (result i32) i32.const 0) (func (export \"tw:table/free\") (param i32)) (global (export \"tw:table/stack_base\") i32 (i32.const 1)) (global (export \"tw:table/const_base\") i32 (i32.const 128)) (global (export \"tw:table/heap_base\") i32 (i32.const 132))";
    let len = |m: &str| {
        err(
            "len(s: str): u32;",
            &format!(
                "(func (export \"util.len(str)\") (param i32 i32) (result i32) i32.const 0) {m}"
            ),
        )
    };
    assert!(
        len("").contains("it must export `memory` (Memory)"),
        "{}",
        len("")
    );
    assert_eq!(len(memory), "");
    let count = |m: &str| {
        err(
            "count(xs: arr<C>): u32;",
            &format!("(func (export \"util.count(arr<C>)\") (param i32 i32) (result i32) i32.const 0) {m}"),
        )
    };
    assert!(
        count(memory).contains("it must export `tw:table` (Table)"),
        "{}",
        count(memory)
    );
    assert_eq!(count(&format!("{memory} {table}")), "");
}

#[test]
fn spill_and_js_throws_need_their_settings() {
    let ir = text::parse(
        "ir 1;\nenum E extends err { @js(RangeError) R(Str); }\nclass C { f(): (u32, str); }",
    )
    .unwrap();
    let err = |profile: &str, module: &str| {
        let p = Profile::parse(profile).unwrap();
        let lang = wat::parse_str(format!("(module {module})")).unwrap();
        match ts_gen_bridge::generate(&ir, &p, &lang) {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    let pop = "(import \"tw:ret\" \"i32\" (func (result i32)))";
    assert!(err("all-host", pop).contains("the spill needs `results = single`"));
    assert_eq!(err("all-host\nresults = single", pop), "");
    let read = "(import \"tw:err\" \"E\" (func (param externref) (result i32 externref)))";
    assert!(err("all-host", read).contains("`tw:err` needs `throws = js`"));
    assert_eq!(err("all-host\nthrows = js", read), "");
    let tag = "(import \"tw:tag\" \"E\" (tag (param i32 externref)))";
    assert!(err("all-host\nthrows = js", tag).contains("`throws = js` uses no tags"));
    // Slot 0 only: the rest spill.
    let f = |results: &str| {
        format!("(import \"tw:js/global\" \"C#f()\" (func (param externref) (result {results})))")
    };
    assert_eq!(err("all-host\nresults = single", &f("i32")), "");
    assert!(err("all-host\nresults = single", &f("i32 externref")).contains("C#f()"));
    let table = "(table (export \"tw:table\") 132 externref) (global (export \"tw:table/stack_base\") i32 (i32.const 1)) (global (export \"tw:table/const_base\") i32 (i32.const 128)) (global (export \"tw:table/heap_base\") i32 (i32.const 132))";
    assert!(err("all-host\nresults = single", table).contains("the language must not export one"));
}

#[test]
fn mono_exports_dispatch_on_arguments() {
    let ir = text::parse(
        "ir 1;\nexport namespace u {\n  make<T in (u32, f64)>(): T;\n  id<T in (u32, str)>(x: T): T;\n}",
    )
    .unwrap();
    let err = |exports: &[(&str, &str)]| {
        let fs: Vec<String> = exports
            .iter()
            .map(|(k, ty)| format!("(func (export \"{k}\") {ty} unreachable)"))
            .collect();
        let lang = wat::parse_str(format!("(module {})", fs.join(" "))).unwrap();
        match ts_gen_bridge::generate(&ir, &Profile::all_host(), &lang) {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    let id = ("u.id<u32>(T)", "(param i32) (result i32)");
    let make = |t: &'static str, ty: &'static str| (t, ty);
    assert_eq!(err(&[id, make("u.make<u32>()", "(result i32)")]), "");
    assert!(err(&[
        id,
        make("u.make<u32>()", "(result i32)"),
        make("u.make<f64>()", "(result f64)")
    ])
    .contains("can't be told apart by their arguments"));
    assert!(err(&[id, make("u.make<i64>()", "(result i64)")]).contains("isn't in the bound of `T`"));
}

#[test]
fn mono_variants_respect_concrete_bounds() {
    let ir = text::parse("ir 1;\nclass Bag<T in (u32, str)> { put(v: T); }").unwrap();
    let lang = |key: &str, ty: &str| {
        wat::parse_str(format!(
            "(module (import \"tw:js/global\" \"{key}\" (func (param externref {ty}))))"
        ))
        .unwrap()
    };
    let p = Profile::all_linear();
    let err = |key: &str, ty: &str| match ts_gen_bridge::generate(&ir, &p, &lang(key, ty)) {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    };
    assert_eq!(err("Bag#put<u32>(T)", "i32"), "");
    assert!(err("Bag#put<f64>(T)", "f64").contains("`f64` isn't in the bound of `T`"));
    assert!(err("Bag#take<u32>(T)", "i32").contains("no generic IR member"));
}

/// Benchmarks (`just bench`, `BENCH=<name>` to pick one): each
/// `benches/<name>/` has an `ir.ts`, a `profiles` file of `<name>: <profile>`
/// lines, an optional `protocol` (plus `protocol.<profile name>` additions),
/// and a `bench.mjs` run once per profile. The language is the pass-through
/// module with `loops`, over `benches/runtime.wat` (a bump allocator whose
/// `free` is free, so the allocator isn't measured).
#[test]
#[ignore]
fn bench() {
    let node = node().expect("Node 26+");
    let benches = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches");
    let only = std::env::var("BENCH").ok();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&benches)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("ir.ts").exists())
        .collect();
    dirs.sort();
    let bench_runtime = std::fs::read_to_string(benches.join("runtime.wat")).unwrap();
    let results = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bench/results.jsonl");
    std::fs::create_dir_all(results.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&results);
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
        let ir = text::parse(&read("ir.ts").unwrap()).unwrap_or_else(|e| panic!("ir.ts: {e}"));
        let libs: Vec<Ir> = read("lib.ts")
            .map(|l| text::parse(&l).unwrap_or_else(|e| panic!("lib.ts: {e}")))
            .into_iter()
            .collect();
        for line in read("profiles")
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let (pname, ptext) = line.split_once(':').expect("`name: profile`");
            let pname = pname.trim();
            let profile = Profile::parse(ptext).unwrap_or_else(|e| panic!("{pname}: {e}"));
            let protocol = format!(
                "{}\n{}\nloops\n",
                read("protocol").unwrap_or_default(),
                read(&format!("protocol.{pname}")).unwrap_or_default()
            );
            RUNTIME.with(|r| *r.borrow_mut() = Some(bench_runtime.clone()));
            let lang_src = passthrough(&ir, &libs, &profile, &protocol);
            let lang = wat::parse_str(&lang_src).unwrap_or_else(|e| panic!("{name}/{pname}: {e}"));
            let bridge = ts_gen_bridge::generate_with(&ir, &libs, &profile, &lang)
                .unwrap_or_else(|e| panic!("{name}/{pname}: {e}"));
            let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .join("bench")
                .join(&name)
                .join(pname);
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).unwrap();
            std::fs::copy(dir.join("bench.mjs"), out.join("bench.mjs")).unwrap();
            std::fs::copy(benches.join("lib.mjs"), out.join("lib.mjs")).unwrap();
            std::fs::write(out.join("bridge.js"), &bridge.js).unwrap();
            std::fs::write(out.join("bridge.wasm"), &bridge.wasm).unwrap();
            std::fs::write(out.join("lang.wasm"), &lang).unwrap();
            if let Some(b) = read("baseline.wat") {
                let b = wat::parse_str(&b).unwrap_or_else(|e| panic!("baseline.wat: {e}"));
                std::fs::write(out.join("baseline.wasm"), b).unwrap();
            }
            let run = Command::new(&node)
                .arg("--expose-gc")
                .arg("bench.mjs")
                .env("BENCH_PROFILE", pname)
                .env("BENCH_NAME", &name)
                .env("BENCH_OUT", &results)
                .current_dir(&out)
                .output()
                .unwrap();
            print!("{}", String::from_utf8_lossy(&run.stdout));
            if !run.status.success() {
                panic!(
                    "{name}/{pname}: bench.mjs failed\n{}",
                    String::from_utf8_lossy(&run.stderr)
                );
            }
        }
    }
    println!("results: {}", results.display());
}
