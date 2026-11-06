//! What the emitters bind, reported as they emit it, so the IR builder
//! (`crate::abi`) sees exactly the bindings, names and order of the Rust
//! output without deciding any of them again.

use crate::codegen::signatures::FunctionSignature;
use crate::codegen::typemap::CodegenContext;
use crate::context::TypeId;
use crate::ir::{Heritage, ModuleContext, Throws, TypeRef};
use crate::parse::scope::ScopeId;

#[derive(Clone, Debug)]
pub(crate) enum Record {
    /// A declaration starts emitting; what follows until the next `Decl`
    /// comes from it. `decl` identifies the [`crate::ir::TypeDeclaration`].
    Decl {
        id: TypeId,
        decl: usize,
        js_namespace: Vec<String>,
    },
    /// A class-like extern block; the bindings that follow are its members.
    Class(ClassRecord),
    /// The bindings of one JS callable or setter, in every form.
    Callable {
        kind: Binding,
        sigs: Vec<FunctionSignature>,
    },
    Getter(GetterRecord),
    /// A variable bound as a value.
    Static {
        rust_name: String,
        js_name: String,
        type_ref: TypeRef,
        scope: ScopeId,
    },
    /// A variable's construct signatures, bound as constructors of
    /// `target` that `new` the variable.
    Construct {
        target: TypeId,
        js_class: String,
        sigs: Vec<FunctionSignature>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    Constructor,
    Method,
    StaticMethod,
    Setter,
    StaticSetter,
    Function,
}

#[derive(Clone, Debug)]
pub(crate) struct ClassRecord {
    pub js_name: String,
    pub module: ModuleContext,
    pub type_params: Vec<String>,
    pub heritage: Vec<Heritage>,
    pub runtime_class: bool,
    pub dictionary: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct GetterRecord {
    pub is_static: bool,
    pub rust_name: String,
    pub js_name: String,
    /// As read: an optional property admits `undefined`.
    pub type_ref: TypeRef,
    pub optional: bool,
    pub throws: Throws,
    pub doc: Option<String>,
    pub scope: ScopeId,
}

impl CodegenContext<'_> {
    pub(crate) fn record(&self, record: impl FnOnce() -> Record) {
        if let Some(records) = self.recorder.borrow_mut().as_mut() {
            records.push(record());
        }
    }
}

/// Report through `cgctx` when there is one.
pub(crate) fn record(cgctx: Option<&CodegenContext<'_>>, record: impl FnOnce() -> Record) {
    if let Some(cgctx) = cgctx {
        cgctx.record(record);
    }
}
