//! `embed`: implement the ABI for one language module.
//!
//! Reads the module's imports, keeps the ABI functions it uses, and emits
//! three artifacts that need no hand-written JavaScript:
//!
//! * `bindings.wasm`: one export per used ABI function, built from the
//!   primitives in `builtins.js` and imported string constants.
//! * `wrapper.wasm`: imports the module's provided functions and
//!   publishes them at their JS paths, one externref global per first
//!   path segment, each wrapped with `WebAssembly.promising` in JSPI
//!   mode.
//! * `builtins.js`: the primitives those two import. JavaScript cannot
//!   be derived from Wasm for two things, reading a property and
//!   invoking a function value, so `reflect_get` and the fixed-arity
//!   `call`/`call_new` families are the core; `reflect_set`,
//!   `reflect_apply` and `is_undefined` are shortcuts for what every
//!   module needs, and everything else (`Array`, `Reflect.construct`,
//!   `WebAssembly.promising`) is reached through `reflect_get` + `call`.

use std::collections::BTreeMap;
use std::fmt::Write;

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexSet;

use super::wat::{ident, result as wat_result};
use super::{
    Abi, AbiFunction, AbiOp, AbiParam, WasmType, BINDINGS_SPECIFIER, BUILTINS_SPECIFIER,
    STRING_CONSTANTS, UNDEFINED_GLOBAL,
};

pub const MAX_ARITY: usize = 8;

#[derive(Debug, Clone)]
pub struct Options {
    /// Specifier `wrapper.wasm` imports the language module under.
    pub module_specifier: String,
}

#[derive(Debug, Default, Clone)]
pub struct Output {
    pub bindings_wat: String,
    pub wrapper_wat: String,
    pub builtins_js: String,
    pub jspi: bool,
    /// ABI functions the module imports, in import order.
    pub used: Vec<String>,
    /// Module exports the wrapper republishes at their JS paths (and which
    /// `postprocess` therefore strips from the merged module).
    pub handlers: Vec<String>,
}

/// A bindings import the module uses, with the types it declared for it.
struct UsedFunction {
    name: String,
    param_types: Vec<WasmType>,
    result_type: Option<WasmType>,
}

/// An export the module provides, with the types it declared for it.
struct HandlerInfo {
    name: String,
    path: Vec<String>,
    param_types: Vec<WasmType>,
    result_type: Option<WasmType>,
}

/// What `embed` learned from the language module.
struct ModuleInfo {
    used: Vec<UsedFunction>,
    jspi: bool,
    /// The module imports the `undefined` global from the builtins.
    undefined: bool,
    handlers: Vec<HandlerInfo>,
}

pub fn embed(abi: &Abi, module: &[u8], options: &Options) -> Result<Output> {
    let info = inspect(abi, module)?;
    let mut builtins = Builtins::default();

    let bindings_wat = generate_bindings(abi, &info.used, &mut builtins)?;
    let wrapper_wat = generate_wrapper(
        &info.handlers,
        info.jspi,
        &options.module_specifier,
        &mut builtins,
    )?;
    if info.jspi {
        builtins.await_ = true;
    }
    if info.undefined {
        builtins.undefined = true;
    }

    Ok(Output {
        bindings_wat,
        wrapper_wat,
        builtins_js: builtins.render(),
        jspi: info.jspi,
        used: info.used.iter().map(|u| u.name.clone()).collect(),
        handlers: info.handlers.iter().map(|h| h.name.clone()).collect(),
    })
}

/// Link the language module, bindings and wrapper into one module with
/// binaryen's `wasm-merge`, so the result imports only `builtins.js` and
/// the string constants. Each module is registered under the specifier the
/// others import it by.
pub fn merge(
    wasm_merge: &std::path::Path,
    module: &[u8],
    module_specifier: &str,
    bindings: &[u8],
    wrapper: &[u8],
) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir().context("creating temp dir for wasm-merge")?;
    // The wrapper goes first: with `--skip-export-conflicts` the earlier
    // export wins, and a published global (`version`) must beat the raw
    // module function of the same name, which `postprocess` strips.
    let paths = [
        (dir.path().join("wrapper.wasm"), wrapper, "./wrapper.wasm"),
        (dir.path().join("module.wasm"), module, module_specifier),
        (
            dir.path().join("bindings.wasm"),
            bindings,
            BINDINGS_SPECIFIER,
        ),
    ];
    for (path, bytes, _) in &paths {
        std::fs::write(path, bytes)?;
    }
    let out = dir.path().join("out.wasm");
    let mut cmd = std::process::Command::new(wasm_merge);
    // Everything but the encodings engines don't ship: compact imports
    // would make the output unloadable, and custom descriptors are not yet
    // standard.
    cmd.args([
        "--all-features",
        "--disable-compact-imports",
        "--disable-custom-descriptors",
        "--skip-export-conflicts",
    ]);
    for (path, _, spec) in &paths {
        cmd.arg(path).arg(spec);
    }
    cmd.arg("-o").arg(&out);
    let status = cmd.output().with_context(|| {
        format!(
            "running binaryen's wasm-merge ({}); point --wasm-merge or $WASM_MERGE at it",
            wasm_merge.display()
        )
    })?;
    if !status.status.success() {
        bail!(
            "wasm-merge failed:\n{}{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
    }
    std::fs::read(&out).context("reading wasm-merge output")
}

/// Assemble WAT and validate it with the features the ABI relies on.
pub fn assemble(wat_text: &str) -> Result<Vec<u8>> {
    let bytes = wat::parse_str(wat_text).context("assembling generated WAT")?;
    let mut validator = wasmparser::Validator::new_with_features(
        wasmparser::WasmFeatures::default() | wasmparser::WasmFeatures::GC,
    );
    validator
        .validate_all(&bytes)
        .context("validating generated Wasm")?;
    Ok(bytes)
}

/// Clean up a merged module: strip the raw function exports the wrapper
/// republished (the bindings' `Type.member`s and the module's `--provides`
/// functions), leaving the published globals and whatever else the module
/// exported.
pub fn postprocess(
    wasm: &[u8],
    bindings_exports: &[String],
    handler_exports: &[String],
) -> Result<Vec<u8>> {
    use wasm_encoder::RawSection;
    use wasmparser::{ExternalKind, Payload};

    let strip: std::collections::HashSet<&str> = bindings_exports
        .iter()
        .chain(handler_exports.iter())
        .map(String::as_str)
        .collect();

    let mut out = wasm_encoder::Module::new();

    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload?;
        match &payload {
            Payload::ExportSection(reader) => {
                let mut exports = wasm_encoder::ExportSection::new();
                for export in reader.clone() {
                    let export = export?;
                    // Only the raw functions are internal; the wrapper's
                    // published globals may share their names.
                    let raw_function =
                        matches!(export.kind, ExternalKind::Func | ExternalKind::FuncExact);
                    if !(raw_function && strip.contains(export.name)) {
                        // The wrapper's globals carry a prefix so a top-level
                        // function export (`version`) does not collide with its
                        // raw function during the merge; it comes off here.
                        let name = export
                            .name
                            .strip_prefix(PUBLISHED_PREFIX)
                            .unwrap_or(export.name);
                        let kind = match export.kind {
                            ExternalKind::Func | ExternalKind::FuncExact => {
                                wasm_encoder::ExportKind::Func
                            }
                            ExternalKind::Table => wasm_encoder::ExportKind::Table,
                            ExternalKind::Memory => wasm_encoder::ExportKind::Memory,
                            ExternalKind::Global => wasm_encoder::ExportKind::Global,
                            ExternalKind::Tag => wasm_encoder::ExportKind::Tag,
                        };
                        exports.export(name, kind, export.index);
                    }
                }
                out.section(&exports);
            }
            _ => {
                if let Some((id, range)) = payload.as_section() {
                    let start = range.start as usize;
                    let end = range.end as usize;
                    out.section(&RawSection {
                        id,
                        data: &wasm[start..end],
                    });
                }
            }
        }
    }

    let bytes = out.finish();
    let mut validator = wasmparser::Validator::new_with_features(
        wasmparser::WasmFeatures::default() | wasmparser::WasmFeatures::GC,
    );
    validator
        .validate_all(&bytes)
        .context("validating postprocessed Wasm")?;
    Ok(bytes)
}

fn inspect(abi: &Abi, module: &[u8]) -> Result<ModuleInfo> {
    use wasmparser::{Payload, TypeRef};

    let mut types: Vec<wasmparser::SubType> = Vec::new();
    let mut imports: Vec<(String, String, TypeRef)> = Vec::new();
    let mut exports: Vec<(String, wasmparser::ExternalKind, u32)> = Vec::new();
    let mut func_type_indices: Vec<u32> = Vec::new();

    for payload in wasmparser::Parser::new(0).parse_all(module) {
        match payload? {
            Payload::TypeSection(reader) => {
                for group in reader {
                    types.extend(group?.into_types());
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    if let TypeRef::Func(idx) = import.ty {
                        func_type_indices.push(idx);
                    }
                    imports.push((
                        import.module.to_string(),
                        import.name.to_string(),
                        import.ty,
                    ));
                }
            }
            Payload::FunctionSection(reader) => {
                for type_idx in reader {
                    func_type_indices.push(type_idx?);
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    exports.push((export.name.to_string(), export.kind, export.index));
                }
            }
            _ => {}
        }
    }

    let func_type = |idx: u32, what: &str| -> Result<&wasmparser::FuncType> {
        match types.get(idx as usize).map(|t| &t.composite_type.inner) {
            Some(wasmparser::CompositeInnerType::Func(f)) => Ok(f),
            _ => bail!("bad type index for {what}"),
        }
    };
    let func_types: Vec<&wasmparser::FuncType> = func_type_indices
        .iter()
        .map(|&idx| func_type(idx, "function"))
        .collect::<Result<_>>()?;

    let mut export_func_indices: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    for (name, kind, idx) in &exports {
        if matches!(
            kind,
            wasmparser::ExternalKind::Func | wasmparser::ExternalKind::FuncExact
        ) {
            export_func_indices.insert(name.clone(), *idx);
        }
    }

    let mut used = Vec::new();
    let mut jspi = false;
    let mut undefined = false;
    for (module_name, field, ty) in &imports {
        if module_name == BINDINGS_SPECIFIER {
            let f = abi.function(field).ok_or_else(|| {
                anyhow!("module imports {BINDINGS_SPECIFIER:?} {field:?}, which is not in the ABI")
            })?;
            let TypeRef::Func(idx) = ty else {
                bail!("import {field:?} must be a function");
            };
            let actual = func_type(*idx, &format!("import {field:?}"))?;
            let expected = Expected {
                what: &format!("import {field:?}"),
                params: &f.params,
                result: f.result,
                variadic: f.variadic,
                import: true,
            };
            let sig = check_signature(&expected, actual, &types)?;
            if !used.iter().any(|u: &UsedFunction| u.name == *field) {
                used.push(UsedFunction {
                    name: field.clone(),
                    param_types: sig.param_types,
                    result_type: sig.result_type,
                });
            }
        } else if module_name == BUILTINS_SPECIFIER && field == UNDEFINED_GLOBAL {
            if !matches!(ty, TypeRef::Global(g) if !g.mutable && ref_type_of(&g.content_type).is_some())
            {
                bail!("the `undefined` import must be an immutable externref or anyref global");
            }
            undefined = true;
        } else if module_name == BUILTINS_SPECIFIER && field == "await" {
            let TypeRef::Func(idx) = ty else {
                bail!("await import must be a function");
            };
            check_await_signature(func_type(*idx, "await import")?)?;
            jspi = true;
        } else if module_name == BUILTINS_SPECIFIER {
            bail!("module imports {BUILTINS_SPECIFIER:?} {field:?}; only `await` and `undefined` may be imported from the builtins");
        }
    }

    let mut handlers = Vec::new();
    for e in &abi.handlers {
        if let Some(&func_idx) = export_func_indices.get(&e.name) {
            let actual = func_types
                .get(func_idx as usize)
                .ok_or_else(|| anyhow!("bad function index for export {:?}", e.name))?;
            let expected = Expected {
                what: &format!("export {:?}", e.name),
                params: &e.params,
                result: e.result,
                variadic: false,
                import: false,
            };
            let sig = check_signature(&expected, actual, &types)?;
            handlers.push(HandlerInfo {
                name: e.name.clone(),
                path: e.path.clone(),
                param_types: sig.param_types,
                result_type: sig.result_type,
            });
        }
    }
    if handlers.is_empty() && !abi.handlers.is_empty() {
        bail!(
            "module provides none of the exports ({})",
            abi.handlers
                .iter()
                .map(|h| h.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // `promising` is applied to the provided functions the wrapper
    // publishes; a suspending export nothing wraps throws on first use.
    if jspi && handlers.is_empty() {
        bail!("module imports `await` but provides nothing to wrap with WebAssembly.promising; pass --provides");
    }
    // The wrapper publishes one global per first path segment and
    // `postprocess` strips only the raw handler functions, so any other
    // module export of that name would survive the merge as a duplicate.
    for (name, _, _) in &exports {
        let published = handlers
            .iter()
            .find(|h| h.path.first() == Some(name) && h.name != *name);
        if let Some(h) = published {
            bail!(
                "module export {name:?} collides with the {name:?} namespace published for {}; rename the export",
                h.name
            );
        }
    }

    Ok(ModuleInfo {
        used,
        jspi,
        undefined,
        handlers,
    })
}

struct SignatureInfo {
    param_types: Vec<WasmType>,
    result_type: Option<WasmType>,
}

/// The ABI side of a signature check: a bindings import or a handler export.
struct Expected<'a> {
    what: &'a str,
    params: &'a [AbiParam],
    result: Option<WasmType>,
    variadic: bool,
    /// A bindings import (the module calls out) rather than a handler
    /// export (JS calls in). Decides which positions JS produces.
    import: bool,
}

/// Check that the actual type matches the ABI signature: arity exact,
/// scalars exact, and each reference position independently externref or
/// anyref. Returns the actual types.
fn check_signature(
    f: &Expected,
    actual: &wasmparser::FuncType,
    types: &[wasmparser::SubType],
) -> Result<SignatureInfo> {
    let actual_params = actual.params();
    if actual_params.len() != f.params.len() {
        bail!(
            "{} has {} params but the ABI declares {}",
            f.what,
            actual_params.len(),
            f.params.len()
        );
    }

    let mut param_types = Vec::with_capacity(f.params.len());
    for (i, p) in f.params.iter().enumerate() {
        let actual_p = &actual_params[i];

        if f.variadic && i == f.params.len() - 1 {
            if !is_anyref_array(actual_p, types) {
                bail!(
                    "{} rest param has type {} but expected (ref null (array (mut anyref)))",
                    f.what,
                    describe_type(actual_p, types)
                );
            }
            param_types.push(p.ty);
            continue;
        }

        if matches!(p.ty, WasmType::ExternRef { .. } | WasmType::AnyRef { .. }) {
            match ref_type_of(actual_p) {
                Some(ty) => param_types.push(if f.import {
                    ty
                } else {
                    accept_ref(f.what, &format!("param {i}"), p.ty, ty)?
                }),
                None => bail!(
                    "{} param {} has type {} but expected externref or anyref",
                    f.what,
                    i,
                    actual_p
                ),
            }
        } else {
            let expected = val_type(p.ty);
            if *actual_p != expected {
                bail!(
                    "{} param {} has type {} but expected {}",
                    f.what,
                    i,
                    actual_p,
                    expected
                );
            }
            param_types.push(p.ty);
        }
    }

    let actual_result = match actual.results() {
        [] => None,
        [r] => Some(*r),
        rs => bail!("{} has {} results", f.what, rs.len()),
    };
    let shown = |r: Option<wasmparser::ValType>| {
        r.map(|r| describe_type(&r, types))
            .unwrap_or_else(|| "void".into())
    };
    let result_type = match (f.result, actual_result) {
        (None, None) => None,
        (Some(e @ (WasmType::ExternRef { .. } | WasmType::AnyRef { .. })), Some(r))
            if ref_type_of(&r).is_some() =>
        {
            let actual = ref_type_of(&r).unwrap();
            Some(if f.import {
                accept_ref(f.what, "result", e, actual)?
            } else {
                actual
            })
        }
        (Some(e), Some(r)) if val_type(e) == r => Some(e),
        (e, r) => bail!(
            "{} has result {} but expected {}",
            f.what,
            shown(r),
            shown(e.map(val_type))
        ),
    };

    Ok(SignatureInfo {
        param_types,
        result_type,
    })
}

/// `await` is `(ref) -> (ref)` with each position independently externref or
/// anyref; the JS side is `WebAssembly.Suspending((p) => p)` either way.
fn check_await_signature(actual: &wasmparser::FuncType) -> Result<()> {
    let params = actual.params();
    let results = actual.results();
    if params.len() != 1 || results.len() != 1 {
        bail!("await import must have signature (ref) -> (ref)");
    }
    let (Some(_), Some(result)) = (ref_type_of(&params[0]), ref_type_of(&results[0])) else {
        bail!(
            "await import has non-reference type: {:?} -> {:?}",
            params[0],
            results[0]
        );
    };
    // A promise may resolve to null, so the result is JS-produced and nullable.
    accept_ref("await import", "result", R, result)?;
    Ok(())
}

/// The bindings export the rest param as `(ref null $anyrefs)`, so the module
/// must declare exactly `(array (mut anyref))` for wasm-merge to link it.
fn is_anyref_array(ty: &wasmparser::ValType, types: &[wasmparser::SubType]) -> bool {
    let wasmparser::ValType::Ref(r) = ty else {
        return false;
    };
    let Some(idx) = concrete_index(r.heap_type()) else {
        return false;
    };
    match types.get(idx as usize).map(|t| &t.composite_type.inner) {
        Some(wasmparser::CompositeInnerType::Array(a)) => {
            a.0.mutable
                && matches!(a.0.element_type, wasmparser::StorageType::Val(v) if is_any_heap_type_val(&v))
        }
        _ => false,
    }
}

fn concrete_index(ht: wasmparser::HeapType) -> Option<u32> {
    match ht {
        wasmparser::HeapType::Concrete(i) => i.as_module_index(),
        _ => None,
    }
}

fn is_any_heap_type_val(v: &wasmparser::ValType) -> bool {
    matches!(v, wasmparser::ValType::Ref(r) if r.is_nullable() && is_any_heap_type(r.heap_type()))
}

/// Render a value type with concrete heap types expanded from the type section.
fn describe_type(ty: &wasmparser::ValType, types: &[wasmparser::SubType]) -> String {
    if let wasmparser::ValType::Ref(r) = ty {
        if let Some(idx) = concrete_index(r.heap_type()) {
            if let Some(t) = types.get(idx as usize) {
                let null = if r.is_nullable() { "null " } else { "" };
                return format!("(ref {null}{})", t.composite_type);
            }
        }
    }
    format!("{ty}")
}

/// The reference types a module may use for a JS value position. The
/// module side only knows wasm nullability: `externref` is `JS`,
/// `(ref extern)` is `JS_NON_NULL`, and `anyref` / `(ref any)` likewise.
fn ref_type_of(ty: &wasmparser::ValType) -> Option<WasmType> {
    match ty {
        wasmparser::ValType::Ref(r) if r.heap_type() == wasmparser::HeapType::EXTERN => {
            Some(if r.is_nullable() {
                R
            } else {
                WasmType::JS_NON_NULL
            })
        }
        wasmparser::ValType::Ref(r) if is_any_heap_type(r.heap_type()) => Some(WasmType::AnyRef {
            null: r.is_nullable(),
        }),
        _ => None,
    }
}

/// A reference position the module declared non-nullable.
fn narrowed(ty: WasmType) -> bool {
    matches!(ty, WasmType::ExternRef { .. } | WasmType::AnyRef { .. }) && !ty.admits_null()
}

/// A module may take a position as `(ref extern)` or `(ref any)` unless JS
/// produces the value there and the ABI type admits `null`: `undefined` is
/// a non-null reference, so `T | undefined` is sound, while a `null` would
/// have nothing to cross as. Positions the module itself produces (its
/// import arguments, its handler results) are its own promise to keep.
fn accept_ref(what: &str, position: &str, abi: WasmType, actual: WasmType) -> Result<WasmType> {
    if abi.admits_null() && narrowed(actual) {
        bail!(
            "{what} {position} is {} but the ABI type admits null; use {} there",
            wire_wat(actual),
            wire(actual).wat()
        );
    }
    Ok(actual)
}

fn is_any_heap_type(ht: wasmparser::HeapType) -> bool {
    matches!(
        ht,
        wasmparser::HeapType::Abstract {
            shared: false,
            ty: wasmparser::AbstractHeapType::Any
        }
    )
}

fn val_type(ty: WasmType) -> wasmparser::ValType {
    match ty {
        WasmType::I32 => wasmparser::ValType::I32,
        WasmType::I64 => wasmparser::ValType::I64,
        WasmType::F64 => wasmparser::ValType::F64,
        WasmType::ExternRef { .. } => wasmparser::ValType::EXTERNREF,
        WasmType::FuncRef => wasmparser::ValType::FUNCREF,
        WasmType::AnyRef { .. } => wasmparser::ValType::Ref(wasmparser::RefType::ANYREF),
    }
}

/// The primitives `builtins.js` exports, recorded as they're needed.
#[derive(Default)]
struct Builtins {
    global: bool,
    reflect_get: bool,
    reflect_set: bool,
    reflect_apply: bool,
    call: [bool; MAX_ARITY + 1],
    call_new: [bool; MAX_ARITY + 1],
    await_: bool,
    /// `x === undefined`; a single import site, so a different host
    /// primitive is a module/name change.
    is_undefined: bool,
    undefined: bool,
}

impl Builtins {
    fn render(&self) -> String {
        let mut js = String::from("// Generated by ts-gen. Primitives the Wasm ABI is built on.\n");
        if self.global {
            js.push_str("export const global = globalThis;\n");
        }
        if self.reflect_get {
            js.push_str("export const reflect_get = Reflect.get;\n");
        }
        if self.reflect_set {
            js.push_str("export const reflect_set = Reflect.set;\n");
        }
        if self.reflect_apply {
            js.push_str("export const reflect_apply = Reflect.apply;\n");
        }
        for (n, used) in self.call.iter().enumerate() {
            if *used {
                let args = arg_list(n);
                let _ = writeln!(
                    js,
                    "export const call{n} = (f, t{args}) => f.call(t{args});"
                );
            }
        }
        for (n, used) in self.call_new.iter().enumerate() {
            if *used {
                let args = arg_list(n);
                let _ = writeln!(
                    js,
                    "export const call_new{n} = (C{args}) => new C({});",
                    args.trim_start_matches(", ")
                );
            }
        }
        if self.is_undefined {
            js.push_str("export const is_undefined = (x) => x === undefined;\n");
        }
        if self.undefined {
            js.push_str("const undefined_ = undefined;\nexport { undefined_ as undefined };\n");
        }
        if self.await_ {
            js.push_str("const await_ = new WebAssembly.Suspending((p) => p);\nexport { await_ as await };\n");
        }
        js
    }
}

fn arg_list(n: usize) -> String {
    (0..n).map(|i| format!(", a{i}")).collect()
}

/// Accumulates one Wasm module's imports: builtins under every signature
/// they're called with, and string constants.
#[derive(Default)]
struct Imports {
    /// (js name, param types, result) -> wat identifier
    funcs: IndexSet<(String, Vec<WasmType>, Option<WasmType>)>,
    strings: IndexSet<String>,
    global: bool,
    anyrefs_type: bool,
    /// `globalThis.Boolean`, fetched once at start for boolean writes.
    boolean: bool,
    /// (specifier, export) pairs imported as externref globals.
    module_exports: IndexSet<(String, String)>,
}

impl Imports {
    fn func(&mut self, name: &str, params: Vec<WasmType>, result: Option<WasmType>) -> String {
        // Builtins are typed at the wire: nullability is the ABI's claim, not JS's.
        let params: Vec<WasmType> = params.into_iter().map(wire).collect();
        let key = (name.to_string(), params, result.map(wire));
        let id = func_ident(&key.0, &key.1, key.2);
        self.funcs.insert(key);
        id
    }

    fn string(&mut self, s: &str) -> String {
        self.strings.insert(s.to_string());
        string_ident(s)
    }

    /// An ES module export, which ESM integration coerces into an
    /// immutable externref global.
    fn module_export(&mut self, spec: &str, export: &str) -> String {
        self.module_exports
            .insert((spec.to_string(), export.to_string()));
        module_export_ident(export)
    }

    fn render(&self) -> String {
        let mut out = String::new();
        if self.anyrefs_type {
            out.push_str("  (type $anyrefs (array (mut anyref)))\n");
        }
        if self.global {
            let _ = writeln!(
                out,
                "  (import {BUILTINS_SPECIFIER:?} \"global\" (global $global externref))"
            );
        }
        for (name, params, result) in &self.funcs {
            let _ = writeln!(
                out,
                "  (import {BUILTINS_SPECIFIER:?} {name:?} (func ${}{}{}))",
                func_ident(name, params, *result),
                params
                    .iter()
                    .map(|p| format!(" (param {})", p.wat()))
                    .collect::<String>(),
                wat_result(*result)
            );
        }
        for s in &self.strings {
            let _ = writeln!(
                out,
                "  (import {STRING_CONSTANTS:?} {s:?} (global ${} externref))",
                string_ident(s)
            );
        }
        for (spec, export) in &self.module_exports {
            let _ = writeln!(
                out,
                "  (import {spec:?} {export:?} (global ${} externref))",
                module_export_ident(export)
            );
        }
        out
    }
}

fn wire(ty: WasmType) -> WasmType {
    match ty {
        WasmType::ExternRef { .. } => R,
        WasmType::AnyRef { .. } => ANY,
        other => other,
    }
}

fn module_export_ident(export: &str) -> String {
    format!("m:{}", ident(export))
}

fn func_ident(name: &str, params: &[WasmType], result: Option<WasmType>) -> String {
    let mut id = name.to_string();
    // Suffix with type signature when it deviates from the "all externref, externref result"
    // default. This includes void results since the same JS function may be called with
    // different Wasm signatures.
    let default_sig = params.iter().all(|p| *p == R) && result == Some(R);
    if !default_sig {
        id.push('_');
        for p in params {
            id.push_str(short(*p));
        }
        id.push('_');
        id.push_str(result.map(short).unwrap_or("v"));
    }
    id
}

fn short(ty: WasmType) -> &'static str {
    match ty {
        WasmType::I32 => "i",
        WasmType::I64 => "l",
        WasmType::F64 => "f",
        WasmType::ExternRef { .. } => "r",
        WasmType::FuncRef => "F",
        WasmType::AnyRef { .. } => "a",
    }
}

fn string_ident(s: &str) -> String {
    let mut id = String::from("s:");
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '$') {
            id.push(c);
        } else {
            let _ = write!(id, "_{:x}", c as u32);
        }
    }
    id
}
fn wat_params_typed(params: &[AbiParam], types: &[WasmType], variadic: bool) -> String {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if variadic && i == params.len() - 1 {
                format!(" (param ${} (ref null $anyrefs))", p.name)
            } else {
                format!(" (param ${} {})", p.name, wire_wat(types[i]))
            }
        })
        .collect()
}

/// The module-facing spelling of a type: non-nullable references are
/// `(ref extern)` / `(ref any)`, everything else as the ABI prints it.
fn wire_wat(ty: WasmType) -> &'static str {
    match ty {
        WasmType::JS_NON_NULL => "(ref extern)",
        WasmType::AnyRef { null: false } => "(ref any)",
        other => other.wat(),
    }
}

/// Code generation for one bindings function body.
///
/// Values the body looks up or creates itself (`globalThis`, path segments,
/// fetched function values, scratch arrays) never cross back into the
/// language module, so they are plain `externref`. Only the exported
/// function's own receiver, arguments and result carry the module's
/// per-position types, and those are threaded through as `(expr, type)`.
struct Body<'a> {
    imports: &'a mut Imports,
    builtins: &'a mut Builtins,
}

const R: WasmType = WasmType::JS;
const ANY: WasmType = WasmType::AnyRef { null: true };

impl Body<'_> {
    fn global(&mut self) -> String {
        self.imports.global = true;
        self.builtins.global = true;
        "(global.get $global)".into()
    }

    /// `receiver[name]`; the key is a string constant, hence externref.
    fn get(
        &mut self,
        (receiver, receiver_ty): (&str, WasmType),
        name: &str,
        result: WasmType,
    ) -> String {
        self.builtins.reflect_get = true;
        let f = self
            .imports
            .func("reflect_get", vec![receiver_ty, R], Some(result));
        let k = self.imports.string(name);
        format!("(call ${f} {receiver} (global.get ${k}))")
    }

    /// `globalThis.a.b.c` for a path; `globalThis` for an empty one. Inside
    /// a module the first segment is one of its exports instead.
    fn path(&mut self, module: Option<&str>, path: &[String]) -> String {
        let (mut expr, rest) = match (module, path) {
            (Some(spec), [first, rest @ ..]) => {
                let g = self.imports.module_export(spec, first);
                (format!("(global.get ${g})"), rest)
            }
            _ => (self.global(), path),
        };
        for seg in rest {
            expr = self.get((&expr, R), seg, R);
        }
        expr
    }

    /// `path[name]` as a reference, where an empty path inside a module
    /// means `name` is itself one of the module's exports.
    fn static_ref(&mut self, module: Option<&str>, path: &[String], name: &str) -> String {
        match (module, path) {
            (Some(spec), []) => format!("(global.get ${})", self.imports.module_export(spec, name)),
            _ => {
                let recv = self.path(module, path);
                self.get((&recv, R), name, R)
            }
        }
    }

    fn set(
        &mut self,
        (receiver, receiver_ty): (&str, WasmType),
        name: &str,
        (value, ty): (&str, WasmType),
    ) -> String {
        self.builtins.reflect_set = true;
        let f = self
            .imports
            .func("reflect_set", vec![receiver_ty, R, ty], None);
        let k = self.imports.string(name);
        format!("(call ${f} {receiver} (global.get ${k}) {value})")
    }

    fn call(
        &mut self,
        func: &str,
        (this, this_ty): (&str, WasmType),
        args: &[(String, WasmType)],
        result: Option<WasmType>,
    ) -> String {
        let n = args.len();
        assert!(n <= MAX_ARITY);
        self.builtins.call[n] = true;
        let mut params = vec![R, this_ty];
        params.extend(args.iter().map(|(_, t)| *t));
        let f = self.imports.func(&format!("call{n}"), params, result);
        let args = args
            .iter()
            .map(|(a, _)| format!(" {a}"))
            .collect::<String>();
        format!("(call ${f} {func} {this}{args})")
    }

    fn apply(
        &mut self,
        func: &str,
        (this, this_ty): (&str, WasmType),
        array: &str,
        result: Option<WasmType>,
    ) -> String {
        self.builtins.reflect_apply = true;
        let f = self
            .imports
            .func("reflect_apply", vec![R, this_ty, R], result);
        format!("(call ${f} {func} {this} {array})")
    }

    /// `Reflect.construct(class, array)`, reached through `call2`.
    fn construct_apply(&mut self, class: &str, array: &str, result: WasmType) -> String {
        let reflect = self.path(None, &["Reflect".to_string()]);
        let construct = self.get((&reflect, R), "construct", R);
        self.call(
            &construct,
            (&reflect, R),
            &[(class.to_string(), R), (array.to_string(), R)],
            Some(result),
        )
    }

    fn construct(&mut self, class: &str, args: &[(String, WasmType)], result: WasmType) -> String {
        let n = args.len();
        assert!(n <= MAX_ARITY);
        self.builtins.call_new[n] = true;
        let mut params = vec![R];
        params.extend(args.iter().map(|(_, t)| *t));
        let f = self
            .imports
            .func(&format!("call_new{n}"), params, Some(result));
        let args = args
            .iter()
            .map(|(a, _)| format!(" {a}"))
            .collect::<String>();
        format!("(call ${f} {class}{args})")
    }

    /// `new Array()` via `reflect_get(global, "Array")` + `call_new0`.
    fn new_array(&mut self) -> String {
        let g = self.global();
        let array_ctor = self.get((&g, R), "Array", R);
        self.construct(&array_ctor, &[], R)
    }
}

struct FuncBody {
    locals: String,
    /// Statements run before the value expression (variadic setup).
    stmts: String,
    /// The expression producing the function's result.
    value: String,
}

fn generate_bindings(abi: &Abi, used: &[UsedFunction], builtins: &mut Builtins) -> Result<String> {
    let mut imports = Imports::default();
    let mut funcs = String::new();

    for uf in used {
        let f = abi.function(&uf.name).expect("checked in inspect");
        let mut body = Body {
            imports: &mut imports,
            builtins,
        };
        let fb = function_body(f, uf, &mut body)?;
        let params_str = wat_params_typed(&f.params, &uf.param_types, f.variadic);
        let result_str = uf
            .result_type
            .map(|t| format!(" (result {})", wire_wat(t)))
            .unwrap_or_default();
        // A non-null result the module asked for: JS handed back externref,
        // so a null here is a contract violation and traps.
        let value = if uf.result_type.is_some_and(narrowed) {
            format!("(ref.as_non_null {})", fb.value)
        } else {
            fb.value
        };
        let _ = writeln!(
            funcs,
            "  (func (export {:?}){}{}{}\n    {}{value})",
            f.name, params_str, result_str, fb.locals, fb.stmts
        );
    }

    let mut start = String::new();
    if imports.boolean {
        let mut body = Body {
            imports: &mut imports,
            builtins,
        };
        let g = body.global();
        let boolean = body.get((&g, R), "Boolean", R);
        let _ = writeln!(
            start,
            "  (global $Boolean (mut externref) (ref.null extern))\n  (start $init)\n  (func $init\n    (global.set $Boolean {boolean}))"
        );
    }

    Ok(format!(
        ";; Generated by ts-gen. Wasm implementation of the ABI over the JS primitives in builtins.js.\n(module\n{}\n{start}{funcs})\n",
        imports.render()
    ))
}

fn function_body(f: &AbiFunction, uf: &UsedFunction, body: &mut Body) -> Result<FuncBody> {
    // An i32 argument is a TS boolean (`Lower::of`), which JS would
    // otherwise receive as the Number 1/0; route it through `Boolean()`.
    let skip_receiver = if has_receiver(&f.op) { 1 } else { 0 };
    let args: Vec<(String, WasmType)> = f
        .params
        .iter()
        .zip(&uf.param_types)
        .skip(skip_receiver)
        .map(|(p, ty)| match ty {
            WasmType::I32 => {
                body.imports.boolean = true;
                let g = body.global();
                let call = body.call(
                    "(global.get $Boolean)",
                    (&g, R),
                    &[(format!("(local.get ${})", p.name), WasmType::I32)],
                    Some(R),
                );
                (call, R)
            }
            _ => (format!("(local.get ${})", p.name), *ty),
        })
        .collect();
    function_body_inner(f, uf, body, &args)
}

fn function_body_inner(
    f: &AbiFunction,
    uf: &UsedFunction,
    body: &mut Body,
    args: &[(String, WasmType)],
) -> Result<FuncBody> {
    let this_expr = f
        .params
        .first()
        .map(|p| format!("(local.get ${})", p.name))
        .unwrap_or_default();
    let this = (
        this_expr.as_str(),
        uf.param_types.first().copied().unwrap_or(R),
    );
    let value = || {
        args.first()
            .map(|(v, t)| (v.as_str(), *t))
            .ok_or_else(|| anyhow!("{}: setter without a value", f.name))
    };

    let result = uf.result_type.unwrap_or(R);
    let module = f.module.as_deref();
    let expr = match &f.op {
        AbiOp::Get { name } => body.get(this, name, result),
        AbiOp::Set { name } => body.set(this, name, value()?),
        AbiOp::Call { name } => {
            let func = body.get(this, name, R);
            return invoke(body, &func, this, args, f, uf.result_type);
        }
        AbiOp::Construct { path } => {
            let class = body.path(module, path);
            if f.variadic || args.len() > MAX_ARITY {
                let (locals, stmts) = collect_args(body, args, f);
                let value = body.construct_apply(&class, "(local.get $arr)", result);
                return Ok(FuncBody {
                    locals,
                    stmts,
                    value,
                });
            }
            body.construct(&class, args, result)
        }
        // The export is an externref global; a module reading it as
        // anyref converts, and `generate_bindings` narrows a `(ref any)`.
        AbiOp::StaticGet { path, name } if module.is_some() && path.is_empty() => {
            let export = body.static_ref(module, path, name);
            match result {
                WasmType::AnyRef { .. } => format!("(any.convert_extern {export})"),
                _ => export,
            }
        }
        AbiOp::StaticGet { path, name } => {
            let recv = body.path(module, path);
            body.get((&recv, R), name, result)
        }
        AbiOp::StaticSet { path, name } => {
            if module.is_some() && path.is_empty() {
                bail!("{}: module exports cannot be assigned", f.name);
            }
            let recv = body.path(module, path);
            body.set((&recv, R), name, value()?)
        }
        AbiOp::StaticCall { path, name } => {
            // A module's own export is called with globalThis as `this`,
            // as an ESM free function would be.
            let recv = match (module, path.is_empty()) {
                (Some(_), true) => body.global(),
                _ => body.path(module, path),
            };
            let func = body.static_ref(module, path, name);
            return invoke(body, &func, (&recv, R), args, f, uf.result_type);
        }
        AbiOp::NewObject => {
            let object = body.path(None, &["Object".to_string()]);
            body.construct(&object, &[], result)
        }
        AbiOp::IsUndefined => {
            body.builtins.is_undefined = true;
            let f = body
                .imports
                .func("is_undefined", vec![this.1], Some(WasmType::I32));
            format!("(call ${f} {})", this.0)
        }
        AbiOp::NumberValue => {
            let number = body.path(None, &["Number".to_string()]);
            let g = body.global();
            body.call(
                &number,
                (&g, R),
                &[(this.0.to_string(), this.1)],
                Some(WasmType::F64),
            )
        }
    };
    Ok(FuncBody {
        locals: String::new(),
        stmts: String::new(),
        value: expr,
    })
}

fn invoke(
    body: &mut Body,
    func: &str,
    this: (&str, WasmType),
    args: &[(String, WasmType)],
    f: &AbiFunction,
    result: Option<WasmType>,
) -> Result<FuncBody> {
    if f.variadic || args.len() > MAX_ARITY {
        let (locals, stmts) = collect_args(body, args, f);
        let value = body.apply(func, this, "(local.get $arr)", result);
        Ok(FuncBody {
            locals,
            stmts,
            value,
        })
    } else {
        Ok(FuncBody {
            locals: String::new(),
            stmts: String::new(),
            value: body.call(func, this, args, result),
        })
    }
}

/// Gather `args` into a JS array in `$arr` for `Reflect.apply` /
/// `Reflect.construct`: the fixed arguments one `push` each, then the
/// elements of the GC rest array when the function is variadic. Returns
/// the locals and setup statements.
fn collect_args(body: &mut Body, args: &[(String, WasmType)], f: &AbiFunction) -> (String, String) {
    let (fixed, rest) = if f.variadic {
        let rest = f.params.last().expect("variadic function has a rest param");
        (&args[..args.len() - 1], Some(rest.name.as_str()))
    } else {
        (args, None)
    };
    let mut locals = String::from(" (local $arr externref) (local $push externref)");
    let mut stmts = String::new();
    let new_arr = body.new_array();
    let _ = writeln!(stmts, "(local.set $arr {new_arr})");
    let push_fn = body.get(("(local.get $arr)", R), "push", R);
    let _ = writeln!(stmts, "    (local.set $push {push_fn})");
    for (val, ty) in fixed {
        let push = body.call(
            "(local.get $push)",
            ("(local.get $arr)", R),
            &[(val.clone(), *ty)],
            None,
        );
        let _ = writeln!(stmts, "    {push}");
    }
    if let Some(rest) = rest {
        body.imports.anyrefs_type = true;
        locals.push_str(" (local $rest (ref null $anyrefs)) (local $i i32) (local $len i32)");
        let push_rest = body.call(
            "(local.get $push)",
            ("(local.get $arr)", R),
            &[(
                "(array.get $anyrefs (local.get $rest) (local.get $i))".into(),
                ANY,
            )],
            None,
        );
        let _ = writeln!(
            stmts,
            "    (local.set $rest (local.get ${rest}))\n    (if (ref.is_null (local.get $rest)) (then) (else\n      (local.set $len (array.len (local.get $rest)))\n      (block $break (loop $continue\n        (br_if $break (i32.ge_u (local.get $i) (local.get $len)))\n        {push_rest}\n        (local.set $i (i32.add (local.get $i) (i32.const 1)))\n        (br $continue)))))"
        );
    }
    stmts.push_str("    ");
    (locals, stmts)
}

fn has_receiver(op: &AbiOp) -> bool {
    matches!(
        op,
        AbiOp::Get { .. }
            | AbiOp::Set { .. }
            | AbiOp::Call { .. }
            | AbiOp::IsUndefined
            | AbiOp::NumberValue
    )
}

/// The module namespace the wrapper publishes: each distinct first path
/// segment becomes an exported global holding either the function itself
/// or an object tree of them.
#[derive(Default)]
struct Namespace<'a> {
    children: std::collections::BTreeMap<String, Namespace<'a>>,
    func: Option<&'a HandlerInfo>,
}

fn namespace<'a>(handlers: &'a [HandlerInfo]) -> Result<Namespace<'a>> {
    let mut root = Namespace::default();
    for h in handlers {
        let mut node = &mut root;
        for seg in &h.path {
            node = node.children.entry(seg.clone()).or_default();
        }
        node.func = Some(h);
    }
    fn check(path: &str, n: &Namespace) -> Result<()> {
        if n.func.is_some() && !n.children.is_empty() {
            bail!("export {path:?} is both a function and an object");
        }
        for (k, c) in &n.children {
            check(&format!("{path}.{k}"), c)?;
        }
        Ok(())
    }
    for (k, c) in &root.children {
        check(k, c)?;
    }
    Ok(root)
}

fn generate_wrapper(
    handlers: &[HandlerInfo],
    jspi: bool,
    module_specifier: &str,
    builtins: &mut Builtins,
) -> Result<String> {
    if handlers.is_empty() {
        return Ok(";; Generated by ts-gen. No --provides: the module's own exports pass through.\n(module)\n".into());
    }
    let ns = namespace(handlers)?;
    let mut imports = Imports::default();
    let mut init = String::new();
    let mut locals = 0usize;
    {
        let mut body = Body {
            imports: &mut imports,
            builtins,
        };
        // The exported function object: a funcref handed to a JS import
        // arrives as the exported-function object, which is what the
        // namespace and `promising` need.
        let mut func_value =
            |body: &mut Body, h: &HandlerInfo, top_level: bool| -> (String, WasmType) {
                let value = (
                    format!("(ref.func ${})", handler_ident(&h.name)),
                    WasmType::FuncRef,
                );
                if jspi {
                    let wa = body.path(None, &["WebAssembly".to_string()]);
                    let promising = body.get((&wa, R), "promising", R);
                    (body.call(&promising, (&wa, R), &[value], Some(R)), R)
                } else if top_level {
                    // An externref global cannot hold a funcref; `Object(f)`
                    // returns the same function object as an externref.
                    let object = body.path(None, &["Object".to_string()]);
                    let g = body.global();
                    (body.call(&object, (&g, R), &[value], Some(R)), R)
                } else {
                    value
                }
            };
        // Builds `node` into a fresh object local and returns `(local.get $oN)`.
        fn build_object(
            body: &mut Body,
            node: &Namespace,
            locals: &mut usize,
            init: &mut String,
            func_value: &mut dyn FnMut(&mut Body, &HandlerInfo, bool) -> (String, WasmType),
        ) -> String {
            let name = format!("$o{locals}");
            *locals += 1;
            let ctor = body.path(None, &["Object".to_string()]);
            let new_object = body.construct(&ctor, &[], R);
            let _ = writeln!(init, "    (local.set {name} {new_object})");
            let local = format!("(local.get {name})");
            for (key, child) in &node.children {
                let value = match child.func {
                    Some(h) => func_value(body, h, false),
                    None => (build_object(body, child, locals, init, func_value), R),
                };
                let set = body.set((&local, R), key, (&value.0, value.1));
                let _ = writeln!(init, "    {set}");
            }
            local
        }
        for (name, node) in &ns.children {
            let value = match node.func {
                Some(h) => func_value(&mut body, h, true).0,
                None => build_object(&mut body, node, &mut locals, &mut init, &mut func_value),
            };
            let _ = writeln!(init, "    (global.set ${} {value})", export_ident(name));
        }
    }

    let mut handler_imports = String::new();
    for h in handlers {
        let params: String = h
            .param_types
            .iter()
            .enumerate()
            .map(|(i, ty)| format!(" (param $p{} {})", i, wire_wat(*ty)))
            .collect();
        let result = h
            .result_type
            .map(|t| format!(" (result {})", wire_wat(t)))
            .unwrap_or_default();
        let _ = writeln!(
            handler_imports,
            "  (import {module_specifier:?} {:?} (func ${}{}{}))",
            h.name,
            handler_ident(&h.name),
            params,
            result
        );
    }
    let declared = handlers
        .iter()
        .map(|h| format!(" ${}", handler_ident(&h.name)))
        .collect::<String>();
    let globals: String = ns
        .children
        .keys()
        .map(|name| {
            format!(
                "  (global ${} (export \"{PUBLISHED_PREFIX}{name}\") (mut externref) (ref.null extern))\n",
                export_ident(name)
            )
        })
        .collect();
    let local_decls: String = (0..locals)
        .map(|i| format!(" (local $o{i} externref)"))
        .collect();

    Ok(format!(
        ";; Generated by ts-gen. Publishes the module's exports at their JS paths{}.\n(module\n{}{handler_imports}  (elem declare func{declared})\n{globals}  (start $init)\n  (func $init{local_decls}\n{}))\n",
        if jspi { " through WebAssembly.promising" } else { "" },
        imports.render(),
        init.trim_end(),
    ))
}

/// Export-name prefix on the wrapper's published globals until `postprocess`.
const PUBLISHED_PREFIX: &str = "ts-gen:export.";

fn export_ident(name: &str) -> String {
    format!("export.{}", ident(name))
}

fn handler_ident(name: &str) -> String {
    format!("handler.{}", ident(name))
}

/// Group names by owner for diagnostics and listings.
pub fn group_by_owner(names: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in names {
        let (owner, member) = n.rsplit_once('.').unwrap_or(("", n));
        groups
            .entry(owner.to_string())
            .or_default()
            .push(member.to_string());
    }
    groups
}
