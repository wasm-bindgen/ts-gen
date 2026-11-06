//! The ts-gen IR: a `.d.ts` expanded into uniquely named bindings with
//! wasm-level types. Language generators and the bridge builder consume
//! it; the text form is specified in IR.md.
//!
//! Every member is one import, named `<class name>.<member name>` (or
//! `<name>` for a top-level function) in the module `./bindings.wasm`, or
//! the specifier of the `module` it is declared in. Names are final: the
//! front end has already expanded overloads and applied naming options.

use crate::{OwnerRef, WasmType, BINDINGS_SPECIFIER};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ir {
    pub classes: Vec<Class>,
    pub kinds: Vec<Kind>,
    pub functions: Vec<Function>,
    pub provides: Vec<Provide>,
}

/// A JS value type with members: every uppercase type that is not a kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Class {
    /// The binding name, unique in the module, that member imports and
    /// [`Named`] references use: `path`, unless the type is renamed apart
    /// from another of that path (a block's local `H` beside an exported
    /// one is `H2`).
    pub name: String,
    /// Dotted JS path, `WebAssembly.Module`.
    pub path: String,
    pub module: Option<String>,
    pub kind: ClassKind,
    /// Type parameter names.
    pub params: Vec<String>,
    pub extends: Vec<Named>,
    /// Other names the class is exported under; each member is also
    /// imported under each alias.
    pub aliases: Vec<String>,
    pub doc: Option<String>,
    pub members: Vec<Member>,
}

/// What is at a class's path at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClassKind {
    /// A constructor, which `instanceof` tests against.
    Class,
    /// Nothing: a shape, whose values only a structural check can tell.
    Interface,
    /// An object with only static members, which is not a type.
    Namespace,
}

/// A named union, which a generator matches on in a result and takes any
/// arm of in an argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kind {
    /// The binding name, which [`Named`] references use.
    pub name: String,
    /// Dotted JS path of the declaration: where a numeric enum's object
    /// is, and `name` for a union no declaration names.
    pub path: String,
    pub module: Option<String>,
    pub arms: Vec<Arm>,
    /// Open: a value no other arm matches is the last non-literal arm's,
    /// a shape that is not tested for (IR.md, Recognition).
    pub fallback: bool,
    pub doc: Option<String>,
}

impl Kind {
    /// The import name of the kind's discriminator (IR.md, Bridge), in
    /// [`Kind::specifier`].
    pub fn discriminator(&self) -> String {
        format!("kind:{}", self.name)
    }

    /// The module the language imports the discriminator from.
    pub fn specifier(&self) -> &str {
        self.module.as_deref().unwrap_or(BINDINGS_SPECIFIER)
    }
}

/// What a kind's discriminator returns for a value no arm holds.
pub const NO_ARM: i32 = -1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    /// The variant name, unique in the kind.
    pub name: String,
    pub ty: JsType,
    /// Other names of the same value: later enum members repeating it.
    pub aliases: Vec<String>,
    pub doc: Option<String>,
}

/// A top-level member: a free function or variable, static on the module
/// namespace or the global object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub module: Option<String>,
    pub member: Member,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The import name's last segment, unique within its class.
    pub name: String,
    /// The JS property or function name the binding targets. Equal to
    /// `name` unless the front end renamed it.
    pub js: String,
    pub kind: MemberKind,
    /// Type parameters the member declares itself, which [`Head::Var`]
    /// names beside its class's.
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub result: Type,
    /// The binding catches a JS exception and reports it to the caller.
    pub throws: bool,
    /// The declared exception types (`@throws {A | B}`, one per arm),
    /// which a catching form reports, whether or not this binding catches.
    pub errors: Vec<JsType>,
    /// Declared never to throw (`@throws {never}`): no catching form
    /// derives from the binding.
    pub nothrow: bool,
    /// The last param is a rest parameter, `Array<T>`, spread into the call.
    pub variadic: bool,
    /// For an arm of a literal-keyed cohort, the name of the binding the
    /// cohort's arms share: the members of a cohort differ only in the
    /// literals they pass (each a [`Head::Literal`] parameter), which the
    /// shared binding takes as plain values.
    pub cohort: Option<String>,
    /// On a property's `Get`: declared optional (`x?: T`), which a
    /// plain-object builder may leave out, as it may not `x: T | undefined`.
    pub optional: bool,
    /// The module the binding is imported from when it is not its
    /// owner's: a constructor another module's variable exports.
    pub module: Option<String>,
    pub aliases: Vec<String>,
    pub doc: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberKind {
    /// `new C(...args)`, `C` being `js`: the constructor's dotted path
    /// from the module root, usually the class's own path.
    Constructor,
    /// `{}`, for a class of plain objects.
    Object,
    Get,
    Set,
    Method,
    StaticGet,
    StaticSet,
    StaticMethod,
}

impl MemberKind {
    pub fn has_receiver(self) -> bool {
        matches!(self, MemberKind::Get | MemberKind::Set | MemberKind::Method)
    }
}

/// A function the language module exports to JS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provide {
    /// The export name.
    pub name: String,
    /// Where JS reads it: `["default", "fetch"]`.
    pub js: Vec<String>,
    pub params: Vec<Param>,
    /// For an async provide, what the promise resolves to.
    pub result: Type,
    pub is_async: bool,
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

/// Lowercase types are wasm values; uppercase ones are JS values carried
/// as `externref`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Void,
    F64,
    I64,
    /// A JS boolean as `i32`.
    Bool,
    Js(JsType),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsType {
    pub head: Head,
    /// Admits `null`, which arrives as a wasm null: the position is a
    /// nullable `externref`.
    pub null: bool,
    /// Admits `undefined`, a non-null reference.
    pub undefined: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Any,
    String,
    Number,
    Boolean,
    BigInt,
    /// A class or kind by identity, or an undeclared global such as
    /// `Promise`, `Array` or `Record`. Type arguments erase on the wire.
    Named(Named),
    /// A type parameter of the enclosing class or member.
    Var(String),
    /// A JS function value.
    Callback {
        params: Vec<Type>,
        result: Box<Type>,
    },
    /// A JS array of fixed length with a type per element.
    Tuple(Vec<JsType>),
    /// Exactly this value.
    Literal(Literal),
    /// No value but the empties the position admits: `null` alone, or
    /// `undefined` alone as in `Promise<void>`.
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Literal {
    String(String),
    /// An integer that fits an `i32`, as a C-style enum arm.
    Int(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    pub name: OwnerRef,
    pub args: Vec<JsType>,
}

impl Type {
    pub fn js(head: Head) -> Type {
        Type::Js(JsType {
            head,
            null: false,
            undefined: false,
        })
    }

    /// The wire type, `None` for `Void`. A JS type admitting neither empty
    /// may also be taken as `(ref extern)`, and a variadic `Array<T>` is
    /// spread from `(ref null $anyrefs)` instead.
    pub fn wasm(&self) -> Option<WasmType> {
        Some(match self {
            Type::Void => return None,
            Type::F64 => WasmType::F64,
            Type::I64 => WasmType::I64,
            Type::Bool => WasmType::I32,
            Type::Js(j) => WasmType::ExternRef {
                null: j.null,
                undefined: j.undefined,
            },
        })
    }
}

impl Ir {
    /// The class of binding name `name`.
    pub fn class(&self, module: Option<&str>, name: &str) -> Option<&Class> {
        self.classes
            .iter()
            .find(|c| c.module.as_deref() == module && c.name == name)
    }

    /// The kind of binding name `name`.
    pub fn kind(&self, module: Option<&str>, name: &str) -> Option<&Kind> {
        self.kinds
            .iter()
            .find(|k| k.module.as_deref() == module && k.name == name)
    }
}

/// A member as the language module imports it (IR.md, Imports).
#[derive(Clone, Debug)]
pub struct Import<'a> {
    /// The module its JS path is rooted at, `None` for the global object.
    pub module: Option<&'a str>,
    /// Its import name, then those of its aliases.
    pub names: Vec<String>,
    /// `None` for a top-level member.
    pub class: Option<&'a Class>,
    pub member: &'a Member,
}

impl Import<'_> {
    pub fn name(&self) -> &str {
        &self.names[0]
    }

    /// The module the language imports it from.
    pub fn specifier(&self) -> &str {
        self.module.unwrap_or(BINDINGS_SPECIFIER)
    }
}

impl Ir {
    /// Every member with the names it is imported under: classes' members
    /// in order, then the top-level members.
    pub fn imports(&self) -> impl Iterator<Item = Import<'_>> {
        let members = self.classes.iter().flat_map(|c| {
            c.members.iter().map(move |m| {
                let owners = || std::iter::once(&c.name).chain(&c.aliases);
                let names = std::iter::once(&m.name)
                    .chain(&m.aliases)
                    .flat_map(|n| owners().map(move |o| format!("{o}.{n}")))
                    .collect();
                Import {
                    module: m.module.as_deref().or(c.module.as_deref()),
                    names,
                    class: Some(c),
                    member: m,
                }
            })
        });
        let functions = self.functions.iter().map(|f| Import {
            module: f.member.module.as_deref().or(f.module.as_deref()),
            names: std::iter::once(&f.member.name)
                .chain(&f.member.aliases)
                .cloned()
                .collect(),
            class: None,
            member: &f.member,
        });
        members.chain(functions)
    }
}

/// The functions every `bindings.wasm` has besides the IR's members,
/// fixed by the spec rather than listed in an [`Ir`] (IR.md, Bridge).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sys {
    /// `x === undefined`.
    IsUndefined,
    /// `undefined`.
    Undefined,
    /// `Number(x)`.
    NumberValue,
}

impl Sys {
    pub const ALL: [Sys; 3] = [Sys::IsUndefined, Sys::Undefined, Sys::NumberValue];

    /// The import name, in `./bindings.wasm`.
    pub fn name(self) -> &'static str {
        match self {
            Sys::IsUndefined => "JsValue.is_undefined",
            Sys::Undefined => "JsValue.undefined",
            Sys::NumberValue => "Number.value",
        }
    }

    /// The JS it performs on its parameter `x`.
    pub fn js(self) -> &'static str {
        match self {
            Sys::IsUndefined => "x === undefined",
            Sys::Undefined => "undefined",
            Sys::NumberValue => "Number(x)",
        }
    }

    /// Its parameter `x`, if any.
    pub fn param(self) -> Option<WasmType> {
        match self {
            Sys::IsUndefined => Some(WasmType::JS),
            Sys::Undefined => None,
            Sys::NumberValue => Some(WasmType::JS_NON_NULL),
        }
    }

    pub fn result(self) -> WasmType {
        match self {
            Sys::IsUndefined => WasmType::I32,
            Sys::Undefined => WasmType::ExternRef {
                null: false,
                undefined: true,
            },
            Sys::NumberValue => WasmType::F64,
        }
    }

    pub fn from_name(name: &str) -> Option<Sys> {
        Sys::ALL.into_iter().find(|s| s.name() == name)
    }
}
