//! The ts-gen IR: a `.d.ts` expanded into uniquely named bindings with
//! wasm-level types, specified in `IR.md`.
//!
//! This crate holds the model ([`ir`]), its text format ([`text`]) and
//! nothing that depends on TypeScript. An [`ir::Ir`] is written with
//! [`text::emit`]; a bindings generator reads it back with
//! [`text::parse`].

pub mod ir;
pub mod text;

/// Specifier a language module imports the bindings under.
pub const BINDINGS_SPECIFIER: &str = "./bindings.wasm";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WasmType {
    /// Only ever a `bool`; `embed` relies on that to wrap writes in `Boolean()`.
    I32,
    I64,
    F64,
    /// A JS value. `null` / `undefined` record which empties the type
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
    /// on the wire, and any reference position of a module written that
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

/// A class or kind by its identity: the module it is declared in, if any,
/// and its binding name. A global `DurableObject` and `"cloudflare:workers"`'s
/// `DurableObject` are two, so a reference to either names its module.
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
