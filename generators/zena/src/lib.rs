//! Zena bindings for the ts-gen IR (IR.md), compiled with Zena's `-t js`.
//!
//! A flat layer of `@external` declarations, one per IR member; a sys
//! layer with wasm-bindgen's `JsValue` helpers over `externref`; and an
//! exported class per IR class holding the host reference in `#ref`.
//! Names are the IR's, verbatim: `ts-gen --naming zena` makes them Zena
//! identifiers, and a dotted one is `_`-joined. TS strings are Zena `String`s, which on `-t js` wrap
//! the JS string itself. See the README for the full mapping.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::hash::Hash;

use ts_gen_abi::ir::{
    Class, ClassKind, Head, Ir, JsType, Kind, Literal, Member, MemberKind, Named, Param, Provide,
    Type, NO_ARM,
};
use ts_gen_abi::{OwnerRef, BINDINGS_SPECIFIER, BUILTINS_SPECIFIER, UNDEFINED_GLOBAL};

/// The local a wrapper holds a possibly empty result in.
const RESULT: &str = "__r";

/// Names the generated code defines or references besides the IR's,
/// which its classes, kinds and top-level members therefore cannot take.
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

/// The `bindings.wasm` functions every IR has (IR.md, Imports): import
/// name, flat parameters, flat result and the JS they run.
const SYS: [(&str, &str, &str, &str); 3] = [
    (
        "JsValue.is_undefined",
        "x: externref | null",
        "boolean",
        "x === undefined",
    ),
    ("JsValue.undefined", "", "externref", "undefined"),
    ("Number.value", "x: externref", "f64", "Number(x)"),
];

pub struct Output {
    pub bindings: String,
    /// With provides: `main.zena`, the compile entry, and the `app.zena`
    /// scaffold it calls.
    pub provides: Option<(String, String)>,
}

/// `bindings_file` is the output's file name, which `main.zena` and
/// `app.zena` import.
pub fn generate(ir: &Ir, bindings_file: &str) -> anyhow::Result<Output> {
    let ctx = Ctx::new(ir, bindings_file);
    let provides = if ir.provides.is_empty() {
        None
    } else {
        Some((ctx.main()?, ctx.app()))
    };
    Ok(Output {
        bindings: ctx.bindings(),
        provides,
    })
}

/// A declaration by its module and binding name.
type Key<'a> = (Option<&'a str>, &'a str);

/// A top-level member's names: its module, JS name and the index of the
/// name among the member's and its aliases'.
type Family<'a> = (Option<&'a str>, &'a str, usize);

/// Type arguments of a base, by its parameter names.
type Subst = HashMap<String, JsType>;

/// A member presented on a class: its own, or an inherited instance member
/// with the base's type arguments substituted, under its name or an alias.
struct Bound<'a> {
    member: Cow<'a, Member>,
    name: &'a str,
    flat: &'a str,
}

struct Ctx<'a> {
    ir: &'a Ir,
    bindings_file: &'a str,
    classes: HashMap<Key<'a>, usize>,
    kinds: HashMap<Key<'a>, usize>,
    /// Zena names of classes and kinds, by index.
    class_names: Vec<String>,
    kind_names: Vec<String>,
    class_aliases: Vec<Vec<String>>,
    /// Exported names of each top-level member and its aliases.
    functions: Vec<Vec<String>>,
    /// Flat declaration names, by class and member.
    member_flats: Vec<Vec<String>>,
    function_flats: Vec<String>,
    sys_flats: Vec<String>,
    /// Each kind's discriminator declaration and the names matching on it.
    kind_flats: Vec<String>,
    kind_arms: Vec<ArmNames>,
}

/// The sealed class a kind's value is matched as, a case per arm, and the
/// function returning it.
struct ArmNames {
    class: String,
    cases: Vec<String>,
    func: String,
}

/// How a reference position is presented: a class around `#ref`, or a
/// `String` around the JS string.
#[derive(Clone, PartialEq)]
enum Wrap {
    Class(String),
    Str,
}

impl Wrap {
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

/// A JS position as Zena sees it: its wrapper, the Zena type without
/// `| null`, and the empties it admits.
struct Pres {
    wrap: Option<Wrap>,
    ty: String,
    null: bool,
    undefined: bool,
}

impl Pres {
    fn nullable(&self) -> String {
        if self.null || self.undefined {
            format!("{} | null", self.ty)
        } else {
            self.ty.clone()
        }
    }
}

fn import_module<'a>(owner: Option<&'a str>, m: &'a Member) -> Option<&'a str> {
    m.module.as_deref().or(owner)
}

fn flat_name(module: Option<&str>, name: &str) -> String {
    match module {
        Some(spec) => format!("__{}_{}", ident(spec), ident(name)),
        None => format!("__{}", ident(name)),
    }
}

/// Names for `(module, name, family)`s in order, globals first so that a
/// module's name only takes the specifier prefix when a global or earlier
/// name holds it. The names of one family (a function's overloads) take
/// the prefix together.
fn assign_names<F: Copy + Eq + Hash>(
    keys: &[(Option<&str>, String, F)],
    used: &mut HashSet<String>,
) -> Vec<String> {
    let mut names = vec![String::new(); keys.len()];
    for global in [true, false] {
        let group: Vec<usize> = (0..keys.len())
            .filter(|&i| keys[i].0.is_none() == global)
            .collect();
        let mut prefixed: HashMap<F, bool> = HashMap::new();
        for &i in &group {
            let (module, ref name, family) = keys[i];
            let prefix = *prefixed.entry(family).or_insert_with(|| {
                group
                    .iter()
                    .any(|&j| keys[j].2 == family && used.contains(&keys[j].1))
            });
            let name = match module {
                Some(spec) if prefix => format!("{}_{name}", ident(spec)),
                _ => name.clone(),
            };
            names[i] = unique(used, name);
        }
    }
    names
}

/// What a [`Named`] refers to.
enum Decl {
    Class(usize),
    Kind(usize),
}

impl<'a> Ctx<'a> {
    fn new(ir: &'a Ir, bindings_file: &'a str) -> Self {
        let mut used = HashSet::from(["__undefined".to_string()]);
        let sys_flats = SYS
            .iter()
            .map(|(name, ..)| unique(&mut used, flat_name(None, name)))
            .collect();
        let member_flats = ir
            .classes
            .iter()
            .map(|c| {
                c.members
                    .iter()
                    .map(|m| {
                        let module = import_module(c.module.as_deref(), m);
                        unique(
                            &mut used,
                            flat_name(module, &format!("{}.{}", c.name, m.name)),
                        )
                    })
                    .collect()
            })
            .collect();
        let function_flats = ir
            .functions
            .iter()
            .map(|f| {
                let module = import_module(f.module.as_deref(), &f.member);
                unique(&mut used, flat_name(module, &f.member.name))
            })
            .collect();
        let kind_flats = ir
            .kinds
            .iter()
            .map(|k| {
                unique(
                    &mut used,
                    flat_name(k.module.as_deref(), &k.discriminator()),
                )
            })
            .collect();

        let mut used: HashSet<String> =
            RESERVED.iter().map(|s| s.to_string()).chain(used).collect();
        let classes = ir
            .classes
            .iter()
            .enumerate()
            .map(|(i, c)| ((c.module.as_deref(), c.name.as_str()), i))
            .collect();
        let kinds = ir
            .kinds
            .iter()
            .enumerate()
            .map(|(i, k)| ((k.module.as_deref(), k.name.as_str()), i))
            .collect();
        // A class's aliases are named right after it, the kinds after the
        // classes.
        let mut type_keys: Vec<(Option<&str>, String, usize)> = Vec::new();
        for c in ir.classes.iter().filter(|c| !is_empty_namespace(c)) {
            let module = c.module.as_deref();
            for name in std::iter::once(&c.name).chain(&c.aliases) {
                type_keys.push((module, name.replace('.', "_"), type_keys.len()));
            }
        }
        for k in &ir.kinds {
            type_keys.push((
                k.module.as_deref(),
                k.name.replace('.', "_"),
                type_keys.len(),
            ));
        }
        let mut names = assign_names(&type_keys, &mut used).into_iter();
        let mut class_names = Vec::new();
        let mut class_aliases = Vec::new();
        for c in &ir.classes {
            if is_empty_namespace(c) {
                class_names.push(String::new());
                class_aliases.push(Vec::new());
                continue;
            }
            class_names.push(names.next().expect("a name per class"));
            class_aliases.push(names.by_ref().take(c.aliases.len()).collect());
        }
        let kind_names = names.collect();
        // A member and each of its aliases is a family of its own, so an
        // alias keeps its name when only the member's is taken.
        let function_keys: Vec<(Option<&str>, String, Family)> = ir
            .functions
            .iter()
            .flat_map(|f| {
                let module = f.module.as_deref();
                let js = f.member.js.as_str();
                std::iter::once(&f.member.name)
                    .chain(&f.member.aliases)
                    .enumerate()
                    .map(move |(n, name)| (module, name.clone(), (module, js, n)))
            })
            .collect();
        let mut function_names = assign_names(&function_keys, &mut used).into_iter();
        let functions = ir
            .functions
            .iter()
            .map(|f| {
                let n = 1 + f.member.aliases.len();
                function_names.by_ref().take(n).collect()
            })
            .collect();
        // Derived from the kinds' names, after every name the IR gives.
        let kind_arms = ir
            .kinds
            .iter()
            .zip(&kind_names)
            .map(|(k, name)| ArmNames {
                class: unique(&mut used, format!("{name}_Arm")),
                cases: k
                    .arms
                    .iter()
                    .map(|a| unique(&mut used, format!("{name}_{}", a.name)))
                    .collect(),
                func: unique(&mut used, format!("{name}_arm")),
            })
            .collect();

        Ctx {
            ir,
            bindings_file,
            classes,
            kinds,
            class_names,
            kind_names,
            class_aliases,
            functions,
            member_flats,
            function_flats,
            sys_flats,
            kind_flats,
            kind_arms,
        }
    }

    /// A class, or a kind of its name, which wins over a namespace's.
    fn resolve(&self, r: &OwnerRef) -> Option<Decl> {
        let key = (r.module.as_deref(), r.path.as_str());
        let class = self
            .classes
            .get(&key)
            .copied()
            .filter(|&i| !is_empty_namespace(&self.ir.classes[i]));
        let namespace = class.is_some_and(|i| self.ir.classes[i].kind == ClassKind::Namespace);
        match (class, self.kinds.get(&key)) {
            (Some(i), _) if !namespace => Some(Decl::Class(i)),
            (_, Some(&k)) => Some(Decl::Kind(k)),
            (Some(i), None) => Some(Decl::Class(i)),
            (None, None) => None,
        }
    }

    fn kind(&self, r: &OwnerRef) -> Option<(usize, &'a Kind)> {
        match self.resolve(r)? {
            Decl::Kind(k) => Some((k, &self.ir.kinds[k])),
            Decl::Class(_) => None,
        }
    }

    fn class_name(&self, i: usize) -> &str {
        &self.class_names[i]
    }

    /// The wrapper for a head; a kind's when all its arms share one.
    fn wrap(&self, head: &Head) -> Option<Wrap> {
        if let Head::Named(n) = head {
            if let Some((_, k)) = self.kind(&n.name) {
                return self.kind_wrap(k);
            }
        }
        self.arm_wrap(head)
    }

    fn kind_wrap(&self, k: &Kind) -> Option<Wrap> {
        let mut arms = k.arms.iter().map(|a| self.arm_wrap(&a.ty.head));
        let first = arms.next()??;
        arms.all(|w| w.as_ref() == Some(&first)).then_some(first)
    }

    fn arm_wrap(&self, head: &Head) -> Option<Wrap> {
        match head {
            Head::String | Head::Literal(Literal::String(_)) => Some(Wrap::Str),
            Head::Named(n) => match self.resolve(&n.name)? {
                Decl::Class(i) => Some(Wrap::Class(self.class_name(i).to_string())),
                Decl::Kind(_) => None,
            },
            _ => None,
        }
    }

    fn pres(&self, j: &JsType) -> Pres {
        let wrap = self.wrap(&j.head);
        let ty = match &j.head {
            Head::Named(n) => match self.kind(&n.name) {
                Some((i, _)) => self.kind_names[i].clone(),
                None => match &wrap {
                    Some(Wrap::Class(c)) => c.clone(),
                    _ => "externref".to_string(),
                },
            },
            _ if wrap.is_some() => "String".to_string(),
            _ => "externref".to_string(),
        };
        Pres {
            wrap,
            ty,
            null: j.null,
            undefined: j.undefined,
        }
    }

    /// The flat layer's type: a reference admitting `null` is `externref
    /// | null`, as Zena keeps `null` out of `externref`.
    fn flat_type(&self, t: &Type) -> String {
        match t {
            Type::Void => "void",
            Type::F64 => "f64",
            Type::I64 => "i64",
            Type::Bool => "boolean",
            Type::Js(j) if self.pres(j).null => "externref | null",
            Type::Js(_) => "externref",
        }
        .to_string()
    }

    /// A class's members with their names: its own, then the instance
    /// members of its bases it does not declare, depth first.
    fn members(&self, ci: usize) -> Vec<Bound<'_>> {
        let ir = self.ir;
        let mut out = Vec::new();
        let mut taken = HashSet::new();
        let class = &ir.classes[ci];
        for (m, flat) in class.members.iter().zip(&self.member_flats[ci]) {
            for name in std::iter::once(&m.name).chain(&m.aliases) {
                taken.insert(name.as_str());
                out.push(Bound {
                    member: Cow::Borrowed(m),
                    name,
                    flat,
                });
            }
        }
        let mut seen = HashSet::from([ci]);
        self.inherit(class, &Subst::new(), &mut seen, &mut taken, &mut out);
        out
    }

    fn inherit<'s>(
        &'s self,
        class: &'s Class,
        subst: &Subst,
        seen: &mut HashSet<usize>,
        taken: &mut HashSet<&'s str>,
        out: &mut Vec<Bound<'s>>,
    ) {
        for base in &class.extends {
            let Some(Decl::Class(bi)) = self.resolve(&base.name) else {
                continue;
            };
            if !seen.insert(bi) {
                continue;
            }
            let b = &self.ir.classes[bi];
            let args: Subst = b
                .params
                .iter()
                .zip(&base.args)
                .map(|(p, a)| (p.clone(), subst_js(a, subst)))
                .collect();
            for (m, flat) in b.members.iter().zip(&self.member_flats[bi]) {
                if !m.kind.has_receiver() {
                    continue;
                }
                for name in std::iter::once(&m.name).chain(&m.aliases) {
                    if taken.insert(name) {
                        out.push(Bound {
                            member: Cow::Owned(subst_member(m, &args)),
                            name,
                            flat,
                        });
                    }
                }
            }
            self.inherit(b, &args, seen, taken, out);
        }
    }

    fn has_modules(&self) -> bool {
        let ir = self.ir;
        ir.classes
            .iter()
            .any(|c| c.module.is_some() || c.members.iter().any(|m| m.module.is_some()))
            || ir.kinds.iter().any(|k| k.module.is_some())
            || ir
                .functions
                .iter()
                .any(|f| f.module.is_some() || f.member.module.is_some())
    }

    fn bindings(&self) -> String {
        let mut out = String::new();
        out.push_str("// Generated by ts-gen-zena from the ts-gen IR. Compile with\n");
        out.push_str(
            "// `-t js --imported-string-constants wasm:js/string-constants` for embed.\n",
        );
        out.push_str("// `__` declarations import the bindings; the exported classes and\n");
        out.push_str("// functions wrap them. `externref` holds any JS value but null,\n");
        out.push_str("// `undefined` included; positions that admit null are `externref |\n");
        out.push_str("// null`. TS strings are `String`s over the JS string. A class or string\n");
        out.push_str("// result that admits an empty is `T | null`, null for null and\n");
        out.push_str("// undefined. A kind is a type alias of its arms' common wrapper, and\n");
        out.push_str("// `<Kind>_arm(x)` tells its arm as a case of the sealed `<Kind>_Arm`.\n");
        if self.has_modules() {
            out.push_str("// Declarations of a `module \"spec\"` are imported from \"spec\".\n");
        }
        self.emit_flat(&mut out);
        self.emit_sys(&mut out);
        self.emit_kinds(&mut out);
        for ci in 0..self.ir.classes.len() {
            self.emit_class(&mut out, ci);
        }
        self.emit_functions(&mut out);
        emit_await(&mut out);
        self.emit_provides(&mut out);
        out
    }

    fn emit_flat(&self, out: &mut String) {
        let mut owner = None;
        for ((name, params, result, js), flat) in SYS.iter().zip(&self.sys_flats) {
            let this_owner = name.split_once('.').map(|(o, _)| o);
            out.push('\n');
            if this_owner != owner {
                let _ = writeln!(out, "// {}", this_owner.unwrap_or_default());
                owner = this_owner;
            }
            let _ = writeln!(out, "// {js}");
            let _ = writeln!(out, "@external({BINDINGS_SPECIFIER:?}, {name:?})");
            let _ = writeln!(out, "declare function {flat}({params}): {result};");
        }
        for (c, flats) in self.ir.classes.iter().zip(&self.member_flats) {
            for (n, (m, flat)) in c.members.iter().zip(flats).enumerate() {
                out.push('\n');
                if n == 0 {
                    let _ = writeln!(out, "// {}", class_header(c));
                }
                let module = import_module(c.module.as_deref(), m);
                let name = format!("{}.{}", c.name, m.name);
                self.emit_external(out, Some(c), m, module, &name, flat);
            }
        }
        for (f, flat) in self.ir.functions.iter().zip(&self.function_flats) {
            out.push('\n');
            let module = import_module(f.module.as_deref(), &f.member);
            self.emit_external(out, None, &f.member, module, &f.member.name, flat);
        }
        for (k, flat) in self.ir.kinds.iter().zip(&self.kind_flats) {
            out.push('\n');
            let _ = writeln!(
                out,
                "// kind {}: the index of the arm holding x, {NO_ARM} for none",
                k.path
            );
            let _ = writeln!(
                out,
                "@external({:?}, {:?})",
                k.specifier(),
                k.discriminator()
            );
            let _ = writeln!(out, "declare function {flat}(x: externref | null): i32;");
        }
    }

    fn emit_external(
        &self,
        out: &mut String,
        class: Option<&Class>,
        m: &Member,
        module: Option<&str>,
        name: &str,
        flat: &str,
    ) {
        let receiver = m
            .kind
            .has_receiver()
            .then(|| "this_: externref".to_string());
        let params =
            receiver
                .into_iter()
                .chain(m.params.iter().zip(param_names(&m.params)).enumerate().map(
                    |(n, (p, name))| {
                        // Nullable, as the IR's `(ref null $anyrefs)`.
                        let ty = if is_rest(m, n) {
                            "FixedArray<anyref> | null".to_string()
                        } else {
                            self.flat_type(&p.ty)
                        };
                        format!("{name}: {ty}")
                    },
                ))
                .collect::<Vec<_>>()
                .join(", ");
        let module = module.unwrap_or(BINDINGS_SPECIFIER);
        let _ = writeln!(out, "// {}", signature(class, m));
        let _ = writeln!(out, "@external({module:?}, {name:?})");
        let _ = writeln!(
            out,
            "declare function {flat}({params}): {};",
            self.flat_type(&m.result)
        );
    }

    fn emit_sys(&self, out: &mut String) {
        let [is_undefined, _, number_value] = &self.sys_flats[..] else {
            unreachable!("three sys functions");
        };
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

    fn emit_kinds(&self, out: &mut String) {
        for (k, name) in self.ir.kinds.iter().zip(&self.kind_names) {
            let arms: Vec<String> = k
                .arms
                .iter()
                .map(|a| format!("{} = {}", a.name, show_js(&a.ty)))
                .collect();
            let fallback = if k.fallback { " fallback" } else { "" };
            let ty = match self.kind_wrap(k) {
                Some(Wrap::Class(c)) => c,
                Some(Wrap::Str) => "String".to_string(),
                None => "externref".to_string(),
            };
            let _ = writeln!(out, "\n// kind {}{fallback}: {}", k.path, arms.join(", "));
            let _ = writeln!(out, "export type {name} = {ty};");
        }
        for ki in 0..self.ir.kinds.len() {
            self.emit_arm(out, ki);
        }
    }

    /// The sealed class a kind's value is matched as, and the function
    /// telling its arm with the discriminator: a literal arm is a unit
    /// case, another holds the value as its position would. In a fallback
    /// kind the last other arm takes what no arm before it does; otherwise
    /// a value of no arm throws, as Rust's binding does.
    fn emit_arm(&self, out: &mut String, ki: usize) {
        let k = &self.ir.kinds[ki];
        let names = &self.kind_arms[ki];
        let kind = &self.kind_names[ki];
        let class = &names.class;
        let _ = writeln!(out, "\n// The arm a `{kind}` holds, from `{}`.", names.func);
        let _ = writeln!(out, "export sealed class {class} {{");
        let mut values = Vec::new();
        for (a, case) in k.arms.iter().zip(&names.cases) {
            let value = match &a.ty.head {
                Head::Literal(_) => None,
                Head::Number => Some(("f64".to_string(), format!("numberValue({RESULT})"))),
                _ => {
                    let pres = self.pres(&a.ty);
                    let wrapped = match &pres.wrap {
                        Some(w) => w.wrap(RESULT),
                        None => RESULT.to_string(),
                    };
                    Some((pres.ty, wrapped))
                }
            };
            match &value {
                Some((ty, _)) => {
                    let _ = writeln!(out, "  case {case}(value: {ty})");
                }
                None => {
                    let _ = writeln!(out, "  case {case}");
                }
            }
            values.push(match value {
                Some((_, v)) => format!("new {case}({v})"),
                None => format!("new {case}()"),
            });
        }
        out.push_str("}\n");
        let x = match self.kind_wrap(k) {
            Some(w) => w.unwrap("x"),
            None => "x".to_string(),
        };
        let fallback = k
            .fallback
            .then(|| {
                k.arms
                    .iter()
                    .rposition(|a| !matches!(a.ty.head, Head::Literal(_)))
            })
            .flatten();
        let mut cases: Vec<String> = values
            .iter()
            .enumerate()
            .filter(|&(i, _)| Some(i) != fallback)
            .map(|(i, v)| format!("    case {i}: {v}"))
            .collect();
        cases.push(match fallback {
            Some(i) => format!("    case _: {}", values[i]),
            None => format!(
                "    case _: throw new Error({:?})",
                format!("no arm of {}", k.path)
            ),
        });
        let _ = writeln!(
            out,
            "export function {}(x: {kind}): {class} {{\n  let {RESULT} = {x};\n  return match ({}({RESULT})) {{\n{}\n  }};\n}}",
            names.func,
            self.kind_flats[ki],
            cases.join("\n")
        );
    }

    fn emit_class(&self, out: &mut String, ci: usize) {
        if is_empty_namespace(&self.ir.classes[ci]) {
            return;
        }
        let class = self.class_name(ci);
        let _ = writeln!(out, "\nexport class {class} {{");
        out.push_str("  #ref: externref;\n");
        out.push_str("  new(ref: externref) : #ref = ref;\n");
        out.push_str("  ref(): externref { return this.#ref; }\n");
        for b in self.members(ci) {
            let m = &*b.member;
            let (sig, mut args) = self.params(m);
            if m.kind.has_receiver() {
                args.insert(0, "this.#ref".to_string());
            }
            let call = format!("{}({})", b.flat, args.join(", "));
            let (ret, body) = self.result(&m.result, call);
            let name = b.name;
            let _ = match m.kind {
                MemberKind::Get if m.params.is_empty() => {
                    writeln!(out, "  {name}: {ret} {{ get {{ {body} }} }}")
                }
                MemberKind::StaticGet if m.params.is_empty() => {
                    writeln!(out, "  static {name}: {ret} {{ get {{ {body} }} }}")
                }
                MemberKind::Get | MemberKind::Set | MemberKind::Method => {
                    writeln!(out, "  {name}({sig}): {ret} {{ {body} }}")
                }
                MemberKind::Constructor
                | MemberKind::Object
                | MemberKind::StaticGet
                | MemberKind::StaticSet
                | MemberKind::StaticMethod => {
                    writeln!(out, "  static {name}({sig}): {ret} {{ {body} }}")
                }
            };
        }
        out.push_str("}\n");
        for alias in &self.class_aliases[ci] {
            let _ = writeln!(out, "\nexport type {alias} = {class};");
        }
    }

    fn emit_functions(&self, out: &mut String) {
        let mut first = true;
        for ((f, flat), names) in self
            .ir
            .functions
            .iter()
            .zip(&self.function_flats)
            .zip(&self.functions)
        {
            let m = &f.member;
            let (sig, args) = self.params(m);
            let call = format!("{flat}({})", args.join(", "));
            let (ret, body) = self.result(&m.result, call);
            for name in names {
                if std::mem::take(&mut first) {
                    out.push('\n');
                }
                let _ = writeln!(out, "export function {name}({sig}): {ret} {{ {body} }}");
            }
        }
    }

    /// The wrapper's parameter list and the argument expressions passing
    /// each to the flat declaration.
    fn params(&self, m: &Member) -> (String, Vec<String>) {
        let mut sig = Vec::new();
        let mut args = Vec::new();
        for (n, (p, name)) in m.params.iter().zip(param_names(&m.params)).enumerate() {
            let (ty, arg) = match &p.ty {
                _ if is_rest(m, n) => ("FixedArray<anyref>".to_string(), name.clone()),
                Type::Js(j) => {
                    let pres = self.pres(j);
                    match &pres.wrap {
                        Some(w) => {
                            let unwrapped = w.unwrap(&name);
                            match (pres.null, pres.undefined) {
                                (false, false) => (pres.ty.clone(), unwrapped),
                                (true, _) => (
                                    pres.nullable(),
                                    format!("if ({name} == null) null else {unwrapped}"),
                                ),
                                // `null` for a `T | undefined` position crosses as `undefined`.
                                (false, true) => (
                                    pres.nullable(),
                                    format!("if ({name} == null) Undefined else {unwrapped}"),
                                ),
                            }
                        }
                        None if pres.null => (format!("{} | null", pres.ty), name.clone()),
                        None => (pres.ty, name.clone()),
                    }
                }
                t => (self.flat_type(t), name.clone()),
            };
            sig.push(format!("{name}: {ty}"));
            args.push(arg);
        }
        (sig.join(", "), args)
    }

    /// The wrapper's result type and the body returning `call`'s value in it.
    fn result(&self, t: &Type, call: String) -> (String, String) {
        let j = match t {
            Type::Void => return ("void".to_string(), format!("{call};")),
            Type::Js(j) => j,
            t => return (self.flat_type(t), format!("return {call};")),
        };
        let pres = self.pres(j);
        match (&pres.wrap, pres.null, pres.undefined) {
            (Some(w), false, false) => (pres.ty.clone(), format!("return {};", w.wrap(&call))),
            (Some(w), null, undefined) => {
                let empty = match (null, undefined) {
                    (true, true) => format!("{RESULT} == null || isUndefined({RESULT})"),
                    (true, false) => format!("{RESULT} == null"),
                    (false, _) => format!("isUndefined({RESULT})"),
                };
                (
                    pres.nullable(),
                    format!(
                        "let {RESULT} = {call}; return if ({empty}) null else {};",
                        w.wrap(RESULT)
                    ),
                )
            }
            (None, true, _) => (format!("{} | null", pres.ty), format!("return {call};")),
            (None, false, _) => (pres.ty, format!("return {call};")),
        }
    }

    /// The export's wire result: an async `void` still settles, to
    /// `undefined`.
    fn provide_result(&self, p: &Provide) -> Option<String> {
        match (&p.result, p.is_async) {
            (Type::Void, true) => Some("externref".to_string()),
            (Type::Void, false) => None,
            (t, _) => Some(self.flat_type(t)),
        }
    }

    fn emit_provides(&self, out: &mut String) {
        if self.ir.provides.is_empty() {
            return;
        }
        out.push_str("\n// Provided: main.zena provides these over the typed app.zena.\n");
        for p in &self.ir.provides {
            let params = p
                .params
                .iter()
                .zip(param_names(&p.params))
                .map(|(param, name)| format!("{name}: {}", self.flat_type(&param.ty)))
                .collect::<Vec<_>>()
                .join(", ");
            let result = self.provide_result(p).unwrap_or_else(|| "void".into());
            let _ = writeln!(
                out,
                "// {}: export function {}({params}): {result}",
                p.js.join("."),
                p.name
            );
        }
    }

    /// `main.zena`: the compile entry, one raw export per provide
    /// converting the wire types to and from the typed `app.zena`. Zena
    /// exports are plain identifiers, so nested export paths cannot be
    /// provided.
    fn main(&self) -> anyhow::Result<String> {
        let mut used = Vec::new();
        let mut body = String::new();
        for p in &self.ir.provides {
            if !is_plain_ident(&p.name) || KEYWORDS.contains(&p.name.as_str()) {
                anyhow::bail!(
                    "ts-gen-zena cannot provide the export {:?}: Zena exports are identifiers, so only `default.<name>` and top-level functions that are not Zena keywords are supported",
                    p.name
                );
            }
            let mut params = Vec::new();
            let mut args = Vec::new();
            for (param, name) in p.params.iter().zip(param_names(&p.params)) {
                params.push(format!("{name}: {}", self.flat_type(&param.ty)));
                let pres = match &param.ty {
                    Type::Js(j) => self.pres(j),
                    _ => {
                        args.push(name);
                        continue;
                    }
                };
                args.push(match &pres.wrap {
                    Some(w) => {
                        if let Wrap::Class(c) = w {
                            used.push(c.clone());
                        }
                        match (pres.null, pres.undefined) {
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
                    None => name,
                });
            }
            let call = format!("app.{}({})", p.name, args.join(", "));
            let ret = match &p.result {
                Type::Void if p.is_async => {
                    used.push("Undefined".into());
                    format!("{{ {call}; return Undefined; }}")
                }
                Type::Void => format!("{{ {call}; }}"),
                Type::Js(j) => {
                    let pres = self.pres(j);
                    match (&pres.wrap, pres.null || pres.undefined) {
                        (Some(w), false) => format!("{{ return {}; }}", w.unwrap(&call)),
                        (Some(w), true) => {
                            let empty = if pres.null {
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
                        (None, _) => format!("{{ return {call}; }}"),
                    }
                }
                _ => format!("{{ return {call}; }}"),
            };
            let result = self
                .provide_result(p)
                .map_or(String::new(), |r| format!(": {r}"));
            let _ = writeln!(
                body,
                "export function {}({}){result} {ret}",
                p.name,
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
    /// docs and bodies that compile. An async provide returns its value
    /// itself, for JSPI.
    fn app(&self) -> String {
        let mut used = Vec::new();
        let mut body = String::new();
        for p in &self.ir.provides {
            let mut params = Vec::new();
            for (param, name) in p.params.iter().zip(param_names(&p.params)) {
                let ty = self.app_type(&param.ty, &mut used);
                params.push(format!("{name}: {ty}"));
            }
            let (ret, stmt) = match &p.result {
                Type::Void => (String::new(), String::new()),
                r => {
                    let ty = self.app_type(r, &mut used);
                    (format!(": {ty}"), self.placeholder(r, &mut used))
                }
            };
            if let Some(doc) = &p.doc {
                for line in doc.lines() {
                    let _ = writeln!(body, "// {line}");
                }
            }
            let _ = writeln!(
                body,
                "export function {}({}){ret} {{{stmt}}}\n",
                p.name,
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
    /// type admits one, the class's zero-argument constructor, or a throw
    /// the author replaces.
    fn placeholder(&self, t: &Type, used: &mut Vec<String>) -> String {
        let value = match t {
            Type::Void => return String::new(),
            Type::Bool => "false".to_string(),
            Type::I64 => "0 as i64".to_string(),
            Type::F64 => "0.0".to_string(),
            Type::Js(j) => {
                let pres = self.pres(j);
                match (&pres.wrap, pres.null, pres.undefined) {
                    (Some(Wrap::Str), false, false) => "\"\"".to_string(),
                    (Some(Wrap::Class(c)), false, false) => match self.zero_arg_ctor(&j.head) {
                        Some(ctor) => format!("{c}.{ctor}()"),
                        None => return " throw new Error(\"not implemented\"); ".to_string(),
                    },
                    (None, false, false) => {
                        return " throw new Error(\"not implemented\"); ".to_string()
                    }
                    (None, false, true) => {
                        used.push("Undefined".into());
                        "Undefined".to_string()
                    }
                    _ => "null".to_string(),
                }
            }
        };
        format!(" return {value}; ")
    }

    /// The name of a class's constructor of its own path taking nothing.
    fn zero_arg_ctor(&self, head: &Head) -> Option<&'a str> {
        let Head::Named(n) = head else {
            return None;
        };
        let Decl::Class(i) = self.resolve(&n.name)? else {
            return None;
        };
        let class = &self.ir.classes[i];
        class
            .members
            .iter()
            .find(|m| {
                m.params.is_empty()
                    && match m.kind {
                        MemberKind::Constructor => m.js == class.path,
                        MemberKind::Object => true,
                        _ => false,
                    }
            })
            .map(|m| m.name.as_str())
    }

    /// The Zena type the app sees for a wire position.
    fn app_type(&self, t: &Type, used: &mut Vec<String>) -> String {
        let Type::Js(j) = t else {
            return self.flat_type(t);
        };
        let pres = self.pres(j);
        if pres.ty != "externref" && pres.ty != "String" {
            used.push(pres.ty.clone());
        }
        if pres.wrap.is_some() {
            pres.nullable()
        } else if pres.null {
            format!("{} | null", pres.ty)
        } else {
            pres.ty
        }
    }
}

/// A namespace that only holds the paths of nested declarations, which
/// has nothing to bind.
fn is_empty_namespace(c: &Class) -> bool {
    c.kind == ClassKind::Namespace && c.members.is_empty() && c.aliases.is_empty()
}

fn subst_type(t: &Type, s: &Subst) -> Type {
    match t {
        Type::Js(j) => Type::Js(subst_js(j, s)),
        t => t.clone(),
    }
}

/// `j` with the type parameters `s` names replaced by their heads. The
/// empties stay `j`'s: they are the wire's.
fn subst_js(j: &JsType, s: &Subst) -> JsType {
    let head = match &j.head {
        Head::Var(v) => match s.get(v) {
            Some(a) if a.head != Head::Empty => a.head.clone(),
            _ => j.head.clone(),
        },
        Head::Named(n) => Head::Named(Named {
            name: n.name.clone(),
            args: n.args.iter().map(|a| subst_js(a, s)).collect(),
        }),
        Head::Callback { params, result } => Head::Callback {
            params: params.iter().map(|t| subst_type(t, s)).collect(),
            result: Box::new(subst_type(result, s)),
        },
        Head::Tuple(elems) => Head::Tuple(elems.iter().map(|e| subst_js(e, s)).collect()),
        h => h.clone(),
    };
    JsType { head, ..j.clone() }
}

fn subst_member(m: &Member, s: &Subst) -> Member {
    Member {
        params: m
            .params
            .iter()
            .map(|p| Param {
                name: p.name.clone(),
                ty: subst_type(&p.ty, s),
            })
            .collect(),
        result: subst_type(&m.result, s),
        ..m.clone()
    }
}

/// A class as the IR writes it, for the flat layer's comments.
fn class_header(c: &Class) -> String {
    let kind = match c.kind {
        ClassKind::Class => "",
        ClassKind::Interface => "shape ",
        ClassKind::Namespace => "static ",
    };
    let mut s = format!("{kind}class {}", c.path);
    if !c.params.is_empty() {
        let _ = write!(s, "<{}>", c.params.join(", "));
    }
    if c.name != c.path {
        let _ = write!(s, " as {}", c.name);
    }
    if !c.extends.is_empty() {
        let bases: Vec<String> = c.extends.iter().map(show_named).collect();
        let _ = write!(s, " extends {}", bases.join(", "));
    }
    s
}

/// A member as the IR writes it, without its binding name, for the flat
/// layer's comments.
fn signature(class: Option<&Class>, m: &Member) -> String {
    let tparams = if m.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", m.type_params.join(", "))
    };
    let params = m
        .params
        .iter()
        .enumerate()
        .map(|(n, p)| {
            let rest = if is_rest(m, n) { "..." } else { "" };
            format!("{rest}{}: {}", show_name(&p.name), show_type(&p.ty))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let result = match &m.result {
        Type::Void => String::new(),
        t => format!(": {}", show_type(t)),
    };
    let js = show_name(&m.js);
    let opt = if m.optional { "?" } else { "" };
    let statik = match m.kind {
        MemberKind::StaticGet | MemberKind::StaticSet | MemberKind::StaticMethod
            if class.is_some() =>
        {
            "static "
        }
        _ => "",
    };
    let mut s = match m.kind {
        MemberKind::Constructor => {
            let path = match class {
                Some(c) if c.path == m.js => String::new(),
                _ => format!(" {}", m.js),
            };
            format!("new{path}{tparams}({params})")
        }
        MemberKind::Object => "new {}".to_string(),
        MemberKind::Get | MemberKind::StaticGet if m.params.is_empty() => {
            format!("{statik}get {js}{opt}{tparams}{result}")
        }
        MemberKind::Get | MemberKind::StaticGet => {
            format!("{statik}get {js}{opt}{tparams}({params}){result}")
        }
        MemberKind::Set | MemberKind::StaticSet => {
            format!("{statik}set {js}{tparams}({params}){result}")
        }
        MemberKind::Method | MemberKind::StaticMethod if class.is_none() => {
            format!("function {js}{tparams}({params}){result}")
        }
        MemberKind::Method | MemberKind::StaticMethod => {
            format!("{statik}{js}{tparams}({params}){result}")
        }
    };
    let errors: Vec<String> = m.errors.iter().map(show_js).collect();
    match (m.throws, errors.is_empty()) {
        (true, true) => s.push_str(" throws"),
        (true, false) => {
            let _ = write!(s, " throws {}", errors.join(" | "));
        }
        (false, false) => {
            let _ = write!(s, " raises {}", errors.join(" | "));
        }
        (false, true) => {}
    }
    if m.nothrow {
        s.push_str(" nothrow");
    }
    if let Some(module) = &m.module {
        let _ = write!(s, " from {module:?}");
    }
    s
}

fn show_name(name: &str) -> String {
    if is_ident(name) {
        name.to_string()
    } else {
        format!("{name:?}")
    }
}

fn show_type(t: &Type) -> String {
    match t {
        Type::Void => "void".to_string(),
        Type::F64 => "f64".to_string(),
        Type::I64 => "i64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Js(j) => show_js(j),
    }
}

fn show_js(j: &JsType) -> String {
    let mut arms = Vec::new();
    match &j.head {
        Head::Empty => {}
        h @ Head::Callback { .. } if j.null || j.undefined => {
            arms.push(format!("({})", show_head(h)))
        }
        h => arms.push(show_head(h)),
    }
    if j.null {
        arms.push("Null".to_string());
    }
    if j.undefined {
        arms.push("Undefined".to_string());
    }
    arms.join(" | ")
}

fn show_head(h: &Head) -> String {
    match h {
        Head::Any => "Any".to_string(),
        Head::String => "String".to_string(),
        Head::Number => "Number".to_string(),
        Head::Boolean => "Boolean".to_string(),
        Head::BigInt => "BigInt".to_string(),
        Head::Named(n) => show_named(n),
        Head::Var(v) => v.clone(),
        Head::Callback { params, result } => {
            let params: Vec<String> = params.iter().map(show_type).collect();
            format!("({}) => {}", params.join(", "), show_type(result))
        }
        Head::Tuple(elems) => {
            let elems: Vec<String> = elems.iter().map(show_js).collect();
            format!("[{}]", elems.join(", "))
        }
        Head::Literal(Literal::String(s)) => format!("{s:?}"),
        Head::Literal(Literal::Int(n)) => n.to_string(),
        Head::Empty => String::new(),
    }
}

fn show_named(n: &Named) -> String {
    if n.args.is_empty() {
        return n.name.path.clone();
    }
    let args: Vec<String> = n.args.iter().map(show_js).collect();
    format!("{}<{}>", n.name.path, args.join(", "))
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
    out.push_str("// selects JSPI mode in `embed`, which wraps the provides with promising.\n");
    let _ = writeln!(out, "@external({BUILTINS_SPECIFIER:?}, \"await\")");
    out.push_str("export declare function await_(p: externref): externref | null;\n");
}

fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn is_plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_rest(m: &Member, i: usize) -> bool {
    m.variadic && i + 1 == m.params.len()
}

/// A module specifier or an import name as part of a flat declaration's
/// or prefixed name, which the preset does not name.
fn ident(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

/// Zena's keywords, which a provide's export name, as the IR has it, may
/// not be.
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

/// Parameter names, none of which may be the receiver's or the result
/// local's.
fn param_names(params: &[Param]) -> Vec<String> {
    let mut used = HashSet::from(["this_".to_string(), RESULT.to_string()]);
    params
        .iter()
        .map(|p| unique(&mut used, p.name.clone()))
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
