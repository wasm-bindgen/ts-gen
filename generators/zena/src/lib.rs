//! Zena bindings for the Wasm ABI, read from the template `ts-gen --target
//! wat` writes and compiled with Zena's `-t js`.
//!
//! A flat layer of `@external` declarations, one per import the template
//! lists; a sys layer with wasm-bindgen's `JsValue` helpers over
//! `externref`; and an exported class per owner holding the host
//! reference in `#ref`. TS strings are Zena `String`s, which on `-t js`
//! wrap the JS string itself. See the README for the full mapping.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::hash::Hash;

use ts_gen_abi::{
    Abi, AbiFunction, AbiHandler, AbiOp, AbiParam, Head, OwnerRef, WasmType, BUILTINS_SPECIFIER,
    UNDEFINED_GLOBAL,
};

/// The local a wrapper holds a possibly empty result in.
const RESULT: &str = "__r";

/// Names the generated code defines or references besides the TS
/// declarations', which those therefore cannot take.
const RESERVED: &[&str] = &[
    "Undefined",
    "isUndefined",
    "isEmpty",
    "numberValue",
    "await_",
    "String",
    "Error",
    "FixedArray",
    RESULT,
];

pub struct Output {
    pub bindings: String,
    /// With `--provides`: `main.zena`, the compile entry, and the
    /// `app.zena` scaffold it calls.
    pub provides: Option<(String, String)>,
}

/// `bindings_file` is the output's file name, which `main.zena` and
/// `app.zena` import.
pub fn generate(abi: &Abi, bindings_file: &str) -> anyhow::Result<Output> {
    let ctx = Ctx::new(abi, bindings_file);
    let provides = if abi.handlers.is_empty() {
        None
    } else {
        Some((ctx.main()?, ctx.app()))
    };
    Ok(Output {
        bindings: ctx.bindings(),
        provides,
    })
}

/// An owner or free function: the module it is imported from and its
/// dotted name there.
type Key<'a> = (Option<&'a str>, &'a str);

/// A free function's overloads: its module, its op's JS name and the index
/// of the name it is imported under.
type Family<'a> = (Option<&'a str>, &'a str, usize);

/// One `@external` declaration: an ABI function under one of its names.
struct Import<'a> {
    f: &'a AbiFunction,
    name: &'a str,
    flat: String,
}

impl<'a> Import<'a> {
    fn owner(&self) -> Option<(&'a str, &'a str)> {
        self.name.rsplit_once('.')
    }
}

struct Ctx<'a> {
    abi: &'a Abi,
    bindings_file: &'a str,
    imports: Vec<Import<'a>>,
    /// Owners in first-appearance order, with their Zena names.
    owners: Vec<(Key<'a>, String)>,
    classes: HashMap<Key<'a>, String>,
    /// Owners that only alias another, emitted as a `type` of its class.
    aliases: HashMap<Key<'a>, Key<'a>>,
    /// Exported forwarder names of the free functions, by import index.
    free: HashMap<usize, String>,
}

/// How a reference position is presented: an owner class around `#ref`,
/// or a `String` around the JS string.
enum Wrap {
    Class(String),
    Str,
}

impl Wrap {
    fn ty(&self) -> &str {
        match self {
            Wrap::Class(c) => c,
            Wrap::Str => "String",
        }
    }

    fn wrap(&self, r: &str) -> String {
        match self {
            Wrap::Class(c) => format!("new {c}({r})"),
            Wrap::Str => format!("String.fromHandle({r})"),
        }
    }

    fn unwrap(&self, v: &str) -> String {
        match self {
            Wrap::Class(_) => format!("{v}.ref()"),
            Wrap::Str => format!("{v}.handle"),
        }
    }
}

fn constructs(f: &AbiFunction) -> bool {
    matches!(f.op, AbiOp::Construct { .. } | AbiOp::NewObject)
}

fn is_sys(f: &AbiFunction) -> bool {
    f.op.is_sys()
}

/// Names for keys in order, globals first so that a module's name only
/// takes the specifier prefix when a global or earlier name holds it. The
/// keys of one `family` (a function's overloads) take the prefix together.
fn assign_names<'a, F: Copy + Eq + Hash>(
    keys: &[(Key<'a>, F)],
    used: &mut HashSet<String>,
) -> HashMap<Key<'a>, String> {
    let mut names = HashMap::new();
    for global in [true, false] {
        let group: Vec<(Key, F)> = keys
            .iter()
            .copied()
            .filter(|(k, _)| k.0.is_none() == global)
            .collect();
        let mut prefixed: HashMap<F, bool> = HashMap::new();
        for &(key, family) in &group {
            let prefix = *prefixed.entry(family).or_insert_with(|| {
                group
                    .iter()
                    .any(|(k, f)| *f == family && used.contains(&ident(k.1)))
            });
            let name = match key.0 {
                Some(spec) if prefix => ident(&format!("{spec}_{}", key.1)),
                _ => ident(key.1),
            };
            names.insert(key, unique(used, name));
        }
    }
    names
}

impl<'a> Ctx<'a> {
    fn new(abi: &'a Abi, bindings_file: &'a str) -> Self {
        let mut used = HashSet::from(["__undefined".to_string()]);
        let imports: Vec<Import> = abi
            .functions
            .iter()
            .flat_map(|f| {
                std::iter::once(&f.name)
                    .chain(&f.aliases)
                    .map(move |n| (f, n))
            })
            .map(|(f, name)| {
                let flat = match &f.module {
                    Some(spec) => format!("__{}_{}", ident(spec), ident(name)),
                    None => format!("__{}", ident(name)),
                };
                Import {
                    f,
                    name,
                    flat: unique(&mut used, flat),
                }
            })
            .collect();

        // An owner some function is named under is a class. One reached
        // only through aliases of a single other owner's functions is a
        // `type` of that class.
        let mut keys: Vec<Key> = Vec::new();
        let mut primary: HashSet<Key> = HashSet::new();
        let mut alias_of: HashMap<Key, Option<Key>> = HashMap::new();
        for i in imports.iter().filter(|i| !is_sys(i.f)) {
            let Some((owner, _)) = i.owner() else {
                continue;
            };
            let module = i.f.module.as_deref();
            let key = (module, owner);
            if !keys.contains(&key) {
                keys.push(key);
            }
            let named = i.f.name.rsplit_once('.').map(|(o, _)| (module, o));
            match named {
                Some(k) if k != key => {
                    let target = alias_of.entry(key).or_insert(Some(k));
                    if *target != Some(k) {
                        *target = None;
                    }
                }
                _ => {
                    primary.insert(key);
                }
            }
        }
        let aliases: HashMap<Key, Key> = alias_of
            .into_iter()
            .filter(|(k, _)| !primary.contains(k))
            .filter_map(|(k, target)| Some((k, target?)))
            .collect();

        let mut used: HashSet<String> = RESERVED
            .iter()
            .map(|s| s.to_string())
            .chain(imports.iter().map(|i| i.flat.clone()))
            .collect();
        let owner_keys: Vec<(Key, Key)> = keys.iter().map(|&k| (k, k)).collect();
        let classes = assign_names(&owner_keys, &mut used);
        let owners = keys.iter().map(|k| (*k, classes[k].clone())).collect();

        let free_keys: Vec<(usize, Key, Family)> = imports
            .iter()
            .enumerate()
            .filter(|(_, i)| i.owner().is_none())
            .map(|(n, i)| {
                let module = i.f.module.as_deref();
                let op = match &i.f.op {
                    AbiOp::StaticCall { name, .. } | AbiOp::StaticGet { name, .. } => name.as_str(),
                    _ => i.name,
                };
                let alias = std::iter::once(&i.f.name)
                    .chain(&i.f.aliases)
                    .position(|n| n == i.name)
                    .unwrap_or(0);
                (n, (module, i.name), (module, op, alias))
            })
            .collect();
        let keys: Vec<(Key, _)> = free_keys.iter().map(|&(_, k, f)| (k, f)).collect();
        let names = assign_names(&keys, &mut used);
        let free = free_keys
            .into_iter()
            .map(|(n, k, _)| (n, names[&k].clone()))
            .collect();

        Ctx {
            abi,
            bindings_file,
            imports,
            owners,
            classes,
            aliases,
            free,
        }
    }

    /// The wrapper for a position: an alias owner wraps as the class it
    /// aliases.
    fn wrap(&self, head: Option<&'a Head>) -> Option<Wrap> {
        match head? {
            Head::String => Some(Wrap::Str),
            Head::Declared(r) => {
                let key = self.class_key(r)?;
                Some(Wrap::Class(self.classes[&key].clone()))
            }
            Head::Number | Head::Boolean | Head::BigInt => None,
        }
    }

    fn class_key(&self, r: &'a OwnerRef) -> Option<Key<'a>> {
        let (key, _) = self
            .classes
            .get_key_value(&(r.module.as_deref(), r.path.as_str()))?;
        Some(*self.aliases.get(key).unwrap_or(key))
    }

    /// An owner's members with their Zena names. Declared members are
    /// named first, so a constructor's `create` yields to a declared one.
    fn members(&self, key: Key) -> Vec<(&Import<'a>, String)> {
        let members: Vec<(&Import, &str)> = self
            .imports
            .iter()
            .filter(|i| !is_sys(i.f))
            .filter_map(|i| {
                let (owner, member) = i.owner()?;
                ((i.f.module.as_deref(), owner) == key).then_some((i, member))
            })
            .collect();
        let mut used: HashSet<String> = ["new", "ref"].iter().map(|s| s.to_string()).collect();
        let mut names = vec![String::new(); members.len()];
        for ctor in [false, true] {
            for (n, (i, member)) in members.iter().enumerate() {
                if constructs(i.f) != ctor {
                    continue;
                }
                let name = if ctor {
                    match member.strip_prefix("new") {
                        Some(rest) => ident(&format!("create{rest}")),
                        None => ident(&format!("create_{member}")),
                    }
                } else {
                    ident(member)
                };
                names[n] = unique(&mut used, name);
            }
        }
        members.into_iter().map(|(i, _)| i).zip(names).collect()
    }

    fn sys_flat(&self, op: fn(&AbiOp) -> bool) -> &str {
        self.imports
            .iter()
            .find(|i| op(&i.f.op))
            .map(|i| i.flat.as_str())
            .expect("sys functions are always in the ABI")
    }

    fn bindings(&self) -> String {
        let mut out = String::new();
        out.push_str("// Generated by ts-gen: Zena bindings for the Wasm ABI. Compile with\n");
        out.push_str(
            "// `-t js --imported-string-constants wasm:js/string-constants` for embed.\n",
        );
        out.push_str("// `__` declarations import the ABI; the exported classes and functions\n");
        out.push_str("// wrap them. `externref` holds any JS value but null, `undefined`\n");
        out.push_str("// included; positions TS allows to be null are `externref | null`. TS\n");
        out.push_str("// strings are `String`s over the JS string. An owner or string result\n");
        out.push_str("// TS allows to be empty is `T | null`, null for null and undefined.\n");
        if !self.abi.modules().is_empty() {
            out.push_str(
                "// Declarations from `declare module \"spec\"` are imported from \"spec\".\n",
            );
        }
        self.emit_flat(&mut out);
        self.emit_sys(&mut out);
        for (key, class) in &self.owners {
            match self.aliases.get(key) {
                Some(target) => {
                    let _ = writeln!(out, "\nexport type {class} = {};", self.classes[target]);
                }
                None => self.emit_class(&mut out, *key, class),
            }
        }
        self.emit_free_functions(&mut out);
        emit_await(&mut out);
        self.emit_handlers(&mut out);
        out
    }

    fn emit_flat(&self, out: &mut String) {
        let mut owner = None;
        for i in &self.imports {
            let f = i.f;
            out.push('\n');
            let this_owner = i.owner().map(|(o, _)| o);
            if this_owner != owner {
                if let Some(o) = this_owner {
                    let _ = writeln!(out, "// {o}");
                }
                owner = this_owner;
            }
            let params = f
                .params
                .iter()
                .zip(param_names(&f.params))
                .enumerate()
                .map(|(n, (p, name))| {
                    // Nullable, as the template's `(ref null $anyrefs)`.
                    let ty = if is_rest(f, n) {
                        "FixedArray<anyref> | null".to_string()
                    } else {
                        flat_type(p.ty)
                    };
                    format!("{name}: {ty}")
                })
                .collect::<Vec<_>>()
                .join(", ");
            let result = f.result.map_or("void".to_string(), flat_type);
            let _ = writeln!(out, "// {}", f.ts);
            let _ = writeln!(out, "@external({:?}, {:?})", f.import_module(), i.name);
            let _ = writeln!(out, "declare function {}({params}): {result};", i.flat);
        }
    }

    fn emit_sys(&self, out: &mut String) {
        let is_undefined = self.sys_flat(|op| matches!(op, AbiOp::IsUndefined));
        let number_value = self.sys_flat(|op| matches!(op, AbiOp::NumberValue));
        let _ = write!(
            out,
            r#"
// sys: wasm-bindgen's JsValue surface. `externref` is JsValue; `null`
// is wasm null, `undefined` the JS value below.
@external({BUILTINS_SPECIFIER:?}, {UNDEFINED_GLOBAL:?})
declare let __undefined: externref;
export let Undefined: externref = __undefined;
export function isUndefined(x: externref | null): boolean {{ return {is_undefined}(x); }}
export function isEmpty(x: externref | null): boolean {{ return x == null || {is_undefined}(x); }}
export function numberValue(x: externref): f64 {{ return {number_value}(x); }}
"#
        );
    }

    fn emit_class(&self, out: &mut String, key: Key, class: &str) {
        let _ = writeln!(out, "\nexport class {class} {{");
        out.push_str("  #ref: externref;\n");
        out.push_str("  new(ref: externref) : #ref = ref;\n");
        out.push_str("  ref(): externref { return this.#ref; }\n");

        for (i, name) in self.members(key) {
            let f = i.f;
            let instance = matches!(
                f.op,
                AbiOp::Get { .. } | AbiOp::Set { .. } | AbiOp::Call { .. }
            );
            let skip = usize::from(instance);

            let (sig, mut args) = self.params(f, skip);
            if instance {
                args.insert(0, "this.#ref".to_string());
            }
            let call = format!("{}({})", i.flat, args.join(", "));
            let (ret, body) = if constructs(f) {
                (class.to_string(), format!("return new {class}({call});"))
            } else {
                self.result(f, call)
            };

            let _ = match f.op {
                AbiOp::Get { .. } => writeln!(out, "  {name}: {ret} {{ get {{ {body} }} }}"),
                AbiOp::StaticGet { .. } => {
                    writeln!(out, "  static {name}: {ret} {{ get {{ {body} }} }}")
                }
                AbiOp::Set { .. } | AbiOp::Call { .. } => {
                    writeln!(out, "  {name}({sig}): {ret} {{ {body} }}")
                }
                AbiOp::Construct { .. }
                | AbiOp::NewObject
                | AbiOp::StaticSet { .. }
                | AbiOp::StaticCall { .. }
                | AbiOp::IsUndefined
                | AbiOp::NumberValue
                | AbiOp::Undefined => {
                    writeln!(out, "  static {name}({sig}): {ret} {{ {body} }}")
                }
            };
        }
        out.push_str("}\n");
    }

    fn emit_free_functions(&self, out: &mut String) {
        let mut first = true;
        for (n, i) in self.imports.iter().enumerate() {
            let Some(name) = self.free.get(&n) else {
                continue;
            };
            if std::mem::take(&mut first) {
                out.push('\n');
            }
            let (sig, args) = self.params(i.f, 0);
            let call = format!("{}({})", i.flat, args.join(", "));
            let (ret, body) = self.result(i.f, call);
            let _ = writeln!(out, "export function {name}({sig}): {ret} {{ {body} }}");
        }
    }

    /// The wrapper's parameter list from `f.params[skip..]` and the
    /// argument expressions passing each to the flat declaration.
    fn params(&self, f: &AbiFunction, skip: usize) -> (String, Vec<String>) {
        let mut sig = Vec::new();
        let mut args = Vec::new();
        let names = param_names(&f.params);
        for (n, (p, name)) in f.params.iter().zip(names).enumerate().skip(skip) {
            let wrap = self.wrap(p.head.as_ref());
            let (ty, arg) = match (p.ty, wrap) {
                _ if is_rest(f, n) => ("FixedArray<anyref>".to_string(), name.clone()),
                (WasmType::ExternRef { null, undefined }, Some(w)) => {
                    let unwrapped = w.unwrap(&name);
                    match (null, undefined) {
                        (false, false) => (w.ty().to_string(), unwrapped),
                        // `null` for a `T | undefined` position crosses as `undefined`.
                        (true, _) => (
                            format!("{} | null", w.ty()),
                            format!("if ({name} == null) null else {unwrapped}"),
                        ),
                        (false, true) => (
                            format!("{} | null", w.ty()),
                            format!("if ({name} == null) Undefined else {unwrapped}"),
                        ),
                    }
                }
                _ => (flat_type(p.ty), name.clone()),
            };
            sig.push(format!("{name}: {ty}"));
            args.push(arg);
        }
        (sig.join(", "), args)
    }

    /// The wrapper's result type and the body returning `call`'s value
    /// in it.
    fn result(&self, f: &AbiFunction, call: String) -> (String, String) {
        let Some(r) = f.result else {
            return ("void".to_string(), format!("{call};"));
        };
        match (r, self.wrap(f.result_head.as_ref())) {
            (
                WasmType::ExternRef {
                    null: false,
                    undefined: false,
                },
                Some(w),
            ) => (w.ty().to_string(), format!("return {};", w.wrap(&call))),
            (WasmType::ExternRef { null, undefined }, Some(w)) => {
                let empty = match (null, undefined) {
                    (true, true) => format!("{RESULT} == null || isUndefined({RESULT})"),
                    (true, false) => format!("{RESULT} == null"),
                    (false, _) => format!("isUndefined({RESULT})"),
                };
                (
                    format!("{} | null", w.ty()),
                    format!(
                        "let {RESULT} = {call}; return if ({empty}) null else {};",
                        w.wrap(RESULT)
                    ),
                )
            }
            _ => (flat_type(r), format!("return {call};")),
        }
    }

    fn emit_handlers(&self, out: &mut String) {
        if self.abi.handlers.is_empty() {
            return;
        }
        out.push_str(
            "\n// Provided (--provides): main.zena provides these over the typed app.zena.\n",
        );
        for h in &self.abi.handlers {
            let params = h
                .params
                .iter()
                .zip(param_names(&h.params))
                .map(|(p, name)| format!("{name}: {}", flat_type(p.ty)))
                .collect::<Vec<_>>()
                .join(", ");
            let result = h.result.map_or("void".to_string(), flat_type);
            let _ = writeln!(
                out,
                "// {}: export function {}({params}): {result}",
                h.path.join("."),
                h.name
            );
        }
    }

    /// `main.zena`: the compile entry, one raw export per handler
    /// converting the wire types to and from the typed `app.zena`. Zena
    /// exports are plain identifiers, so nested export paths cannot be
    /// provided.
    fn main(&self) -> anyhow::Result<String> {
        let mut used = Vec::new();
        let mut body = String::new();
        for h in &self.abi.handlers {
            if !is_plain_ident(&h.name) || ident(&h.name) != h.name {
                anyhow::bail!(
                    "ts-gen-zena cannot provide the export {:?}: Zena exports are identifiers, so only `default.<name>` and top-level functions that are not Zena keywords are supported",
                    h.name
                );
            }
            let mut params = Vec::new();
            let mut args = Vec::new();
            for (p, name) in h.params.iter().zip(param_names(&h.params)) {
                params.push(format!("{name}: {}", flat_type(p.ty)));
                args.push(match (p.ty, self.wrap(p.head.as_ref())) {
                    (WasmType::ExternRef { null, undefined }, Some(w)) => {
                        if let Wrap::Class(c) = &w {
                            used.push(c.clone());
                        }
                        match (null, undefined) {
                            (false, false) => w.wrap(&name),
                            (true, _) => {
                                used.push("isUndefined".into());
                                format!(
                                    "if (isUndefined({name}) || {name} == null) null else {}",
                                    w.wrap(&name)
                                )
                            }
                            (false, true) => {
                                used.push("isUndefined".into());
                                format!("if (isUndefined({name})) null else {}", w.wrap(&name))
                            }
                        }
                    }
                    _ => name,
                });
            }
            let call = format!("app.{}({})", h.name, args.join(", "));
            let ret = match (h.result, self.wrap(h.result_head.as_ref())) {
                (None, _) => format!("{{ {call}; }}"),
                (
                    Some(WasmType::ExternRef {
                        null: false,
                        undefined: false,
                    }),
                    Some(w),
                ) => format!("{{ return {}; }}", w.unwrap(&call)),
                (Some(WasmType::ExternRef { null, .. }), Some(w)) => {
                    let empty = if null {
                        "null"
                    } else {
                        used.push("Undefined".into());
                        "Undefined"
                    };
                    format!(
                        "{{ let {RESULT} = {call}; return if ({RESULT} == null) {empty} else {}; }}",
                        w.unwrap(RESULT)
                    )
                }
                _ => format!("{{ return {call}; }}"),
            };
            let result = h
                .result
                .map_or(String::new(), |r| format!(": {}", flat_type(r)));
            let _ = writeln!(
                body,
                "export function {}({}){result} {ret}",
                h.name,
                params.join(", ")
            );
        }
        let mut out = String::from("import * as app from './app.zena';\n");
        push_bindings_import(&mut out, used, self.bindings_file);
        out.push('\n');
        out.push_str(&body);
        Ok(out)
    }

    /// `app.zena`: the typed functions `main.zena` delegates to, with the
    /// TS JSDoc and bodies that compile.
    fn app(&self) -> String {
        let mut used = Vec::new();
        let mut body = String::new();
        for h in &self.abi.handlers {
            let mut params = Vec::new();
            for (p, name) in h.params.iter().zip(param_names(&h.params)) {
                let ty = self.app_type(p.ty, p.head.as_ref(), &mut used);
                params.push(format!("{name}: {ty}"));
            }
            let (ret, stmt) = match h.result {
                None => (String::new(), String::new()),
                Some(r) => {
                    let ty = self.app_type(r, h.result_head.as_ref(), &mut used);
                    (format!(": {ty}"), self.placeholder(h, r, &mut used))
                }
            };
            if let Some(doc) = &h.doc {
                for line in doc.lines() {
                    let _ = writeln!(body, "// {line}");
                }
            }
            let _ = writeln!(
                body,
                "export function {}({}){ret} {{{stmt}}}\n",
                h.name,
                params.join(", ")
            );
        }
        let mut out = String::new();
        if push_bindings_import(&mut out, used, self.bindings_file) {
            out.push('\n');
        }
        out.push_str(body.trim_end());
        out.push('\n');
        out
    }

    /// A body that compiles for the scaffold: an empty value where the
    /// type admits one, the owner's zero-argument constructor, or a throw
    /// the author replaces.
    fn placeholder(&self, h: &AbiHandler, r: WasmType, used: &mut Vec<String>) -> String {
        let wrap = self.wrap(h.result_head.as_ref());
        let ctor = self.zero_arg_ctor(h.result_head.as_ref());
        const NON_NULL: WasmType = WasmType::JS_NON_NULL;
        let value = match (r, &wrap, ctor) {
            (NON_NULL, Some(Wrap::Str), _) => "\"\"".to_string(),
            (NON_NULL, Some(Wrap::Class(class)), Some(ctor)) => format!("{class}.{ctor}()"),
            (NON_NULL, ..) => return " throw new Error(\"not implemented\"); ".to_string(),
            (WasmType::ExternRef { null: false, .. }, None, _) => {
                used.push("Undefined".into());
                "Undefined".to_string()
            }
            (WasmType::ExternRef { .. }, ..) => "null".to_string(),
            (WasmType::I32, ..) => "false".to_string(),
            (WasmType::I64, ..) => "0 as i64".to_string(),
            (WasmType::F64, ..) => "0.0".to_string(),
            (WasmType::AnyRef { .. } | WasmType::FuncRef, ..) => "null".to_string(),
        };
        format!(" return {value}; ")
    }

    /// The Zena name of a global owner's `new()`, when it has one.
    fn zero_arg_ctor(&self, head: Option<&'a Head>) -> Option<String> {
        let Some(Head::Declared(r)) = head else {
            return None;
        };
        let key = self.class_key(r)?;
        self.members(key)
            .into_iter()
            .find(|(i, _)| {
                constructs(i.f)
                    && i.owner().is_some_and(|(_, m)| m == "new")
                    && i.f.params.is_empty()
            })
            .map(|(_, name)| name)
    }

    /// The Zena type the app sees for a wire position.
    fn app_type(&self, ty: WasmType, head: Option<&Head>, used: &mut Vec<String>) -> String {
        match (ty, self.wrap(head)) {
            (WasmType::ExternRef { null, undefined }, Some(w)) => {
                if let Wrap::Class(c) = &w {
                    used.push(c.clone());
                }
                if null || undefined {
                    format!("{} | null", w.ty())
                } else {
                    w.ty().to_string()
                }
            }
            _ => flat_type(ty),
        }
    }
}

fn push_bindings_import(out: &mut String, mut names: Vec<String>, file: &str) -> bool {
    if names.is_empty() {
        return false;
    }
    names.sort();
    names.dedup();
    let _ = writeln!(out, "import {{ {} }} from './{file}';", names.join(", "));
    true
}

fn emit_await(out: &mut String) {
    out.push_str("\n// JSPI: suspends on a Promise and returns its value. Importing this\n");
    out.push_str("// selects JSPI mode in `embed`, which wraps the handlers with promising.\n");
    let _ = writeln!(out, "@external({BUILTINS_SPECIFIER:?}, \"await\")");
    out.push_str("export declare function await_(p: externref): externref | null;\n");
}

fn is_plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_rest(f: &AbiFunction, i: usize) -> bool {
    f.variadic && i + 1 == f.params.len()
}

/// Only TS booleans lower to i32 in the ABI.
fn flat_type(ty: WasmType) -> String {
    match ty {
        WasmType::I32 => "boolean",
        WasmType::I64 => "i64",
        WasmType::F64 => "f64",
        // Only `null` is a wasm null; `undefined` is a value an `externref` holds.
        WasmType::ExternRef { null: true, .. } => "externref | null",
        WasmType::ExternRef { .. } => "externref",
        WasmType::AnyRef { .. } | WasmType::FuncRef => "anyref",
    }
    .to_string()
}

/// ABI names carry TS property keys verbatim, which may be any string.
fn ident(abi_name: &str) -> String {
    let mut s: String = abi_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    escape_keyword(s)
}

/// Same convention as Zena's own WIT bindings: a trailing underscore.
fn escape_keyword(name: String) -> String {
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
    if KEYWORDS.contains(&name.as_str()) {
        name + "_"
    } else {
        name
    }
}

/// Parameter names as distinct Zena identifiers: a keyword's escape
/// (`class_`) can meet a parameter already named so.
fn param_names(params: &[AbiParam]) -> Vec<String> {
    let mut used = HashSet::new();
    params
        .iter()
        .map(|p| unique(&mut used, ident(&p.name)))
        .collect()
}

fn unique(used: &mut HashSet<String>, name: String) -> String {
    let mut candidate = name.clone();
    let mut n = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{name}_{n}");
        n += 1;
    }
    candidate
}
