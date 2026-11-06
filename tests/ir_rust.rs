//! Audit: how far is the ts-gen IR from being a sufficient input for the
//! Rust generator?
//!
//! The IR is modelled on the Rust bindings: its member set should equal
//! the set of `#[wasm_bindgen] extern "C"` fns ts-gen emits for the same
//! input, so the Rust generator could one day run on the IR alone. For
//! every fixture this builds the IR the way `--target wat` does, parses
//! the blessed `default.rs` snapshot, and matches each extern to an IR
//! member by module, owner and Rust name:
//!
//! * **direct**: the member is the binding;
//! * **derived**: a Rust-only presentation of a member, by a named rule
//!   (see [`Rule`]);
//! * **renamed**: no member of that name, but one with the same owner,
//!   operation, JS name and arity is left over; a naming divergence;
//! * **rust-only** / **abi-only**: no counterpart.
//!
//! Direct, renamed and private-binding matches are also compared by
//! parameter count and type correspondence, and every kind with the Rust
//! enum of its name, by variant names, count and aliases. The report goes to
//! `target/ir-rust-report.md`. Any rust-only, abi-only, renamed or
//! mismatch entry not in [`KNOWN`] fails the test, as does a [`KNOWN`]
//! entry that no longer occurs; `IR_RUST_REPORT=1` prints the report (with
//! `-- --nocapture`) and does not fail on divergences.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use quote::ToTokens;
use syn::{FnArg, ForeignItem, Item, Meta, Pat, ReturnType, Type};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

// ---------------------------------------------------------------------------
// The IR side, as a small model `ir::Ir` maps to.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Op {
    Constructor,
    Object,
    Get,
    Set,
    Method,
    StaticGet,
    StaticSet,
    StaticMethod,
}

impl Op {
    fn has_receiver(self) -> bool {
        matches!(self, Op::Get | Op::Set | Op::Method)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum H {
    Any,
    String,
    Number,
    Boolean,
    BigInt,
    Named(Option<String>, String),
    Var(String),
    Callback,
    Tuple,
    StringLiteral,
    IntLiteral,
    /// Only the empties the position admits.
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
enum Ty {
    Void,
    F64,
    I64,
    Bool,
    Js {
        head: H,
        null: bool,
        undefined: bool,
    },
}

impl Ty {
    fn admits_empty(&self) -> bool {
        matches!(self, Ty::Js { null, undefined, .. } if *null || *undefined)
    }

    fn non_empty(&self) -> Ty {
        match self {
            Ty::Js { head, .. } => Ty::Js {
                head: head.clone(),
                null: false,
                undefined: false,
            },
            t => t.clone(),
        }
    }

    fn show(&self) -> String {
        match self {
            Ty::Void => "void".into(),
            Ty::F64 => "f64".into(),
            Ty::I64 => "i64".into(),
            Ty::Bool => "i32".into(),
            Ty::Js {
                head,
                null,
                undefined,
            } => {
                let mut s = match head {
                    H::Any => "any".into(),
                    H::String => "string".into(),
                    H::Number => "number".into(),
                    H::Boolean => "boolean".into(),
                    H::BigInt => "bigint".into(),
                    H::Named(Some(m), p) => format!("\"{m}\".{p}"),
                    H::Named(None, p) => p.clone(),
                    H::Var(v) => format!("var {v}"),
                    H::Callback => "callback".into(),
                    H::Tuple => "tuple".into(),
                    H::StringLiteral => "string literal".into(),
                    H::IntLiteral => "int literal".into(),
                    H::Empty => "empty".into(),
                };
                if *null {
                    s.push_str(" null");
                }
                if *undefined {
                    s.push_str(" undefined");
                }
                format!("ref({s})")
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Binding {
    module: Option<String>,
    owner: Option<String>,
    /// The import name's member segment.
    name: String,
    /// Further owner paths the member is imported under.
    alias_owners: Vec<String>,
    js: Option<String>,
    op: Op,
    /// Without the receiver.
    params: Vec<Ty>,
    result: Ty,
    /// The binding itself catches. `None` when the source does not model
    /// it, as for `nothrow`.
    throws: Option<bool>,
    /// No catching form derives from the binding.
    nothrow: Option<bool>,
    variadic: bool,
    ts: String,
}

impl Binding {
    fn import(&self) -> String {
        let name = match &self.owner {
            Some(o) => format!("{o}.{}", self.name),
            None => self.name.clone(),
        };
        match &self.module {
            Some(m) => format!("\"{m}\" \"{name}\""),
            None => format!("\"{name}\""),
        }
    }
}

/// A kind's or enum's arms, each with its aliases and whether it is a
/// literal (a unit variant), and its `fallback`: the order and the open
/// last arm recognition relies on.
#[derive(Clone, Debug, PartialEq)]
struct KindArms {
    arms: Vec<(String, Vec<String>, bool)>,
    fallback: bool,
}

#[derive(Default)]
struct Model {
    bindings: Vec<Binding>,
    /// `(module, path)` of every kind.
    kinds: HashSet<(Option<String>, String)>,
    /// Every kind's binding name, its arms and whether it is `fallback`.
    kind_arms: Vec<(Option<String>, String, KindArms)>,
    /// What is at each owner's path; dictionaries are interfaces.
    owners: HashMap<(Option<String>, String), OwnerKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerKind {
    Class,
    Interface,
    Namespace,
}

/// The model of the IR. The arms of a literal cohort are one binding,
/// the cohort's, which takes each literal as a string.
fn from_ir(ir: &ts_gen_abi::ir::Ir) -> Model {
    use ts_gen_abi::ir::{ClassKind, Head as IrHead, Literal, Member, MemberKind, Type as IrType};
    fn ty(t: &IrType) -> Ty {
        match t {
            IrType::Void => Ty::Void,
            IrType::F64 => Ty::F64,
            IrType::I64 => Ty::I64,
            IrType::Bool => Ty::Bool,
            IrType::Js(j) => Ty::Js {
                head: match &j.head {
                    IrHead::Any => H::Any,
                    IrHead::String => H::String,
                    IrHead::Number => H::Number,
                    IrHead::Boolean => H::Boolean,
                    IrHead::BigInt => H::BigInt,
                    IrHead::Named(n) => H::Named(n.name.module.clone(), n.name.path.clone()),
                    IrHead::Var(v) => H::Var(v.clone()),
                    IrHead::Callback { .. } => H::Callback,
                    IrHead::Tuple(_) => H::Tuple,
                    IrHead::Literal(Literal::String(_)) => H::StringLiteral,
                    IrHead::Literal(Literal::Int(_)) => H::IntLiteral,
                    IrHead::Empty => H::Empty,
                },
                null: j.null,
                undefined: j.undefined,
            },
        }
    }
    let binding = |module: &Option<String>, owner: Option<&str>, aliases: &[String], m: &Member| {
        let op = match m.kind {
            MemberKind::Constructor => Op::Constructor,
            MemberKind::Object => Op::Object,
            MemberKind::Get => Op::Get,
            MemberKind::Set => Op::Set,
            MemberKind::Method => Op::Method,
            MemberKind::StaticGet => Op::StaticGet,
            MemberKind::StaticSet => Op::StaticSet,
            MemberKind::StaticMethod => Op::StaticMethod,
        };
        let skip =
            usize::from(op.has_receiver() && m.params.first().is_some_and(|p| p.name == "this"));
        let param = |t: &IrType| match (ty(t), m.cohort.is_some()) {
            (
                Ty::Js {
                    head: H::StringLiteral,
                    null,
                    undefined,
                },
                true,
            ) => Ty::Js {
                head: H::String,
                null,
                undefined,
            },
            (t, _) => t,
        };
        Binding {
            module: m.module.clone().or_else(|| module.clone()),
            owner: owner.map(String::from),
            name: m.cohort.clone().unwrap_or_else(|| m.name.clone()),
            alias_owners: aliases.to_vec(),
            js: Some(m.js.clone()),
            op,
            params: m.params[skip..].iter().map(|p| param(&p.ty)).collect(),
            result: ty(&m.result),
            throws: Some(m.throws),
            nothrow: Some(m.nothrow),
            variadic: m.variadic,
            ts: String::new(),
        }
    };
    let mut bindings: Vec<Binding> = Vec::new();
    let mut push = |b: Binding| {
        let seen = bindings
            .iter()
            .any(|o| o.module == b.module && o.owner == b.owner && o.name == b.name);
        if !seen {
            bindings.push(b);
        }
    };
    for c in &ir.classes {
        for m in &c.members {
            push(binding(&c.module, Some(&c.name), &c.aliases, m));
        }
    }
    for f in &ir.functions {
        push(binding(&f.module, None, &[], &f.member));
    }
    Model {
        bindings,
        kinds: ir
            .kinds
            .iter()
            .map(|k| (k.module.clone(), k.name.clone()))
            .collect(),
        kind_arms: ir
            .kinds
            .iter()
            .map(|k| {
                let arms = k
                    .arms
                    .iter()
                    .map(|a| {
                        let literal = matches!(a.ty.head, IrHead::Literal(_));
                        (a.name.clone(), a.aliases.clone(), literal)
                    })
                    .collect();
                let arms = KindArms {
                    arms,
                    fallback: k.fallback,
                };
                (k.module.clone(), k.name.clone(), arms)
            })
            .collect(),
        owners: ir
            .classes
            .iter()
            .map(|c| {
                let kind = match c.kind {
                    ClassKind::Class => OwnerKind::Class,
                    ClassKind::Interface => OwnerKind::Interface,
                    ClassKind::Namespace => OwnerKind::Namespace,
                };
                ((c.module.clone(), c.name.clone()), kind)
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Building the IR for a fixture, as `ts-gen --target wat` does.

#[derive(Default)]
struct Args {
    inputs: Vec<String>,
    lib_name: Option<String>,
    exports: Vec<String>,
    externals: Vec<String>,
    error_type: Option<String>,
    error_enum: Option<String>,
    errors_as_error: bool,
    fallible: bool,
    provides: Option<String>,
    no_web_sys: bool,
}

fn directive_args(fixture: &Path) -> Vec<String> {
    let content = std::fs::read_to_string(fixture).unwrap_or_default();
    let mut args = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("//! @ts-gen ") {
            args.extend(rest.split_whitespace().map(String::from));
        } else if !trimmed.starts_with("//") && !trimmed.is_empty() {
            break;
        }
    }
    args
}

fn parse_args(tokens: &[String]) -> Args {
    let mut args = Args::default();
    let mut i = 0;
    let values = |i: &mut usize| {
        let mut out = Vec::new();
        while *i + 1 < tokens.len() && !tokens[*i + 1].starts_with('-') {
            *i += 1;
            out.push(tokens[*i].clone());
        }
        out
    };
    while i < tokens.len() {
        let flag = tokens[i].as_str();
        let vals = values(&mut i);
        let one = || vals.first().cloned();
        match flag {
            "--input" | "-i" => args.inputs.extend(vals.iter().cloned()),
            "--export" | "-E" => args.exports.extend(vals.iter().cloned()),
            "--external" | "-e" => args.externals.extend(vals.iter().cloned()),
            "--lib-name" => args.lib_name = one(),
            "--error-type" => args.error_type = one(),
            "--error-enum" => args.error_enum = Some(one().unwrap_or("JsException".into())),
            "--errors-as-error" => args.errors_as_error = true,
            "--fallible" => args.fallible = true,
            "--provides" => args.provides = one(),
            "--no-web-sys" => args.no_web_sys = true,
            other => panic!("unsupported fixture flag {other}"),
        }
        i += 1;
    }
    args
}

fn dts_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for e in entries {
            dts_files(&e, out);
        }
    } else if path.to_string_lossy().ends_with(".d.ts") {
        out.push(path.to_path_buf());
    }
}

/// `main.rs`'s `resolve_exports`, with paths read from the crate root.
fn resolve_exports(
    args: &Args,
    inputs: &[PathBuf],
    gctx: &ts_gen::context::GlobalContext,
) -> HashSet<ts_gen::codegen::ExportSpec> {
    use ts_gen::codegen::ExportSpec;
    let is_decl_name = |spec: &str| {
        spec.split('.').all(|seg| {
            let mut chars = seg.chars();
            matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
    };
    let mut exports = HashSet::new();
    for spec in &args.exports {
        let path = crate_root().join(spec);
        if path.exists() {
            let mut files = Vec::new();
            dts_files(&path, &mut files);
            for f in files {
                exports.insert(ExportSpec::File(f.canonicalize().unwrap_or(f)));
            }
        } else if gctx.find_module(spec).is_none()
            && !gctx.ambient_module_scopes.contains_key(spec)
            && args.lib_name.as_deref() != Some(spec.as_str())
            && is_decl_name(spec)
        {
            exports.insert(ExportSpec::Decl(spec.clone()));
        } else {
            exports.insert(ExportSpec::Module(spec.clone()));
        }
    }
    if args.exports.is_empty() {
        for p in inputs {
            exports.insert(ExportSpec::File(p.canonicalize().unwrap_or(p.clone())));
        }
    }
    exports
}

/// What [`build_model`] builds the model from. Switching to the IR is
/// `ts_gen::abi::build_ir(...)` through [`from_ir`] there, and this text.
const SOURCE: &str = "IR (`ts_gen::abi::build_ir`)";

/// Known divergences: fixture, class (`rust-only`, `abi-only`, `renamed`
/// or `mismatch`), the IR import and the category, each for its reason.
const KNOWN: &[(&str, &str, &str, &str)] = &[
    // `MaybeDay = DayLike | null | undefined`: the IR position admits the
    // alias's empties, which Rust's `MaybeDay` enum, having no empty
    // variant, cannot take; Rust covers them with `pick()` and
    // `pick_with_null` instead.
    (
        "union-alias-reads",
        "mismatch",
        "\"union-alias-reads\" \"Day.pick_with_other\"",
        "param: IR admits null/undefined, Rust not optional",
    ),
];

/// The model of a fixture, and whether its template matches the blessed
/// `default.wat` (a stale snapshot would make the audit compare apples
/// with pears).
fn build_model(fixture: &Path, args: &Args) -> anyhow::Result<(Model, bool)> {
    let root = crate_root();
    let mut inputs = vec![fixture.to_path_buf()];
    inputs.extend(args.inputs.iter().map(|p| root.join(p)));
    let (module, mut gctx) = ts_gen::parse::parse_dts_files(&inputs, args.lib_name.as_deref())?;
    if args.no_web_sys {
        gctx.external_map.clear_defaults();
    }
    for m in &args.externals {
        gctx.external_map.add_mappings(m);
    }
    let exports = resolve_exports(args, &inputs, &gctx);
    let provided = match &args.provides {
        Some(spec) => {
            let path = root.join(spec);
            let spec = if path.is_file() {
                path.to_string_lossy().into_owned()
            } else {
                spec.clone()
            };
            ts_gen::provides::resolve(&spec, &module, &mut gctx)?
        }
        None => Vec::new(),
    };
    let mut options = ts_gen::codegen::GenerateOptions::new()
        .errors_as_error(args.errors_as_error)
        .fallible(args.fallible)
        .exports(exports);
    if let Some(path) = &args.error_type {
        options = options.error_type(path);
    }
    if let Some(name) = &args.error_enum {
        options = options.error_enum(name);
    }
    let ir = ts_gen::abi::build_ir(&module, &gctx, &options, &provided);
    // A type parameter out of scope reads back as an undeclared global.
    let text = ts_gen_abi::text::emit(&ir);
    if ts_gen_abi::text::parse(&text).as_ref() != Ok(&ir) {
        anyhow::bail!("the IR model does not read back from its text");
    }
    let base = fixture
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace(".d.ts", "");
    let blessed = std::fs::read_to_string(root.join(format!("tests/snapshots/{base}/default.wat")));
    let fresh = blessed.is_ok_and(|w| w == ts_gen::abi::wat::generate(&ir));
    Ok((from_ir(&ir), fresh))
}

// ---------------------------------------------------------------------------
// The Rust side: the externs of a `default.rs` snapshot.

#[derive(Clone, Debug)]
struct Extern {
    rust_mod: String,
    /// The Rust module the owner type is declared in.
    owner_mod: String,
    module: Option<String>,
    owner: Option<String>,
    name: String,
    public: bool,
    js: String,
    op: Op,
    catch: bool,
    variadic: bool,
    is_async: bool,
    /// Without `this`.
    params: Vec<Type>,
    ret: Option<Type>,
    generics: Vec<String>,
    sig: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnumShape {
    StringLiterals,
    Numeric,
    Union,
    Error,
}

#[derive(Default)]
struct RustFile {
    externs: Vec<Extern>,
    /// Rust type ident -> `(module, JS path)` of each extern type of that name.
    types: HashMap<String, Vec<(Option<String>, String)>>,
    /// Extern types by (Rust mod, ident), for owner resolution.
    types_in_mod: HashMap<(String, String), (Option<String>, String)>,
    enums: HashMap<String, EnumShape>,
    /// `(Rust mod, ident, variants, fallback)` of every enum but an error
    /// enum, each variant with whether it is a unit variant.
    enum_variants: Vec<(String, String, Vec<(String, bool)>, bool)>,
    /// `(Rust mod, enum, alias, variant)`: `impl E { const A: E = E::V; }`.
    enum_aliases: Vec<(String, String, String, String)>,
    /// `(Rust mod, module, JS path, has is_type_of)` of every extern type.
    type_decls: Vec<(String, Option<String>, String, bool)>,
    /// `pub type A = B;` and `use B as A;`.
    aliases: HashMap<String, Type>,
    /// Private names called from a wrapper, `Display` or a builder.
    called: HashSet<String>,
    /// `(module, Rust mod, JS path)` of types with an inline `new()`:
    /// dictionaries.
    inline_new: HashSet<(Option<String>, String, String)>,
    /// Owners with a `Display` impl.
    display: HashSet<String>,
    /// `--external` mappings: JS path -> Rust type ident.
    externals: HashMap<String, String>,
    /// Presentation rules applied while comparing types.
    presented: std::cell::RefCell<BTreeMap<Rule, usize>>,
    extras: Extras,
}

/// Counts of the non-extern output (step 5 of the audit).
#[derive(Default, Clone)]
struct Extras {
    string_enums: usize,
    numeric_enums: usize,
    union_enums: usize,
    fallback_enums: usize,
    error_enums: usize,
    enum_alias_consts: usize,
    union_from_impls: usize,
    builders: usize,
    required_field_builders: usize,
    literal_wrappers: usize,
    null_wrappers: usize,
    into_wrappers: usize,
    display_impls: usize,
    extends_attrs: usize,
    is_type_of: usize,
    generic_types: usize,
    generic_fns: usize,
    method_generics: usize,
    docs: usize,
    type_aliases: usize,
    external_uses: usize,
    jsvalue_uses: usize,
    statics: usize,
    slice_attrs: usize,
    cfg_splits: usize,
    error_helpers: usize,
    lib_blocks: usize,
    js_named_types: usize,
    typed_errors: usize,
    generic_arg_sigs: usize,
    provides_files: usize,
    try_forms: usize,
}

impl Extras {
    fn add(&mut self, o: &Extras) {
        macro_rules! sum {
            ($($f:ident),*) => { $(self.$f += o.$f;)* };
        }
        sum!(
            string_enums,
            numeric_enums,
            union_enums,
            fallback_enums,
            error_enums,
            enum_alias_consts,
            union_from_impls,
            builders,
            required_field_builders,
            literal_wrappers,
            null_wrappers,
            into_wrappers,
            display_impls,
            extends_attrs,
            is_type_of,
            generic_types,
            generic_fns,
            method_generics,
            docs,
            type_aliases,
            external_uses,
            jsvalue_uses,
            statics,
            slice_attrs,
            cfg_splits,
            error_helpers,
            lib_blocks,
            js_named_types,
            typed_errors,
            generic_arg_sigs,
            provides_files,
            try_forms
        );
    }
}

/// The `#[wasm_bindgen(...)]` arguments of an item, as `(key, value)`
/// token strings (`value` empty for a flag).
fn wb_args(attrs: &[syn::Attribute]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for a in attrs {
        if !a.path().is_ident("wasm_bindgen") {
            continue;
        }
        let Ok(list) = a
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };
        for m in list {
            match m {
                Meta::Path(p) => out.push((p.to_token_stream().to_string(), String::new())),
                Meta::NameValue(nv) => out.push((
                    nv.path.to_token_stream().to_string(),
                    nv.value.to_token_stream().to_string(),
                )),
                Meta::List(l) => out.push((l.path.to_token_stream().to_string(), String::new())),
            }
        }
    }
    out
}

fn arg<'a>(args: &'a [(String, String)], key: &str) -> Option<&'a str> {
    args.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn has(args: &[(String, String)], key: &str) -> bool {
    args.iter().any(|(k, _)| k == key)
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

/// `["A" , "B"]` -> `["A", "B"]`.
fn namespace(v: Option<&str>) -> Vec<String> {
    let Some(v) = v else { return Vec::new() };
    v.trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(unquote)
        .filter(|s| !s.is_empty())
        .collect()
}

fn tidy(s: &str) -> String {
    let mut s = s.to_string();
    for (from, to) in [
        (" :: ", "::"),
        (":: ", "::"),
        (" : ", ": "),
        ("& ", "&"),
        (" < ", "<"),
        ("< ", "<"),
        (" <", "<"),
        (" >", ">"),
        (" ,", ","),
        ("( ", "("),
        (" )", ")"),
        (" (", "("),
        ("# ", "#"),
    ] {
        s = s.replace(from, to);
    }
    s
}

fn unraw(ident: &syn::Ident) -> String {
    let s = ident.to_string();
    s.strip_prefix("r#").unwrap_or(&s).to_string()
}

fn last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Reference(r) => last_ident(&r.elem),
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        Type::Group(g) => last_ident(&g.elem),
        Type::Paren(p) => last_ident(&p.elem),
        _ => None,
    }
}

fn generic_arg(ty: &Type, i: usize) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let syn::PathArguments::AngleBracketed(a) = &p.path.segments.last()?.arguments else {
        return None;
    };
    a.args
        .iter()
        .filter_map(|a| match a {
            syn::GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .nth(i)
}

fn idents(tokens: proc_macro2::TokenStream, out: &mut Vec<String>) {
    for t in tokens {
        match t {
            proc_macro2::TokenTree::Ident(i) => out.push(i.to_string()),
            proc_macro2::TokenTree::Group(g) => idents(g.stream(), out),
            _ => {}
        }
    }
}

fn parse_rust(source: &str) -> RustFile {
    let file = syn::parse_file(source).expect("snapshot parses");
    let mut rf = RustFile::default();
    // Types first: owners of methods may be declared after a use.
    collect_types(&file.items, "", &mut rf);
    collect_items(&file.items, "", &mut rf);
    rf
}

fn block_module(attrs: &[syn::Attribute]) -> Option<String> {
    arg(&wb_args(attrs), "module").map(unquote)
}

fn collect_types(items: &[Item], rust_mod: &str, rf: &mut RustFile) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    collect_types(items, &format!("{rust_mod}::{}", m.ident), rf);
                }
            }
            Item::ForeignMod(fm) => {
                let module = block_module(&fm.attrs);
                rf.extras.lib_blocks += usize::from(rust_mod.is_empty() && module.is_some());
                for fi in &fm.items {
                    if let ForeignItem::Type(t) = fi {
                        let args = wb_args(&t.attrs);
                        rf.extras.js_named_types += usize::from(has(&args, "js_name"));
                        // The IR names a class as Rust does: by the type's
                        // ident, under its JS namespaces.
                        // `Global_`, moved aside from the `js_sys` glob's
                        // `Global`, is the IR's `Global`.
                        let ident = t.ident.to_string();
                        let js = arg(&args, "js_name").map(unquote);
                        let mut path = namespace(arg(&args, "js_namespace"));
                        path.push(match js {
                            Some(js) if ident.strip_suffix('_') == Some(js.as_str()) => js,
                            _ => ident,
                        });
                        let entry = (module.clone(), path.join("."));
                        rf.types
                            .entry(t.ident.to_string())
                            .or_default()
                            .push(entry.clone());
                        rf.type_decls.push((
                            rust_mod.to_string(),
                            module.clone(),
                            entry.1.clone(),
                            has(&args, "is_type_of"),
                        ));
                        rf.types_in_mod
                            .insert((rust_mod.to_string(), t.ident.to_string()), entry);
                    }
                }
            }
            Item::Enum(e) => {
                let has_other = e.variants.iter().any(|v| v.ident == "Other");
                let all_str = e.variants.iter().all(|v| {
                    matches!(&v.discriminant, Some((_, syn::Expr::Lit(l))) if matches!(l.lit, syn::Lit::Str(_)))
                });
                let all_num = e.variants.iter().all(|v| {
                    v.fields.is_empty()
                        && matches!(&v.discriminant, Some((_, d)) if !matches!(d, syn::Expr::Lit(l) if matches!(l.lit, syn::Lit::Str(_))))
                });
                let is_wb = e.attrs.iter().any(|a| a.path().is_ident("wasm_bindgen"));
                let shape = if !is_wb && has_other {
                    EnumShape::Error
                } else if all_str {
                    EnumShape::StringLiterals
                } else if all_num {
                    EnumShape::Numeric
                } else {
                    EnumShape::Union
                };
                rf.enums.insert(e.ident.to_string(), shape);
                let fallback = has(&wb_args(&e.attrs), "fallback");
                if shape != EnumShape::Error {
                    rf.enum_variants.push((
                        rust_mod.to_string(),
                        e.ident.to_string(),
                        e.variants
                            .iter()
                            .map(|v| (unraw(&v.ident), v.fields.is_empty()))
                            .collect(),
                        fallback,
                    ));
                }
                let x = &mut rf.extras;
                match shape {
                    EnumShape::StringLiterals => x.string_enums += 1,
                    EnumShape::Numeric => x.numeric_enums += 1,
                    EnumShape::Union => x.union_enums += 1,
                    EnumShape::Error => x.error_enums += 1,
                }
                if fallback {
                    x.fallback_enums += 1;
                }
            }
            Item::Impl(imp) if imp.trait_.is_none() => {
                let Some(owner) = last_ident(&imp.self_ty) else {
                    continue;
                };
                for item in &imp.items {
                    let syn::ImplItem::Const(c) = item else {
                        continue;
                    };
                    let syn::Expr::Path(target) = &c.expr else {
                        continue;
                    };
                    let segments: Vec<String> = target
                        .path
                        .segments
                        .iter()
                        .map(|s| unraw(&s.ident))
                        .collect();
                    if let [ty, variant] = segments.as_slice() {
                        if *ty == owner && last_ident(&c.ty).as_ref() == Some(&owner) {
                            rf.enum_aliases.push((
                                rust_mod.to_string(),
                                owner.clone(),
                                unraw(&c.ident),
                                variant.clone(),
                            ));
                        }
                    }
                }
            }
            Item::Type(t) => {
                rf.aliases.insert(t.ident.to_string(), (*t.ty).clone());
                rf.extras.type_aliases += 1;
            }
            Item::Use(u) => {
                let text = u.tree.to_token_stream().to_string().replace(' ', "");
                if let Some((from, to)) = text.split_once("as") {
                    if let Ok(ty) = syn::parse_str::<Type>(from) {
                        rf.aliases.insert(to.to_string(), ty);
                    }
                    if from == "JsValue" {
                        rf.extras.jsvalue_uses += 1;
                    } else {
                        rf.extras.external_uses += 1;
                    }
                } else if text.starts_with("::") {
                    rf.extras.external_uses += 1;
                }
            }
            _ => {}
        }
    }
}

fn peel(ty: &Type) -> &Type {
    match ty {
        Type::Reference(r) => peel(&r.elem),
        Type::Group(g) => peel(&g.elem),
        Type::Paren(p) => peel(&p.elem),
        t => t,
    }
}

impl RustFile {
    /// The `(module, IR name)` of a Rust type named in `rust_mod`, and the
    /// Rust module it is declared in.
    fn owner_of(&self, rust_mod: &str, ty: &Type) -> Option<(Option<String>, String, String)> {
        let ident = last_ident(ty)?;
        // A path through modules (`super::super::rpc::Stub`) names the type
        // of the module it reaches.
        if let Type::Path(p) = peel(ty) {
            if p.path.segments.len() > 1 {
                let mut m: Vec<String> = rust_mod.split("::").map(String::from).collect();
                for seg in p.path.segments.iter().take(p.path.segments.len() - 1) {
                    match seg.ident.to_string().as_str() {
                        "super" => {
                            m.pop();
                        }
                        "self" => {}
                        other => m.push(other.to_string()),
                    }
                }
                let m = m.join("::");
                if let Some((module, path)) = self.types_in_mod.get(&(m.clone(), ident.clone())) {
                    return Some((module.clone(), path.clone(), m));
                }
            }
        }
        let ident = match self.aliases.get(&ident).and_then(last_ident) {
            Some(target) if !self.types.contains_key(&ident) => target,
            _ => ident,
        };
        if let Some((module, path)) = self
            .types_in_mod
            .get(&(rust_mod.to_string(), ident.clone()))
        {
            return Some((module.clone(), path.clone(), rust_mod.to_string()));
        }
        self.types
            .get(&ident)
            .and_then(|v| v.first().cloned())
            .map(|(module, path)| (module, path, rust_mod.to_string()))
    }
}

fn collect_items(items: &[Item], rust_mod: &str, rf: &mut RustFile) {
    for item in items {
        match item {
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    collect_items(items, &format!("{rust_mod}::{}", m.ident), rf);
                }
            }
            Item::ForeignMod(fm) => {
                let module = block_module(&fm.attrs);
                for fi in &fm.items {
                    match fi {
                        ForeignItem::Fn(f) => {
                            if let Some(e) = extern_fn(f, rust_mod, &module, rf) {
                                rf.externs.push(e);
                            }
                        }
                        ForeignItem::Static(s) => {
                            let args = wb_args(&s.attrs);
                            let ns = namespace(arg(&args, "js_namespace"));
                            rf.extras.statics += 1;
                            rf.externs.push(Extern {
                                rust_mod: rust_mod.to_string(),
                                owner_mod: rust_mod.to_string(),
                                module: module.clone(),
                                owner: (!ns.is_empty()).then(|| ns.join(".")),
                                name: s.ident.to_string(),
                                public: matches!(s.vis, syn::Visibility::Public(_)),
                                js: arg(&args, "js_name")
                                    .map(unquote)
                                    .unwrap_or(s.ident.to_string()),
                                op: Op::StaticGet,
                                catch: false,
                                variadic: false,
                                is_async: false,
                                params: Vec::new(),
                                ret: Some((*s.ty).clone()),
                                generics: Vec::new(),
                                sig: tidy(&format!(
                                    "static {}: {}",
                                    s.ident,
                                    s.ty.to_token_stream()
                                )),
                            });
                        }
                        ForeignItem::Type(t) => {
                            let args = wb_args(&t.attrs);
                            rf.extras.extends_attrs +=
                                args.iter().filter(|(k, _)| k == "extends").count();
                            rf.extras.is_type_of += usize::from(has(&args, "is_type_of"));
                            rf.extras.generic_types += usize::from(!t.generics.params.is_empty());
                            rf.extras.docs +=
                                usize::from(t.attrs.iter().any(|a| a.path().is_ident("doc")));
                        }
                        _ => {}
                    }
                }
            }
            Item::Impl(imp) => {
                let self_ty = imp.self_ty.as_ref();
                let is_display = imp.trait_.as_ref().is_some_and(|(_, p, _)| {
                    p.segments.last().is_some_and(|s| s.ident == "Display")
                });
                let is_from = imp
                    .trait_
                    .as_ref()
                    .is_some_and(|(_, p, _)| p.segments.last().is_some_and(|s| s.ident == "From"));
                let self_ident = last_ident(self_ty).unwrap_or_default();
                if is_from && rf.enums.get(&self_ident) == Some(&EnumShape::Union) {
                    rf.extras.union_from_impls += 1;
                }
                if is_display {
                    rf.extras.display_impls += 1;
                    if let Some((_, path, _)) = rf.owner_of(rust_mod, self_ty) {
                        rf.display.insert(path);
                    }
                }
                for ii in &imp.items {
                    match ii {
                        syn::ImplItem::Fn(f) => {
                            let mut ids = Vec::new();
                            idents(f.block.to_token_stream(), &mut ids);
                            let body = f.block.to_token_stream().to_string();
                            let private: Vec<_> = ids
                                .iter()
                                .filter(|i| i.starts_with("__"))
                                .cloned()
                                .collect();
                            if !private.is_empty() && !is_display && imp.trait_.is_none() {
                                if body.contains("JsValue :: NULL") {
                                    rf.extras.null_wrappers += 1;
                                } else if body.contains(". into ()") {
                                    rf.extras.into_wrappers += 1;
                                } else {
                                    rf.extras.literal_wrappers += 1;
                                }
                            }
                            rf.called.extend(private);
                            if imp.trait_.is_none() && body.contains("Object :: new ()") {
                                if let Some((m, p, _)) = rf.owner_of(rust_mod, self_ty) {
                                    rf.inline_new.insert((m, rust_mod.to_string(), p));
                                }
                            }
                            if f.sig.ident == "builder" && !f.sig.inputs.is_empty() {
                                rf.extras.required_field_builders += 1;
                            }
                        }
                        syn::ImplItem::Const(_) if rf.enums.contains_key(&self_ident) => {
                            rf.extras.enum_alias_consts += 1;
                        }
                        _ => {}
                    }
                }
            }
            Item::Fn(f) => {
                let mut ids = Vec::new();
                idents(f.block.to_token_stream(), &mut ids);
                let private: Vec<_> = ids
                    .iter()
                    .filter(|i| i.starts_with("__"))
                    .cloned()
                    .collect();
                if !private.is_empty() {
                    let body = f.block.to_token_stream().to_string();
                    if body.contains("JsValue :: NULL") {
                        rf.extras.null_wrappers += 1;
                    } else if body.contains(". into ()") {
                        rf.extras.into_wrappers += 1;
                    } else {
                        rf.extras.literal_wrappers += 1;
                    }
                }
                rf.called.extend(private);
            }
            Item::Struct(s) if s.ident.to_string().ends_with("Builder") => {
                rf.extras.builders += 1;
            }
            _ => {}
        }
    }
}

fn extern_fn(
    f: &syn::ForeignItemFn,
    rust_mod: &str,
    module: &Option<String>,
    rf: &mut RustFile,
) -> Option<Extern> {
    let args = wb_args(&f.attrs);
    let name = f.sig.ident.to_string();
    let mut params: Vec<Type> = Vec::new();
    let mut this: Option<Type> = None;
    for (i, input) in f.sig.inputs.iter().enumerate() {
        if let FnArg::Typed(pt) = input {
            let is_this = matches!(&*pt.pat, Pat::Ident(p) if p.ident == "this");
            if i == 0 && is_this {
                this = Some((*pt.ty).clone());
            } else {
                rf.extras.slice_attrs +=
                    usize::from(pt.attrs.iter().any(|a| a.path().is_ident("wasm_bindgen")));
                params.push((*pt.ty).clone());
            }
        }
    }
    let ret = match &f.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, t) => Some((**t).clone()),
    };
    let getter = has(&args, "getter");
    let setter = has(&args, "setter");
    let ns = namespace(arg(&args, "js_namespace"));
    let js_class = arg(&args, "js_class").map(unquote);
    let class_path = |js_class: &str| {
        let mut p = ns.clone();
        p.push(js_class.to_string());
        p.join(".")
    };
    let mut owner_mod = rust_mod.to_string();
    let (op, owner) = if has(&args, "constructor") {
        let ty = ret.as_ref().map(|r| match last_ident(r).as_deref() {
            Some("Result") => generic_arg(r, 0).cloned().unwrap_or(r.clone()),
            _ => r.clone(),
        });
        let owner = ty
            .and_then(|t| rf.owner_of(rust_mod, &t))
            .map(|o| {
                owner_mod = o.2;
                o.1
            })
            .or_else(|| js_class.as_deref().map(class_path));
        (Op::Constructor, owner)
    } else if has(&args, "method") {
        let owner = this
            .as_ref()
            .and_then(|t| rf.owner_of(rust_mod, t))
            .map(|o| {
                owner_mod = o.2;
                o.1
            })
            .or_else(|| js_class.as_deref().map(class_path));
        let op = if getter {
            Op::Get
        } else if setter {
            Op::Set
        } else {
            Op::Method
        };
        (op, owner)
    } else if let Some(of) = arg(&args, "static_method_of") {
        let ty: Type = syn::parse_str(of).ok()?;
        let owner = rf
            .owner_of(rust_mod, &ty)
            .map(|o| {
                owner_mod = o.2;
                o.1
            })
            .or_else(|| js_class.as_deref().map(class_path));
        let op = if getter {
            Op::StaticGet
        } else if setter {
            Op::StaticSet
        } else {
            Op::StaticMethod
        };
        (op, owner)
    } else {
        let op = if getter {
            Op::StaticGet
        } else if setter {
            Op::StaticSet
        } else {
            Op::StaticMethod
        };
        (op, (!ns.is_empty()).then(|| ns.join(".")))
    };
    let js = match (arg(&args, "js_name"), op) {
        (Some(j), _) => unquote(j),
        (None, Op::Constructor) => owner
            .as_deref()
            .map(|o| o.rsplit('.').next().unwrap().to_string())
            .unwrap_or_default(),
        (None, Op::Set | Op::StaticSet) => name.strip_prefix("set_").unwrap_or(&name).to_string(),
        (None, _) => name.clone(),
    };
    rf.extras.cfg_splits += usize::from(f.attrs.iter().any(|a| a.path().is_ident("cfg")));
    rf.extras.generic_fns += usize::from(!f.sig.generics.params.is_empty());
    rf.extras.docs += usize::from(f.attrs.iter().any(|a| a.path().is_ident("doc")));
    let mut generics: Vec<String> = f
        .sig
        .generics
        .type_params()
        .map(|p| p.ident.to_string())
        .collect();
    if let Some(this) = &this {
        let mut receiver = Vec::new();
        idents(this.to_token_stream(), &mut receiver);
        rf.extras.method_generics += usize::from(generics.iter().any(|g| !receiver.contains(g)));
    }
    if let Some(r) = &ret {
        if last_ident(r).as_deref() == Some("Result") {
            let err = generic_arg(r, 1).and_then(last_ident);
            rf.extras.typed_errors += usize::from(err.is_some_and(|e| e != "JsValue"));
        }
    }
    let typed_args = params.iter().chain(&ret).any(|t| {
        let mut ids = Vec::new();
        idents(t.to_token_stream(), &mut ids);
        t.to_token_stream().to_string().contains('<') && !matches!(ids.first().map(String::as_str), Some("Result" | "Option") if ids.len() == 2)
    });
    rf.extras.generic_arg_sigs += usize::from(typed_args);
    rf.extras.try_forms +=
        usize::from(has(&args, "catch") && name.trim_start_matches("__").starts_with("try_"));
    if name == "__ts_gen_js_string" {
        rf.extras.error_helpers += 1;
    }
    generics.dedup();
    let attrs = args
        .iter()
        .filter(|(k, _)| k != "js_namespace")
        .map(|(k, v)| {
            if v.is_empty() {
                k.clone()
            } else {
                format!("{k} = {v}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut sig = f.sig.clone();
    sig.generics = Default::default();
    let vis = if matches!(f.vis, syn::Visibility::Public(_)) {
        "pub "
    } else {
        ""
    };
    Some(Extern {
        rust_mod: rust_mod.to_string(),
        owner_mod,
        module: module.clone(),
        owner,
        public: !vis.is_empty(),
        name,
        js,
        op,
        catch: has(&args, "catch"),
        variadic: has(&args, "variadic"),
        is_async: f.sig.asyncness.is_some(),
        params,
        ret,
        generics,
        sig: tidy(&format!("#[{attrs}] {vis}{}", sig.to_token_stream())),
    })
}

// ---------------------------------------------------------------------------
// Matching.

/// A rule deriving a Rust-only binding from another binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Rule {
    /// `try_<x>`: the catching twin of a sync callable that may throw
    /// (`signatures::build_signatures`, `allow_try`).
    TryCompanion,
    /// `<x>_promise`: the un-awaited form of an `async fn` binding.
    PromiseForm,
    /// `<x>_js_string`: the additive `JsString`-returning form under
    /// generic mono (`push_with_js_string_return`, getters).
    JsStringVariant,
    /// `__<x>`: a private extern behind an inline wrapper named `<x>`:
    /// literal-arm cohorts (`inline_wrappers`), `null` arms and union
    /// alias `impl Into` arguments (`null_arms`), and `Display`'s
    /// `__to_string` (`classes::display_impl`).
    PrivateBinding,
    /// `try_to_string` alone: the primary `to_string` is suppressed so it
    /// does not shadow `ToString`, `Display` calls the catching form.
    DisplayToString,
    /// The `--error-enum` `Display` helper `__ts_gen_js_string` (the global
    /// `String`), which no member stands for.
    ErrorEnumHelper,
    /// An IR `Object` member: the Rust dictionary's inline `new()` (or
    /// literal-discriminant `new_<n>()`) and builder over `Object::new()`.
    DictionaryNew,
    // Presentation rules: a Rust type standing for an IR type it does not
    // name (`typemap`, `signatures`).
    /// `Record<V>` is `js_sys::Object` (`Object<V>` under generics).
    RecordObject,
    /// A typed array is a slice of its element in Rust (`Clamped<&[u8]>`
    /// for `Uint8ClampedArray`).
    TypedArraySlice,
    /// `ArrayBufferView` is `&Object` or a `T: TypedArray` generic as an
    /// argument, and `Uint8Array` as a result.
    BufferView,
    /// `--external <path>=<rust path>` presents the JS type as the mapped
    /// Rust type.
    ExternalMapping,
}

impl Rule {
    fn name(self) -> &'static str {
        match self {
            Rule::TryCompanion => "try_companion",
            Rule::PromiseForm => "promise_form",
            Rule::JsStringVariant => "js_string_variant",
            Rule::PrivateBinding => "private_binding",
            Rule::DisplayToString => "display_to_string",
            Rule::ErrorEnumHelper => "error_enum_helper",
            Rule::DictionaryNew => "dictionary_new",
            Rule::RecordObject => "record_object",
            Rule::TypedArraySlice => "typed_array_slice",
            Rule::BufferView => "buffer_view",
            Rule::ExternalMapping => "external_mapping",
        }
    }
}

#[derive(Clone, Debug)]
enum Class {
    Direct(usize),
    Derived(Vec<Rule>, Option<usize>),
    Renamed(usize),
    /// Category and detail.
    RustOnly(String, String),
}

/// A divergence: its category, the Rust side, the IR side and a detail.
struct Entry {
    category: String,
    rust: String,
    ir: String,
    detail: String,
}

#[derive(Default)]
struct FixtureReport {
    name: String,
    fresh: bool,
    error: Option<String>,
    rust_count: usize,
    direct: usize,
    derived: usize,
    /// IR members a Rust non-extern item presents (`dictionary_new`).
    abi_derived: usize,
    renamed: Vec<Entry>,
    rust_only: Vec<Entry>,
    abi_only: Vec<Entry>,
    mismatches: Vec<Entry>,
    rules: BTreeMap<Rule, usize>,
    throws_unmodelled: bool,
    extras: Extras,
}

type Key = (Option<String>, Option<String>, String);

fn audit(fixture: &Path) -> FixtureReport {
    let base = fixture
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace(".d.ts", "");
    let mut report = FixtureReport {
        name: base.clone(),
        ..Default::default()
    };
    let args = parse_args(&directive_args(fixture));
    let (model, fresh) = match build_model(fixture, &args) {
        Ok(m) => m,
        Err(e) => {
            report.error = Some(format!("{e:#}"));
            return report;
        }
    };
    report.fresh = fresh;
    let rs =
        std::fs::read_to_string(crate_root().join(format!("tests/snapshots/{base}/default.rs")))
            .expect("default.rs snapshot");
    let mut rf = parse_rust(&rs);
    kinds_match_enums(&model, &rf, &mut report);
    for m in &args.externals {
        if let Some((js, rust)) = m.split_once('=') {
            let ident = rust.rsplit("::").next().unwrap_or(rust);
            rf.externals.insert(js.to_string(), ident.to_string());
        }
    }
    report.extras = rf.extras.clone();
    report.extras.provides_files = usize::from(
        crate_root()
            .join(format!("tests/snapshots/{base}/default.provides.rs"))
            .is_file(),
    );

    // One logical binding per cfg-split pair.
    let mut seen = HashSet::new();
    let mut externs: Vec<Extern> = rf
        .externs
        .iter()
        .filter(|e| {
            seen.insert((
                e.rust_mod.clone(),
                e.module.clone(),
                e.owner.clone(),
                e.name.clone(),
            ))
        })
        .cloned()
        .collect();
    report.rust_count = externs.len();
    let owners: HashSet<(Option<String>, String)> = model
        .bindings
        .iter()
        .flat_map(|b| {
            b.owner
                .iter()
                .chain(&b.alias_owners)
                .map(|o| (b.module.clone(), o.clone()))
        })
        .chain(model.owners.keys().cloned())
        .collect();
    // An interface has no constructor object, so Rust tests its values
    // structurally (`is_type_of`).
    for (rust_mod, module, path, structural) in &rf.type_decls {
        let path = normalize_owner(path, module, rust_mod, &owners);
        let Some(kind) = model.owners.get(&(module.clone(), path.clone())) else {
            continue;
        };
        let expected = *kind == OwnerKind::Interface;
        if expected != *structural {
            report.mismatches.push(Entry {
                category: format!(
                    "owner kind: IR {kind:?}, Rust {}",
                    if *structural {
                        "is_type_of (interface)"
                    } else {
                        "instanceof (class)"
                    }
                ),
                rust: format!("type `{path}`"),
                ir: path.clone(),
                detail: String::new(),
            });
        }
    }
    for e in &mut externs {
        if let Some(o) = &e.owner {
            e.owner = Some(normalize_owner(o, &e.module, &e.owner_mod, &owners));
        }
    }
    let inline_new: HashSet<(Option<String>, String)> = rf
        .inline_new
        .iter()
        .map(|(m, rust_mod, o)| (m.clone(), normalize_owner(o, m, rust_mod, &owners)))
        .collect();
    let mut index: HashMap<Key, usize> = HashMap::new();
    for (i, b) in model.bindings.iter().enumerate() {
        index
            .entry((b.module.clone(), b.owner.clone(), b.name.clone()))
            .or_insert(i);
        for a in &b.alias_owners {
            index
                .entry((b.module.clone(), Some(a.clone()), b.name.clone()))
                .or_insert(i);
        }
    }
    let key = |e: &Extern, name: &str| (e.module.clone(), e.owner.clone(), name.to_string());
    let by_name: HashMap<Key, usize> = externs
        .iter()
        .enumerate()
        .map(|(i, e)| (key(e, &e.name), i))
        .collect();

    // The logical name of an extern: a private wrapped binding stands for
    // its wrapper.
    let logical = |e: &Extern| -> (String, bool) {
        if !e.public && e.name.starts_with("__") && rf.called.contains(&e.name) {
            (e.name[2..].to_string(), true)
        } else {
            (e.name.clone(), false)
        }
    };
    let rust_base = |e: &Extern, name: &str, private: bool| -> Option<usize> {
        let n = if private {
            format!("__{name}")
        } else {
            name.to_string()
        };
        by_name.get(&key(e, &n)).copied()
    };
    // One derivation step on `e`: the rule and the Rust extern it derives
    // from, if the Rust output has the base.
    let step = |e: &Extern| -> Option<(Rule, Option<usize>, String)> {
        let (name, private) = logical(e);
        if let Some(b) = name.strip_prefix("try_") {
            if e.catch {
                // `try_<x>_<n>`: the companion's own name collided, so it
                // took a numeric suffix.
                let unsuffixed = b
                    .rsplit_once('_')
                    .filter(|(_, n)| n.parse::<u32>().is_ok())
                    .map(|(b, _)| b);
                for b in std::iter::once(b).chain(unsuffixed) {
                    if let Some(i) = rust_base(e, b, private) {
                        if externs[i].js == e.js && !externs[i].catch {
                            return Some((Rule::TryCompanion, Some(i), b.to_string()));
                        }
                    }
                }
                if b == "to_string" && e.owner.as_ref().is_some_and(|o| rf.display.contains(o)) {
                    return Some((Rule::DisplayToString, None, b.to_string()));
                }
            }
        }
        if let Some(b) = name.strip_suffix("_promise") {
            if !e.is_async {
                if let Some(i) = rust_base(e, b, private) {
                    if externs[i].is_async && externs[i].js == e.js {
                        return Some((Rule::PromiseForm, Some(i), b.to_string()));
                    }
                }
            }
        }
        if let Some(b) = name.strip_suffix("_js_string") {
            if let Some(i) = rust_base(e, b, private) {
                if externs[i].js == e.js {
                    return Some((Rule::JsStringVariant, Some(i), b.to_string()));
                }
            }
        }
        None
    };

    let mut used: HashSet<usize> = HashSet::new();
    let mut classes: Vec<Option<Class>> = vec![None; externs.len()];
    // Roots: externs no rule derives from another extern, matched by
    // their logical name.
    let mut chains: Vec<(Vec<Rule>, usize)> = Vec::new();
    for (i, e) in externs.iter().enumerate() {
        let mut rules = Vec::new();
        let mut cur = i;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 8 {
                break;
            }
            let (_, private) = logical(&externs[cur]);
            if private && !rules.contains(&Rule::PrivateBinding) {
                rules.push(Rule::PrivateBinding);
            }
            match step(&externs[cur]) {
                Some((rule, Some(b), _)) => {
                    rules.push(rule);
                    cur = b;
                }
                Some((rule, None, base)) => {
                    rules.push(rule);
                    let k = key(e, &base);
                    match index.get(&k) {
                        Some(&a) => {
                            used.insert(a);
                            classes[i] = Some(Class::Derived(rules.clone(), Some(a)));
                        }
                        None => {
                            classes[i] = Some(Class::RustOnly(
                                format!("{} base absent from the IR", rule.name()),
                                format!("no member `{base}`"),
                            ));
                        }
                    }
                    break;
                }
                None => break,
            }
        }
        if e.name == "__ts_gen_js_string" {
            classes[i] = Some(Class::Derived(vec![Rule::ErrorEnumHelper], None));
        }
        chains.push((rules, cur));
    }
    for (i, e) in externs.iter().enumerate() {
        if classes[i].is_some() || chains[i].1 != i {
            continue;
        }
        // A literal cohort's shared binding is a member under its own name.
        if let Some(&a) = index
            .get(&key(e, &e.name))
            .filter(|&&a| e.name.starts_with("__") && model.bindings[a].op == e.op)
        {
            used.insert(a);
            classes[i] = Some(Class::Direct(a));
            continue;
        }
        let (name, _) = logical(e);
        if let Some(&a) = index
            .get(&key(e, &name))
            .filter(|&&a| model.bindings[a].op == e.op)
        {
            used.insert(a);
            classes[i] = Some(if chains[i].0.is_empty() {
                Class::Direct(a)
            } else {
                Class::Derived(chains[i].0.clone(), Some(a))
            });
        }
    }
    // Renamed roots: a leftover member with the same owner, operation,
    // JS name and arity, preferring one whose parameter types correspond,
    // in declaration order.
    for strict in [true, false] {
        for (i, e) in externs.iter().enumerate() {
            if classes[i].is_some() || chains[i].1 != i {
                continue;
            }
            let candidate = model.bindings.iter().enumerate().find(|(a, b)| {
                !used.contains(a)
                    && b.module == e.module
                    && b.owner == e.owner
                    && b.op == e.op
                    && (b.op == Op::Constructor || b.js.as_deref() == Some(e.js.as_str()))
                    && b.params.len() == e.params.len()
                    && (!strict || params_correspond(e, b, &model, &rf))
            });
            if let Some((a, _)) = candidate {
                used.insert(a);
                classes[i] = Some(Class::Renamed(a));
            }
        }
    }
    // Derived externs follow their root.
    for i in 0..externs.len() {
        if classes[i].is_some() {
            continue;
        }
        let (rules, root) = &chains[i];
        let root_class = classes[*root].clone();
        classes[i] = Some(match root_class {
            _ if *root == i => {
                let (category, detail) = why_rust_only(&externs[i], &model, &owners);
                Class::RustOnly(category, detail)
            }
            Some(Class::Direct(a)) | Some(Class::Renamed(a)) => {
                Class::Derived(rules.clone(), Some(a))
            }
            Some(Class::Derived(r, a)) => {
                let mut all = rules.clone();
                all.extend(r);
                Class::Derived(all, a)
            }
            _ => Class::RustOnly(
                "derived from a rust-only binding".into(),
                format!(
                    "{} of `{}`",
                    rules.iter().map(|r| r.name()).collect::<Vec<_>>().join("+"),
                    externs[*root].name
                ),
            ),
        });
    }

    // Fallibility: a member throws when its Rust binding catches or has a
    // `try_` twin.
    let try_twin: HashSet<usize> = externs
        .iter()
        .filter_map(|e| match step(e) {
            Some((Rule::TryCompanion, Some(b), _)) => Some(b),
            _ => None,
        })
        .collect();

    let label_of = |e: &Extern| match &e.owner {
        Some(o) => format!("{o}.{}", e.name),
        None => e.name.clone(),
    };
    for (i, e) in externs.iter().enumerate() {
        match classes[i].as_ref().unwrap() {
            Class::Direct(a) => {
                report.direct += 1;
                compare(
                    e,
                    &model.bindings[*a],
                    &model,
                    &rf,
                    try_twin.contains(&i),
                    &mut report,
                );
            }
            Class::Renamed(a) => {
                let b = &model.bindings[*a];
                let category =
                    if e.name == format!("{}_", b.name) || e.name == format!("_{}", b.name) {
                        "Rust escapes a keyword or digit name".to_string()
                    } else if !e.public && e.name.starts_with("__") {
                        "private (literal-arm) binding named differently".to_string()
                    } else if b.js.as_deref() == Some(b.name.as_str()) {
                        format!("IR keeps the JS name of a {}", op_noun(e.op))
                    } else {
                        "overload suffix differs".to_string()
                    };
                report.renamed.push(Entry {
                    category,
                    rust: format!("`{}`", e.sig),
                    ir: b.import(),
                    detail: format!("Rust `{}` vs IR `{}`", e.name, b.name),
                });
                compare(e, b, &model, &rf, try_twin.contains(&i), &mut report);
            }
            Class::Derived(rules, a) => {
                report.derived += 1;
                for r in rules {
                    *report.rules.entry(*r).or_default() += 1;
                }
                let promise =
                    rules.contains(&Rule::PromiseForm) && !rules.contains(&Rule::TryCompanion);
                if let (Some(a), true) = (a, promise) {
                    let b = &model.bindings[*a];
                    if let Some(throws) = b.throws {
                        if e.catch != throws {
                            report.mismatches.push(Entry {
                                category: "throws".into(),
                                rust: format!("`{}`", e.sig),
                                ir: b.import(),
                                detail: format!(
                                    "Rust `_promise` catch {} vs IR throws {throws}",
                                    e.catch
                                ),
                            });
                        }
                    }
                }
                if let (Some(a), true) = (a, rules == &[Rule::PrivateBinding]) {
                    compare(
                        e,
                        &model.bindings[*a],
                        &model,
                        &rf,
                        try_twin.contains(&i),
                        &mut report,
                    );
                }
            }
            Class::RustOnly(category, detail) => {
                report.rust_only.push(Entry {
                    category: category.clone(),
                    rust: format!("{} — `{}`", label_of(e), e.sig),
                    ir: String::new(),
                    detail: detail.clone(),
                });
            }
        }
    }
    for (a, b) in model.bindings.iter().enumerate() {
        if used.contains(&a) {
            continue;
        }
        if b.op == Op::Object
            && b.owner
                .as_ref()
                .is_some_and(|o| inline_new.contains(&(b.module.clone(), o.clone())))
        {
            report.abi_derived += 1;
            *report.rules.entry(Rule::DictionaryNew).or_default() += 1;
            continue;
        }
        let rust_owners: HashSet<(&Option<String>, &Option<String>)> =
            externs.iter().map(|e| (&e.module, &e.owner)).collect();
        let same_js = externs.iter().any(|e| {
            e.module == b.module
                && e.owner == b.owner
                && e.op == b.op
                && (b.op == Op::Constructor || b.js.as_deref() == Some(e.js.as_str()))
        });
        let category = if same_js {
            "Rust expands the JS member differently"
        } else if b.op == Op::Object {
            "dictionary `{}` with no Rust `new`/builder"
        } else if b.op == Op::Constructor {
            "constructor Rust does not bind"
        } else if b.owner.is_some() && !rust_owners.contains(&(&b.module, &b.owner)) {
            "owner absent from the Rust output"
        } else {
            "member absent from the Rust output"
        };
        report.abi_only.push(Entry {
            category: category.into(),
            rust: String::new(),
            ir: b.import(),
            detail: b.ts.clone(),
        });
    }
    for (r, n) in rf.presented.take() {
        *report.rules.entry(r).or_default() += n;
    }
    report.throws_unmodelled = model.bindings.iter().any(|b| b.throws.is_none());
    report
}

// ---------------------------------------------------------------------------
// Type correspondence.

#[derive(Clone, Debug)]
enum RT {
    Void,
    Undefined,
    Str,
    Num,
    BigInt,
    Bool,
    Any,
    Array,
    /// `&[T]`, by element type.
    Slice(String),
    Promise,
    Callback,
    Tuple,
    Optional(Box<RT>),
    Enum(EnumShape),
    Generic,
    Named(String),
    Unclassified,
}

fn classify(ty: &Type, rf: &RustFile, generics: &[String], depth: usize) -> RT {
    if depth > 8 {
        return RT::Unclassified;
    }
    let rec = |t: &Type| classify(t, rf, generics, depth + 1);
    match ty {
        Type::Reference(r) => rec(&r.elem),
        Type::Group(g) => rec(&g.elem),
        Type::Paren(p) => rec(&p.elem),
        Type::Slice(sl) => match &*sl.elem {
            Type::Path(p) if p.path.segments.len() == 1 => {
                RT::Slice(p.path.segments[0].ident.to_string())
            }
            _ => RT::Array,
        },
        Type::Array(_) => RT::Array,
        Type::Tuple(t) if t.elems.is_empty() => RT::Void,
        Type::BareFn(_) => RT::Callback,
        Type::TraitObject(_) => RT::Callback,
        Type::ImplTrait(i) => {
            for b in &i.bounds {
                if let syn::TypeParamBound::Trait(t) = b {
                    let seg = t.path.segments.last().unwrap();
                    let name = seg.ident.to_string();
                    if let syn::PathArguments::AngleBracketed(a) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(t)) = a.args.first() {
                            if name == "Into" || name == "AsRef" {
                                return rec(t);
                            }
                        }
                    }
                    if name == "JsStringLike" {
                        return RT::Str;
                    }
                    if name.starts_with("Fn") {
                        return RT::Callback;
                    }
                }
            }
            RT::Unclassified
        }
        Type::Path(p) => {
            let seg = p.path.segments.last().unwrap();
            let name = seg.ident.to_string();
            if p.path.segments.len() == 1 && generics.contains(&name) {
                return RT::Generic;
            }
            let arg0 = generic_arg(ty, 0);
            match name.as_str() {
                "str" | "String" | "JsString" => RT::Str,
                "f64" | "f32" | "i8" | "i16" | "i32" | "u8" | "u16" | "u32" | "usize" | "isize"
                | "Number" => RT::Num,
                "i64" | "u64" | "i128" | "u128" | "BigInt" => RT::BigInt,
                "bool" | "Boolean" => RT::Bool,
                "JsValue" => RT::Any,
                "Undefined" => RT::Undefined,
                "Option" | "Nullable" | "JsNullable" | "JsOption" => {
                    RT::Optional(Box::new(arg0.map_or(RT::Any, rec)))
                }
                "Vec" | "Array" => RT::Array,
                "Clamped" => match arg0.map(rec) {
                    Some(RT::Slice(e)) => RT::Slice(format!("Clamped<{e}>")),
                    _ => RT::Unclassified,
                },
                "ArrayTuple" => RT::Tuple,
                "Promise" => RT::Promise,
                "Function" | "Closure" | "ScopedClosure" => RT::Callback,
                _ => {
                    if let Some(s) = rf.enums.get(&name) {
                        return RT::Enum(*s);
                    }
                    if !rf.types.contains_key(&name) {
                        if let Some(target) = rf.aliases.get(&name) {
                            return rec(target);
                        }
                    }
                    RT::Named(name)
                }
            }
        }
        _ => RT::Unclassified,
    }
}

/// Each kind is a Rust enum of its last segment's name, with its arms'
/// names as the variants in order, its literal arms as the unit variants,
/// its aliases as the enum's alias consts and its `fallback` as the enum's
/// `#[wasm_bindgen(fallback)]`; each Rust enum is a kind. A kind may match
/// any enum of that name, as namespaced kinds are enums in nested Rust mods.
fn kinds_match_enums(model: &Model, rf: &RustFile, report: &mut FixtureReport) {
    let rust_arms =
        |rust_mod: &str, ident: &str, variants: &[(String, bool)], fallback: bool| KindArms {
            arms: variants
                .iter()
                .map(|(v, unit)| {
                    let aliases = rf
                        .enum_aliases
                        .iter()
                        .filter(|(m, e, _, t)| m == rust_mod && e == ident && t == v)
                        .map(|(_, _, a, _)| a.clone())
                        .collect();
                    (v.clone(), aliases, *unit)
                })
                .collect(),
            fallback,
        };
    let show = |k: &KindArms| {
        let arms = k
            .arms
            .iter()
            .map(|(n, a, literal)| {
                let n = if *literal {
                    n.clone()
                } else {
                    format!("{n}(..)")
                };
                if a.is_empty() {
                    n
                } else {
                    format!("{n} alias {}", a.join(", "))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        if k.fallback {
            format!("fallback {{ {arms} }}")
        } else {
            format!("{{ {arms} }}")
        }
    };
    let mut matched = HashSet::new();
    for (module, name, arms) in &model.kind_arms {
        let ident = name.rsplit('.').next().unwrap_or(name);
        let ir = match module {
            Some(m) => format!("\"{m}\" kind {name}"),
            None => format!("kind {name}"),
        };
        let candidates: Vec<(usize, KindArms)> = rf
            .enum_variants
            .iter()
            .enumerate()
            .filter(|(_, (_, e, _, _))| e == ident)
            .map(|(i, (m, e, v, f))| (i, rust_arms(m, e, v, *f)))
            .collect();
        if let Some((i, _)) = candidates.iter().find(|(_, r)| r == arms) {
            matched.insert(*i);
            continue;
        }
        let Some((i, rust)) = candidates.first() else {
            report.mismatches.push(Entry {
                category: "kind without a Rust enum".into(),
                rust: String::new(),
                ir,
                detail: String::new(),
            });
            continue;
        };
        matched.insert(*i);
        let (r, a) = (&rust.arms, &arms.arms);
        let category = if r.len() != a.len() {
            "kind arm count differs from the Rust enum's"
        } else if r.iter().zip(a).any(|(r, a)| r.0 != a.0) {
            "kind arm names differ from the Rust enum's variants"
        } else if r.iter().zip(a).any(|(r, a)| r.2 != a.2) {
            "kind literal arms differ from the Rust enum's unit variants"
        } else if rust.fallback != arms.fallback {
            "kind fallback differs from the Rust enum's `#[wasm_bindgen(fallback)]`"
        } else {
            "kind arm aliases differ from the Rust enum's"
        };
        report.mismatches.push(Entry {
            category: category.into(),
            rust: format!("enum {ident} {}", show(rust)),
            ir,
            detail: show(arms),
        });
    }
    for (i, (m, e, _, _)) in rf.enum_variants.iter().enumerate() {
        if !matched.contains(&i)
            && !model
                .kind_arms
                .iter()
                .any(|(_, n, _)| n.rsplit('.').next() == Some(e))
        {
            report.mismatches.push(Entry {
                category: "Rust enum without a kind".into(),
                rust: format!("enum {m}::{e}"),
                ir: String::new(),
                detail: String::new(),
            });
        }
    }
}

/// `None` when `rt` corresponds to `ty`, else the mismatch category.
fn corresponds(rt: &RT, ty: &Ty, model: &Model, rf: &RustFile) -> Option<String> {
    if let RT::Optional(inner) = rt {
        return match ty {
            // `Option<Nullable<T>>`: each layer takes one of the empties.
            Ty::Js { .. } if matches!(**inner, RT::Optional(_)) => {
                corresponds(inner, ty, model, rf)
            }
            Ty::Js { .. } if ty.admits_empty() => corresponds(inner, &ty.non_empty(), model, rf),
            Ty::Js { .. } => Some("Option in Rust, IR admits no empty".into()),
            _ => Some(format!("Option in Rust, IR scalar {}", ty.show())),
        };
    }
    let open = matches!(rt, RT::Any | RT::Generic | RT::Undefined | RT::Unclassified);
    if ty.admits_empty() && !open {
        if let Some(m) = corresponds(rt, &ty.non_empty(), model, rf) {
            return Some(m);
        }
        return Some("IR admits null/undefined, Rust not optional".into());
    }
    let head = match ty {
        Ty::Js { head, .. } => Some(head),
        _ => None,
    };
    let ok = match (rt, ty) {
        (RT::Void, Ty::Void) => true,
        (RT::Undefined, Ty::Void | Ty::Js { .. }) => true,
        (RT::Void, Ty::Js { head: H::Empty, .. }) => true,
        (RT::Num, Ty::F64) => true,
        (RT::BigInt, Ty::I64) => true,
        (RT::Bool, Ty::Bool) => true,
        (RT::Enum(EnumShape::Numeric), Ty::F64) => false,
        (_, Ty::Js { .. }) => match (rt, head.unwrap()) {
            (RT::Unclassified, _) => return Some("unclassified Rust type".into()),
            (RT::Any, _) => true,
            (RT::Generic, H::Var(_) | H::Any) => true,
            (RT::Str, H::String) => true,
            (RT::Num, H::Number) => true,
            (RT::BigInt, H::BigInt) => true,
            (RT::Bool, H::Boolean) => true,
            (RT::Array | RT::Slice(_), H::Named(_, p)) if p == "Array" => true,
            (RT::Named(n), H::Empty) if n == "Null" => true,
            (RT::Promise, H::Named(_, p)) if p == "Promise" => true,
            (RT::Callback, H::Callback) => true,
            (RT::Tuple, H::Tuple) => true,
            (RT::Str, H::StringLiteral) => true,
            (RT::Num, H::IntLiteral) => true,
            (RT::Void, H::Empty) => true,
            (RT::Callback, H::Named(_, p)) if p == "Function" => true,
            (RT::Enum(_), H::Named(m, p)) => model.kinds.contains(&(m.clone(), p.clone())),
            (RT::Named(n), H::Named(_, p)) if named_matches(n, p, rf) => true,
            (_, H::Any) => return Some(format!("IR `any` for Rust {}", kind_of(rt))),
            (rt, H::Named(_, p)) => match presentation(rt, p, rf) {
                Some(rule) => {
                    *rf.presented.borrow_mut().entry(rule).or_default() += 1;
                    true
                }
                None => false,
            },
            _ => false,
        },
        _ => false,
    };
    (!ok).then(|| format!("Rust {} vs IR {}", kind_of(rt), kind_of_ty(ty)))
}

/// The presentation rule by which Rust `rt` stands for the IR type at
/// JS path `path`.
fn presentation(rt: &RT, path: &str, rf: &RustFile) -> Option<Rule> {
    let element = match path {
        "Int8Array" => "i8",
        "Uint8Array" => "u8",
        "Uint8ClampedArray" => "Clamped<u8>",
        "Int16Array" => "i16",
        "Uint16Array" => "u16",
        "Int32Array" => "i32",
        "Uint32Array" => "u32",
        "Float32Array" => "f32",
        "Float64Array" => "f64",
        "BigInt64Array" => "i64",
        "BigUint64Array" => "u64",
        _ => "",
    };
    match rt {
        RT::Slice(e) if e == element => Some(Rule::TypedArraySlice),
        RT::Named(n) if n == "Object" && path == "Record" => Some(Rule::RecordObject),
        RT::Named(n) if (n == "Object" || n == "Uint8Array") && path == "ArrayBufferView" => {
            Some(Rule::BufferView)
        }
        RT::Generic if path == "ArrayBufferView" => Some(Rule::BufferView),
        RT::Named(n) if rf.externals.get(path) == Some(n) => Some(Rule::ExternalMapping),
        _ => None,
    }
}

fn kind_of(rt: &RT) -> String {
    match rt {
        RT::Enum(s) => format!("enum({s:?})"),
        RT::Named(_) => "class".into(),
        RT::Slice(e) => format!("&[{e}]"),
        RT::Generic => "generic".into(),
        RT::Optional(i) => format!("Option<{}>", kind_of(i)),
        RT::Unclassified => "unclassified".into(),
        other => format!("{other:?}"),
    }
}

fn kind_of_ty(ty: &Ty) -> String {
    match ty {
        Ty::Js { head, .. } => match head {
            H::Named(_, p) if p == "Array" || p == "Promise" || p == "Function" => p.clone(),
            H::Named(..) => "named".into(),
            H::Var(_) => "var".into(),
            h => format!("{h:?}").to_lowercase(),
        },
        t => t.show(),
    }
}

fn named_matches(rust: &str, path: &str, rf: &RustFile) -> bool {
    let last = path.rsplit('.').next().unwrap_or(path);
    if rust == last || rust == path || rust.trim_end_matches('_') == last {
        return true;
    }
    if let Some(v) = rf.types.get(rust) {
        if v.iter().any(|(_, p)| p == path) {
            return true;
        }
    }
    rf.aliases
        .get(rust)
        .and_then(last_ident)
        .is_some_and(|t| t == last)
}

/// The IR owner a Rust owner path stands for. An interface in a namespace
/// has no `js_namespace` in Rust (no JS object to reach), only its Rust
/// module: `mod shadow { type Inner }` is the IR's `Shadow.Inner`. A
/// Rust-escaped name (`Box_`) is the IR's `Box`.
fn normalize_owner(
    o: &str,
    module: &Option<String>,
    rust_mod: &str,
    owners: &HashSet<(Option<String>, String)>,
) -> String {
    if o.contains('.') {
        return o.to_string();
    }
    let norm = |s: &str| s.to_lowercase().replace('_', "");
    let mods: Vec<String> = rust_mod
        .split("::")
        .filter(|s| !s.is_empty())
        .map(norm)
        .collect();
    for base in [o, o.trim_end_matches('_')] {
        let found: Vec<&String> = owners
            .iter()
            .filter(|(m, p)| {
                let Some(prefix) = p.strip_suffix(&format!(".{base}")) else {
                    return false;
                };
                let segs: Vec<String> = prefix.split('.').map(norm).collect();
                m == module && mods.ends_with(&segs)
            })
            .map(|(_, p)| p)
            .collect();
        if let [p] = found[..] {
            return p.clone();
        }
        if owners.contains(&(module.clone(), base.to_string())) {
            return base.to_string();
        }
    }
    o.to_string()
}

fn op_noun(op: Op) -> &'static str {
    match op {
        Op::StaticMethod => "function",
        Op::StaticGet | Op::StaticSet => "static",
        Op::Get | Op::Set => "property",
        _ => "member",
    }
}

/// Why an extern has no member: a category and a detail.
fn why_rust_only(
    e: &Extern,
    model: &Model,
    owners: &HashSet<(Option<String>, String)>,
) -> (String, String) {
    let owner = e.owner.as_deref().unwrap_or("<top level>");
    if e.name.starts_with("__") && e.name.contains("_with_null") {
        return (
            "null arm: the IR folds `null` into a nullable parameter".into(),
            format!("no member `{}`", &e.name[2..]),
        );
    }
    if let Some(o) = &e.owner {
        if !owners.contains(&(e.module.clone(), o.clone())) {
            return ("owner not in the IR".into(), format!("no owner `{o}`"));
        }
    }
    let same_js: Vec<String> = model
        .bindings
        .iter()
        .filter(|b| {
            b.module == e.module
                && b.owner == e.owner
                && b.op == e.op
                && (b.op == Op::Constructor || b.js.as_deref() == Some(e.js.as_str()))
        })
        .map(|b| b.name.clone())
        .collect();
    if !same_js.is_empty() {
        return (
            "overload expansion differs".into(),
            format!(
                "the IR binds `{owner}` `{}` as {}",
                e.js,
                same_js.join(", ")
            ),
        );
    }
    let elsewhere: Vec<String> = model
        .bindings
        .iter()
        .filter(|b| b.module == e.module && b.op == e.op && b.js.as_deref() == Some(e.js.as_str()))
        .map(|b| b.import())
        .collect();
    if !elsewhere.is_empty() {
        return (
            "member the IR has on another owner (inherited or merged in Rust)".into(),
            format!("`{owner}` `{}`; IR: {}", e.js, elsewhere.join(", ")),
        );
    }
    (
        "no member for the JS member".into(),
        format!("`{owner}` `{}` ({:?})", e.js, e.op),
    )
}

fn params_correspond(e: &Extern, b: &Binding, model: &Model, rf: &RustFile) -> bool {
    e.params.iter().zip(&b.params).all(|(p, t)| {
        corresponds(&classify(p, rf, &e.generics, 0), t, model, rf)
            .is_none_or(|m| m.starts_with("IR `any`"))
    })
}

fn compare(
    e: &Extern,
    b: &Binding,
    model: &Model,
    rf: &RustFile,
    has_try_twin: bool,
    report: &mut FixtureReport,
) {
    let mut push = |category: String, detail: String| {
        report.mismatches.push(Entry {
            category,
            rust: format!("`{}`", e.sig),
            ir: b.import(),
            detail,
        });
    };
    if e.op != b.op {
        push(
            "operation".into(),
            format!("Rust {:?} vs IR {:?}", e.op, b.op),
        );
    }
    if e.variadic != b.variadic {
        push(
            "variadic".into(),
            format!("Rust {} vs IR {}", e.variadic, b.variadic),
        );
    }
    if e.params.len() != b.params.len() {
        push(
            "param count".into(),
            format!("Rust {} vs IR {}", e.params.len(), b.params.len()),
        );
    } else {
        for (i, (p, t)) in e.params.iter().zip(&b.params).enumerate() {
            let rt = classify(p, rf, &e.generics, 0);
            if let Some(m) = corresponds(&rt, t, model, rf) {
                push(
                    format!("param: {m}"),
                    format!(
                        "param {i}: Rust `{}` vs IR {}",
                        tidy(&p.to_token_stream().to_string()),
                        t.show()
                    ),
                );
            }
        }
    }
    let (ret, mut catches) = match &e.ret {
        Some(r) if last_ident(r).as_deref() == Some("Result") => (generic_arg(r, 0).cloned(), true),
        r => (r.clone(), false),
    };
    // `Display` calls the catching form of `toString`, the only one bound:
    // it stands for a non-catching binding and its `try_` twin.
    let display = e.name == "__to_string"
        && catches
        && e.owner.as_ref().is_some_and(|o| rf.display.contains(o));
    if display {
        catches = false;
    }
    // An `async fn` catches the rejection unless declared never to throw;
    // the IR's `throws` is the un-awaited `_promise` form's, compared there.
    if e.is_async {
        if let Some(nothrow) = b.nothrow {
            if catches == nothrow {
                push(
                    "throws".into(),
                    format!("Rust async catch {catches} vs IR nothrow {nothrow}"),
                );
            }
        }
    } else if let Some(throws) = b.throws {
        if catches != throws {
            push(
                "throws".into(),
                format!("Rust catch {catches} vs IR throws {throws}"),
            );
        }
    }
    let try_capable = matches!(e.op, Op::Method | Op::StaticMethod) && !e.is_async;
    if let (Some(nothrow), true) = (b.nothrow, try_capable && !display) {
        let rust_nothrow = !catches && !has_try_twin;
        if rust_nothrow != nothrow {
            push(
                "nothrow".into(),
                format!("Rust has no catching form: {rust_nothrow}, IR nothrow {nothrow}"),
            );
        }
    }
    let shown = |r: &Option<Type>| {
        r.as_ref()
            .map_or("()".into(), |r| tidy(&r.to_token_stream().to_string()))
    };
    if e.is_async {
        let promise = matches!(&b.result, Ty::Js { head: H::Named(_, p), .. } if p == "Promise");
        if !promise {
            push(
                "result: async fn, IR result not a Promise".into(),
                format!("Rust async `{}` vs IR {}", shown(&ret), b.result.show()),
            );
        }
        return;
    }
    let rt = match &ret {
        Some(t) => classify(t, rf, &e.generics, 0),
        None => RT::Void,
    };
    if let Some(m) = corresponds(&rt, &b.result, model, rf) {
        push(
            format!("result: {m}"),
            format!("Rust `{}` vs IR {}", shown(&ret), b.result.show()),
        );
    }
}

// ---------------------------------------------------------------------------
// Report.

/// What the Rust generator emits besides extern fns, what the IR would
/// need to carry for it, and whether `abi/src/ir.rs` has it ("new": not in
/// the model as first committed, 9ae1e31b).
fn extras_table(x: &Extras) -> Vec<(&'static str, usize, &'static str, &'static str)> {
    vec![
        (
            "string-literal-union enums (`pub enum K { A = \"a\" }`)",
            x.string_enums,
            "a kind per named literal union: each arm's literal and its Rust variant name",
            "partial: `Kind` with `Head::Literal(String)` arms (new); no variant names",
        ),
        (
            "numeric enums (`#[repr(u32)]`, TS `enum`)",
            x.numeric_enums,
            "a kind with member names and values; its path places wasm-bindgen's JS object",
            "partial: `Literal::Int` arms (new); no member names",
        ),
        (
            "union enums (payload variants)",
            x.union_enums,
            "a kind with typed arms and a variant name per arm",
            "partial: `Kind.arms: Vec<JsType>`; no variant names",
        ),
        (
            "  of which `fallback`",
            x.fallback_enums,
            "whether unknown values decode to a catch-all arm",
            "no",
        ),
        (
            "  `From<arm>` impls on union enums",
            x.union_from_impls,
            "the arm types of the kind",
            "yes (`Kind.arms`)",
        ),
        (
            "enum alias consts (repeated enum values)",
            x.enum_alias_consts,
            "kind member aliases (names sharing a value)",
            "no",
        ),
        (
            "error enum (`--error-enum`, `Other(JsValue)`)",
            x.error_enums,
            "every `@throws` type, unions kept, and their inheritance for `instanceof` order",
            "yes: `Member.errors` (new), one type per union arm; inheritance is `Class.extends`",
        ),
        (
            "  error enum `Display` helper extern",
            x.error_helpers,
            "nothing beyond the option",
            "n/a",
        ),
        (
            "`Result<T, E>` with a typed `E` (`@throws {E}`, `--error-type`)",
            x.typed_errors,
            "per member the thrown type (the default is a generator option)",
            "yes: `Member.errors` (new)",
        ),
        (
            "`try_` companions and their suppression (`@throws {never}`)",
            x.try_forms,
            "whether a non-catching binding gets a catching form",
            "yes: `Member.nothrow` (new); counted under derivation rules",
        ),
        (
            "dictionary builders (`XBuilder`, `new()`, `Default`)",
            x.builders,
            "which classes are plain-object dictionaries, and their settable properties",
            "partial: an `Object` member marks it; `Set` members give the fields",
        ),
        (
            "  builders taking required fields",
            x.required_field_builders,
            "which properties are required, as opposed to `T | undefined`",
            "partial: a `Get` result not admitting `undefined`; `x?: T` vs `x: T | undefined` is lost",
        ),
        (
            "literal-arm inline wrappers (`get_text` over `__get_text`)",
            x.literal_wrappers,
            "the literal-keyed cohort: literal positions, each arm's literals and wrapper name, the shared private binding",
            "partial: `Head::Literal` (new) can type an arm's parameter; the cohort grouping is not carried",
        ),
        (
            "`null`-arm inline wrappers (`f_with_null` over `__f_with_null`)",
            x.null_wrappers,
            "a parameter that is only `null`",
            "yes: `Head::Empty` with `null` (new)",
        ),
        (
            "union-alias `impl Into<Alias>` wrappers",
            x.into_wrappers,
            "that a parameter takes a named union alias whole",
            "yes, as `Head::Named` of the kind",
        ),
        (
            "`Display` impls over `toString`",
            x.display_impls,
            "a member whose JS name is `toString` with no parameters",
            "yes (`Member.js`)",
        ),
        (
            "`extends = ...` attributes (full ancestor chain)",
            x.extends_attrs,
            "direct bases of every class",
            "yes (`Class.extends`)",
        ),
        (
            "`is_type_of = JsValue::is_object` (interfaces)",
            x.is_type_of,
            "whether a constructor object exists at the class's path",
            "yes: `ClassKind` (new); no dictionary kind",
        ),
        (
            "generic extern types (`pub type Foo<T>`)",
            x.generic_types,
            "type parameter names",
            "yes (`Class.params`)",
        ),
        (
            "generic extern fns (`<T: JsGeneric>` bounds)",
            x.generic_fns,
            "the type parameters each member uses",
            "partial: `Head::Var` names class params only",
        ),
        (
            "  member-level type parameters (not the receiver's)",
            x.method_generics,
            "type parameters declared on the member itself",
            "no",
        ),
        (
            "signatures with typed generic arguments (`Promise<JsString>`, `Map<K, V>`)",
            x.generic_arg_sigs,
            "type arguments, array elements, tuple elements, callback signatures",
            "yes (`Named.args`, `Head::Tuple` (new), `Head::Callback`)",
        ),
        (
            "extern types renamed from their JS name (`js_name` on the type)",
            x.js_named_types,
            "the Rust name of a class when it differs from its path (clashes such as `H2`)",
            "no: `Class.path` only",
        ),
        (
            "extern blocks of the `--lib-name` module at the crate root",
            x.lib_blocks,
            "which module is the bound library",
            "no (could stay a generator option)",
        ),
        (
            "`--provides` wrapper (`default.provides.rs`)",
            x.provides_files,
            "provided functions: JS paths, params, results, async",
            "yes (`Ir.provides`)",
        ),
        (
            "docs on externs and types",
            x.docs,
            "JSDoc per class and member",
            "yes (`Class.doc`, `Member.doc`)",
        ),
        (
            "`pub type A = B;` aliases",
            x.type_aliases,
            "TS aliases of classes, and other exported names of a class",
            "partial: `Class.aliases` for export names only",
        ),
        (
            "external type `use` mappings (`::web_sys::Blob`)",
            x.external_uses,
            "the undeclared names referenced (the mapping is a generator option)",
            "yes (`Head::Named` of undeclared globals)",
        ),
        (
            "`use JsValue as X` for unresolvable types",
            x.jsvalue_uses,
            "names referenced that resolve to nothing",
            "yes (`Head::Named`), if kept rather than erased to `Any`",
        ),
        (
            "extern statics (`thread_local_v2`)",
            x.statics,
            "a static get of a variable",
            "yes (`MemberKind::StaticGet`)",
        ),
        (
            "per-param `slice = ...` attributes",
            x.slice_attrs,
            "array element types and typed-array parameters",
            "yes (`Named.args`)",
        ),
        (
            "`#[cfg(target_feature = \"atomics\")]` view/copy splits",
            x.cfg_splits,
            "typed-array parameters",
            "yes, from the parameter type",
        ),
    ]
}

fn write_report(reports: &[FixtureReport], source: &str) -> (String, String) {
    let ok: Vec<&FixtureReport> = reports.iter().filter(|r| r.error.is_none()).collect();
    let sum = |f: &dyn Fn(&FixtureReport) -> usize| ok.iter().map(|r| f(r)).sum::<usize>();
    let externs = sum(&|r| r.rust_count);
    let direct = sum(&|r| r.direct);
    let derived = sum(&|r| r.derived);
    let renamed = sum(&|r| r.renamed.len());
    let rust_only = sum(&|r| r.rust_only.len());
    let abi_only = sum(&|r| r.abi_only.len());
    let abi_derived = sum(&|r| r.abi_derived);
    let mismatches = sum(&|r| r.mismatches.len());

    let mut md = String::new();
    let mut summary = String::new();
    let _ = writeln!(md, "# IR ↔ Rust extern audit\n");
    let _ = writeln!(
        md,
        "Source: {source}, `default` variant. Rust: `tests/snapshots/<fixture>/default.rs`. \
         Generated by `tests/ir_rust.rs`.\n"
    );
    let headline = format!(
        "{} fixtures, {externs} Rust externs: {direct} direct, {derived} derived, {renamed} renamed, \
         {rust_only} rust-only. IR: {abi_only} abi-only, {abi_derived} presented by non-extern Rust. \
         {mismatches} type/shape mismatches on matched pairs.",
        ok.len()
    );
    let _ = writeln!(md, "{headline}\n");
    let _ = writeln!(summary, "IR/Rust audit ({source}): {headline}");
    let stale: Vec<_> = ok
        .iter()
        .filter(|r| !r.fresh)
        .map(|r| r.name.as_str())
        .collect();
    if !stale.is_empty() {
        let line = format!(
            "Built template differs from the blessed `default.wat` (stale snapshot?): {}",
            stale.join(", ")
        );
        let _ = writeln!(md, "{line}\n");
        let _ = writeln!(summary, "  {line}");
    }
    for r in reports.iter().filter(|r| r.error.is_some()) {
        let _ = writeln!(
            md,
            "Failed to build {}: {}\n",
            r.name,
            r.error.as_ref().unwrap()
        );
    }
    if ok.iter().any(|r| r.throws_unmodelled) {
        let _ = writeln!(
            md,
            "The source does not model `throws`, so Result ↔ throws is not compared.\n"
        );
    }

    type Pick = fn(&FixtureReport) -> &Vec<Entry>;
    let classes: [(&str, Pick); 4] = [
        ("rust-only", |r| &r.rust_only),
        ("abi-only", |r| &r.abi_only),
        ("renamed", |r| &r.renamed),
        ("mismatch", |r| &r.mismatches),
    ];
    let _ = writeln!(md, "## Divergences by category\n");
    for (class, pick) in classes {
        let mut cats: BTreeMap<&str, (usize, String)> = BTreeMap::new();
        for r in &ok {
            for e in pick(r) {
                cats.entry(&e.category)
                    .or_insert_with(|| (0, example(&r.name, e)))
                    .0 += 1;
            }
        }
        let mut cats: Vec<_> = cats.into_iter().collect();
        cats.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
        let total: usize = cats.iter().map(|c| c.1 .0).sum();
        let _ = writeln!(
            md,
            "### {class} ({total})\n\n| category | count | example |\n|---|---:|---|"
        );
        let _ = writeln!(summary, "  {class}: {total}");
        for (cat, (n, ex)) in &cats {
            let _ = writeln!(md, "| {} | {n} | {} |", cell(cat), cell(ex));
            let _ = writeln!(summary, "    {n:>5}  {cat}");
        }
        let _ = writeln!(md);
    }

    let mut rules: BTreeMap<Rule, usize> = BTreeMap::new();
    for r in &ok {
        for (k, v) in &r.rules {
            *rules.entry(*k).or_default() += v;
        }
    }
    let _ = writeln!(md, "## Derivation rules\n\n| rule | uses |\n|---|---:|");
    let _ = writeln!(summary, "  derivation rules:");
    for (k, v) in &rules {
        let _ = writeln!(md, "| `{}` | {v} |", k.name());
        let _ = writeln!(summary, "    {v:>5}  {}", k.name());
    }

    let mut extras = Extras::default();
    for r in &ok {
        extras.add(&r.extras);
    }
    let _ = writeln!(
        md,
        "\n## Beyond externs\n\nRust output that is not an extern binding, whose input the IR \
         would need to carry.\n\n| output | count | IR needs | in `abi/src/ir.rs` |\n|---|---:|---|---|"
    );
    for (what, n, needs, has) in extras_table(&extras) {
        let _ = writeln!(
            md,
            "| {} | {n} | {} | {} |",
            cell(what),
            cell(needs),
            cell(has)
        );
    }

    let _ = writeln!(
        md,
        "\n## Per fixture\n\n| fixture | externs | direct | derived | renamed | rust-only | abi-only | abi-derived | mismatches |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|"
    );
    for r in &ok {
        let _ = writeln!(
            md,
            "| {}{} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.name,
            if r.fresh { "" } else { " (stale wat)" },
            r.rust_count,
            r.direct,
            r.derived,
            r.renamed.len(),
            r.rust_only.len(),
            r.abi_only.len(),
            r.abi_derived,
            r.mismatches.len()
        );
    }
    let _ = writeln!(
        md,
        "| **total** | {externs} | {direct} | {derived} | {renamed} | {rust_only} | {abi_only} | {abi_derived} | {mismatches} |\n"
    );
    for r in &ok {
        if classes.iter().all(|(_, pick)| pick(r).is_empty()) {
            continue;
        }
        let _ = writeln!(md, "### {}\n", r.name);
        for (class, pick) in classes {
            for e in pick(r) {
                let _ = writeln!(md, "- {class} [{}]: {}", e.category, entry_text(e));
            }
        }
        let _ = writeln!(md);
    }
    (md, summary)
}

fn entry_text(e: &Entry) -> String {
    let mut parts = Vec::new();
    if !e.rust.is_empty() {
        parts.push(e.rust.clone());
    }
    if !e.ir.is_empty() {
        parts.push(format!("IR {}", e.ir));
    }
    let mut s = parts.join(" ↔ ");
    if !e.detail.is_empty() {
        let _ = write!(s, " ({})", e.detail);
    }
    s
}

fn example(fixture: &str, e: &Entry) -> String {
    format!("{fixture}: {}", entry_text(e))
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[test]
fn ir_matches_rust_externs() {
    let fixtures_dir = crate_root().join("tests/fixtures");
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&fixtures_dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.ends_with(".d.ts") && !n.ends_with(".provides.d.ts")
        })
        .collect();
    fixtures.sort();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunks: Vec<&[PathBuf]> = fixtures.chunks(fixtures.len().div_ceil(threads)).collect();
    let reports: Vec<FixtureReport> = std::thread::scope(|s| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                // The CLI's main thread has 8 MiB; the front end recurses deeply.
                std::thread::Builder::new()
                    .stack_size(64 << 20)
                    .spawn_scoped(s, move || {
                        chunk.iter().map(|f| audit(f)).collect::<Vec<_>>()
                    })
                    .expect("spawn audit thread")
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("audit thread"))
            .collect()
    });
    let (md, summary) = write_report(&reports, SOURCE);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_root().join("target"));
    let _ = std::fs::create_dir_all(&target);
    let path = target.join("ir-rust-report.md");
    std::fs::write(&path, &md).expect("write report");
    eprintln!("{summary}  report: {}", path.display());

    let errors: Vec<_> = reports
        .iter()
        .filter_map(|r| r.error.as_ref().map(|e| format!("{}: {e}", r.name)))
        .collect();
    assert!(
        errors.is_empty(),
        "fixtures failed to build:\n{}",
        errors.join("\n")
    );
    if std::env::var("IR_RUST_REPORT").is_ok_and(|v| v == "1") {
        eprintln!("{md}");
        return;
    }
    type Pick = fn(&FixtureReport) -> &Vec<Entry>;
    let classes: [(&str, Pick); 4] = [
        ("rust-only", |r| &r.rust_only),
        ("abi-only", |r| &r.abi_only),
        ("renamed", |r| &r.renamed),
        ("mismatch", |r| &r.mismatches),
    ];
    let mut seen = HashSet::new();
    let mut new = Vec::new();
    for r in &reports {
        for (class, pick) in classes {
            for e in pick(r) {
                let key = (r.name.as_str(), class, e.ir.as_str(), e.category.as_str());
                if KNOWN.contains(&key) {
                    seen.insert(key);
                } else {
                    new.push(format!(
                        "{} {class} [{}]: {}",
                        r.name,
                        e.category,
                        entry_text(e)
                    ));
                }
            }
        }
    }
    let gone: Vec<_> = KNOWN.iter().filter(|k| !seen.contains(*k)).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "IR diverges from the Rust externs (see {}):\n{}\nknown divergences that no longer occur: {gone:?}",
        path.display(),
        new.join("\n")
    );
}
