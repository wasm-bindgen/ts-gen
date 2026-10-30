//! The Wasm-level ABI ts-gen derives from a `.d.ts`: one function per JS
//! member, every JS value as `externref`, scalars as Wasm numeric types.
//!
//! This crate holds the model and its file format, the annotated WAT
//! template described in `ABI.md`, and nothing that depends on
//! TypeScript. ts-gen lowers a `.d.ts` to an [`Abi`] and writes it with
//! [`format::emit`]; a bindings generator reads it back with
//! [`format::parse`].

pub mod format;
mod sexpr;
pub mod wat;

/// Specifier a language module imports the bindings under.
pub const BINDINGS_SPECIFIER: &str = "./bindings.wasm";
/// Specifier for the fixed JS primitive module.
pub const BUILTINS_SPECIFIER: &str = "./builtins.js";
/// Import namespace for imported string constants (js-string builtins).
pub const STRING_CONSTANTS: &str = "wasm:js/string-constants";
/// Name the language module imports `undefined` under from `builtins.js`,
/// as an immutable `externref` global.
pub const UNDEFINED_GLOBAL: &str = "undefined";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WasmType {
    /// Only ever a TS `boolean`; `embed` relies on that to wrap writes in `Boolean()`.
    I32,
    I64,
    F64,
    /// A JS value. `null` / `undefined` record which empties the TS type
    /// admits; the wire type is `externref` either way. Only `null` is a
    /// wasm null, so `embed` lets a module take any position that does not
    /// admit `null` as `(ref extern)`.
    ExternRef {
        null: bool,
        undefined: bool,
    },
    /// Only used by the wrapper to hand a Wasm function to JS as a function object.
    FuncRef,
    /// A JS value a GC language holds as `anyref`: variadic array elements
    /// in the ABI, and any reference position of a module written that
    /// way. `null` as for [`WasmType::ExternRef`].
    AnyRef {
        null: bool,
    },
}

impl WasmType {
    pub fn wat(self) -> &'static str {
        match self {
            WasmType::I32 => "i32",
            WasmType::I64 => "i64",
            WasmType::F64 => "f64",
            WasmType::ExternRef { .. } => "externref",
            WasmType::FuncRef => "funcref",
            WasmType::AnyRef { .. } => "anyref",
        }
    }

    /// A JS value position with no nullability claim: intermediates in
    /// the bindings, `any`, type parameters.
    pub const JS: WasmType = WasmType::ExternRef {
        null: true,
        undefined: true,
    };
    /// A JS value position the TS type says is never `null`/`undefined`.
    pub const JS_NON_NULL: WasmType = WasmType::ExternRef {
        null: false,
        undefined: false,
    };

    /// A reference position that may hold wasm `null`.
    pub fn admits_null(self) -> bool {
        matches!(
            self,
            WasmType::ExternRef { null: true, .. } | WasmType::AnyRef { null: true }
        )
    }

    pub fn is_ref(self) -> bool {
        matches!(
            self,
            WasmType::ExternRef { .. } | WasmType::AnyRef { .. } | WasmType::FuncRef
        )
    }
}

/// An owner, by its identity: the `declare module` it is declared in, if
/// any, and its dotted JS path. A global `DurableObject` interface and
/// `"cloudflare:workers"`'s `DurableObject` class are two owners, so a
/// reference to either names its module.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OwnerRef {
    pub module: Option<String>,
    pub path: String,
}

impl OwnerRef {
    pub fn global(path: impl Into<String>) -> Self {
        OwnerRef {
            module: None,
            path: path.into(),
        }
    }
}

/// What a reference position names, ignoring empty arms, for a target
/// to wrap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// An owner (`WebAssembly.Module`, or `"cloudflare:sockets"`'s `Socket`).
    Declared(OwnerRef),
    /// A TS `string`.
    String,
    /// A scalar that crosses as a reference because the position admits
    /// an empty value.
    Number,
    Boolean,
    BigInt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiParam {
    pub name: String,
    pub ty: WasmType,
    /// The reference's [`Head`] (`Headers` for `Headers | null`), on
    /// reference positions only.
    pub head: Option<Head>,
}

/// What the bridge has to do to implement a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiOp {
    /// `receiver[name]`
    Get { name: String },
    /// `receiver[name] = value`
    Set { name: String },
    /// `receiver[name](...args)`
    Call { name: String },
    /// `new path(...args)`
    Construct { path: Vec<String> },
    /// `path[name]`, where `path` is a global object such as a class or
    /// namespace.
    StaticGet { path: Vec<String>, name: String },
    /// `path[name] = value`
    StaticSet { path: Vec<String>, name: String },
    /// `path[name](...args)` with `path` as the receiver.
    StaticCall { path: Vec<String>, name: String },
    /// `{}`
    NewObject,
    /// `x === undefined`; `null` needs nothing, `ref.is_null` in the language.
    IsUndefined,
    /// `Number(x)`: unboxes a `number` that crossed as a reference
    /// (a nullable scalar, or a union arm) to `f64`.
    NumberValue,
    /// `undefined`, for a language that cannot import a global.
    Undefined,
}

impl AbiOp {
    /// The JS name of the member the operation reaches, as written in the
    /// `.d.ts`: the property for an access or call, the class for a
    /// constructor.
    pub fn js_name(&self) -> Option<&str> {
        match self {
            AbiOp::Get { name }
            | AbiOp::Set { name }
            | AbiOp::Call { name }
            | AbiOp::StaticGet { name, .. }
            | AbiOp::StaticSet { name, .. }
            | AbiOp::StaticCall { name, .. } => Some(name),
            AbiOp::Construct { path } => path.last().map(String::as_str),
            _ => None,
        }
    }

    /// The operation's first parameter is the receiver.
    pub fn has_receiver(&self) -> bool {
        matches!(
            self,
            AbiOp::Get { .. } | AbiOp::Set { .. } | AbiOp::Call { .. }
        )
    }

    /// One of the functions every ABI has, answering what only JS can.
    pub fn is_sys(&self) -> bool {
        matches!(
            self,
            AbiOp::IsUndefined | AbiOp::NumberValue | AbiOp::Undefined
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiFunction {
    /// Export name in `bindings.wasm` and import field in the language
    /// module: `Type.member` with wasm-bindgen's variant suffixes.
    pub name: String,
    /// Further names `bindings.wasm` exports the same function under: a
    /// type exported under several names has each member under each.
    pub aliases: Vec<String>,
    /// The owner the function is a member of, as its dotted JS path in
    /// the function's module. `None` for a free function, a global
    /// variable and the sys functions.
    pub owner: Option<String>,
    pub op: AbiOp,
    /// Includes the receiver as the first parameter for instance ops.
    pub params: Vec<AbiParam>,
    pub result: Option<WasmType>,
    /// As [`AbiParam::head`], for the result.
    pub result_head: Option<Head>,
    /// TS declaration this came from, for documentation.
    pub ts: String,
    pub doc: Option<String>,
    /// Trailing parameter is a JS array spread into the call.
    pub variadic: bool,
    /// For declarations inside `declare module "spec"`: the specifier whose
    /// export roots the op's path (or is the callee itself when the path is
    /// empty), and which the language module imports the function from.
    /// `None` roots at `globalThis` and imports from `./bindings.wasm`.
    pub module: Option<String>,
}

impl AbiFunction {
    /// The module a language module imports this function from.
    pub fn import_module(&self) -> &str {
        self.module.as_deref().unwrap_or(BINDINGS_SPECIFIER)
    }
}

/// A function the module provides: the reverse of an [`AbiFunction`].
/// Parameters arrive by value (the language owns them), results are owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiHandler {
    /// Wasm export name the module provides: the JS path joined with `.`,
    /// with a leading `default.` dropped unless a top-level provider has
    /// the same path (`fetch`, `foo.bar.baz`, `default.add` beside `add`).
    pub name: String,
    /// JS path from the module namespace, which the wrapper synthesises
    /// (`["default", "fetch"]`, `["foo", "bar", "baz"]`).
    pub path: Vec<String>,
    pub params: Vec<AbiParam>,
    /// Wasm type of the result; for a `Promise<T>` result this is `T`'s,
    /// a nullable reference when `T` is `void`.
    pub result: Option<WasmType>,
    pub result_head: Option<Head>,
    /// The TS result was `Promise<T>`: under JSPI the module returns `T`
    /// and the wrapper's `promising` supplies the promise; otherwise the
    /// module returns the promise itself.
    pub promise: bool,
    pub ts: String,
    pub doc: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OwnerKind {
    /// A JS class: a constructor object exists at the path.
    Class,
    /// A TS interface with methods: a shape, with no JS object at the path.
    Interface,
    /// A property-only interface: a plain object, created by `Type.new`.
    Dictionary,
    /// A JS object at the path whose members are all static.
    Namespace,
}

impl OwnerKind {
    pub fn keyword(self) -> &'static str {
        match self {
            OwnerKind::Class => "class",
            OwnerKind::Interface => "interface",
            OwnerKind::Dictionary => "dictionary",
            OwnerKind::Namespace => "namespace",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Some(match s {
            "class" => OwnerKind::Class,
            "interface" => OwnerKind::Interface,
            "dictionary" => OwnerKind::Dictionary,
            "namespace" => OwnerKind::Namespace,
            _ => return None,
        })
    }
}

/// A class, interface, dictionary or namespace whose members the ABI
/// binds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbiOwner {
    /// Dotted JS path, which the member functions are named under.
    pub path: String,
    pub kind: OwnerKind,
    /// The `declare module` specifier the owner is declared in; `(module,
    /// path)` is its identity.
    pub module: Option<String>,
    /// Other names the owner is exported under, as dotted paths.
    pub aliases: Vec<String>,
    /// Owners this one inherits members from, as written: inherited
    /// members are only on the base.
    pub extends: Vec<OwnerRef>,
    pub doc: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Abi {
    pub owners: Vec<AbiOwner>,
    pub functions: Vec<AbiFunction>,
    pub handlers: Vec<AbiHandler>,
}

impl Abi {
    /// The first function named `name`, its own or an alias, from any
    /// module.
    pub fn function(&self, name: &str) -> Option<&AbiFunction> {
        self.functions
            .iter()
            .find(|f| f.name == name || f.aliases.iter().any(|a| a == name))
    }

    /// The function a language module imports as `name` from `module`:
    /// its own name or an alias.
    pub fn import(&self, module: &str, name: &str) -> Option<&AbiFunction> {
        self.functions.iter().find(|f| {
            f.import_module() == module && (f.name == name || f.aliases.iter().any(|a| a == name))
        })
    }

    /// The owner at `path` in `module`.
    pub fn owner(&self, module: Option<&str>, path: &str) -> Option<&AbiOwner> {
        self.owners
            .iter()
            .find(|o| o.module.as_deref() == module && o.path == path)
    }

    /// Module specifiers the ABI imports from, sorted and deduplicated.
    pub fn modules(&self) -> Vec<&str> {
        let mut specs: Vec<&str> = self
            .functions
            .iter()
            .filter_map(|f| f.module.as_deref())
            .collect();
        specs.sort_unstable();
        specs.dedup();
        specs
    }

    /// Put the owners in the order the template writes them: each before
    /// its first function, and those without functions after the last
    /// function, in their own order. A file read back is in this order.
    pub fn canonicalize(&mut self) {
        let first = |o: &AbiOwner| {
            self.functions
                .iter()
                .position(|f| {
                    f.owner.as_deref() == Some(&o.path)
                        && f.module.as_deref() == o.module.as_deref()
                })
                .unwrap_or(usize::MAX)
        };
        let mut keyed: Vec<(usize, usize, AbiOwner)> = std::mem::take(&mut self.owners)
            .into_iter()
            .enumerate()
            .map(|(i, o)| (first(&o), i, o))
            .collect();
        keyed.sort_by_key(|(f, i, _)| (*f, *i));
        self.owners = keyed.into_iter().map(|(_, _, o)| o).collect();
    }
}
