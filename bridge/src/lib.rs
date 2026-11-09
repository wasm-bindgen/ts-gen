//! The `tw` bridge generator (IR.md §1, §19).
//!
//! It reads the IR, a profile and the compiled language module, and writes
//! the bridge: ESM JS glue for the JS ABI, plus a Wasm module that provides
//! the imports whose wire holds GC carriers. Every other import is a glue
//! function the language calls directly.
//!
//! The glue converts between JS values and linear forms only. The bridge
//! module translates GC carriers to and from linear forms in its own
//! scratch memory, so one set of conversions serves every profile
//! ([`form`]).

mod entry;
mod form;
mod js;
pub mod link;
mod module;
mod plan;
mod runtime;
mod wat;

use std::fmt::{self, Write};

use ts_gen_abi::ir::Ir;
use ts_gen_abi::layout::Layouts;
use ts_gen_abi::profile::{MemoryWidth, Profile, Results, Throws};
use ts_gen_abi::protocol;

use form::Forms;
use js::{js_str, prop, Body, Glue};
use plan::Plan;

#[derive(Debug)]
pub enum Error {
    Profile(String),
    Ir(String),
    Module(String),
    Import(String),
    Layout(ts_gen_abi::layout::Error),
    /// Valid, but not implemented by this bridge yet.
    Unsupported(String),
    Assemble(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Profile(m) => write!(f, "profile: {m}"),
            Error::Ir(m) => write!(f, "IR: {m}"),
            Error::Module(m) => write!(f, "language module: {m}"),
            Error::Import(m) => write!(f, "import {m}"),
            Error::Layout(e) => write!(f, "layout: {e}"),
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
            Error::Assemble(m) => write!(f, "bridge module: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ts_gen_abi::layout::Error> for Error {
    fn from(e: ts_gen_abi::layout::Error) -> Error {
        Error::Layout(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn unsupported<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::Unsupported(m.into()))
}

/// A generated bridge.
pub struct Bridge {
    /// The ESM glue. It exports `instantiate(language, bridge)`, taking both
    /// compiled modules and returning the language's instance.
    pub js: String,
    /// The bridge module, as text and binary.
    pub wat: String,
    pub wasm: Vec<u8>,
    /// Approximations, by the import or export they're in.
    pub warnings: Vec<String>,
}

pub fn generate(ir: &Ir, profile: &Profile, language: &[u8]) -> Result<Bridge> {
    generate_with(ir, &[], profile, language)
}

/// [`generate`] for an IR resolving against the lib IRs its `with` names
/// (§5). A lib's declarations are JS values like the IR's own; only the
/// language's imports add glue.
pub fn generate_with(ir: &Ir, libs: &[Ir], profile: &Profile, language: &[u8]) -> Result<Bridge> {
    profile.check().map_err(Error::Profile)?;
    let opts = ts_gen_abi::validate::Options {
        libs,
        ..Default::default()
    };
    ts_gen_abi::validate::validate_with(ir, opts).map_err(|es| {
        Error::Ir(
            es.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    let libs = ts_gen_abi::resolve::libs_for(ir, libs).map_err(|m| Error::Ir(m.join(", ")))?;
    let l = Layouts::with_libs(ir, &libs, profile);
    let f = Forms::new(profile);
    let m = module::Module::read(language)?;
    let plan = Plan::new(&l, &m)?;
    let warnings = warnings(&l, &plan);

    let mut g = Glue::new(&l, &f, ir, libs);
    g.lang_exports = m.export_funcs.iter().map(|(n, _)| n.clone()).collect();
    let mut w = wat::Wat::new(&l, &f);
    let mut funcs = Vec::new();
    let mut imports: Vec<(String, String, String)> = Vec::new();
    let mut glue_imports: Vec<String> = Vec::new();
    for (i, func) in plan.funcs.iter().enumerate() {
        if let plan::Op::RetPop(vt) | plan::Op::RetPush(vt) = &func.op {
            let op = if matches!(func.op, plan::Op::RetPush(_)) {
                "push"
            } else {
                "pop"
            };
            imports.push((
                func.module.clone(),
                func.name.clone(),
                format!("$B.{op}_{vt}"),
            ));
            continue;
        }
        if let plan::Op::Constant(c) = &func.op {
            let name = w.constant(c);
            let value = match *c {
                protocol::TRUE => "true",
                protocol::FALSE => "false",
                _ => "undefined",
            };
            let glue = format!("{name}: {value}");
            if !glue_imports.contains(&glue) {
                glue_imports.push(glue);
            }
            imports.push((func.module.clone(), func.name.clone(), format!("$B.{name}")));
            continue;
        }
        spill_fits(&func.core.spill, &|| js_comment(&func.module, &func.name))?;
        let name = format!("$f{i}");
        let gc = func
            .sig
            .params
            .iter()
            .chain(&func.sig.results)
            .any(|p| f.has_gc(&p.repr));
        let rs: Vec<String> = func.core.ty.results.iter().map(|v| w.ty_name(v)).collect();
        let area = if gc {
            w.glue_area(func)?
        } else if profile.results == Results::Multi {
            w.ret_area(&rs)
        } else {
            None
        };
        g.ret = area.clone();
        g.ret_used = false;
        let src = g.func(&name, func);
        g.ret = None;
        funcs.push(format!(
            "// {}\n{}",
            js_comment(&func.module, &func.name),
            src?
        ));
        let glue = match &area {
            Some(a) if !g.ret_used => {
                let n = func
                    .sig
                    .params
                    .iter()
                    .map(|p| f.jsw(&p.repr).map(|j| j.len()))
                    .sum::<Result<usize>>()?;
                funcs.push(ret_js(&name, n, a));
                format!("{name}$r")
            }
            _ => name.clone(),
        };
        let value = if gc {
            let export = format!("f{i}");
            w.forwarder(&export, &format!("g{i}"), func, area.as_ref())?;
            glue_imports.push(format!("g{i}: {glue}"));
            format!("$B.{export}")
        } else if let Some(a) = &area {
            let export = format!("f{i}");
            w.ret_shim(&export, &format!("r{i}"), &func.core.ty, a);
            glue_imports.push(format!("r{i}: {glue}"));
            format!("$B.{export}")
        } else if !func.core.spill.is_empty() {
            // `results = single` (§12): slot 0 returns, the rest spill.
            let n = func.core.ty.params.len();
            let ps: Vec<String> = (0..n).map(|j| format!("a{j}")).collect();
            let mut body = format!(
                "function {name}$s({}) {{\n  $spillCheck();\n  const r = {name}({});\n",
                ps.join(", "),
                ps.join(", ")
            );
            for (j, v) in func.core.spill.iter().enumerate() {
                let vt = ts_gen_abi::layout::spill_name(v).expect("a spill type");
                let _ = writeln!(body, "  $B.push_{vt}(r[{}]);", j + 1);
            }
            body.push_str("  return r[0];\n}");
            funcs.push(body);
            format!("{name}$s")
        } else {
            name.clone()
        };
        imports.push((func.module.clone(), func.name.clone(), value));
    }
    for t in &plan.tags {
        let v = g.tag_var(&t.layout);
        imports.push((t.module.clone(), t.name.clone(), v));
    }
    let exports = g.exports()?;
    let outgoing = g.outgoing()?;
    for (i, r) in g.convs.iter().enumerate() {
        w.conv(i, r)?;
    }
    // Entries with GC positions go through the bridge, which translates
    // them out of and into scratch.
    let mut entries = Vec::new();
    for (key, sig) in &g.entries {
        let gc = sig
            .params
            .iter()
            .chain(&sig.results)
            .any(|p| f.has_gc(&p.repr));
        let slot = if gc { Some(w.entry(sig)?) } else { None };
        let spill = l.lang_core(&sig.core())?.spill;
        spill_fits(&spill, &|| format!("the export `{key}`"))?;
        let pending = profile.throws == Throws::Js && sig.throws.is_some();
        // A GC entry drains the spill in the bridge module.
        let takes = if gc { Vec::new() } else { spill };
        entries.push(Entry {
            key: key.clone(),
            slot,
            takes,
            pending,
        });
    }
    // The language exports driving futures and loops (§16).
    let polls = plan.funcs.iter().any(|f| matches!(f.op, plan::Op::Poll));
    let mut driving = Vec::new();
    if polls {
        driving.push(protocol::FUTURE_WAKE);
    }
    if g.loops {
        driving.extend([protocol::LOOP_TICK, protocol::LOOP_NEXT_SLEEP]);
    }
    let driving: Vec<(String, ts_gen_abi::layout::FuncType)> = driving
        .into_iter()
        .map(|d| {
            let ft = l.fixed_op(None, d).expect("a fixed export");
            (profile.name(d).to_string(), ft)
        })
        .collect();
    check_entries(&l, &g, &driving, &m)?;
    if profile.results == Results::Single {
        glue_imports.push("spill_fault: $spillFault".into());
    }
    check_exports(&l, &f, &plan, &g, &m)?;

    w.allocn = g.allocn;
    let js_string = w.js_string;
    let wat_src = w.finish();
    let debug = |e: String| {
        if std::env::var_os("TW_DEBUG_WAT").is_some() {
            eprintln!("{wat_src}");
        }
        Error::Assemble(e)
    };
    let wasm = ::wat::parse_str(&wat_src).map_err(|e| debug(format!("{e:?}")))?;
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&wasm)
        .map_err(|e| debug(e.to_string()))?;

    let mut js = String::new();
    js.push_str("// Generated by ts-gen-bridge.\n");
    for (i, spec) in g.modules.iter().enumerate() {
        let _ = writeln!(js, "import * as $m{i} from {};", js_str(spec));
    }
    js.push('\n');
    let runtime_at = js.len();
    let _ = writeln!(
        js,
        "let $L = null;\nlet $S = null;\nlet $B = null;\nlet $X = null;\nlet $T = null;\nconst $E = [];"
    );
    if profile.throws == Throws::Js {
        let _ = writeln!(js, "let $pending = null;");
    }
    // Every entry point calls $checkLive (§15); entries arrive with exports.
    if profile.termination_flag {
        let _ = writeln!(js, "let $checkLive = () => {{}};");
    }
    // Finalizers skip the drop once the language has terminated (§15).
    let live = if profile.termination_flag {
        "  try {\n    $checkLive();\n  } catch {\n    return;\n  }\n"
    } else {
        ""
    };
    if polls || g.loops {
        let live = if profile.termination_flag {
            "try {\n    $checkLive();\n    return true;\n  } catch {\n    return false;\n  }"
        } else {
            "return true;"
        };
        let _ = writeln!(js, "function $live() {{\n  {live}\n}}");
    }
    if polls {
        let wake = prop("$X", profile.name(protocol::FUTURE_WAKE));
        let _ = writeln!(js, "function $wake(w) {{\n  if ($live()) {wake}(w);\n}}");
    }
    if g.loops {
        let _ = writeln!(
            js,
            "const $loops = new $Loops((l, n) => {}(l, n), (l) => {}(l), $live);",
            prop("$X", profile.name(protocol::LOOP_TICK)),
            prop("$X", profile.name(protocol::LOOP_NEXT_SLEEP)),
        );
    }
    if !g.drops.is_empty() {
        let _ = writeln!(js, "function $drop(name, h) {{\n{live}  $X[name](h);\n}}");
    }
    js.push('\n');
    for h in &g.handles {
        let _ = writeln!(js, "{h}");
    }
    for i in 0..g.classes.len() {
        let _ = writeln!(js, "let $cls{i} = null;");
    }
    for (path, v, _) in &g.tags {
        let _ = writeln!(
            js,
            "// tag:{path}\nconst {v} = new WebAssembly.Tag({{ parameters: [\"i32\", \"externref\"] }});"
        );
    }
    for h in &g.helpers {
        let _ = writeln!(js, "\n{h}");
    }
    for f in &funcs {
        let _ = writeln!(js, "\n{f}");
    }
    if !g.entries.is_empty() {
        let _ = writeln!(js, "\n{outgoing}");
    }
    js.push_str(&exports);
    js.push('\n');
    let lang_table = m.exports(
        profile.name(protocol::TABLE),
        wasmparser::ExternalKind::Table,
    );
    if profile.results == Results::Single && lang_table {
        return Err(Error::Module(format!(
            "under `results = single` the bridge defines `{}` for its spill, so the language must not export one",
            profile.name(protocol::TABLE)
        )));
    }
    js.push_str(&instantiate(
        profile,
        if lang_table {
            Some("$X")
        } else if profile.results == Results::Single {
            Some("$B")
        } else {
            None
        },
        &imports,
        &glue_imports,
        &entries,
        &g.classes,
        js_string,
    ));
    let runtime = runtime::used(&js[runtime_at..]);
    js.insert_str(runtime_at, &format!("{runtime}\n"));
    Ok(Bridge {
        js,
        wat: wat_src,
        wasm,
        warnings,
    })
}

/// The layout approximations of the members the language imports and
/// exports.
fn warnings(l: &Layouts<'_>, plan: &Plan<'_>) -> Vec<String> {
    let mut out = Vec::new();
    l.take_warnings();
    let mut note = |at: String| {
        for w in l.take_warnings() {
            out.push(format!("{at}: {w}"));
        }
    };
    for func in &plan.funcs {
        if let plan::Op::Member(k) = &func.op {
            let _ = l.member(k, &[]);
            note(js_comment(&func.module, &func.name));
        }
    }
    for k in l.keys.members() {
        if k.role == ts_gen_abi::key::Role::Export {
            let _ = l.member(&k, &[]);
            note(format!("the export `{}`", k.key));
        }
    }
    out
}

fn js_comment(module: &str, name: &str) -> String {
    format!("(\"{module}\", \"{name}\")").replace('\n', " ")
}

/// The language must export what the conversions use (§11, §19).
fn check_exports(
    l: &Layouts<'_>,
    f: &Forms,
    plan: &Plan<'_>,
    g: &Glue<'_, '_>,
    m: &module::Module,
) -> Result<()> {
    use wasmparser::ExternalKind as K;
    let p = l.profile;
    let mut memory = false;
    let mut table = false;
    let sigs = plan.funcs.iter().map(|func| &func.sig);
    for sig in sigs.chain(g.entries.iter().map(|(_, sig)| sig)) {
        for pos in sig.params.iter().chain(&sig.results) {
            uses(f, &pos.repr, &mut memory, &mut table)?;
        }
    }
    let mut need = Vec::new();
    if memory {
        need.push((p.name(protocol::MEMORY), K::Memory));
        need.push((p.name(protocol::REALLOC), K::Func));
        need.push((p.name(protocol::FREE), K::Func));
    }
    if table {
        need.push((p.name(protocol::TABLE), K::Table));
        need.push((p.name(protocol::TABLE_ALLOC), K::Func));
        need.push((p.name(protocol::TABLE_FREE), K::Func));
    }
    for (name, kind) in need {
        if !m.exports(name, kind) {
            return Err(Error::Module(format!("it must export `{name}` ({kind:?})")));
        }
    }
    if m.exports(p.name(protocol::TABLE), K::Table) {
        for g in table_bases(p) {
            if m.global(g) != Some((wasmparser::ValType::I32, false)) {
                return Err(Error::Module(format!(
                    "the table needs an exported immutable `i32` global `{g}`"
                )));
            }
        }
    }
    if p.termination_flag {
        // §15: the address of a state byte with a memory, else the state.
        let name = p.name(protocol::TERMINATED);
        let want = match p.memory {
            Some(MemoryWidth::M64) => (wasmparser::ValType::I64, false),
            Some(MemoryWidth::M32) => (wasmparser::ValType::I32, false),
            None => (wasmparser::ValType::I32, true),
        };
        if m.global(name) != Some(want) {
            let (ty, mutable) = want;
            let mutable = if mutable { "mutable" } else { "immutable" };
            return Err(Error::Module(format!(
                "the termination flag needs an exported {mutable} `{ty}` global `{name}`"
            )));
        }
    }
    Ok(())
}

fn table_bases(p: &Profile) -> [&str; 3] {
    [
        p.name(protocol::TABLE_STACK_BASE),
        p.name(protocol::TABLE_CONST_BASE),
        p.name(protocol::TABLE_HEAP_BASE),
    ]
}

fn uses(
    f: &Forms,
    r: &ts_gen_abi::layout::Repr,
    memory: &mut bool,
    table: &mut bool,
) -> Result<()> {
    use ts_gen_abi::layout::Kind;
    use ts_gen_abi::profile::{Carrier, StrCarrier};
    match &r.kind {
        Kind::Str {
            carrier: StrCarrier::Linear(_),
            ..
        }
        | Kind::Vec {
            carrier: Carrier::Linear,
            ..
        }
        | Kind::View { .. } => *memory = true,
        Kind::Array { elem, carrier, .. } => {
            if *carrier == Carrier::Linear {
                let e = f.elem(elem, form::Space::Lang)?;
                *memory |= e.size > 0;
                *table |= e.refs > 0;
            }
            if *carrier != Carrier::Host {
                uses(f, elem, memory, table)?;
            }
        }
        Kind::Tuple { elems, .. } => {
            for e in elems {
                uses(f, e, memory, table)?;
            }
        }
        Kind::Enum { layout, .. } => {
            for a in &layout.arms {
                for p in &a.payload {
                    uses(f, p, memory, table)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// The language must export each entry and drop with the layout's type.
fn check_entries(
    l: &Layouts<'_>,
    g: &Glue<'_, '_>,
    driving: &[(String, ts_gen_abi::layout::FuncType)],
    m: &module::Module,
) -> Result<()> {
    let mut want: Vec<(&str, ts_gen_abi::layout::FuncType)> = Vec::new();
    for (name, ft) in driving {
        want.push((name, ft.clone()));
    }
    for (key, sig) in &g.entries {
        want.push((key, l.lang_core(&sig.core())?.ty));
    }
    for d in &g.drops {
        want.push((d, l.handle_op()?));
    }
    for (name, ft) in want {
        let Some(got) = m.export_func(name) else {
            return Err(Error::Module(format!("it must export `{name}`")));
        };
        let got = got?;
        let what = format!("the export `{name}`");
        plan::check(&what, &got.params, &ft.params)?;
        plan::check(&what, &got.results, &ft.results)?;
    }
    Ok(())
}

/// A language export the glue calls through `$E[i]`.
struct Entry {
    key: String,
    /// The bridge module's entry slot, for GC positions.
    slot: Option<usize>,
    /// The spilled result slots JS drains after the call (§12).
    takes: Vec<ts_gen_abi::layout::ValType>,
    /// Whether a pending `throws = js` error replaces the results (§15).
    pending: bool,
}

/// The return area's wrapper for a glue function returning an array.
fn ret_js(name: &str, n: usize, a: &wat::RetArea) -> String {
    let ps: Vec<String> = (0..n).map(|j| format!("a{j}")).collect();
    let mut body = format!(
        "function {name}$r({}) {{\n  const r = {name}({});\n  const ra = $ret();\n",
        ps.join(", "),
        ps.join(", ")
    );
    for (j, vt, off) in &a.slots {
        let at = if vt.ends_with("32") { off / 4 } else { off / 8 };
        let _ = writeln!(body, "  ra.{vt}[{at}] = r[{j}];");
    }
    let _ = write!(body, "  return r[{}];\n}}", a.direct);
    body
}

/// The spill's capacity: the table range below `const_base`, and the
/// numeric scratch slots.
fn spill_fits(spill: &[ts_gen_abi::layout::ValType], what: &dyn Fn() -> String) -> Result<()> {
    let refs = spill.iter().filter(|v| v.is_ref()).count() as u32;
    let nums = spill.len() as u32 - refs;
    let room = protocol::DEFAULT_CONST_BASE - protocol::DEFAULT_STACK_BASE;
    if refs > room || nums > wat::SPILL_NUMBERS {
        return Err(Error::Unsupported(format!(
            "{}: {refs} references and {nums} numbers overflow the spill",
            what()
        )));
    }
    Ok(())
}

/// `table` is the instance defining `tw:table`, when one does.
fn instantiate(
    p: &Profile,
    table: Option<&str>,
    imports: &[(String, String, String)],
    glue: &[String],
    entries: &[Entry],
    classes: &[String],
    js_string: bool,
) -> String {
    let mut b = Body::default();
    let names = format!(
        "{{ memory: {}, table: {}, realloc: {}, free: {}, talloc: {}, tfree: {} }}",
        js_str(p.name(protocol::MEMORY)),
        js_str(p.name(protocol::TABLE)),
        js_str(p.name(protocol::REALLOC)),
        js_str(p.name(protocol::FREE)),
        js_str(p.name(protocol::TABLE_ALLOC)),
        js_str(p.name(protocol::TABLE_FREE)),
    );
    let _ = &mut b;
    let mut s = String::new();
    let mut modules: Vec<&str> = Vec::new();
    for (m, _, _) in imports {
        if !modules.contains(&m.as_str()) {
            modules.push(m);
        }
    }
    let _ = writeln!(
        s,
        "export function instantiate(language, bridge, imports = {{}}) {{"
    );
    let _ = writeln!(
        s,
        "  if ($B) throw new Error(\"this bridge is already instantiated\");"
    );
    for (i, c) in classes.iter().enumerate() {
        let _ = writeln!(s, "  $cls{i} = $class({c});");
    }
    let js_string_imports = if js_string {
        "\n    \"wasm:js-string\": {\n      length: (s) => s.length,\n      fromCharCodeArray: (a, start, end) => {\n        let out = \"\";\n        for (let i = start; i < end; i++) out += String.fromCharCode($B.a16get(a, i));\n        return out;\n      },\n      intoCharCodeArray: (s, a, start) => {\n        for (let i = 0; i < s.length; i++) $B.a16set(a, start + i, s.charCodeAt(i));\n        return s.length;\n      },\n    },"
    } else {
        ""
    };
    let _ = writeln!(
        s,
        "  $B = new WebAssembly.Instance(bridge, {{\n    \"{}\": {{ {} }},{js_string_imports}\n  }}).exports;",
        wat::GLUE,
        glue.join(", ")
    );
    let _ = writeln!(s, "  $S = new $Scratch($B);");
    let _ = writeln!(
        s,
        "  const instance = new WebAssembly.Instance(language, {{\n    ...imports,"
    );
    for m in modules {
        let _ = writeln!(s, "    {}: {{", js_str(m));
        for (mm, n, v) in imports {
            if mm == m {
                let _ = writeln!(s, "      {}: {v},", js_str(n));
            }
        }
        let _ = writeln!(s, "    }},");
    }
    let _ = writeln!(s, "  }});");
    let _ = writeln!(s, "  $X = instance.exports;");
    if let Some(x) = table {
        let [stack, constant, heap] = table_bases(p).map(|g| prop(x, g));
        let _ = writeln!(
            s,
            "  $T = $table({}, {stack}.value, {constant}.value, {heap}.value);",
            prop(x, p.name(protocol::TABLE))
        );
    }
    for (i, e) in entries.iter().enumerate() {
        let f = prop("$X", &e.key);
        let f = match e.slot {
            Some(n) => {
                let _ = writeln!(s, "  $B.entries.set({n}, {f});");
                format!("$B.e{n}")
            }
            None => f,
        };
        let single = p.results == Results::Single && (e.slot.is_some() || !e.takes.is_empty());
        if single || e.pending {
            let takes: Vec<String> = e
                .takes
                .iter()
                .map(|v| {
                    let vt = ts_gen_abi::layout::spill_name(v).expect("a spill type");
                    format!("$B.pop_{vt}")
                })
                .collect();
            let _ = writeln!(
                s,
                "  $E[{i}] = $spilled({f}, [{}], {}, {});",
                takes.join(", "),
                p.results == Results::Single,
                e.pending
            );
        } else {
            let _ = writeln!(s, "  $E[{i}] = {f};");
        }
    }
    if p.memory.is_some() {
        let wide = p.memory == Some(MemoryWidth::M64);
        let _ = writeln!(s, "  $L = new $Lang(instance.exports, {names}, {wide});");
    }
    if p.termination_flag {
        let g = prop("instance.exports", p.name(protocol::TERMINATED));
        let state = if p.memory.is_some() {
            format!("$L.u8()[Number({g}.value)]")
        } else {
            format!("{g}.value")
        };
        let _ = writeln!(s, "  $checkLive = () => {{");
        let _ = writeln!(
            s,
            "    if ({state} !== {}) throw new Error(\"Module terminated\");",
            protocol::LIVE
        );
        let _ = writeln!(s, "  }};");
    }
    let _ = writeln!(s, "  return instance;");
    s.push_str("}\n\n");
    // Diagnostics: the scratch bump pointers, which every call restores.
    s.push_str("export function scratchUse() {\n  return $S ? $S.mark() : null;\n}\n");
    if p.results == Results::Single {
        s.push_str(
            "export function spillEmpty() {\n  return $B ? $B.spill_empty() === 1 : null;\n}\n",
        );
    }
    s
}
