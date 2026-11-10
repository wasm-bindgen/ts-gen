//! Zena bindings for the ts-gen IR (IR.md), compiled with Zena's `-t js`
//! against a `tw` bridge in the GC profile with host strings ([`profile`]).
//!
//! Every imported IR member is one `@external(module, key, "tw")` declare
//! function whose Zena types lower exactly to the member's wire layout from
//! [`ts_gen_abi::layout`]. The `tw` option makes a `String` cross as its JS
//! string handle and a GC reference as itself, so most positions need no
//! conversion; the rest (nullable scalars, payload enums, `arr<str>`, tuples)
//! are converted by the member's wrapper.
//!
//! Host classes and interfaces are extension classes on `externref`. They
//! are erased, so a host value is its handle wherever it goes, including a
//! generic external instantiated at it. Members of a generic class and
//! generic members are imported per instantiation through a templated name
//! (`Map#get<{K},{V}>(K)`), which the Zena compiler renders with the IR
//! spelling of each type argument: the bindings are fully monomorphized.
//! A callback over type parameters is called through a generic trampoline
//! whose `@export` name is templated the same way (`tw:call:fn({T})->{U}`),
//! so it exports at each instantiation it is passed at.
//!
//! What has no Zena form yet is left out with a `// skipped` line and
//! listed in [`Output::skipped`]: type parameters anywhere but whole
//! positions, host type arguments and generic callbacks' whole positions.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write;

use ts_gen_abi::ir::*;
use ts_gen_abi::key::{location, Keyed, Role};
use ts_gen_abi::layout::{
    EnumLayout, Flow, FnLayout, Host, Kind, Layouts, Origin, Pos, Repr, ScalarNull, Shape, Storage,
    ValType,
};
use ts_gen_abi::naming::Slot;
use ts_gen_abi::profile::{Profile, StrCarrier};
use ts_gen_abi::resolve::{Context, Location, TypeDecl};

/// The profile the bindings are generated for: every category GC, with
/// strings on the host carrier, which is what a Zena `String` is on `-t js`,
/// and a box's none `ref.null`, Zena's `null`.
pub fn profile() -> Profile {
    Profile {
        str: StrCarrier::Host,
        ..Profile::all_gc()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Export a `__probe_<n>` function calling each non-generic import, so
    /// a compiled module keeps every import for inspection.
    pub probe: bool,
    /// Write an entry module ([`Output::entry`]) exporting the IR's exports
    /// under their keys, each calling the app module's implementation.
    pub entry: Option<Entry>,
}

/// The modules an entry module imports, as Zena import specifiers.
#[derive(Clone, Debug)]
pub struct Entry {
    pub bindings: String,
    pub app: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub key: String,
    pub reason: String,
}

pub struct Output {
    pub zena: String,
    /// With [`Options::entry`]: the module to compile, whose exports are
    /// the IR's exports.
    pub entry: Option<String>,
    pub skipped: Vec<Skipped>,
}

/// The tw loop over Zena's async runtime (IR.md §16), for bindings with
/// futures.
const ASYNC_RUNTIME: &str = r#"import { Future, Completer, drainMicrotasks } from 'zena:async';

@external("tw:future", "poll", "tw")
declare function __tw_future_poll(future: externref, waker: i32): i32;

@external("tw:future", "take<{T}>", "tw")
declare function __tw_future_take<T>(future: externref): T;

@external("tw:future", "error<err>", "tw")
declare function __tw_future_error(future: externref): inline (i32, String);

@external("tw:future", "drop", "tw")
declare function __tw_future_drop(future: externref): void;

@external("tw:future", "new", "tw")
declare function __tw_future_new(): inline (externref, externref);

@external("tw:future", "resolve<{T}>", "tw")
declare function __tw_future_resolve<T>(resolver: externref, value: T): void;

@external("tw:future", "reject<err>", "tw")
declare function __tw_future_reject(resolver: externref, arm: i32, message: String): void;

@external("tw:loop", "work_available", "tw")
declare function __tw_work_available(loop: i32): void;

// The bindings run one tw loop, Zena's microtask queue: a JS future
// settling wakes its awaiter, whose completion asks the bridge for a
// tick, which drains the queue.
interface __TwPoll {
  poll(): void;
}

let __tw_polls = new HashMap<i32, __TwPoll>();
var __tw_next_waker = 1;

class __TwAwait<T> implements __TwPoll {
  future: externref;
  waker: i32;
  completer: Completer<T>;
  fail: (future: externref) => Error;

  new(future: externref, waker: i32, fail: (future: externref) => Error)
      : future = future, waker = waker, completer = new Completer<T>(), fail = fail {}

  poll(): void {
    let state = __tw_future_poll(this.future, this.waker);
    if (state == 0) {
      __tw_polls[this.waker] = this;
      return;
    }
    if (state == 1) {
      this.completer.complete(__tw_future_take<T>(this.future));
    } else {
      let fail = this.fail;
      this.completer.fail(fail(this.future));
    }
    __tw_work_available(1);
  }
}

/** A JS future as a Zena future, `fail` mapping its rejection to the
 * error it fails with. Cancelling its awaiter drops the JS future, which
 * cancels it. */
export async function __tw_await_with<T>(
    future: externref, fail: (future: externref) => Error): Future<T> {
  let waker = __tw_next_waker;
  __tw_next_waker += 1;
  let a = new __TwAwait<T>(future, waker, fail);
  a.poll();
  try {
    return await a.completer.future;
  } cancel {
    __tw_polls.delete(waker);
    __tw_future_drop(future);
  }
}

function __tw_err_fail(future: externref): Error {
  let (arm, message) = __tw_future_error(future);
  return new ErrException(arm as ErrArm, message);
}

export function __tw_await<T>(future: externref): Future<T> {
  return __tw_await_with<T>(future, __tw_err_fail);
}

async function __tw_settle<T>(
    resolver: externref, future: Future<T>,
    reject: (resolver: externref, e: Error) => void): Future<void> {
  try {
    __tw_future_resolve<T>(resolver, await future);
  } catch (e) {
    reject(resolver, e);
  }
}

function __tw_err_reject(resolver: externref, e: Error): void {
  if (e is ErrException) {
    __tw_future_reject(resolver, e.arm as i32, e.message);
  } else {
    __tw_future_reject(resolver, -1, e.message);
  }
}

/** A Zena future as a JS future, `reject` rejecting it with the error
 * the Zena future fails with. */
export function __tw_promise_with<T>(
    future: Future<T>, reject: (resolver: externref, e: Error) => void): externref {
  let (promise, resolver) = __tw_future_new();
  __tw_settle<T>(resolver, future, reject);
  __tw_work_available(1);
  return promise;
}

export function __tw_promise<T>(future: Future<T>): externref {
  return __tw_promise_with<T>(future, __tw_err_reject);
}

@export("tw:future/wake")
export function __tw_wake(waker: i32): void {
  if (let (true, p) = __tw_polls.get(waker)) {
    __tw_polls.delete(waker);
    p.poll();
  }
}

@export("tw:loop/tick")
export function __tw_tick(loop: i32, max: i32): i32 {
  drainMicrotasks();
  return 0;
}

@export("tw:loop/next_sleep")
export function __tw_next_sleep(loop: i32): i64 {
  return (-9223372036854775808 as i64);
}
"#;

/// Names the generated code relies on, which IR declarations can't take.
const RESERVED: &[&str] = &[
    "String",
    "FixedArray",
    "Option",
    "Some",
    "None",
    "some",
    "none",
    "Error",
    "Future",
    "Completer",
    "drainMicrotasks",
    "ErrException",
    "ErrArm",
];

/// Zena's keywords (docs/language-reference.md).
const KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "declare",
    "decorator",
    "distinct",
    "else",
    "enum",
    "export",
    "extends",
    "extension",
    "false",
    "final",
    "finally",
    "for",
    "from",
    "function",
    "gen",
    "if",
    "implements",
    "import",
    "in",
    "inline",
    "interface",
    "is",
    "let",
    "match",
    "mixin",
    "new",
    "null",
    "on",
    "opaque",
    "operator",
    "ref",
    "return",
    "sealed",
    "static",
    "super",
    "symbol",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "using",
    "var",
    "while",
    "with",
    "yield",
];

/// A lib IR the generated IR resolves against (`with`, §5), and the Zena
/// module its own bindings are, which the generated bindings import its
/// declarations from.
pub struct Lib<'a> {
    pub ir: &'a Ir,
    pub module: String,
}

pub fn generate(ir: &Ir, opts: &Options) -> anyhow::Result<Output> {
    generate_with(ir, &[], opts)
}

/// [`generate`] for an IR with libs, in its `with` order.
pub fn generate_with(ir: &Ir, libs: &[Lib<'_>], opts: &Options) -> anyhow::Result<Output> {
    let profile = profile();
    let irs: Vec<&Ir> = libs.iter().map(|l| l.ir).collect();
    let layouts = Layouts::with_libs(ir, &irs, &profile);
    let mut g = Gen::new(&layouts, opts);
    g.collect(&ir.items, None, &mut Vec::new());
    g.assign_names();
    g.assign_specs();
    // A lib's declarations take the names its own bindings give them.
    for (i, lib) in libs.iter().enumerate() {
        let ll = Layouts::with_libs(lib.ir, &irs[i + 1..], &profile);
        let mut lg = Gen::new(&ll, opts);
        lg.collect(&lib.ir.items, None, &mut Vec::new());
        lg.assign_names();
        lg.assign_specs();
        let mut specs: Vec<(String, Vec<Spec>)> = lg.specs.into_iter().collect();
        specs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut names: Vec<(String, String)> = lg.names.into_iter().collect();
        names.sort();
        for (canonical, name) in names {
            if !g.names.contains_key(&canonical) {
                let local = g.lib_name(&lib.module, &name);
                g.names.insert(canonical, local);
            }
        }
        for (canonical, rows) in specs {
            if g.specs.contains_key(&canonical) {
                continue;
            }
            let rows = rows
                .into_iter()
                .map(|s| Spec {
                    name: g.lib_name(&lib.module, &s.name),
                    arg: s.arg,
                })
                .collect();
            g.specs.insert(canonical, rows);
        }
    }
    let members = layouts.keys.members();
    g.enums()?;
    g.classes(&members)?;
    g.functions(&members)?;
    Ok(g.finish())
}

impl<'a> Gen<'a> {
    fn new(layouts: &'a Layouts<'a>, opts: &'a Options) -> Gen<'a> {
        Gen {
            layouts,
            opts,
            names: HashMap::new(),
            used: RESERVED.iter().map(|s| s.to_string()).collect(),
            decls: Vec::new(),
            externs: String::new(),
            types: String::new(),
            funcs: String::new(),
            helpers: String::new(),
            probes: String::new(),
            helper_done: HashSet::new(),
            parents: HashSet::new(),
            specs: HashMap::new(),
            skipped: Vec::new(),
            raw_count: 0,
            probe_count: 0,
            tmp: 0,
            uses_promise: false,
            skipped_enums: HashSet::new(),
            in_export: false,
            exports: String::new(),
            export_uses: BTreeSet::new(),
            app_uses: BTreeSet::new(),
            generic_trampolines: HashMap::new(),
            lib_names: HashMap::new(),
            lib_used: RefCell::new(BTreeSet::new()),
            arm: None,
        }
    }

    /// The local name of a lib's declaration `name`, aliased if one of
    /// ours has it.
    fn lib_name(&mut self, module: &str, name: &str) -> String {
        let local = unique(&mut self.used, name.to_string());
        self.lib_names
            .insert(local.clone(), (module.to_string(), name.to_string()));
        local
    }
}

/// A class, interface or enum, with where it's declared.
struct DeclRef<'a> {
    loc: Location,
    decl: TypeDecl<'a>,
}

struct Gen<'a> {
    layouts: &'a Layouts<'a>,
    opts: &'a Options,
    /// Zena names by canonical location (`key::location`).
    names: HashMap<String, String>,
    used: HashSet<String>,
    decls: Vec<DeclRef<'a>>,
    externs: String,
    types: String,
    funcs: String,
    helpers: String,
    probes: String,
    helper_done: HashSet<String>,
    /// Classes another extends, which take a `__wrap` constructor.
    parents: HashSet<String>,
    /// Per class over a type parameter bounded `in (...)`, by canonical
    /// location, a class per element of the bound. Associated types vary
    /// with the argument, which a generic Zena class can't say, and the
    /// elements are mostly enums, over which a generic Zena class can't
    /// import per instantiation.
    specs: HashMap<String, Vec<Spec>>,
    skipped: Vec<Skipped>,
    raw_count: usize,
    probe_count: usize,
    tmp: usize,
    uses_promise: bool,
    /// Generating an entry module export, where the app's classes are in
    /// scope.
    in_export: bool,
    /// Enums left out, by canonical location: what names one is left out too.
    skipped_enums: HashSet<String>,
    /// The entry module's export wrappers, and the names it imports from
    /// the bindings and the app.
    exports: String,
    export_uses: BTreeSet<String>,
    app_uses: BTreeSet<String>,
    /// Generic callbacks' trampolines, by templated key.
    generic_trampolines: HashMap<String, GenericTrampoline>,
    /// Lib declarations by local name: their module and name there.
    lib_names: HashMap<String, (String, String)>,
    /// The lib declarations named, which the bindings import.
    lib_used: RefCell<BTreeSet<String>>,
    /// The arm variant (IR.md §7) being emitted: its key and parameter
    /// types.
    arm: Option<Arm>,
}

type Arm = (String, Vec<Type>);

/// A generic callback's trampoline: the function's Zena type, and the
/// statement that instantiates the trampoline where it is passed.
#[derive(Clone)]
struct GenericTrampoline {
    surface: String,
    keep: String,
}

/// A class instantiated at one element of its type parameter's bound.
struct Spec {
    name: String,
    /// The argument as keys spell it.
    arg: String,
}

/// Why a member has no binding.
struct Skip(String);

type R<T> = std::result::Result<T, Skip>;

fn skip<T>(why: impl Into<String>) -> R<T> {
    Err(Skip(why.into()))
}

fn class_cx(loc: &Location) -> Context<'_> {
    Context {
        module: loc.module.as_deref(),
        namespace: loc.path[..loc.path.len() - 1]
            .iter()
            .map(String::as_str)
            .collect(),
        ..Context::default()
    }
}

/// A Zena identifier for any text: invalid characters become `_`, and a
/// keyword takes a trailing `_`.
pub fn ident(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

fn unique(used: &mut HashSet<String>, base: String) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}{n}"))
        .find(|n| used.insert(n.clone()))
        .unwrap()
}

/// The Zena type of a scalar on the wire and in the surface API.
fn scalar(s: Scalar) -> &'static str {
    match s {
        Scalar::Bool => "boolean",
        Scalar::I8 => "i8",
        Scalar::U8 => "u8",
        Scalar::I16 => "i16",
        Scalar::U16 => "u16",
        Scalar::I32 => "i32",
        Scalar::U32 => "u32",
        Scalar::I53 | Scalar::I64 => "i64",
        Scalar::U53 | Scalar::U64 => "u64",
        Scalar::F16 | Scalar::F32 => "f32",
        Scalar::F64 => "f64",
    }
}

/// A `T[]` element: the storage `abi::layout` gives it.
fn vec_elem(s: Scalar) -> &'static str {
    match s {
        Scalar::F16 => "u16",
        s => scalar(s),
    }
}

/// An enum of unit arms over an `i32`, which a Zena enum is.
fn i32_enum(layout: &EnumLayout) -> bool {
    match layout.shape {
        Shape::Index => true,
        Shape::Carrier(s) => matches!(layout::carrier_of(s), ValType::I32) && s != Scalar::Bool,
        Shape::Payload { .. } => false,
    }
}

/// The zero of a scalar's Zena type, for an absent value's slot.
fn zero(s: Scalar) -> &'static str {
    match s {
        Scalar::Bool => "false",
        Scalar::I53 | Scalar::I64 => "(0 as i64)",
        Scalar::U53 | Scalar::U64 => "(0 as u64)",
        Scalar::F16 | Scalar::F32 => "(0.0 as f32)",
        Scalar::F64 => "0.0",
        Scalar::U32 => "(0 as u32)",
        Scalar::I8 => "(0 as i8)",
        Scalar::U8 => "(0 as u8)",
        Scalar::I16 => "(0 as i16)",
        Scalar::U16 => "(0 as u16)",
        Scalar::I32 => "0",
    }
}

/// The Zena type of a wire value standing alone: an enum slot.
fn wire_type(v: &ValType) -> R<String> {
    Ok(match v {
        ValType::I32 => "i32".into(),
        ValType::I64 => "i64".into(),
        ValType::F32 => "f32".into(),
        ValType::F64 => "f64".into(),
        ValType::Ref(r) => {
            use ts_gen_abi::layout::{Def, Heap};
            let base = match &r.heap {
                Heap::Extern => "externref".to_string(),
                Heap::Any => "anyref".to_string(),
                Heap::Def(d) => match &**d {
                    Def::Array(Storage::I8) => "array<var u8>".into(),
                    Def::Array(Storage::I16) => "array<var u16>".into(),
                    Def::Array(Storage::Val(v)) => format!("array<var {}>", wire_type(v)?),
                    Def::Struct(_) => return skip("a tuple slot"),
                },
            };
            nullable_ty(base, r.nullable)
        }
    })
}

/// The literal for an absent value in a slot of wire type `v`.
fn slot_zero(v: &ValType) -> &'static str {
    match v {
        ValType::I32 => "0",
        ValType::I64 => "(0 as i64)",
        ValType::F32 => "(0.0 as f32)",
        ValType::F64 => "0.0",
        ValType::Ref(_) => "null",
    }
}

/// How a member's wrapper reaches its import.
struct Call {
    /// The wrapper's own parameters.
    params: Vec<(String, String)>,
    /// The import's parameters, by wire value.
    raw_params: Vec<String>,
    /// Statements before the call.
    pre: Vec<String>,
    args: Vec<String>,
    raw_results: Vec<String>,
    /// The wrapper's result type and the expression over the import's
    /// results (`$r0`, `$r1`, …).
    result: String,
    decode: String,
}

impl<'a> Gen<'a> {
    fn collect(&mut self, items: &'a [Decl], module: Option<&str>, ns: &mut Vec<String>) {
        for d in items {
            match d {
                Decl::Namespace(n) => {
                    ns.push(n.name.clone());
                    self.collect(&n.items, module, ns);
                    ns.pop();
                }
                Decl::Module(m) => self.collect(&m.items, Some(&m.spec), &mut Vec::new()),
                Decl::Class(c) => self.push_decl(module, ns, &c.name, TypeDecl::Class(c)),
                Decl::Enum(e) => self.push_decl(module, ns, &e.name, TypeDecl::Enum(e)),
                Decl::Member(_) | Decl::Alias(_) => {}
            }
        }
    }

    fn push_decl(&mut self, module: Option<&str>, ns: &[String], name: &str, decl: TypeDecl<'a>) {
        let mut path = ns.to_vec();
        path.push(name.into());
        self.decls.push(DeclRef {
            loc: Location {
                module: module.map(Into::into),
                path,
            },
            decl,
        });
    }

    /// Globals first, so a module declaration only takes its specifier as
    /// a prefix when a global holds its name. A type argument spells as
    /// its Zena name in a templated import, so only a global declaration
    /// outside namespaces keeps the spelling the IR gives it (see the
    /// README).
    fn assign_names(&mut self) {
        for global in [true, false] {
            for d in &self.decls {
                if d.loc.module.is_none() != global {
                    continue;
                }
                let base = ident(&d.loc.path.join("_"));
                let base = match &d.loc.module {
                    Some(spec) if self.used.contains(&base) => {
                        format!("{}_{base}", ident(spec))
                    }
                    _ => base,
                };
                let name = unique(&mut self.used, base);
                self.names.insert(location(&d.loc), name);
            }
        }
    }

    fn assign_specs(&mut self) {
        for i in 0..self.decls.len() {
            let TypeDecl::Class(c) = self.decls[i].decl else {
                continue;
            };
            let [TypeParam {
                bound: Some(Bound::In(rows)),
                ..
            }] = c.tparams.as_slice()
            else {
                continue;
            };
            if c.export {
                continue;
            }
            let canonical = location(&self.decls[i].loc);
            let base = self.names[&canonical].clone();
            let loc = self.decls[i].loc.clone();
            let cx = class_cx(&loc);
            let args: Vec<String> = rows.iter().map(|row| self.spelling(row, &cx)).collect();
            let specs = args
                .into_iter()
                .map(|arg| {
                    let name = unique(&mut self.used, ident(&format!("{base}{arg}")));
                    Spec { name, arg }
                })
                .collect();
            self.specs.insert(canonical, specs);
        }
    }

    /// `t` as keys spell a type argument.
    fn spelling(&self, t: &Type, cx: &Context<'_>) -> String {
        match t {
            Type::Named(r) if r.args.is_empty() => {
                match self.layouts.keys.symbols.resolve_decl(&r.path, cx) {
                    Some((loc, _)) => location(&loc),
                    None => ts_gen_abi::text::print_type(t),
                }
            }
            t => ts_gen_abi::text::print_type(t),
        }
    }

    /// The class instantiating the class at `canonical` with `args`, where
    /// its bound makes it one.
    fn spec_of(&self, canonical: &str, args: &[Type], cx: &Context<'_>) -> Option<String> {
        let specs = self.specs.get(canonical)?;
        let [arg] = args else { return None };
        let arg = self.spelling(arg, cx);
        let name = specs
            .iter()
            .find(|s| s.arg == arg)
            .map(|s| s.name.clone())?;
        self.note_lib(&name);
        Some(name)
    }

    fn name_of(&self, canonical: &str) -> R<String> {
        match self.names.get(canonical) {
            Some(n) => {
                self.note_lib(n);
                Ok(n.clone())
            }
            None => skip(format!("`{canonical}` has no Zena declaration")),
        }
    }

    fn note_lib(&self, name: &str) {
        if self.lib_names.contains_key(name) {
            self.lib_used.borrow_mut().insert(name.to_string());
        }
    }

    /// `import { … } from "<lib>";` for the lib declarations in `names`.
    fn lib_imports<'n>(&self, names: impl Iterator<Item = &'n String>) -> String {
        let mut by_module: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for local in names {
            if let Some((module, name)) = self.lib_names.get(local) {
                by_module.entry(module).or_default().push(if name == local {
                    local.clone()
                } else {
                    format!("{name} as {local}")
                });
            }
        }
        by_module
            .into_iter()
            .map(|(m, names)| {
                format!(
                    "import {{ {} }} from {};\n",
                    names.join(", "),
                    zena_string(m)
                )
            })
            .collect()
    }

    fn fresh(&mut self) -> String {
        self.tmp += 1;
        format!("t{}", self.tmp)
    }

    fn finish(mut self) -> Output {
        if self.uses_promise {
            if let Ok(err) = self.layouts.enum_of(
                &ts_gen_abi::ir::Path::err(),
                &Context::default(),
                Flow::ToLang,
            ) {
                let _ = self.tag_class(&err);
            }
        }
        let mut out = String::new();
        out.push_str("// Generated by ts-gen-zena from the ts-gen IR. Do not edit.\n");
        out.push_str("// Compile with `zena build -t js` and link against the tw bridge.\n\n");
        out.push_str("import { Option, Some, None, none, some } from 'zena:core';\n");
        out.push_str(&self.lib_imports(self.lib_used.borrow().iter()));
        out.push('\n');
        if self.uses_promise {
            out.push_str(ASYNC_RUNTIME);
            out.push('\n');
        }
        // A callback can complete Zena futures outside a tick, so it asks
        // for one.
        if self.helper_done.iter().any(|h| h.starts_with("tramp ")) {
            out.push_str(if self.uses_promise {
                "function __tw_entered(): void {\n  __tw_work_available(1);\n}\n\n"
            } else {
                "function __tw_entered(): void {}\n\n"
            });
        }
        out.push_str(&self.types);
        out.push_str(&self.externs);
        out.push_str(&self.helpers);
        out.push_str(&self.funcs);
        out.push_str(&self.probes);
        let entry = self.opts.entry.as_ref().map(|e| {
            let mut m = String::new();
            m.push_str("// Generated by ts-gen-zena from the ts-gen IR. Do not edit.\n");
            m.push_str("// The IR's exports, under their keys, over the app's functions.\n\n");
            if self.exports.contains("Future<") {
                m.push_str("import { Future } from 'zena:async';\n");
            }
            // The exported classes are the app's, and a lib's declarations
            // the lib's.
            let ours =
                |s: &&String| !self.app_uses.contains(*s) && !self.lib_names.contains_key(*s);
            m.push_str(&self.lib_imports(self.export_uses.iter()));
            if self.export_uses.iter().any(|s| ours(&s)) {
                let names: Vec<&str> = self
                    .export_uses
                    .iter()
                    .filter(ours)
                    .map(|s| s.as_str())
                    .collect();
                let _ = writeln!(
                    m,
                    "import {{ {} }} from {};",
                    names.join(", "),
                    zena_string(&e.bindings)
                );
            }
            if !self.app_uses.is_empty() {
                let names: Vec<&str> = self.app_uses.iter().map(|s| s.as_str()).collect();
                let _ = writeln!(
                    m,
                    "import {{ {} }} from {};",
                    names.join(", "),
                    zena_string(&e.app)
                );
            }
            m.push('\n');
            m.push_str(&self.exports);
            m.trim_end().to_string() + "\n"
        });
        Output {
            zena: out.trim_end().to_string() + "\n",
            entry,
            skipped: self.skipped,
        }
    }

    fn export(&mut self, k: &Keyed<'_>) -> R<()> {
        self.in_export = true;
        let r = self.export_with(k);
        self.in_export = false;
        r?;
        if matches!(k.member.kind, MemberKind::New(_)) {
            if let Err(Skip(why)) = self.construct(k) {
                let _ = writeln!(self.funcs, "// skipped tw:new {}: {why}", k.key);
            }
        }
        Ok(())
    }

    /// `new<Class>(...)`: constructs an exported class with an imported
    /// base through JS (`tw:new`), which runs its JS constructor and
    /// re-enters the constructor export, and returns the instance's handle
    /// for the app to cast to its class (§17). The app constructs a
    /// base-less one itself, and the bridge wraps it on crossing. Under
    /// GC the pairing is keyed by the handle, so the wrapper lives as long
    /// as the app keeps the instance.
    fn construct(&mut self, k: &Keyed<'_>) -> R<()> {
        let class = k.owner.unwrap();
        let path = Path::new([class.name.as_str()]);
        let Some((loc, _)) = self.layouts.keys.symbols.resolve_decl(&path, &k.cx) else {
            return skip("an unresolved class");
        };
        if !self.layouts.imported_base(&loc) {
            return Ok(());
        }
        let mut import = k.clone();
        import.role = Role::Import(ts_gen_abi::protocol::NEW.into());
        if let Err(e) = self.layouts.construct(k) {
            return skip(e.to_string());
        }
        let call = self.call(&import, &[])?;
        let raw = self.external(&import, &call, false, None);
        let canonical = location(&loc);
        let unbox = format!("__tw_unbox_{}", ident(&canonical));
        if self.used.insert(unbox.clone()) {
            let _ = writeln!(
                self.externs,
                "@external(\"tw:unbox\", {}, \"tw\")\ndeclare function {unbox}(p0: externref): anyref;\n",
                zena_string(&canonical)
            );
        }
        let suffix = k.member.kind.name().map(upper_first).unwrap_or_default();
        let fname = unique(&mut self.used, format!("new{}{suffix}", ident(&class.name)));
        let invoke = format!("{unbox}({raw}({}))", call.args.join(", "));
        let mut body = String::new();
        for p in &call.pre {
            let _ = writeln!(body, "  {p}");
        }
        let _ = writeln!(body, "  return {invoke};");
        let _ = writeln!(
            self.funcs,
            "/** A new `{}` constructed through JS, as its handle. */\nexport function {fname}({}): anyref {{\n{body}}}\n",
            class.name,
            param_list(&call.params)
        );
        Ok(())
    }

    /// An export: an `@export` function in the entry module that decodes
    /// its wire values, calls the app's function named after the member
    /// (a `default` namespace dropped), and encodes the result. An
    /// exported class's members are the app's class of the same name, its
    /// instances their handles; the constructor doesn't see the JS
    /// instance.
    fn export_with(&mut self, k: &Keyed<'_>) -> R<()> {
        if self.opts.entry.is_none() {
            return skip("no entry module (--entry)");
        }
        if !k.member.tparams.is_empty() {
            return skip("a generic export");
        }
        let sig = match self.layouts.member(k, &[]) {
            Ok(s) => s,
            Err(e) => return skip(e.to_string()),
        };
        let mut raw_params = Vec::new();
        let mut args = Vec::new();
        let mut decodes = Vec::new();
        let mut used: HashSet<String> = HashSet::new();
        let class = k.owner.map(|c| ident(&c.name));
        if let Some(c) = &class {
            self.app_uses.insert(c.clone());
        }
        for p in &sig.params {
            let i = match p.role {
                Pos::Param(i) => i,
                Pos::This => {
                    raw_params.push("this_: externref".into());
                    continue;
                }
                Pos::Handle => {
                    raw_params.push("handle: anyref".into());
                    decodes.push(format!(
                        "  let self_ = (handle as {});\n",
                        class.as_deref().unwrap_or_default()
                    ));
                    continue;
                }
                _ => return skip(format!("{:?} positions", p.role)),
            };
            let pname = unique(&mut used, ident(&k.member.params[i].name));
            let raw = self.raw(&p.repr)?;
            let raws: Vec<String> = (0..raw.len()).map(|j| format!("{pname}_{j}")).collect();
            for (n, t) in raws.iter().zip(&raw) {
                raw_params.push(format!("{n}: {t}"));
            }
            let surface = self.surface(&p.repr)?;
            decodes.push(format!(
                "  let {pname}: {surface} = {};\n",
                self.decode(&p.repr, &raws)?
            ));
            self.uses_surface(&surface);
            args.push(pname);
        }
        let (app, shown, call) = match &class {
            Some(c) => {
                let on = if k.member.is_static {
                    c.as_str()
                } else {
                    "self_"
                };
                let name = k.member.kind.name().map(ident);
                let call = match (&k.member.kind, name) {
                    (MemberKind::New(_), None) => format!("new {c}({})", args.join(", ")),
                    (MemberKind::New(_), Some(n)) => {
                        format!("new {c}.{n}({})", args.join(", "))
                    }
                    (MemberKind::Get(_), Some(n)) => format!("{on}.{n}"),
                    (MemberKind::Set(_), Some(n)) => format!("{on}.{n} = {}", args.join(", ")),
                    (_, n) => format!("{on}.{}({})", n.unwrap_or_default(), args.join(", ")),
                };
                let name = k.member.kind.name().unwrap_or("new");
                let sep = if k.member.is_static || matches!(k.member.kind, MemberKind::New(_)) {
                    "."
                } else {
                    "#"
                };
                (
                    ident(&format!("{c}_{name}")),
                    format!("{c}{sep}{name}"),
                    call,
                )
            }
            None => {
                let mut path: Vec<String> = k.cx.namespace.iter().map(|s| s.to_string()).collect();
                if path.first().is_some_and(|s| s == "default") {
                    path.remove(0);
                }
                path.push(k.member.kind.name().unwrap_or("new").to_string());
                let app = ident(&path.join("_"));
                self.app_uses.insert(app.clone());
                let call = format!("{app}({})", args.join(", "));
                (app.clone(), app, call)
            }
        };
        let mut body: String = decodes.concat();
        let result = match sig.results.first() {
            None => {
                let _ = writeln!(body, "  {call};");
                "void".to_string()
            }
            Some(r) if matches!(r.repr.kind, Kind::Never) => {
                let _ = writeln!(body, "  {call};");
                "never".to_string()
            }
            Some(r) => {
                let raw = self.raw(&r.repr)?;
                let surface = self.surface(&r.repr)?;
                self.uses_surface(&surface);
                let _ = writeln!(body, "  let result: {surface} = {call};");
                let mut pre = Vec::new();
                let enc = self.encode(&r.repr, "result", &mut pre)?;
                for st in pre {
                    let _ = writeln!(body, "  {st}");
                }
                match &raw[..] {
                    [raw] => {
                        let _ = writeln!(body, "  return {};", enc.join(", "));
                        raw.clone()
                    }
                    _ => {
                        let _ = writeln!(body, "  return ({});", enc.join(", "));
                        format!("inline ({})", raw.join(", "))
                    }
                }
            }
        };
        // The conversions' helpers are the bindings'.
        for w in body.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if w.starts_with("__") && (self.used.contains(w) || self.helper_done.contains(w)) {
                self.export_uses.insert(w.to_string());
            }
        }
        let (option, body) = match &sig.throws {
            Some(layout) => {
                let body = self.rethrowing(layout, &body)?;
                let class = self.names[&format!("tag {}", layout.path)].clone();
                let fallback = &self.names[&format!("tagfallback {}", layout.path)];
                let arm = fallback.split('.').next().unwrap_or_default().to_string();
                self.export_uses.insert(class);
                self.export_uses.insert(arm);
                (format!("tw, throws:{}", layout.path), body)
            }
            None => ("tw".to_string(), body),
        };
        let name = unique(&mut self.used, format!("__export_{app}"));
        let _ = writeln!(
            self.exports,
            "@export({}, {})\nexport function {name}({}): {result} {{\n{body}}}\n",
            zena_string(&brace_escape(&k.key)),
            zena_string(&option),
            raw_params.join(", ")
        );
        let _ = writeln!(
            self.funcs,
            "// export {}: the entry module's {name}, over the app's {shown}",
            k.key
        );
        Ok(())
    }

    /// Records the bindings' type names a surface type mentions, for the
    /// entry module's import.
    fn uses_surface(&mut self, surface: &str) {
        for w in surface.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if w.is_empty()
                || matches!(
                    w,
                    "String"
                        | "FixedArray"
                        | "boolean"
                        | "null"
                        | "void"
                        | "Option"
                        | "Some"
                        | "None"
                        | "Future"
                )
                || w.chars().next().is_some_and(|c| c.is_lowercase())
            {
                continue;
            }
            self.export_uses.insert(w.to_string());
        }
    }

    fn enums(&mut self) -> anyhow::Result<()> {
        for i in 0..self.decls.len() {
            let TypeDecl::Enum(_) = self.decls[i].decl else {
                continue;
            };
            let canonical = location(&self.decls[i].loc);
            let path = Path::new(self.decls[i].loc.path.clone());
            let cx = Context {
                module: self.decls[i].loc.module.as_deref(),
                ..Context::default()
            };
            let name = self.names[&canonical].clone();
            let ty = match self.layouts.enum_of(&path, &cx, Flow::ToLang) {
                Ok(layout) => self.enum_type(&name, &layout),
                Err(e) => skip(e.to_string()),
            };
            match ty {
                Ok(t) => self.types.push_str(&t),
                Err(Skip(why)) => {
                    let _ = writeln!(self.types, "// skipped enum {canonical}: {why}\n");
                    self.skipped_enums.insert(canonical);
                }
            }
        }
        Ok(())
    }

    fn enum_type(&mut self, name: &str, layout: &EnumLayout) -> R<String> {
        let mut s = String::new();
        match &layout.shape {
            Shape::Index => {
                let _ = writeln!(s, "export enum {name} {{");
                for a in &layout.arms {
                    let _ = writeln!(s, "  {} = {},", ident(&a.name), a.index);
                }
                s.push_str("}\n\n");
            }
            Shape::Carrier(c) => {
                if !matches!(layout::carrier_of(*c), ValType::I32) {
                    let _ = writeln!(s, "export distinct type {name} = {};\n", scalar(*c));
                    return Ok(s);
                }
                let _ = writeln!(s, "export enum {name} {{");
                for a in &layout.arms {
                    let _ = writeln!(s, "  {} = {},", ident(&a.name), a.index as i32);
                }
                s.push_str("}\n\n");
            }
            Shape::Payload { .. } => {
                let _ = writeln!(s, "export sealed class {name} {{");
                for a in &layout.arms {
                    let case = self.arm_class(name, &a.name);
                    if a.payload.is_empty() {
                        let _ = writeln!(s, "  case {case}");
                        continue;
                    }
                    let mut fields = Vec::new();
                    for (i, p) in a.payload.iter().enumerate() {
                        let field = field_name(i, a.payload.len());
                        fields.push(format!("{field}: {}", self.surface(p)?));
                    }
                    let _ = writeln!(s, "  case {case}({})", fields.join(", "));
                }
                s.push_str("}\n\n");
            }
        }
        Ok(s)
    }

    /// The variant class of an arm: `<Enum><Arm>`, since variants share
    /// the module's namespace.
    fn arm_class(&mut self, enum_name: &str, arm: &str) -> String {
        let key = format!("arm:{enum_name}.{arm}");
        if let Some(n) = self.names.get(&key) {
            return n.clone();
        }
        let n = unique(&mut self.used, ident(&format!("{enum_name}{arm}")));
        self.names.insert(key, n.clone());
        n
    }

    fn classes(&mut self, members: &[Keyed<'a>]) -> anyhow::Result<()> {
        // A bound element with no Zena type, such as a skipped enum, has
        // no class; a class with none is generic.
        let mut specs = std::mem::take(&mut self.specs);
        for v in specs.values_mut() {
            v.retain(|s| {
                ts_gen_abi::text::parse_type(&s.arg)
                    .is_ok_and(|t| self.generic_surface(&t, &Context::default()).is_ok())
            });
        }
        specs.retain(|_, v| !v.is_empty());
        self.specs = specs;
        let mut parents = Vec::new();
        for i in 0..self.decls.len() {
            let parent = match self.decls[i].decl {
                TypeDecl::Class(c) if !c.export => {
                    let loc = self.decls[i].loc.clone();
                    let cx = class_cx(&loc).with_tparams(&c.tparams);
                    c.extends.first().and_then(|p| self.host_ref(p, &cx).ok())
                }
                _ => None,
            };
            if let Some(p) = &parent {
                let bare = p.split('<').next().unwrap().to_string();
                self.parents.insert(bare);
            }
            parents.push(parent);
        }
        for c in self.specs.keys() {
            self.parents.insert(self.names[c].clone());
        }
        for (i, parent) in parents.into_iter().enumerate() {
            let TypeDecl::Class(c) = self.decls[i].decl else {
                continue;
            };
            if c.export {
                for k in members {
                    if k.role == Role::Export && k.owner.is_some_and(|o| std::ptr::eq(o, c)) {
                        if let Err(Skip(why)) = self.export(k) {
                            let _ = writeln!(self.funcs, "// skipped export {}: {why}", k.key);
                            self.skipped.push(Skipped {
                                key: k.key.clone(),
                                reason: why,
                            });
                        }
                    }
                }
                continue;
            }
            self.class(i, c, parent, members, None)?;
            let canonical = location(&self.decls[i].loc);
            let specs = self.specs.remove(&canonical).unwrap_or_default();
            for s in &specs {
                self.class(i, c, None, members, Some(s))?;
            }
            self.specs.insert(canonical, specs);
        }
        Ok(())
    }

    /// A generic class's type parameters, each with its default where Zena
    /// can spell it, so a mention without arguments instantiates at it.
    fn class_tparams(&mut self, c: &'a Class, loc: &Location) -> String {
        if c.tparams.iter().all(|p| p.default.is_none()) {
            return tparam_list(&c.tparams);
        }
        let cx = class_cx(loc).with_class(c);
        let ps: Vec<String> = c
            .tparams
            .iter()
            .map(|p| {
                let name = ident(&p.name);
                match p.default.as_ref().map(|d| self.generic_surface(d, &cx)) {
                    Some(Ok(d)) => format!("{name} = {d}"),
                    _ => name,
                }
            })
            .collect();
        format!("<{}>", ps.join(", "))
    }

    fn class(
        &mut self,
        i: usize,
        c: &'a Class,
        parent: Option<String>,
        members: &[Keyed<'a>],
        spec: Option<&Spec>,
    ) -> anyhow::Result<()> {
        let loc = self.decls[i].loc.clone();
        let canonical = location(&loc);
        let base = self.names[&canonical].clone();
        let mut head = String::new();
        if let Some(doc) = &c.meta.doc {
            push_doc(&mut head, "", doc);
        }
        let (name, tparams, parent, row) = match spec {
            Some(s) => {
                let arg = ts_gen_abi::text::parse_type(&s.arg)
                    .map_err(|e| anyhow::anyhow!("`{}`: {e}", s.arg))?;
                let zarg = self
                    .generic_surface(&arg, &Context::default())
                    .map_err(|Skip(why)| anyhow::anyhow!("`{}`: {why}", s.arg))?;
                let spelling = format!("{canonical}<{}>", s.arg);
                let _ = writeln!(head, "@spelling({})", zena_string(&spelling));
                (
                    s.name.clone(),
                    String::new(),
                    Some(format!("{base}<{zarg}>")),
                    vec![arg],
                )
            }
            None => {
                if base != canonical {
                    let _ = writeln!(head, "@spelling({})", zena_string(&canonical));
                }
                (base, self.class_tparams(c, &loc), parent, Vec::new())
            }
        };
        let specialized = self.specs.contains_key(&canonical);
        // A subclass's constructors reach the value through its parent's
        // `__wrap`, since `super(...)` calls the parent's constructor.
        let init = if parent.is_some() {
            "super.__wrap"
        } else {
            "super"
        };
        let _ = match &parent {
            Some(p) => writeln!(
                head,
                "export extension class {name}{tparams} extends {p} {{"
            ),
            _ => writeln!(
                head,
                "export extension class {name}{tparams} on externref {{"
            ),
        };
        let mut body = String::new();
        if self.parents.contains(&name) {
            let _ = writeln!(body, "  new __wrap(value: externref) : {init}(value);");
        }
        let own: Vec<&Keyed> = members
            .iter()
            .filter(|k| k.owner.is_some_and(|o| std::ptr::eq(o, c)))
            .collect();
        // By staticness and name: a static and an instance accessor can
        // share a name.
        let mut accessors: BTreeMap<(bool, String), Accessor> = BTreeMap::new();
        let mut methods = String::new();
        let ctor_names = Self::ctor_names(&own);
        // Overloads Zena can't tell apart, such as `f64[]` and `arr<f64>`
        // (both `FixedArray<f64>`), take numbered names.
        let mut sigs: HashSet<String> = HashSet::new();
        let own = match spec {
            None if c.tparams.is_empty() => self.with_arms(&own),
            _ => own.into_iter().map(|k| (k, None)).collect(),
        };
        for (k, arm) in own {
            self.arm = arm;
            if k.role == Role::Export {
                continue;
            }
            // A specialized class's members are its rows': a generic Zena
            // class over an enum can't import per instantiation, since Zena
            // shares the instantiation with the enum's backing type. One
            // with type parameters of its own stays generic in those.
            let own_tparams = !k.member.tparams.is_empty();
            let call = match spec {
                Some(_) if own_tparams => self.generic_call_at(k, c, &row),
                Some(_) => self.call(k, &row),
                None if specialized => continue,
                None if !c.tparams.is_empty() || own_tparams => self.generic_call(k),
                None => self.call(k, &[]),
            };
            let generic = own_tparams || (row.is_empty() && !c.tparams.is_empty());
            let call = match call {
                Ok(call) => call,
                Err(Skip(why)) => {
                    let key = match (spec, &self.arm) {
                        (Some(s), _) => instance_key(k, &[s.arg.as_str()]),
                        (None, Some((key, _))) => key.clone(),
                        (None, None) => k.key.clone(),
                    };
                    let _ = writeln!(body, "  // skipped {key}: {why}");
                    self.skipped.push(Skipped { key, reason: why });
                    continue;
                }
            };
            let raw = self.external(k, &call, generic, spec.map(|s| s.arg.as_str()));
            let m = k.member;
            let targs = match (generic, spec) {
                (true, Some(_)) => raw_targs_member(m),
                (true, None) => raw_targs(c, m),
                (false, _) => String::new(),
            };
            let invoke = format!("{raw}{targs}({})", call.args.join(", "));
            let doc = m.meta.doc.as_deref();
            match &m.kind {
                MemberKind::New(ctor_name) => {
                    let label = match ctor_name {
                        Some(n) => format!(" {}", ident(n)),
                        None => match ctor_names.get(k.key.as_str()) {
                            Some(n) if n != "new" => format!(" {}", ident(n)),
                            _ => String::new(),
                        },
                    };
                    if !generic {
                        let named = label.trim_start();
                        let ctor = if named.is_empty() {
                            name.clone()
                        } else {
                            format!("{name}.{named}")
                        };
                        let expr = format!("new {ctor}({})", arg_names(&call.params));
                        self.probe(None, &call.params, &name, &expr);
                    }
                    if let Some(d) = doc {
                        push_doc(&mut methods, "  ", d);
                    }
                    let params = param_list(&call.params);
                    let pre: String = call.pre.iter().map(|p| format!(" {p}")).collect();
                    if pre.is_empty() {
                        let _ = writeln!(methods, "  new{label}({params}) : {init}({invoke});");
                    } else {
                        // An initializer list takes one expression, so the
                        // conversions go through a helper.
                        let ctparams: &[TypeParam] = if generic { &c.tparams } else { &[] };
                        let helper = self.ctor_helper(&name, ctparams, &call, &invoke);
                        let names: Vec<&str> =
                            call.params.iter().map(|(n, _)| n.as_str()).collect();
                        let _ = writeln!(
                            methods,
                            "  new{label}({params}) : {init}({helper}{}({}));",
                            tparam_list(ctparams),
                            names.join(", ")
                        );
                    }
                }
                MemberKind::Get(n) | MemberKind::Set(n) => {
                    let entry = accessors.entry((m.is_static, n.clone())).or_default();
                    entry.is_static = m.is_static;
                    let body_text = wrapper_body(&call, &invoke, "    ");
                    if matches!(m.kind, MemberKind::Get(_)) {
                        entry.get = Some((call.result.clone(), body_text, doc.map(Into::into)));
                    } else {
                        let (pn, pt) = call.params.first().cloned().unwrap_or_default();
                        entry.sets.push((pn, pt, body_text, call.params.is_empty()));
                    }
                }
                MemberKind::Method(n) => {
                    let mname = overload_name(&mut sigs, m.is_static, &ident(n), &call.params);
                    if let Some(d) = doc {
                        push_doc(&mut methods, "  ", d);
                    }
                    if !generic {
                        let (recv, on) = if m.is_static {
                            (None, name.as_str())
                        } else {
                            (Some(name.as_str()), "__self")
                        };
                        let expr = format!("{on}.{mname}({})", arg_names(&call.params));
                        self.probe(recv, &call.params, &call.result, &expr);
                    }
                    let stat = if m.is_static { "static " } else { "" };
                    let mt = if m.is_static {
                        tparam_list(
                            &c.tparams
                                .iter()
                                .chain(&m.tparams)
                                .cloned()
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        tparam_list(&m.tparams)
                    };
                    let _ = writeln!(
                        methods,
                        "  {stat}{mname}{mt}({}): {} {{\n{}  }}",
                        param_list(&call.params),
                        call.result,
                        wrapper_body(&call, &invoke, "    ")
                    );
                }
            }
        }
        self.arm = None;
        // Names that are one identifier in Zena (`$_`, `$+`) take numbered ones.
        let mut accessor_names: [HashSet<String>; 2] = Default::default();
        for ((_, n), a) in accessors {
            let is_static = a.is_static;
            let zname = unique(&mut accessor_names[usize::from(is_static)], ident(&n));
            for (params, result, access) in emit_accessor(&mut body, &zname, a, &mut sigs) {
                if c.tparams.is_empty() || spec.is_some() {
                    let (recv, on) = if is_static {
                        (None, name.as_str())
                    } else {
                        (Some(name.as_str()), "__self")
                    };
                    self.probe(recv, &params, &result, &format!("{on}{access}"));
                }
            }
        }
        body.push_str(&methods);
        self.types.push_str(&head);
        self.types.push_str(&body);
        self.types.push_str("}\n\n");
        Ok(())
    }

    /// Each member, then the arm variants of a method's or setter's `union`
    /// parameters, which no member's key already is: Zena overloads them,
    /// as it does no constructor or function.
    fn with_arms<'k>(&self, members: &[&'k Keyed<'a>]) -> Vec<(&'k Keyed<'a>, Option<Arm>)> {
        let keys = &self.layouts.keys;
        let mut out = Vec::new();
        for &k in members {
            out.push((k, None));
            if !k.member.tparams.is_empty()
                || k.role == Role::Export
                || matches!(k.member.kind, MemberKind::New(_))
            {
                continue;
            }
            for arm in ts_gen_abi::union::variants(keys, k) {
                if !members.iter().any(|m| m.key == arm.0) {
                    out.push((k, Some(arm)));
                }
            }
        }
        out
    }

    /// Names for a class's unnamed constructors by key: the first is `new`,
    /// and the others are named for how they differ from it.
    fn ctor_names<'k>(own: &[&'k Keyed<'_>]) -> HashMap<&'k str, String> {
        let ctors: Vec<&Keyed> = own
            .iter()
            .copied()
            .filter(|k| k.role != Role::Export && matches!(k.member.kind, MemberKind::New(None)))
            .collect();
        let slots: Vec<Vec<Slot>> = ctors
            .iter()
            .map(|k| {
                k.member
                    .params
                    .iter()
                    .map(|p| Slot::Param {
                        name: &p.name,
                        ty: &p.ty,
                        rest: false,
                    })
                    .collect()
            })
            .collect();
        if ctors.is_empty() {
            return HashMap::new();
        }
        let names = ts_gen_abi::naming::variant_names("new", &slots);
        ctors.iter().map(|k| k.key.as_str()).zip(names).collect()
    }

    /// A constructor's conversions, as a function its initializer calls.
    fn ctor_helper(
        &mut self,
        class: &str,
        tparams: &[TypeParam],
        call: &Call,
        invoke: &str,
    ) -> String {
        let name = unique(&mut self.used, format!("__{class}_new"));
        let _ = writeln!(
            self.helpers,
            "export function {name}{}({}): externref {{\n{}}}\n",
            tparam_list(tparams),
            param_list(&call.params),
            wrapper_body(call, invoke, "  ")
        );
        name
    }

    /// `Parent<Args>` for a host parent with a Zena declaration.
    fn host_ref(&mut self, r: &TypeRef, cx: &Context<'_>) -> R<String> {
        let Some((loc, TypeDecl::Class(c))) = self.layouts.keys.symbols.resolve_decl(&r.path, cx)
        else {
            return skip("not a class");
        };
        if c.export {
            return skip("an exported class (T6)");
        }
        let canonical = location(&loc);
        let name = self.name_of(&canonical)?;
        if r.args.is_empty() {
            return Ok(name);
        }
        if let Some(s) = self.spec_of(&canonical, &r.args, cx) {
            return Ok(s);
        }
        let args: R<Vec<String>> = r.args.iter().map(|a| self.generic_surface(a, cx)).collect();
        Ok(format!("{name}<{}>", args?.join(", ")))
    }

    /// Zena overloads declared functions only, so a wrapper's overloads
    /// take variant names.
    fn fn_names<'k>(members: &'k [Keyed<'_>]) -> HashMap<&'k str, String> {
        let mut groups: Vec<(Vec<&str>, &str, Vec<&Keyed>)> = Vec::new();
        for k in members {
            if k.owner.is_some() || k.role == Role::Export {
                continue;
            }
            let ns: Vec<&str> = k.cx.namespace.iter().map(|s| s.as_ref()).collect();
            let name = k.member.kind.name().unwrap_or("new");
            match groups.iter_mut().find(|(n, m, _)| *n == ns && *m == name) {
                Some((_, _, g)) => g.push(k),
                None => groups.push((ns, name, vec![k])),
            }
        }
        let mut names = HashMap::new();
        for (ns, name, group) in groups {
            let slots: Vec<Vec<Slot>> = group
                .iter()
                .map(|k| {
                    k.member
                        .params
                        .iter()
                        .map(|p| Slot::Param {
                            name: &p.name,
                            ty: &p.ty,
                            rest: false,
                        })
                        .collect()
                })
                .collect();
            let variants = ts_gen_abi::naming::variant_names(name, &slots);
            for (k, v) in group.iter().zip(variants) {
                let mut path: Vec<&str> = ns.clone();
                path.push(&v);
                names.insert(k.key.as_str(), ident(&path.join("_")));
            }
        }
        names
    }

    fn functions(&mut self, members: &[Keyed<'a>]) -> anyhow::Result<()> {
        let fn_names = Self::fn_names(members);
        for k in members {
            if k.owner.is_some() {
                continue;
            }
            if k.role == Role::Export {
                if let Err(Skip(why)) = self.export(k) {
                    let _ = writeln!(self.funcs, "// skipped export {}: {why}", k.key);
                    self.skipped.push(Skipped {
                        key: k.key.clone(),
                        reason: why,
                    });
                }
                continue;
            }
            let generic = !k.member.tparams.is_empty();
            let call = if generic {
                self.generic_call(k)
            } else {
                self.call(k, &[])
            };
            let call = match call {
                Ok(c) => c,
                Err(Skip(why)) => {
                    let _ = writeln!(self.funcs, "// skipped {}: {why}", k.key);
                    self.skipped.push(Skipped {
                        key: k.key.clone(),
                        reason: why,
                    });
                    continue;
                }
            };
            let raw = self.external(k, &call, generic, None);
            let m = k.member;
            let fname = unique(&mut self.used, fn_names[k.key.as_str()].clone());
            let targs = if generic {
                raw_targs_member(m)
            } else {
                String::new()
            };
            let invoke = format!("{raw}{targs}({})", call.args.join(", "));
            if !generic {
                let expr = format!("{fname}({})", arg_names(&call.params));
                self.probe(None, &call.params, &call.result, &expr);
            }
            if let Some(d) = &m.meta.doc {
                push_doc(&mut self.funcs, "", d);
            }
            let _ = writeln!(
                self.funcs,
                "export function {fname}{}({}): {} {{\n{}}}\n",
                tparam_list(&m.tparams),
                param_list(&call.params),
                call.result,
                wrapper_body(&call, &invoke, "  ")
            );
        }
        Ok(())
    }

    /// The `@external` declaration of a member, and its name.
    fn external(&mut self, k: &Keyed<'_>, call: &Call, generic: bool, arg: Option<&str>) -> String {
        let m = k.member;
        let owner: Vec<String> = match k.owner {
            Some(c) => vec![c.name.clone()],
            None => k.cx.namespace.iter().map(|s| s.to_string()).collect(),
        };
        let mname = m.kind.name().unwrap_or("new");
        let mut base = owner.join("_");
        if !base.is_empty() {
            base.push('_');
        }
        base.push_str(mname);
        self.raw_count += 1;
        let raw = unique(&mut self.used, format!("__{}", ident(&base)));
        let Role::Import(module) = &k.role else {
            unreachable!("exports are skipped")
        };
        let (name, tparams) = match arg {
            Some(a) if generic => (partial_key(k, a), raw_targs_member(m)),
            Some(a) => (brace_escape(&instance_key(k, &[a])), String::new()),
            None if generic => (templated_key(k), raw_tparams(k)),
            None => (
                brace_escape(self.arm.as_ref().map_or(&k.key, |(key, _)| key)),
                String::new(),
            ),
        };
        let results = match call.raw_results.len() {
            0 if call.result == "never" => "never".to_string(),
            0 => "void".to_string(),
            1 => call.raw_results[0].clone(),
            _ => format!("inline ({})", call.raw_results.join(", ")),
        };
        let params: Vec<String> = call
            .raw_params
            .iter()
            .enumerate()
            .map(|(i, t)| format!("p{i}: {t}"))
            .collect();
        let option = match &m.throws {
            Some(path) => self.throws_option(path, &k.cx),
            None => "tw".into(),
        };
        let _ = writeln!(
            self.externs,
            "@external({}, {}, {})\ndeclare function {raw}{tparams}({}): {results};\n",
            zena_string(module),
            zena_string(&name),
            zena_string(&option),
            params.join(", ")
        );
        raw
    }

    /// The `tw` option of a member that throws `path`: `throws:` its tag,
    /// whose class is declared. Without one, the error unwinds through.
    fn throws_option(&mut self, path: &ts_gen_abi::ir::Path, cx: &Context<'_>) -> String {
        match self.layouts.enum_of(path, cx, Flow::ToLang) {
            Ok(layout) => match self.tag_class(&layout) {
                Ok(_) => format!("tw, throws:{}", layout.path),
                Err(_) => "tw".into(),
            },
            Err(_) => "tw".into(),
        }
    }

    /// The class a `throws` enum's tag carries (§15): its arm, an enum of
    /// the arms' indices, and its message. Every arm is `(Str)`.
    fn tag_class(&mut self, layout: &EnumLayout) -> R<String> {
        let key = format!("tag {}", layout.path);
        if let Some(n) = self.names.get(&key) {
            return Ok(n.clone());
        }
        let base = match layout.origin {
            Origin::Err => "Err".to_string(),
            _ => self.name_of(&layout.path)?,
        };
        let (class, arm) = match layout.origin {
            Origin::Err => ("ErrException".to_string(), "ErrArm".to_string()),
            _ => (
                unique(&mut self.used, format!("{base}Exception")),
                unique(&mut self.used, format!("{base}Arm")),
            ),
        };
        let mut t = String::new();
        let _ = writeln!(
            t,
            "/** The arms of `{}`, as its tag carries them. */",
            layout.path
        );
        let _ = writeln!(t, "export enum {arm} {{");
        for a in &layout.arms {
            let _ = writeln!(t, "  {} = {},", ident(&a.name), a.index);
        }
        let _ = writeln!(t, "}}\n");
        let _ = writeln!(
            t,
            "/** `{}` thrown: its arm and message. The bridge throws and catches it as the tag `(\"tw:tag\", {})`. */",
            layout.path,
            zena_string(&layout.path)
        );
        let _ = writeln!(
            t,
            "@external(\"tw:tag\", {}, \"tw\")\nexport class {class} extends Error {{\n  arm: {arm};\n  new(arm: {arm}, message: String) : arm = arm, super(message) {{}}\n}}\n",
            zena_string(&layout.path)
        );
        self.types.push_str(&t);
        let fallback = layout
            .arms
            .iter()
            .find(|a| a.fallback)
            .map(|a| ident(&a.name))
            .unwrap_or_else(|| ident(&layout.arms[0].name));
        self.names.insert(
            format!("tagfallback {}", layout.path),
            format!("{arm}.{fallback}"),
        );
        self.names.insert(key, class.clone());
        Ok(class)
    }

    /// Wraps an entry point's body so an error leaving it is its `throws`
    /// enum's class: another `Error` becomes the fallback arm.
    fn rethrowing(&mut self, layout: &EnumLayout, body: &str) -> R<String> {
        let class = self.tag_class(layout)?;
        let fallback = self.names[&format!("tagfallback {}", layout.path)].clone();
        let inner: String = body.lines().map(|l| format!("  {l}\n")).collect();
        Ok(format!(
            "  try {{\n{inner}  }} catch (e) {{\n    if (e is {class}) {{\n      throw e;\n    }}\n    throw new {class}({fallback}, e.message);\n  }}\n"
        ))
    }

    /// An exported function calling one wrapper, so a compiled module
    /// keeps the wrapper and its import.
    fn probe(&mut self, recv: Option<&str>, params: &[(String, String)], result: &str, expr: &str) {
        if !self.opts.probe {
            return;
        }
        self.probe_count += 1;
        let mut ps = Vec::new();
        if let Some(t) = recv {
            ps.push(format!("__self: {t}"));
        }
        ps.extend(params.iter().map(|(n, t)| format!("{n}: {t}")));
        // A `never` call is a statement: Zena has no value of it.
        let stmt = if result == "void" || result == "never" {
            format!("{expr};")
        } else {
            format!("return {expr};")
        };
        let _ = writeln!(
            self.probes,
            "export function __probe_{}({}): {result} {{\n  {stmt}\n}}\n",
            self.probe_count,
            ps.join(", ")
        );
    }

    /// A member's call from its layout, at the type arguments `targs` of
    /// a generic one.
    fn call(&mut self, k: &Keyed<'_>, targs: &[Type]) -> R<Call> {
        let over = self.arm.as_ref().map(|(_, tys)| tys.as_slice());
        let sig = match self.layouts.member_as(k, targs, over) {
            Ok(s) => s,
            Err(e) => return skip(e.to_string()),
        };
        let mut call = Call {
            params: Vec::new(),
            raw_params: Vec::new(),
            pre: Vec::new(),
            args: Vec::new(),
            raw_results: Vec::new(),
            result: "void".into(),
            decode: String::new(),
        };
        let mut used: HashSet<String> = HashSet::new();
        for p in &sig.params {
            match p.role {
                Pos::This => {
                    call.raw_params.push("externref".into());
                    call.args.push("(this as externref)".into());
                }
                Pos::Param(i) => {
                    let pname = unique(&mut used, ident(&k.member.params[i].name));
                    let surface = self.surface(&p.repr)?;
                    call.raw_params.extend(self.raw(&p.repr)?);
                    let mut pre = Vec::new();
                    let args = self.encode(&p.repr, &pname, &mut pre)?;
                    call.pre.extend(pre);
                    call.args.extend(args);
                    call.params.push((pname, surface));
                }
                _ => return skip(format!("{:?} positions", p.role)),
            }
        }
        if let MemberKind::New(_) = k.member.kind {
            // The constructed value is the extension class's `externref`.
            call.result = "externref".into();
            call.raw_results = vec!["externref".into()];
            call.decode = "r0".into();
        } else if let Some(p) = sig.results.first() {
            call.result = self.surface(&p.repr)?;
            call.raw_results = self.raw(&p.repr)?;
            let raws: Vec<String> = (0..call.raw_results.len())
                .map(|i| format!("r{i}"))
                .collect();
            call.decode = self.decode(&p.repr, &raws)?;
        }
        Ok(call)
    }

    /// [`Self::generic_call`] for a member of the class `c` at its
    /// argument `row`, generic in the member's own type parameters.
    fn generic_call_at(&mut self, k: &Keyed<'_>, c: &Class, row: &[Type]) -> R<Call> {
        let subst: HashMap<&str, &Type> =
            c.tparams.iter().map(|p| p.name.as_str()).zip(row).collect();
        let at = |r: &TypeRef| match &r.path.segments[..] {
            [name] if r.args.is_empty() => subst.get(name.as_str()).map(|t| (*t).clone()),
            _ => None,
        };
        let mut member = k.member.clone();
        for p in &mut member.params {
            p.ty = p.ty.map_named(&at);
        }
        member.result = member.result.map_named(&at);
        let mut cx = k.cx.clone();
        cx.tparams.retain(|t| !subst.contains_key(t));
        let keyed = Keyed {
            role: k.role.clone(),
            key: k.key.clone(),
            member: &member,
            owner: k.owner,
            cx,
        };
        self.generic_call(&keyed)
    }

    /// A generic member's call. A type parameter crosses only as a whole
    /// position or as a host type argument: anywhere else its lowering
    /// depends on the instantiation in a way a Zena declaration can't say.
    fn generic_call(&mut self, k: &Keyed<'_>) -> R<Call> {
        let m = k.member;
        let mut call = Call {
            params: Vec::new(),
            raw_params: Vec::new(),
            pre: Vec::new(),
            args: Vec::new(),
            raw_results: Vec::new(),
            result: "void".into(),
            decode: String::new(),
        };
        let instance = k.owner.is_some() && !m.is_static && !matches!(m.kind, MemberKind::New(_));
        if instance {
            call.raw_params.push("externref".into());
            call.args.push("(this as externref)".into());
        }
        let mut used: HashSet<String> = HashSet::new();
        for p in &m.params {
            let pname = unique(&mut used, ident(&p.name));
            if let (Type::Tuple(elems), true) = (&p.ty, mentions_tparam(&p.ty, &k.cx)) {
                let tv = self.fresh();
                call.pre.push(format!("let {tv} = {pname};"));
                let mut surfaces = Vec::new();
                for (i, e) in elems.iter().enumerate() {
                    let ev = self.fresh();
                    call.pre
                        .push(format!("let {ev} = {};", tuple_at(&tv, i, elems.len())));
                    if mentions_tparam(e, &k.cx) {
                        let t = self.generic_surface(e, &k.cx)?;
                        if t.contains("Future<") {
                            return skip("a future inside a generic type");
                        }
                        call.raw_params.push(t.clone());
                        call.args.push(ev);
                        surfaces.push(t);
                    } else {
                        let repr = self.repr(e, &k.cx, Flow::ToJs)?;
                        surfaces.push(self.surface(&repr)?);
                        call.raw_params.extend(self.raw(&repr)?);
                        let mut pre = Vec::new();
                        let args = self.encode(&repr, &ev, &mut pre)?;
                        call.pre.extend(pre);
                        call.args.extend(args);
                    }
                }
                call.params.push((pname, tuple_type(&surfaces)));
            } else if let (Some((fun, nullable)), true) =
                (fn_of(&p.ty), mentions_tparam(&p.ty, &k.cx))
            {
                // JS calls the function through the trampoline at each
                // instantiation, which a closure never called names.
                let t = self.generic_trampoline(fun, &k.cx)?;
                call.pre.push(t.keep.clone());
                if nullable {
                    call.raw_params.push("anyref | null".into());
                    call.args.push(format!(
                        "(if ({pname} == null) null else ({pname} as anyref))"
                    ));
                    call.params.push((pname, format!("({}) | null", t.surface)));
                } else {
                    call.raw_params.push("anyref".into());
                    call.args.push(format!("({pname} as anyref)"));
                    call.params.push((pname, t.surface));
                }
            } else if let (Type::Future(v, e), true) = (&p.ty, mentions_tparam(&p.ty, &k.cx)) {
                let t = self.generic_surface(&p.ty, &k.cx)?;
                let inner = self.generic_surface(v, &k.cx)?;
                let (_, promise) = self.future_fns_of(e, &k.cx, Flow::ToJs)?;
                call.raw_params.push("externref".into());
                call.args.push(format!("{promise}<{inner}>({pname})"));
                call.params.push((pname, t));
            } else if mentions_tparam(&p.ty, &k.cx) {
                let t = self.generic_surface(&p.ty, &k.cx)?;
                if t.contains("Future<") {
                    return skip("a future inside a generic type");
                }
                call.raw_params.push(t.clone());
                call.args.push(pname.clone());
                call.params.push((pname, t));
            } else {
                let repr = self.repr(&p.ty, &k.cx, Flow::ToJs)?;
                let surface = self.surface(&repr)?;
                call.raw_params.extend(self.raw(&repr)?);
                let mut pre = Vec::new();
                let args = self.encode(&repr, &pname, &mut pre)?;
                call.pre.extend(pre);
                call.args.extend(args);
                call.params.push((pname, surface));
            }
        }
        if let MemberKind::New(_) = m.kind {
            call.result = "externref".into();
            call.raw_results = vec!["externref".into()];
            call.decode = "r0".into();
            return Ok(call);
        }
        if m.result == Type::Unit {
            return Ok(call);
        }
        if let (Type::Tuple(elems), true) = (&m.result, mentions_tparam(&m.result, &k.cx)) {
            // A tuple flattens on the wire, each element crossing as it
            // would alone.
            let mut surfaces = Vec::new();
            let mut parts = Vec::new();
            for e in elems {
                let at = call.raw_results.len();
                if mentions_tparam(e, &k.cx) {
                    let t = self.generic_surface(e, &k.cx)?;
                    if t.contains("Future<") {
                        return skip("a future inside a generic type");
                    }
                    call.raw_results.push(t.clone());
                    parts.push(format!("r{at}"));
                    surfaces.push(t);
                } else {
                    let repr = self.repr(e, &k.cx, Flow::ToLang)?;
                    surfaces.push(self.surface(&repr)?);
                    let raw = self.raw(&repr)?;
                    let raws: Vec<String> = (at..at + raw.len()).map(|i| format!("r{i}")).collect();
                    call.raw_results.extend(raw);
                    parts.push(self.decode(&repr, &raws)?);
                }
            }
            call.result = tuple_type(&surfaces);
            call.decode = tuple_type(&parts);
        } else if let (Type::Future(v, e), true) = (&m.result, mentions_tparam(&m.result, &k.cx)) {
            call.result = self.generic_surface(&m.result, &k.cx)?;
            let inner = self.generic_surface(v, &k.cx)?;
            let (wait, _) = self.future_fns_of(e, &k.cx, Flow::ToLang)?;
            call.raw_results = vec!["externref".into()];
            call.decode = format!("{wait}<{inner}>(r0)");
        } else if mentions_tparam(&m.result, &k.cx) {
            let t = self.generic_surface(&m.result, &k.cx)?;
            if t.contains("Future<") {
                return skip("a future inside a generic type");
            }
            call.result = t.clone();
            call.raw_results = vec![t];
            call.decode = "r0".into();
        } else {
            let repr = self.repr(&m.result, &k.cx, Flow::ToLang)?;
            call.result = self.surface(&repr)?;
            call.raw_results = self.raw(&repr)?;
            let raws: Vec<String> = (0..call.raw_results.len())
                .map(|i| format!("r{i}"))
                .collect();
            call.decode = self.decode(&repr, &raws)?;
        }
        Ok(call)
    }

    fn repr(&self, t: &Type, cx: &Context<'_>, flow: Flow) -> R<Repr> {
        self.layouts
            .repr(t, cx, flow)
            .or_else(|e| skip(e.to_string()))
    }

    /// The Zena type of a type that mentions type parameters: the
    /// parameter itself, or a host type around it.
    fn generic_surface(&mut self, t: &Type, cx: &Context<'_>) -> R<String> {
        match t {
            Type::Named(r) if r.args.is_empty() && is_tparam(&r.path, cx) => {
                Ok(ident(&r.path.segments[0]))
            }
            t if !mentions_tparam(t, cx) => {
                let repr = self.repr(t, cx, Flow::ToLang)?;
                self.surface(&repr)
            }
            Type::Named(r) => match self.layouts.keys.symbols.resolve_decl(&r.path, cx) {
                Some((_, TypeDecl::Class(_))) => self.host_ref(r, cx),
                _ => skip("a type parameter inside an enum"),
            },
            Type::Future(v, e) if e.is_err() => {
                self.uses_async();
                Ok(format!("Future<{}>", self.generic_surface(v, cx)?))
            }
            Type::Array(e) => {
                let elem = match &**e {
                    // A tw import copies an element tuple into the
                    // instantiation's wire struct.
                    Type::Tuple(es) => {
                        let es: R<Vec<String>> =
                            es.iter().map(|e| self.generic_surface(e, cx)).collect();
                        tuple_type(&es?)
                    }
                    e => self.generic_surface(e, cx)?,
                };
                Ok(format!("FixedArray<{elem}>"))
            }
            Type::Nullable(inner, _) => match &**inner {
                // A tw import lays `Option<T>` out as the IR's `T?` per
                // instantiation.
                Type::Named(r) if is_tparam(&r.path, cx) => {
                    Ok(format!("Option<{}>", ident(&r.path.segments[0])))
                }
                inner => Ok(format!("{} | null", self.generic_surface(inner, cx)?)),
            },
            Type::Fn(_) => skip("callbacks (T6)"),
            // A box is opaque, whatever it holds.
            Type::TupleBox(_) | Type::ArrBox(_) | Type::VecBox(_) => Ok("externref".into()),
            Type::Tuple(_) => skip("a tuple over a type parameter outside an array"),
            _ => skip("a type parameter inside a vec or enum has no per-instantiation lowering"),
        }
    }

    /// The Zena type a value is presented as.
    fn surface(&mut self, r: &Repr) -> R<String> {
        Ok(match &r.kind {
            // A Zena function type can't name itself.
            Kind::Rec { .. } => return skip("a recursive function type"),
            Kind::Unit => "void".into(),
            // Zena's bottom type: a call to it ends control flow, and the
            // bridge throws if one returns.
            Kind::Never => "never".into(),
            Kind::Scalar(s) => scalar(*s).into(),
            Kind::NullScalar(s, _) => format!("Option<{}>", scalar(*s)),
            Kind::Str { nullable, .. } => nullable_str(*nullable),
            Kind::Vec { bigint: true, .. } => return skip("`bigint`, which Zena has no type for"),
            Kind::Vec { elem, nullable, .. } => {
                nullable_ty(format!("FixedArray<{}>", vec_elem(*elem)), *nullable)
            }
            Kind::Array { elem, nullable, .. } => {
                nullable_ty(format!("FixedArray<{}>", self.surface(elem)?), *nullable)
            }
            Kind::Tuple { elems, nullable } => {
                let es: R<Vec<String>> = elems.iter().map(|e| self.surface(e)).collect();
                let es = es?;
                if es.len() == 1 && *nullable && elems[0].ty.ends_with('?') {
                    return skip("a nullable 1-tuple of a nullable value");
                }
                nullable_ty(tuple_type(&es), *nullable)
            }
            Kind::Enum { layout, nullable } => match layout.origin {
                Origin::Nested => {
                    let inner = &layout.arms[1].payload[0];
                    format!("Option<{}>", self.surface(inner)?)
                }
                Origin::Err => return skip("`err` values"),
                Origin::Decl(_) => {
                    if self.skipped_enums.contains(&layout.path) {
                        return skip(format!("`{}` was skipped", layout.path));
                    }
                    let name = self.name_of(&layout.path)?;
                    match (&layout.shape, nullable) {
                        (Shape::Payload { .. }, true) => format!("{name} | null"),
                        (_, true) => format!("Option<{name}>"),
                        _ => name,
                    }
                }
            },
            Kind::Host { of, nullable } => {
                let t = match of {
                    Host::Class { path, .. } => self.class_surface(path, &r.ty)?,
                    // Every box but `Str` stays opaque, an `externref` the
                    // bindings only pass back, until a fixture needs typed
                    // box handles.
                    Host::Any | Host::Box(_) => "externref".into(),
                    Host::Future { value, error } => {
                        self.future_fns(error)?;
                        format!("Future<{}>", self.future_value(value)?)
                    }
                };
                nullable_ty(t, *nullable)
            }
            // The app's class, which only the entry module names.
            Kind::Handle {
                class, nullable, ..
            } => {
                if !self.in_export {
                    return skip("an exported class's instance outside the entry module");
                }
                let name = ident(class.rsplit('.').next().unwrap_or(class));
                self.app_uses.insert(name.clone());
                nullable_ty(name, *nullable)
            }
            Kind::Fn { sig, nullable, .. } => {
                let t = self.fn_surface(sig)?;
                if *nullable {
                    format!("({t}) | null")
                } else {
                    t
                }
            }
            Kind::Erased { .. } => return skip("an erased type parameter"),
            Kind::View { .. } => return skip("`*T[]` needs a linear memory"),
        })
    }

    /// A host class's Zena type: its name, with the type arguments its
    /// canonical spelling carries.
    fn class_surface(&mut self, path: &str, ty: &str) -> R<String> {
        let name = self.name_of(path)?;
        let ty = ty.trim_end_matches('?');
        if !ty.ends_with('>') {
            return Ok(name);
        }
        let Ok(Type::Named(r)) = ts_gen_abi::text::parse_type(ty) else {
            return skip(format!("`{ty}` doesn't parse"));
        };
        let cx = Context::default();
        if let Some(s) = self.spec_of(path, &r.args, &cx) {
            return Ok(s);
        }
        let args: R<Vec<String>> = r
            .args
            .iter()
            .map(|a| self.generic_surface(a, &cx))
            .collect();
        Ok(format!("{name}<{}>", args?.join(", ")))
    }

    /// The import's Zena types for a value, one per wire value. Where the
    /// surface type lowers to the wire itself it is used as is.
    fn raw(&mut self, r: &Repr) -> R<Vec<String>> {
        Ok(match &r.kind {
            Kind::Unit | Kind::Never => vec![],
            Kind::NullScalar(s, ScalarNull::Niche(_)) => vec![niche_carrier(*s).into()],
            Kind::NullScalar(s, ScalarNull::Present) => vec!["i32".into(), scalar(*s).into()],
            Kind::Array { elem, nullable, .. } if !self.transparent(elem) => {
                vec![nullable_ty(
                    format!("array<var {}>", self.elem_raw(elem)?),
                    *nullable,
                )]
            }
            Kind::Tuple { elems, nullable } => {
                let mut out = Vec::new();
                if *nullable {
                    out.push("i32".into());
                }
                for e in elems {
                    out.extend(self.raw(e)?);
                }
                out
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Index if !nullable => vec![self.surface(r)?],
                Shape::Index => vec!["i32".into()],
                Shape::Carrier(_) if !nullable => vec![self.name_of(&layout.path)?],
                Shape::Carrier(_) => vec!["i32".into(), self.name_of(&layout.path)?],
                Shape::Payload { slots } => {
                    let mut out = vec!["i32".to_string()];
                    for s in slots {
                        out.push(wire_type(s)?);
                    }
                    out
                }
            },
            Kind::Fn {
                provided, nullable, ..
            } => {
                let t = if provided.is_some() {
                    "anyref"
                } else {
                    "externref"
                };
                vec![nullable_ty(t.into(), *nullable)]
            }
            Kind::Handle { nullable, .. } => vec![nullable_ty("anyref".into(), *nullable)],
            Kind::Host {
                of: Host::Future { .. },
                nullable,
            } => vec![nullable_ty("externref".into(), *nullable)],
            _ => vec![self.surface(r)?],
        })
    }

    /// Whether an `arr<T>` element's Zena type is stored as its GC element.
    fn transparent(&self, elem: &Repr) -> bool {
        match &elem.kind {
            // Stored as the Zena scalar's own array would be (§11).
            Kind::Scalar(s) => !matches!(s, Scalar::Bool | Scalar::F16),
            Kind::Host {
                of: Host::Future { .. },
                ..
            } => false,
            Kind::Host { .. } | Kind::Vec { .. } => true,
            Kind::Enum { layout, nullable } => matches!(layout.shape, Shape::Index) && !nullable,
            Kind::Array { elem, .. } => self.transparent(elem),
            _ => false,
        }
    }

    /// The raw Zena element type of a converted `arr<T>`.
    fn elem_raw(&mut self, elem: &Repr) -> R<String> {
        match &elem.kind {
            Kind::Scalar(Scalar::Bool) => Ok("u8".into()),
            Kind::Scalar(Scalar::F16) => skip("`f16` elements of an `arr<T>`, held as their bits"),
            Kind::Scalar(_) => unreachable!("a transparent element"),
            Kind::Str { .. } => Ok("externref | null".into()),
            Kind::Enum {
                layout,
                nullable: false,
            } if i32_enum(layout) => Ok("i32".into()),
            // A struct of the arm and slots.
            Kind::Enum {
                layout,
                nullable: false,
            } => match &layout.shape {
                Shape::Payload { slots } => {
                    let mut fields = vec!["i32".to_string()];
                    for s in slots {
                        fields.push(wire_type(s)?);
                    }
                    Ok(format!("({}) | null", fields.join(", ")))
                }
                _ => skip(format!("`{}` elements of an `arr<T>`", elem.ty)),
            },
            // A struct of the elements' values, its reference nullable.
            Kind::Tuple {
                elems,
                nullable: false,
            } if elems.len() > 1 => {
                let mut fields = Vec::new();
                for e in elems {
                    fields.push(match &e.kind {
                        Kind::Str { .. } | Kind::Host { of: Host::Any, .. } => {
                            "externref | null".to_string()
                        }
                        Kind::Scalar(s) => scalar(*s).to_string(),
                        _ => return skip(format!("`{}` elements of an `arr<T>`", elem.ty)),
                    });
                }
                Ok(format!("({}) | null", fields.join(", ")))
            }
            _ => skip(format!("`{}` elements of an `arr<T>`", elem.ty)),
        }
    }

    /// The value a fresh `FixedArray` of `elem`s starts out filled with.
    fn elem_zero(&mut self, elem: &Repr) -> R<String> {
        Ok(match &elem.kind {
            Kind::Str {
                nullable: false, ..
            } => "''".into(),
            Kind::Scalar(s) => zero(*s).into(),
            Kind::Enum { layout, .. } => format!("(0 as {})", self.name_of(&layout.path)?),
            Kind::Tuple { elems, .. } => {
                let zs: R<Vec<String>> = elems.iter().map(|e| self.elem_zero(e)).collect();
                format!("({})", zs?.join(", "))
            }
            _ => "null".into(),
        })
    }

    /// The import arguments for `v`, a name of the surface type.
    fn encode(&mut self, r: &Repr, v: &str, pre: &mut Vec<String>) -> R<Vec<String>> {
        Ok(match &r.kind {
            Kind::Unit | Kind::Never => vec![],
            Kind::NullScalar(s, ScalarNull::Niche(n)) => {
                let inner = match s {
                    Scalar::Bool => format!("(if ({v}.value) 1 else 0)"),
                    _ => format!("({v}.value as {})", niche_carrier(*s)),
                };
                vec![format!(
                    "(if ({v} is Some<{}>) {inner} else {})",
                    scalar(*s),
                    niche_literal(*s, *n)
                )]
            }
            Kind::NullScalar(s, ScalarNull::Present) => vec![
                format!("(if ({v} is Some<{}>) 1 else 0)", scalar(*s)),
                format!(
                    "(if ({v} is Some<{}>) {v}.value else {})",
                    scalar(*s),
                    zero(*s)
                ),
            ],
            Kind::Array { elem, nullable, .. } if !self.transparent(elem) => {
                let f = self.array_out(elem)?;
                if *nullable {
                    vec![format!("(if ({v} == null) null else {f}({v}))")]
                } else {
                    vec![format!("{f}({v})")]
                }
            }
            Kind::Tuple { elems, nullable } => {
                let mut out = Vec::new();
                let t = self.fresh();
                pre.push(format!("let {t} = {v};"));
                if *nullable {
                    out.push(format!("(if ({t} == null) 0 else 1)"));
                    if elems.iter().any(|e| !self.direct(e)) {
                        return skip("a nullable tuple of converted elements");
                    }
                }
                for (i, e) in elems.iter().enumerate() {
                    let ev = self.fresh();
                    if *nullable {
                        let surface = self.surface(e)?;
                        let zeros = self.zeros(e)?;
                        pre.push(format!(
                            "let {ev}: {surface} = if ({t} == null) {} else {};",
                            zeros,
                            tuple_at(&t, i, elems.len())
                        ));
                    } else {
                        pre.push(format!("let {ev} = {};", tuple_at(&t, i, elems.len())));
                    }
                    out.extend(self.encode(e, &ev, pre)?);
                }
                out
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Index | Shape::Carrier(_) if !nullable => vec![v.into()],
                Shape::Index => {
                    let name = self.name_of(&layout.path)?;
                    vec![format!(
                        "(if ({v} is Some<{name}>) ({v}.value as i32) else 0)"
                    )]
                }
                Shape::Carrier(_) => {
                    let name = self.name_of(&layout.path)?;
                    vec![
                        format!("(if ({v} is Some<{name}>) 1 else 0)"),
                        format!("(if ({v} is Some<{name}>) {v}.value else (0 as {name}))"),
                    ]
                }
                Shape::Payload { slots } => {
                    let f = self.enum_encoder(layout)?;
                    let mut names = Vec::new();
                    for _ in 0..=slots.len() {
                        names.push(self.fresh());
                    }
                    pre.push(format!("let ({}) = {f}({v});", names.join(", ")));
                    names
                }
            },
            // The language provides the function as its handle, which JS
            // calls through the trampoline.
            Kind::Fn {
                provided: Some(_),
                nullable,
                ..
            } => {
                self.trampoline(r)?;
                if *nullable {
                    vec![format!("(if ({v} == null) null else ({v} as anyref))")]
                } else {
                    vec![format!("({v} as anyref)")]
                }
            }
            Kind::Fn { provided: None, .. } => {
                return skip("a JS function the language passes back")
            }
            // An exported class's instance is its handle (§14).
            Kind::Handle { nullable: true, .. } => {
                vec![format!("(if ({v} == null) null else ({v} as anyref))")]
            }
            Kind::Handle { .. } => vec![format!("({v} as anyref)")],
            Kind::Host {
                of: Host::Future { value, error },
                nullable,
            } => {
                let t = self.future_value(value)?;
                let (_, promise) = self.future_fns(error)?;
                if *nullable {
                    vec![format!("(if ({v} == null) null else {promise}<{t}>({v}))")]
                } else {
                    vec![format!("{promise}<{t}>({v})")]
                }
            }
            _ => vec![v.into()],
        })
    }

    /// Whether a value crosses as itself.
    fn direct(&mut self, r: &Repr) -> bool {
        match &r.kind {
            Kind::Scalar(_) | Kind::Str { .. } | Kind::Vec { .. } | Kind::Host { .. } => true,
            Kind::Array { elem, .. } => self.transparent(elem),
            Kind::Enum { layout, nullable } => {
                matches!(layout.shape, Shape::Index | Shape::Carrier(_)) && !nullable
            }
            _ => false,
        }
    }

    /// A value of `r`'s surface type for an absent tuple's elements.
    fn zeros(&mut self, r: &Repr) -> R<String> {
        Ok(match &r.kind {
            Kind::Scalar(s) => zero(*s).into(),
            Kind::Str { nullable: true, .. } | Kind::Host { .. } => "null".into(),
            _ => return skip("a nullable tuple of non-nullable references"),
        })
    }

    /// The surface value over the import's results `raws`.
    fn decode(&mut self, r: &Repr, raws: &[String]) -> R<String> {
        Ok(match &r.kind {
            Kind::Unit | Kind::Never => String::new(),
            Kind::NullScalar(s, ScalarNull::Niche(n)) => {
                let x = &raws[0];
                let value = match s {
                    Scalar::Bool => format!("{x} != 0"),
                    _ => format!("{x} as {}", scalar(*s)),
                };
                format!(
                    "(if ({x} == {}) none else some<{}>({value}))",
                    niche_literal(*s, *n),
                    scalar(*s)
                )
            }
            Kind::NullScalar(s, ScalarNull::Present) => format!(
                "(if ({} != 0) some<{}>({}) else none)",
                raws[0],
                scalar(*s),
                raws[1]
            ),
            Kind::Array { elem, nullable, .. } if !self.transparent(elem) => {
                let f = self.array_in(elem)?;
                let x = &raws[0];
                if *nullable {
                    format!(
                        "(if ({x} == null) null else {f}({x} as array<var {}>))",
                        self.elem_raw(elem)?
                    )
                } else {
                    format!("{f}({x})")
                }
            }
            Kind::Tuple { elems, nullable } => {
                let mut at = 0;
                let mut parts = Vec::new();
                let start = usize::from(*nullable);
                at += start;
                for e in elems {
                    let n = self.raw(e)?.len();
                    parts.push(self.decode(e, &raws[at..at + n])?);
                    at += n;
                }
                let t = tuple_type(&parts);
                if *nullable {
                    format!("(if ({} == 0) null else {t})", raws[0])
                } else {
                    t
                }
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Index | Shape::Carrier(_) if !nullable => raws[0].clone(),
                Shape::Index => {
                    let name = self.name_of(&layout.path)?;
                    format!(
                        "(if ({x} == 0) none else some<{name}>({x} as {name}))",
                        x = raws[0]
                    )
                }
                Shape::Carrier(_) => {
                    let name = self.name_of(&layout.path)?;
                    format!(
                        "(if ({} != 0) some<{name}>({}) else none)",
                        raws[0], raws[1]
                    )
                }
                Shape::Payload { .. } => {
                    let f = self.enum_decoder(layout)?;
                    let call = format!("{f}({})", raws.join(", "));
                    match layout.origin {
                        Origin::Nested => call,
                        _ if *nullable => call,
                        _ => format!("({call} as {})", self.name_of(&layout.path)?),
                    }
                }
            },
            // A JS function, which the language calls through
            // `("tw:call", F)`.
            Kind::Fn {
                provided: None,
                nullable,
                ..
            } => {
                let wrap = self.fn_wrapper(r)?;
                let x = &raws[0];
                if *nullable {
                    format!("(if ({x} == null) null else {wrap}({x} as externref))")
                } else {
                    format!("{wrap}({x})")
                }
            }
            Kind::Fn { .. } => return skip("a language function JS passes back"),
            Kind::Host {
                of: Host::Future { value, error },
                nullable,
            } => self.future_in(value, error, &raws[0], *nullable)?,
            Kind::Handle { nullable, .. } => {
                let t = self.surface(r)?;
                let x = &raws[0];
                if *nullable {
                    let t = t.trim_end_matches(" | null");
                    format!("(if ({x} == null) null else ({x} as {t}))")
                } else {
                    format!("({x} as {t})")
                }
            }
            _ => raws[0].clone(),
        })
    }

    fn uses_async(&mut self) {
        self.uses_promise = true;
        for n in ["__tw_await", "__tw_promise"] {
            self.used.insert(n.into());
        }
    }

    /// The Zena type of a future's value, which `take<T>` and
    /// `resolve<T>` carry as itself.
    fn future_value(&mut self, value: &Repr) -> R<String> {
        // A `future<never>` never resolves (the bridge rejects it), so it
        // waits as a `Future<void>`, through `take<unit>`.
        if value.kind == Kind::Never {
            return Ok("void".into());
        }
        let as_itself = match &value.kind {
            Kind::Array { .. } | Kind::Vec { .. } => false,
            Kind::Unit | Kind::NullScalar(..) | Kind::Str { .. } | Kind::Host { .. } => true,
            _ => self.direct(value),
        };
        if !as_itself {
            return skip(format!("a future of `{}`, which is converted", value.ty));
        }
        self.surface(value)
    }

    /// The Zena future over a JS future `x`.
    fn future_in(
        &mut self,
        value: &Repr,
        error: &EnumLayout,
        x: &str,
        nullable: bool,
    ) -> R<String> {
        let t = self.future_value(value)?;
        let (wait, _) = self.future_fns(error)?;
        Ok(if nullable {
            format!("(if ({x} == null) null else {wait}<{t}>({x} as externref))")
        } else {
            format!("{wait}<{t}>({x} as externref)")
        })
    }

    fn future_fns_of(
        &mut self,
        error: &ts_gen_abi::ir::Path,
        cx: &Context<'_>,
        flow: Flow,
    ) -> R<(String, String)> {
        let layout = self
            .layouts
            .enum_of(error, cx, flow)
            .or_else(|e| skip(e.to_string()))?;
        self.future_fns(&layout)
    }

    /// The functions over a future rejecting with `error`: the Zena future
    /// over a JS one, and the JS future over a Zena one. A specific error
    /// enum fails a Zena future with its tag class (§15).
    fn future_fns(&mut self, error: &EnumLayout) -> R<(String, String)> {
        self.uses_async();
        if let Origin::Err = error.origin {
            return Ok(("__tw_await".into(), "__tw_promise".into()));
        }
        let key = format!("futurefns {}", error.path);
        if let Some(n) = self.names.get(&key) {
            let (a, p) = n.split_once(' ').unwrap();
            return Ok((a.into(), p.into()));
        }
        if !error
            .arms
            .iter()
            .all(|a| a.payload.len() == 1 && matches!(a.payload[0].ty.as_str(), "str" | "Str"))
        {
            return skip(format!(
                "a future rejecting with `{}`, whose arms aren't `(str)`",
                error.path
            ));
        }
        let class = self.tag_class(error)?;
        let fallback = self.names[&format!("tagfallback {}", error.path)].clone();
        let arm = fallback.split('.').next().unwrap_or_default().to_string();
        let id = type_ident(&error.path);
        let error_fn = unique(&mut self.used, format!("__tw_future_error_{id}"));
        let reject_fn = unique(&mut self.used, format!("__tw_future_reject_{id}"));
        let fail = unique(&mut self.used, format!("__tw_{id}_fail"));
        let reject = unique(&mut self.used, format!("__tw_{id}_reject"));
        let wait = unique(&mut self.used, format!("__tw_await_{id}"));
        let promise = unique(&mut self.used, format!("__tw_promise_{id}"));
        let e = zena_string(&format!("error<{}>", error.path));
        let r = zena_string(&format!("reject<{}>", error.path));
        let _ = writeln!(
            self.helpers,
            "@external(\"tw:future\", {e}, \"tw\")\ndeclare function {error_fn}(future: externref): inline (i32, String);\n\n\
             @external(\"tw:future\", {r}, \"tw\")\ndeclare function {reject_fn}(resolver: externref, arm: i32, message: String): void;\n\n\
             function {fail}(future: externref): Error {{\n  let (arm, message) = {error_fn}(future);\n  return new {class}(arm as {arm}, message);\n}}\n\n\
             function {reject}(resolver: externref, e: Error): void {{\n  if (e is {class}) {{\n    {reject_fn}(resolver, e.arm as i32, e.message);\n  }} else {{\n    {reject_fn}(resolver, {fallback} as i32, e.message);\n  }}\n}}\n\n\
             export function {wait}<T>(future: externref): Future<T> {{\n  return __tw_await_with<T>(future, {fail});\n}}\n\n\
             export function {promise}<T>(future: Future<T>): externref {{\n  return __tw_promise_with<T>(future, {reject});\n}}\n"
        );
        self.names.insert(key, format!("{wait} {promise}"));
        Ok((wait, promise))
    }

    /// A function type's Zena type.
    fn fn_surface(&mut self, sig: &FnLayout) -> R<String> {
        let mut ps = Vec::new();
        for (i, p) in sig.params.iter().enumerate() {
            ps.push(format!("a{i}: {}", self.surface(p)?));
        }
        let r = self.surface(&sig.result)?;
        Ok(format!("({}) => {r}", ps.join(", ")))
    }

    /// The name protocol operations give a function type: its spelling,
    /// less any nullability.
    fn fn_name(r: &Repr) -> String {
        match &r.kind {
            Kind::Fn { nullable: true, .. } => {
                let t = r.ty.strip_suffix('?').unwrap_or(&r.ty);
                t.strip_prefix('(')
                    .and_then(|t| t.strip_suffix(')'))
                    .unwrap_or(t)
                    .to_string()
            }
            _ => r.ty.clone(),
        }
    }

    /// The trampoline of a function type over type parameters, generic in
    /// those it mentions: its export name is templated, so it exports at
    /// each instantiation (`tw:call:fn(Str)->{U}`). A parameter or result
    /// is a type parameter itself or mentions none.
    fn generic_trampoline(&mut self, fun: &FnType, cx: &Context<'_>) -> R<GenericTrampoline> {
        if fun.rest {
            return skip("a generic callback with a rest parameter");
        }
        let whole = Type::Fn(Box::new(fun.clone()));
        let tps: Vec<&str> = cx
            .tparams
            .iter()
            .copied()
            .filter(|t| {
                let mut found = false;
                whole.walk(&mut |x| {
                    found |= matches!(x, Type::Named(r) if r.args.is_empty() && r.path.segments.len() == 1 && r.path.segments[0] == *t);
                });
                found
            })
            .collect();
        let hole = |t: &str| format!("__tw_hole_{t}");
        let holed = whole.map_named(&|r: &TypeRef| match &r.path.segments[..] {
            [n] if r.args.is_empty() && tps.contains(&n.as_str()) => Some(Type::Named(TypeRef {
                path: Path::new(vec![hole(n)]),
                args: Vec::new(),
            })),
            _ => None,
        });
        let mut key = brace_escape(&self.layouts.keys.ty(&holed, cx));
        for t in &tps {
            key = key.replace(&hole(t), &format!("{{{t}}}"));
        }
        if let Some(t) = self.generic_trampolines.get(&key) {
            return Ok(t.clone());
        }
        let bare = |t: &Type| match t {
            Type::Named(r) if r.args.is_empty() && is_tparam(&r.path, cx) => {
                Some(ident(&r.path.segments[0]))
            }
            _ => None,
        };
        let mut params = vec!["env: anyref".to_string()];
        let mut names = vec!["env".to_string()];
        let mut surfaces = Vec::new();
        let mut body = String::new();
        let mut args = Vec::new();
        for (i, p) in fun.params.iter().enumerate() {
            if let Some(t) = bare(p) {
                params.push(format!("p{i}_0: {t}"));
                names.push(format!("p{i}_0"));
                let _ = writeln!(body, "  let a{i}: {t} = p{i}_0;");
                surfaces.push(format!("a{i}: {t}"));
            } else if mentions_tparam(p, cx) {
                // A host type over type parameters crosses as itself.
                let t = self.generic_surface(p, cx)?;
                if t.contains("Future<") {
                    return skip("a future inside a generic callback's parameter");
                }
                params.push(format!("p{i}_0: {t}"));
                names.push(format!("p{i}_0"));
                let _ = writeln!(body, "  let a{i}: {t} = p{i}_0;");
                surfaces.push(format!("a{i}: {t}"));
            } else {
                let repr = self.repr(p, cx, Flow::ToLang)?;
                let raw = self.raw(&repr)?;
                let raws: Vec<String> = (0..raw.len()).map(|j| format!("p{i}_{j}")).collect();
                for (n, t) in raws.iter().zip(&raw) {
                    params.push(format!("{n}: {t}"));
                    names.push(n.clone());
                }
                let surface = self.surface(&repr)?;
                let _ = writeln!(
                    body,
                    "  let a{i}: {surface} = {};",
                    self.decode(&repr, &raws)?
                );
                surfaces.push(format!("a{i}: {surface}"));
            }
            args.push(format!("a{i}"));
        }
        let call = format!("f({})", args.join(", "));
        let (result, result_surface) = if fun.result == Type::Unit {
            let _ = writeln!(body, "  {call};\n  __tw_entered();");
            ("void".to_string(), "void".to_string())
        } else if let Some(t) = bare(&fun.result) {
            let _ = writeln!(
                body,
                "  let result: {t} = {call};\n  __tw_entered();\n  return result;"
            );
            (t.clone(), t)
        } else if mentions_tparam(&fun.result, cx) {
            let t = self.generic_surface(&fun.result, cx)?;
            if t.contains("Future<") {
                return skip("a future inside a generic callback's result");
            }
            let _ = writeln!(
                body,
                "  let result: {t} = {call};\n  __tw_entered();\n  return result;"
            );
            (t.clone(), t)
        } else {
            let repr = self.repr(&fun.result, cx, Flow::ToJs)?;
            let raw = self.raw(&repr)?;
            let surface = self.surface(&repr)?;
            let _ = writeln!(body, "  let result: {surface} = {call};\n  __tw_entered();");
            let mut pre = Vec::new();
            let enc = self.encode(&repr, "result", &mut pre)?;
            for st in pre {
                let _ = writeln!(body, "  {st}");
            }
            let _ = match &enc[..] {
                [one] => writeln!(body, "  return {one};"),
                _ => writeln!(body, "  return ({});", enc.join(", ")),
            };
            let raw = match &raw[..] {
                [one] => one.clone(),
                _ => format!("inline ({})", raw.join(", ")),
            };
            (raw, surface)
        };
        let ft = format!("({}) => {result_surface}", surfaces.join(", "));
        let body = format!("  let f = env as {ft};\n{body}");
        let (option, body) = match &fun.throws {
            Some(path) => {
                let layout = self
                    .layouts
                    .enum_of(path, cx, Flow::ToJs)
                    .or_else(|e| skip(e.to_string()))?;
                (
                    format!("tw, throws:{}", layout.path),
                    self.rethrowing(&layout, &body)?,
                )
            }
            None => ("tw".to_string(), body),
        };
        let name = unique(&mut self.used, "__tw_trampoline".into());
        self.helper_done.insert(format!("tramp {key}"));
        let targs: Vec<String> = tps.iter().map(|t| ident(t)).collect();
        let targs = format!("<{}>", targs.join(", "));
        let _ = writeln!(
            self.helpers,
            "@export({}, {})\nexport function {name}{targs}({}): {result} {{\n{body}}}\n",
            zena_string(&ts_gen_abi::protocol::call_export(&key)),
            zena_string(&option),
            params.join(", ")
        );
        let t = GenericTrampoline {
            surface: ft,
            keep: format!(
                "if (false) {{ let keep = ({}): {result} => {name}{targs}({}); }}",
                params.join(", "),
                names.join(", ")
            ),
        };
        self.generic_trampolines.insert(key, t.clone());
        Ok(t)
    }

    /// The export `tw:call:F` JS calls a language function of type `F`
    /// through, its handle the function itself (§18).
    fn trampoline(&mut self, r: &Repr) -> R<()> {
        let f = Self::fn_name(r);
        if !self.helper_done.insert(format!("tramp {f}")) {
            return Ok(());
        }
        let Kind::Fn { sig: layout, .. } = &r.kind else {
            unreachable!()
        };
        let ft = self.fn_surface(layout)?;
        let sig = self.layouts.fn_op(r).map_err(|e| Skip(e.to_string()))?;
        let mut params = vec!["env: anyref".to_string()];
        let mut body = format!(
            "  let f = env as {ft};
"
        );
        let mut args = Vec::new();
        for p in &sig.params {
            let Pos::Param(i) = p.role else { continue };
            let raw = self.raw(&p.repr)?;
            let raws: Vec<String> = (0..raw.len()).map(|j| format!("p{i}_{j}")).collect();
            for (n, t) in raws.iter().zip(&raw) {
                params.push(format!("{n}: {t}"));
            }
            let surface = self.surface(&p.repr)?;
            let _ = writeln!(
                body,
                "  let a{i}: {surface} = {};",
                self.decode(&p.repr, &raws)?
            );
            args.push(format!("a{i}"));
        }
        let call = format!("f({})", args.join(", "));
        let result = match sig.results.first() {
            None => {
                let _ = writeln!(body, "  {call};\n  __tw_entered();");
                "void".to_string()
            }
            Some(p) if matches!(p.repr.kind, Kind::Never) => {
                let _ = writeln!(body, "  {call};");
                "never".to_string()
            }
            Some(p) => {
                let raw = self.raw(&p.repr)?;
                let surface = self.surface(&p.repr)?;
                let _ = writeln!(body, "  let result: {surface} = {call};\n  __tw_entered();");
                let mut pre = Vec::new();
                let enc = self.encode(&p.repr, "result", &mut pre)?;
                for st in pre {
                    let _ = writeln!(body, "  {st}");
                }
                let _ = match &enc[..] {
                    [one] => writeln!(body, "  return {one};"),
                    _ => writeln!(body, "  return ({});", enc.join(", ")),
                };
                match &raw[..] {
                    [one] => one.clone(),
                    _ => format!("inline ({})", raw.join(", ")),
                }
            }
        };
        let (option, body) = match &sig.throws {
            Some(layout) => (
                format!("tw, throws:{}", layout.path),
                self.rethrowing(layout, &body)?,
            ),
            None => ("tw".to_string(), body),
        };
        let name = unique(&mut self.used, "__tw_trampoline".into());
        let _ = writeln!(
            self.helpers,
            "@export({}, {})\nexport function {name}({}): {result} {{\n{body}}}\n",
            zena_string(&brace_escape(&ts_gen_abi::protocol::call_export(&f))),
            zena_string(&option),
            params.join(", ")
        );
        Ok(())
    }

    /// A function wrapping a JS function of type `F` as a Zena function,
    /// which calls it through `("tw:call", F)` (§18).
    fn fn_wrapper(&mut self, r: &Repr) -> R<String> {
        let f = Self::fn_name(r);
        let key = format!("fnwrap {f}");
        if let Some(n) = self.names.get(&key) {
            return Ok(n.clone());
        }
        let Kind::Fn { sig: layout, .. } = &r.kind else {
            unreachable!()
        };
        let ft = self.fn_surface(layout)?;
        let sig = self.layouts.fn_op(r).map_err(|e| Skip(e.to_string()))?;
        let mut raw_params = vec!["p0: externref".to_string()];
        let mut params = Vec::new();
        let mut pre = Vec::new();
        let mut args = vec!["f".to_string()];
        for p in &sig.params {
            let Pos::Param(i) = p.role else { continue };
            for t in self.raw(&p.repr)? {
                raw_params.push(format!("p{}: {t}", raw_params.len()));
            }
            params.push(format!("a{i}: {}", self.surface(&p.repr)?));
            args.extend(self.encode(&p.repr, &format!("a{i}"), &mut pre)?);
        }
        let (raw_result, result, decode) = match sig.results.first() {
            None => ("void".to_string(), "void".to_string(), None),
            Some(p) => {
                let raw = self.raw(&p.repr)?;
                let raws: Vec<String> = (0..raw.len()).map(|i| format!("r{i}")).collect();
                let raw_result = match &raw[..] {
                    [one] => one.clone(),
                    _ => format!("inline ({})", raw.join(", ")),
                };
                (
                    raw_result,
                    self.surface(&p.repr)?,
                    Some((raws.clone(), self.decode(&p.repr, &raws)?)),
                )
            }
        };
        let option = match &sig.throws {
            Some(layout) => {
                self.tag_class(layout)?;
                format!("tw, throws:{}", layout.path)
            }
            None => "tw".to_string(),
        };
        let import = unique(&mut self.used, "__tw_call".into());
        let _ = writeln!(
            self.externs,
            "@external(\"tw:call\", {}, {})\ndeclare function {import}({}): {raw_result};\n",
            zena_string(&brace_escape(&f)),
            zena_string(&option),
            raw_params.join(", ")
        );
        let name = unique(&mut self.used, "__tw_fn".into());
        self.names.insert(key, name.clone());
        let invoke = format!("{import}({})", args.join(", "));
        let mut body = String::new();
        for st in pre {
            let _ = writeln!(body, "    {st}");
        }
        match decode {
            None => {
                let _ = writeln!(body, "    {invoke};");
            }
            Some((raws, d)) => {
                if raws.len() == 1 {
                    let _ = writeln!(body, "    let r0 = {invoke};");
                } else {
                    let _ = writeln!(body, "    let ({}) = {invoke};", raws.join(", "));
                }
                let _ = writeln!(body, "    return {d};");
            }
        }
        let _ = writeln!(
            self.helpers,
            "export function {name}(f: externref): {ft} {{\n  return ({}): {result} => {{\n{body}  }};\n}}\n",
            params.join(", "),
        );
        Ok(name)
    }

    /// The surface value of one payload value over its slot values, which
    /// are typed by their slot's wire type rather than the value's own.
    fn slot_value(&mut self, p: &Repr, vals: &[String]) -> R<String> {
        Ok(match &p.kind {
            Kind::Tuple {
                elems,
                nullable: false,
            } => {
                let mut parts = Vec::new();
                let mut at = 0;
                for e in elems {
                    let n = self.layouts.wire(e).map_err(|e| Skip(e.to_string()))?.len();
                    parts.push(self.slot_value(e, &vals[at..at + n])?);
                    at += n;
                }
                tuple_type(&parts)
            }
            Kind::Scalar(Scalar::Bool) => format!("{} != 0", vals[0]),
            Kind::Scalar(s) => {
                if matches!(layout::carrier_of(*s), ValType::I32) && *s != Scalar::I32 {
                    format!("{} as {}", vals[0], scalar(*s))
                } else {
                    vals[0].clone()
                }
            }
            Kind::Str {
                nullable: false, ..
            } => format!("String.fromHandle({} as externref)", vals[0]),
            Kind::Str { nullable: true, .. } => format!(
                "(if ({x} == null) null else String.fromHandle({x} as externref))",
                x = vals[0]
            ),
            Kind::Host {
                of: Host::Future { value, error },
                nullable,
            } => self.future_in(value, error, &vals[0], *nullable)?,
            Kind::Host { nullable, .. } => {
                let t = self.surface(&Repr {
                    ty: p.ty.clone(),
                    kind: match &p.kind {
                        Kind::Host { of, .. } => Kind::Host {
                            of: of.clone(),
                            nullable: false,
                        },
                        _ => unreachable!(),
                    },
                    js_null: false,
                })?;
                if *nullable {
                    format!("(if ({x} == null) null else {x} as {t})", x = vals[0])
                } else {
                    format!("({} as {t})", vals[0])
                }
            }
            Kind::Vec { elem, nullable, .. } => {
                let t = format!("FixedArray<{}>", vec_elem(*elem));
                if *nullable {
                    format!("({} as {t} | null)", vals[0])
                } else {
                    format!("({} as {t})", vals[0])
                }
            }
            Kind::Array { elem, nullable, .. } if !self.transparent(elem) => {
                let f = self.array_in(elem)?;
                let t = format!("array<var {}>", self.elem_raw(elem)?);
                if *nullable {
                    format!("(if ({x} == null) null else {f}({x} as {t}))", x = vals[0])
                } else {
                    format!("{f}({} as {t})", vals[0])
                }
            }
            Kind::Enum { layout, nullable } if matches!(layout.shape, Shape::Index) => {
                let name = self.name_of(&layout.path)?;
                if *nullable {
                    format!(
                        "(if ({x} == 0) none else some<{name}>({x} as {name}))",
                        x = vals[0]
                    )
                } else {
                    format!("({} as {name})", vals[0])
                }
            }
            _ => return skip(format!("`{}` payloads", p.ty)),
        })
    }

    /// The slot values of one payload value `v`.
    fn slot_values(&mut self, p: &Repr, v: &str) -> R<Vec<String>> {
        Ok(match &p.kind {
            Kind::Scalar(Scalar::Bool) => vec![format!("(if ({v}) 1 else 0)")],
            Kind::Scalar(s) => {
                if matches!(layout::carrier_of(*s), ValType::I32) && *s != Scalar::I32 {
                    vec![format!("({v} as i32)")]
                } else {
                    vec![v.into()]
                }
            }
            Kind::Str {
                nullable: false, ..
            } => vec![format!("{v}.handle")],
            Kind::Str { nullable: true, .. } => {
                vec![format!("(if ({v} == null) null else {v}.handle)")]
            }
            Kind::Host {
                of: Host::Future { value, error },
                nullable,
            } => {
                let t = self.future_value(value)?;
                let (_, promise) = self.future_fns(error)?;
                if *nullable {
                    vec![format!("(if ({v} == null) null else {promise}<{t}>({v}))")]
                } else {
                    vec![format!("{promise}<{t}>({v})")]
                }
            }
            Kind::Host { of: Host::Any, .. } => vec![v.into()],
            Kind::Host { .. } => vec![format!("({v} as externref)")],
            Kind::Vec { elem, .. } => vec![format!("({v} as array<var {}>)", vec_elem(*elem))],
            // A tuple's slots are its elements'.
            Kind::Tuple {
                elems,
                nullable: false,
            } => {
                let mut out = Vec::new();
                for (i, e) in elems.iter().enumerate() {
                    out.extend(self.slot_values(e, &tuple_at(v, i, elems.len()))?);
                }
                out
            }
            Kind::Array { elem, nullable, .. } if !self.transparent(elem) => {
                let f = self.array_out(elem)?;
                if *nullable {
                    vec![format!("(if ({v} == null) null else {f}({v}))")]
                } else {
                    vec![format!("{f}({v})")]
                }
            }
            Kind::Enum { layout, nullable } if matches!(layout.shape, Shape::Index) => {
                let name = self.name_of(&layout.path)?;
                if *nullable {
                    vec![format!(
                        "(if ({v} is Some<{name}>) ({v}.value as i32) else 0)"
                    )]
                } else {
                    vec![format!("({v} as i32)")]
                }
            }
            _ => return skip(format!("`{}` payloads", p.ty)),
        })
    }

    /// `__<Enum>_decode(arm, slots…)`: the enum value over its wire, null
    /// for arm 0.
    fn enum_decoder(&mut self, layout: &EnumLayout) -> R<String> {
        let (fname, surface) = match layout.origin {
            Origin::Nested => {
                let inner = self.surface(&layout.arms[1].payload[0])?;
                (
                    format!("__nested_{}_decode", type_ident(&layout.path)),
                    format!("Option<{inner}>"),
                )
            }
            Origin::Err => return skip("`err` values"),
            Origin::Decl(_) => {
                let name = self.name_of(&layout.path)?;
                (format!("__{name}_decode"), format!("{name} | null"))
            }
        };
        if self.helper_done.contains(&fname) {
            return Ok(fname);
        }
        let Shape::Payload { slots } = &layout.shape else {
            unreachable!()
        };
        let mut params = vec!["arm: i32".to_string()];
        for (i, s) in slots.iter().enumerate() {
            params.push(format!("s{i}: {}", wire_type(s)?));
        }
        let mut body = String::new();
        let enum_name = match layout.origin {
            Origin::Decl(_) => Some(self.name_of(&layout.path)?),
            _ => None,
        };
        let mut fallback = None;
        for a in &layout.arms {
            let mut vals = Vec::new();
            let mut at = 0;
            for p in &a.payload {
                let n = self.layouts.wire(p).map_err(|e| Skip(e.to_string()))?.len();
                let pv: Vec<String> = a.slots[at..at + n]
                    .iter()
                    .map(|s| format!("s{s}"))
                    .collect();
                vals.push(self.slot_value(p, &pv)?);
                at += n;
            }
            let value = match &enum_name {
                None if a.payload.is_empty() => "none".to_string(),
                None => format!("some<{}>({})", self.surface(&a.payload[0])?, vals[0]),
                Some(e) => {
                    let case = self.arm_class(e, &a.name);
                    format!("new {case}({})", vals.join(", "))
                }
            };
            if a.fallback {
                fallback = Some(value);
                continue;
            }
            let _ = writeln!(body, "  if (arm == {}) {{ return {value}; }}", a.index);
        }
        let last = match (fallback, &layout.origin) {
            (Some(v), _) => v,
            (None, Origin::Nested) => "none".into(),
            (None, _) => "null".into(),
        };
        let _ = writeln!(body, "  return {last};");
        self.helper_done.insert(fname.clone());
        let _ = writeln!(
            self.helpers,
            "export function {fname}({}): {surface} {{\n{body}}}\n",
            params.join(", ")
        );
        Ok(fname)
    }

    /// `__<Enum>_encode(v)`: the arm and slots of an enum value, arm 0 for
    /// null.
    fn enum_encoder(&mut self, layout: &EnumLayout) -> R<String> {
        let Shape::Payload { slots } = &layout.shape else {
            unreachable!()
        };
        let (fname, surface) = match layout.origin {
            Origin::Nested => {
                let inner = self.surface(&layout.arms[1].payload[0])?;
                (
                    format!("__nested_{}_encode", type_ident(&layout.path)),
                    format!("Option<{inner}>"),
                )
            }
            Origin::Err => return skip("`err` values"),
            Origin::Decl(_) => {
                let name = self.name_of(&layout.path)?;
                (format!("__{name}_encode"), format!("{name} | null"))
            }
        };
        if self.helper_done.contains(&fname) {
            return Ok(fname);
        }
        let mut wires = vec!["i32".to_string()];
        for s in slots {
            wires.push(wire_type(s)?);
        }
        let zeros: Vec<&str> = slots.iter().map(slot_zero).collect();
        let tuple = |arm: i64, vals: &[(usize, String)]| {
            let mut all: Vec<String> = zeros.iter().map(|z| z.to_string()).collect();
            for (s, v) in vals {
                all[*s] = v.clone();
            }
            format!("({}, {})", arm, all.join(", "))
        };
        let mut body = String::new();
        let enum_name = match layout.origin {
            Origin::Decl(_) => Some(self.name_of(&layout.path)?),
            _ => None,
        };
        if enum_name.is_some() {
            let _ = writeln!(body, "  if (v == null) {{ return {}; }}", tuple(0, &[]));
        }
        for a in &layout.arms {
            let mut vals = Vec::new();
            let mut at = 0;
            let (test, bind) = match &enum_name {
                None if a.payload.is_empty() => ("v is None".to_string(), None),
                None => {
                    let inner = self.surface(&a.payload[0])?;
                    (format!("v is Some<{inner}>"), Some("v.value".to_string()))
                }
                Some(e) => {
                    let case = self.arm_class(e, &a.name);
                    (format!("v is {case}"), Some(format!("(v as {case})")))
                }
            };
            for (i, p) in a.payload.iter().enumerate() {
                let n = self.layouts.wire(p).map_err(|e| Skip(e.to_string()))?.len();
                let pv = match (&enum_name, &bind) {
                    (None, Some(b)) => b.clone(),
                    (Some(_), Some(b)) => format!("{b}.{}", field_name(i, a.payload.len())),
                    _ => unreachable!(),
                };
                let encoded = self.slot_values(p, &pv)?;
                for (j, e) in encoded.into_iter().enumerate() {
                    vals.push((a.slots[at + j], e));
                }
                at += n;
            }
            let _ = writeln!(
                body,
                "  if ({test}) {{ return {}; }}",
                tuple(a.index, &vals)
            );
        }
        let _ = writeln!(body, "  return {};", tuple(0, &[]));
        self.helper_done.insert(fname.clone());
        let _ = writeln!(
            self.helpers,
            "export function {fname}(v: {surface}): inline ({}) {{\n{body}}}\n",
            wires.join(", ")
        );
        Ok(fname)
    }

    /// `arr<str>` and narrow-scalar `arr<T>` out of the bridge: an element-wise
    /// copy.
    fn array_in(&mut self, elem: &Repr) -> R<String> {
        let raw = self.elem_raw(elem)?;
        let surface = self.surface(elem)?;
        let fname = format!("__{}_array_in", type_ident(&elem.ty));
        // A payload enum has no value to fill a fresh array with, so its
        // elements map into one.
        if let Kind::Enum { layout, .. } = &elem.kind {
            if !i32_enum(layout) && !self.helper_done.contains(&fname) {
                let f = self.enum_decoder(layout)?;
                let fields = raw.strip_suffix(" | null").unwrap_or(&raw).to_string();
                let Shape::Payload { slots } = &layout.shape else {
                    unreachable!()
                };
                let n = slots.len() + 1;
                let vs: Vec<String> = (0..n).map(|i| format!("t[{i}]")).collect();
                self.helper_done.insert(fname.clone());
                let _ = writeln!(
                    self.helpers,
                    "export function {fname}(a: array<var {raw}>): FixedArray<{surface}> {{\n  \
                     return (a as FixedArray<{raw}>).map<{surface}>((e: {raw}, i: i32, s: FixedArray<{raw}>): {surface} => {{\n    \
                     let t = e as {fields};\n    \
                     return {f}({}) as {surface};\n  }});\n}}\n",
                    vs.join(", ")
                );
            }
            if !i32_enum(layout) {
                return Ok(fname);
            }
        }
        if !self.helper_done.contains(&fname) {
            // The value's statements, if any, and the value.
            let (init, pre, conv) = match &elem.kind {
                Kind::Str {
                    nullable: false, ..
                } => (
                    "''".to_string(),
                    String::new(),
                    "String.fromHandle(e as externref)".to_string(),
                ),
                Kind::Str { nullable: true, .. } => (
                    "null".to_string(),
                    String::new(),
                    "if (e == null) null else String.fromHandle(e as externref)".to_string(),
                ),
                Kind::Scalar(Scalar::Bool) => ("false".into(), String::new(), "e != 0".into()),
                Kind::Scalar(s) => (
                    zero(*s).into(),
                    String::new(),
                    format!("e as {}", scalar(*s)),
                ),
                Kind::Enum { layout, .. } => {
                    let name = self.name_of(&layout.path)?;
                    (
                        format!("(0 as {name})"),
                        String::new(),
                        format!("e as {name}"),
                    )
                }
                Kind::Tuple { elems, .. } => {
                    let fields = raw.strip_suffix(" | null").unwrap_or(&raw).to_string();
                    let mut parts = Vec::new();
                    for (i, e) in elems.iter().enumerate() {
                        parts.push(self.slot_value(e, &[format!("t[{i}]")])?);
                    }
                    (
                        self.elem_zero(elem)?,
                        format!("let t = e as {fields};\n    "),
                        format!("({})", parts.join(", ")),
                    )
                }
                _ => return skip(format!("`{}` elements of an `arr<T>`", elem.ty)),
            };
            self.helper_done.insert(fname.clone());
            let _ = writeln!(
                self.helpers,
                "export function {fname}(a: array<var {raw}>): FixedArray<{surface}> {{\n  \
                 let out = new FixedArray<{surface}>(a.length, {init});\n  \
                 for (var i = 0; i < a.length; i += 1) {{\n    let e = a[i];\n    {pre}out[i] = {conv};\n  }}\n  \
                 return out;\n}}\n"
            );
        }
        Ok(fname)
    }

    fn array_out(&mut self, elem: &Repr) -> R<String> {
        let raw = self.elem_raw(elem)?;
        let surface = self.surface(elem)?;
        let fname = format!("__{}_array_out", type_ident(&elem.ty));
        if !self.helper_done.contains(&fname) {
            // The value's statements, if any, and the value.
            let (init, pre, conv) = match &elem.kind {
                Kind::Str {
                    nullable: false, ..
                } => ("null".to_string(), String::new(), "e.handle".to_string()),
                Kind::Str { nullable: true, .. } => (
                    "null".to_string(),
                    String::new(),
                    "if (e == null) null else e.handle".to_string(),
                ),
                Kind::Scalar(Scalar::Bool) => ("0".into(), String::new(), "if (e) 1 else 0".into()),
                Kind::Enum { layout, .. } if !i32_enum(layout) => {
                    let f = self.enum_encoder(layout)?;
                    let Shape::Payload { slots } = &layout.shape else {
                        unreachable!()
                    };
                    let vs: Vec<String> = (0..=slots.len()).map(|i| format!("s{i}")).collect();
                    (
                        "null".into(),
                        format!("let ({}) = {f}(e);\n    ", vs.join(", ")),
                        format!("({})", vs.join(", ")),
                    )
                }
                Kind::Scalar(_) | Kind::Enum { .. } => {
                    ("0".into(), String::new(), "e as i32".into())
                }
                Kind::Tuple { elems, .. } => {
                    let mut parts = Vec::new();
                    for (i, e) in elems.iter().enumerate() {
                        if let Kind::Host { of: Host::Any, .. } = e.kind {
                            parts.push(format!("e[{i}]"));
                            continue;
                        }
                        let [v] = &self.slot_values(e, &format!("e[{i}]"))?[..] else {
                            return skip(format!("`{}` elements of an `arr<T>`", elem.ty));
                        };
                        parts.push(v.clone());
                    }
                    (
                        "null".into(),
                        String::new(),
                        format!("({})", parts.join(", ")),
                    )
                }
                _ => return skip(format!("`{}` elements of an `arr<T>`", elem.ty)),
            };
            self.helper_done.insert(fname.clone());
            let _ = writeln!(
                self.helpers,
                "export function {fname}(a: FixedArray<{surface}>): array<var {raw}> {{\n  \
                 let out = new FixedArray<{raw}>(a.length, {init});\n  \
                 for (var i = 0; i < a.length; i += 1) {{\n    let e = a[i];\n    {pre}out[i] = {conv};\n  }}\n  \
                 return out as array<var {raw}>;\n}}\n"
            );
        }
        Ok(fname)
    }
}

mod layout {
    pub use ts_gen_abi::layout::carrier as carrier_of;
}

#[derive(Default)]
struct Accessor {
    is_static: bool,
    get: Option<(String, String, Option<String>)>,
    /// Parameter name and type, body, and whether it takes no value (a
    /// constant-argument setter like `set signalWithNull()`).
    sets: Vec<(String, String, String, bool)>,
}

/// A way to reach an accessor: its parameters, result and access.
type Reach = (Vec<(String, String)>, String, String);

/// Emits an accessor and its unpaired setters, and returns how to reach
/// each: its parameters, result and access (`.name`, `.name = v`,
/// `.setName(v)`).
/// `name` is the accessor's Zena name.
fn emit_accessor(
    out: &mut String,
    name: &str,
    a: Accessor,
    sigs: &mut HashSet<String>,
) -> Vec<Reach> {
    let mut reach = Vec::new();
    let stat = if a.is_static { "static " } else { "" };
    let mut sets = a.sets;
    let mut paired = None;
    let get = a.get;
    if let Some((ty, _, _)) = &get {
        if let Some(i) = sets.iter().position(|s| !s.3 && &s.1 == ty) {
            paired = Some(sets.remove(i));
        }
    }
    if let Some((ty, get, doc)) = get {
        reach.push((Vec::new(), ty.clone(), format!(".{name}")));
        if let Some((pn, pt, _, _)) = &paired {
            reach.push((
                vec![(pn.clone(), pt.clone())],
                "void".into(),
                format!(".{name} = {pn}"),
            ));
        }
        if let Some(d) = doc {
            push_doc(out, "  ", &d);
        }
        let _ = writeln!(
            out,
            "  {stat}{name}: {ty} {{\n    get {{\n{}    }}",
            indent(&get)
        );
        if let Some((pn, _, body, _)) = paired {
            let _ = writeln!(out, "    set({pn}) {{\n{}    }}", indent(&body));
        }
        out.push_str("  }\n");
    }
    // A setter whose type differs from the getter's, or with no getter, is
    // a method: an accessor has one type.
    for (pn, pt, body, none) in sets {
        let params = if none { vec![] } else { vec![(pn, pt)] };
        let m = overload_name(
            sigs,
            a.is_static,
            &ident(&format!("set{}", upper_first(name))),
            &params,
        );
        let _ = writeln!(
            out,
            "  {stat}{m}({}): void {{\n{body}  }}",
            param_list(&params)
        );
        reach.push((
            params.clone(),
            "void".into(),
            format!(".{m}({})", arg_names(&params)),
        ));
    }
    reach
}

fn overload_name(
    sigs: &mut HashSet<String>,
    is_static: bool,
    base: &str,
    params: &[(String, String)],
) -> String {
    let types: Vec<&str> = params.iter().map(|(_, t)| t.as_str()).collect();
    let mut name = base.to_string();
    let mut n = 2;
    while !sigs.insert(format!("{is_static}|{name}|{}", types.join(","))) {
        name = format!("{base}{n}");
        n += 1;
    }
    name
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("  {l}\n")).collect()
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The statements of a wrapper: its conversions, the call, and the
/// conversion of the result.
fn wrapper_body(call: &Call, invoke: &str, pad: &str) -> String {
    let mut s = String::new();
    for p in &call.pre {
        let _ = writeln!(s, "{pad}{p}");
    }
    match call.raw_results.len() {
        0 => {
            let _ = writeln!(s, "{pad}{invoke};");
        }
        1 if call.decode == "r0" => {
            let _ = writeln!(s, "{pad}return {invoke};");
        }
        1 => {
            let _ = writeln!(s, "{pad}let r0 = {invoke};");
            let _ = writeln!(s, "{pad}return {};", call.decode);
        }
        n => {
            let names: Vec<String> = (0..n).map(|i| format!("r{i}")).collect();
            let _ = writeln!(s, "{pad}let ({}) = {invoke};", names.join(", "));
            let _ = writeln!(s, "{pad}return {};", call.decode);
        }
    }
    s
}

fn push_doc(out: &mut String, pad: &str, doc: &str) {
    let _ = writeln!(out, "{pad}/**");
    for l in doc.lines() {
        if l.is_empty() {
            let _ = writeln!(out, "{pad} *");
        } else {
            let _ = writeln!(out, "{pad} * {}", l.replace("*/", "*\\/"));
        }
    }
    let _ = writeln!(out, "{pad} */");
}

fn field_name(i: usize, n: usize) -> String {
    if n == 1 {
        "value".into()
    } else {
        format!("value{i}")
    }
}

fn nullable_str(n: bool) -> String {
    if n {
        "String | null".into()
    } else {
        "String".into()
    }
}

fn nullable_ty(t: String, yes: bool) -> String {
    if yes {
        format!("{t} | null")
    } else {
        t
    }
}

/// The wire integer type of a scalar with a niche: its carrier, which has
/// room for the niche.
fn niche_carrier(s: Scalar) -> &'static str {
    match layout::carrier_of(s) {
        ValType::I64 => "i64",
        _ => "i32",
    }
}

fn niche_literal(s: Scalar, n: i64) -> String {
    match layout::carrier_of(s) {
        ValType::I64 => format!("({n} as i64)"),
        _ => format!("({n})"),
    }
}

/// A canonical spelling as an identifier fragment.
fn type_ident(t: &str) -> String {
    ident(&t.replace("[]", "_array").replace('?', "_opt"))
}

fn zena_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn arg_names(ps: &[(String, String)]) -> String {
    let ns: Vec<&str> = ps.iter().map(|(n, _)| n.as_str()).collect();
    ns.join(", ")
}

fn param_list(ps: &[(String, String)]) -> String {
    let ps: Vec<String> = ps.iter().map(|(n, t)| format!("{n}: {t}")).collect();
    ps.join(", ")
}

fn tparam_list(ps: &[TypeParam]) -> String {
    if ps.is_empty() {
        return String::new();
    }
    let ps: Vec<String> = ps.iter().map(|p| ident(&p.name)).collect();
    format!("<{}>", ps.join(", "))
}

/// A tuple type or value of `parts`. Zena has no 1-tuples, and the IR's
/// wire is a 1-tuple's element (§11), so a 1-tuple is its element.
fn tuple_type(parts: &[String]) -> String {
    if parts.len() == 1 {
        parts[0].clone()
    } else {
        format!("({})", parts.join(", "))
    }
}

/// Element `i` of a tuple `v` of `len` elements, as [`tuple_type`] has it.
fn tuple_at(v: &str, i: usize, len: usize) -> String {
    if len == 1 {
        v.to_string()
    } else {
        format!("{v}[{i}]")
    }
}

fn raw_targs(c: &Class, m: &Member) -> String {
    let all: Vec<TypeParam> = c.tparams.iter().chain(&m.tparams).cloned().collect();
    tparam_list(&all)
}

fn raw_targs_member(m: &Member) -> String {
    tparam_list(&m.tparams)
}

/// The type arguments a class member passes its templated import: the
/// class's, then the member's.
fn raw_tparams(k: &Keyed<'_>) -> String {
    let all: Vec<TypeParam> = k
        .owner
        .iter()
        .flat_map(|c| c.tparams.iter())
        .chain(&k.member.tparams)
        .cloned()
        .collect();
    tparam_list(&all)
}

/// `s` with its braces doubled, which an `@external` name reads as
/// literal braces rather than placeholders.
fn brace_escape(s: &str) -> String {
    s.replace('{', "{{").replace('}', "}}")
}

/// A generic member's import name: its key with a placeholder per type
/// parameter, which the Zena compiler fills in per instantiation.
fn templated_key(k: &Keyed<'_>) -> String {
    let all: Vec<String> = k
        .owner
        .iter()
        .flat_map(|c| c.tparams.iter())
        .chain(&k.member.tparams)
        .map(|p| format!("{{{}}}", p.name))
        .collect();
    let (head, tail) = split_params(&k.key);
    format!(
        "{}<{}>{}",
        brace_escape(head),
        all.join(","),
        brace_escape(tail)
    )
}

/// A function type, and whether it's nullable.
fn fn_of(t: &Type) -> Option<(&FnType, bool)> {
    match t {
        Type::Fn(f) => Some((f, false)),
        Type::Nullable(inner, _) => match &**inner {
            Type::Fn(f) => Some((f, true)),
            _ => None,
        },
        _ => None,
    }
}

/// A specialized class's generic member's import name: its key at the
/// class's argument `arg`, with a placeholder per type parameter of its own.
fn partial_key(k: &Keyed<'_>, arg: &str) -> String {
    let mut all = vec![brace_escape(arg)];
    all.extend(k.member.tparams.iter().map(|p| format!("{{{}}}", p.name)));
    let (head, tail) = split_params(&k.key);
    format!(
        "{}<{}>{}",
        brace_escape(head),
        all.join(","),
        brace_escape(tail)
    )
}

/// A generic member's key at type arguments `args`.
fn instance_key(k: &Keyed<'_>, args: &[&str]) -> String {
    let (head, tail) = split_params(&k.key);
    format!("{head}<{}>{tail}", args.join(","))
}

/// A key before and from its parameter list, the last parenthesized
/// group at depth 0.
fn split_params(key: &str) -> (&str, &str) {
    let params_at = key.rfind('(').map_or(key.len(), |i| {
        let bytes = key.as_bytes();
        let mut depth = 0;
        let mut at = i;
        for (j, b) in bytes.iter().enumerate().rev() {
            match b {
                b')' => depth += 1,
                b'(' => {
                    depth -= 1;
                    if depth == 0 {
                        at = j;
                        break;
                    }
                }
                _ => {}
            }
        }
        at
    });
    key.split_at(params_at)
}

fn is_tparam(p: &Path, cx: &Context<'_>) -> bool {
    p.segments.len() == 1 && cx.tparams.contains(&p.segments[0].as_str())
}

fn mentions_tparam(t: &Type, cx: &Context<'_>) -> bool {
    match t {
        Type::Named(r) => is_tparam(&r.path, cx) || r.args.iter().any(|a| mentions_tparam(a, cx)),
        Type::Vec(t)
        | Type::VecBox(t)
        | Type::MutVec(t)
        | Type::Array(t)
        | Type::ArrBox(t)
        | Type::Nullable(t, _)
        | Type::Future(t, _) => mentions_tparam(t, cx),
        Type::View { elem, .. } => mentions_tparam(elem, cx),
        Type::Tuple(ts) | Type::TupleBox(ts) => ts.iter().any(|t| mentions_tparam(t, cx)),
        Type::Fn(f) => {
            f.params.iter().any(|t| mentions_tparam(t, cx)) || mentions_tparam(&f.result, cx)
        }
        _ => false,
    }
}
