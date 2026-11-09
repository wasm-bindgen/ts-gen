//! Reading the compiled language module: what it imports from the bridge,
//! with each function import's type resolved structurally so it compares
//! against the layout, and what it exports.

use std::cell::RefCell;

use wasmparser::{
    AbstractHeapType, CompositeInnerType, ExternalKind, HeapType, Parser, Payload, StorageType,
    SubType, TypeRef,
};

use ts_gen_abi::layout::{Def, FuncType, Heap, RefType, Storage, ValType};

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub enum ImportKind {
    Func(FuncType),
    Tag(FuncType),
    /// A function or tag whose type the ABI can't name: an error only if
    /// the import is the bridge's, since the embedder's may use any type.
    Unreadable(String),
    Other,
}

#[derive(Clone, Debug)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub imports: Vec<Import>,
    pub exports: Vec<(String, ExternalKind, u32)>,
    /// Every global's value type and mutability, imported ones first.
    pub globals: Vec<(wasmparser::ValType, bool)>,
    /// Each exported function's type, or why it can't be read.
    pub export_funcs: Vec<(String, std::result::Result<FuncType, String>)>,
}

impl Module {
    pub fn read(bytes: &[u8]) -> Result<Module> {
        let mut types: Vec<SubType> = Vec::new();
        let mut alone: Vec<bool> = Vec::new();
        let mut m = Module::default();
        let mut raw = Vec::new();
        let mut func_types: Vec<u32> = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            match payload.map_err(wasm)? {
                Payload::TypeSection(r) => {
                    for group in r {
                        let group = group.map_err(wasm)?;
                        let n = group.types().len();
                        alone.extend(std::iter::repeat_n(n == 1, n));
                        types.extend(group.into_types());
                    }
                }
                Payload::ImportSection(r) => {
                    for i in r.into_imports() {
                        let i = i.map_err(wasm)?;
                        match i.ty {
                            TypeRef::Global(g) => m.globals.push((g.content_type, g.mutable)),
                            TypeRef::Func(t) | TypeRef::FuncExact(t) => func_types.push(t),
                            _ => {}
                        }
                        raw.push((i.module.to_string(), i.name.to_string(), i.ty));
                    }
                }
                Payload::ExportSection(r) => {
                    for e in r {
                        let e = e.map_err(wasm)?;
                        m.exports.push((e.name.to_string(), e.kind, e.index));
                    }
                }
                Payload::FunctionSection(r) => {
                    for t in r {
                        func_types.push(t.map_err(wasm)?);
                    }
                }
                Payload::GlobalSection(r) => {
                    for g in r {
                        let g = g.map_err(wasm)?;
                        m.globals.push((g.ty.content_type, g.ty.mutable));
                    }
                }
                _ => {}
            }
        }
        let types = Types {
            types: &types,
            alone: &alone,
            visiting: RefCell::new(Vec::new()),
        };
        for (name, kind, i) in &m.exports {
            if *kind == ExternalKind::Func {
                let ty = match func_types.get(*i as usize) {
                    Some(t) => func(&types, *t).map_err(|e| e.to_string()),
                    None => Err("an out-of-range function".into()),
                };
                m.export_funcs.push((name.clone(), ty));
            }
        }
        for (module, name, ty) in raw {
            let read = |t, f: fn(FuncType) -> ImportKind| {
                func(&types, t).map_or_else(|e| ImportKind::Unreadable(e.to_string()), f)
            };
            let kind = match ty {
                TypeRef::Func(t) | TypeRef::FuncExact(t) => read(t, ImportKind::Func),
                TypeRef::Tag(t) => read(t.func_type_idx, ImportKind::Tag),
                _ => ImportKind::Other,
            };
            m.imports.push(Import { module, name, kind });
        }
        Ok(m)
    }

    pub fn exports(&self, name: &str, kind: ExternalKind) -> bool {
        self.exports.iter().any(|(n, k, _)| n == name && *k == kind)
    }

    /// An exported function's type, `None` when it isn't exported.
    pub fn export_func(&self, name: &str) -> Option<Result<FuncType>> {
        self.export_funcs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| {
                t.clone()
                    .map_err(|e| Error::Module(format!("the export `{name}`: {e}")))
            })
    }

    /// An exported global's value type and mutability.
    pub fn global(&self, name: &str) -> Option<(wasmparser::ValType, bool)> {
        self.exports
            .iter()
            .find(|(n, k, _)| n == name && *k == ExternalKind::Global)
            .and_then(|(_, _, i)| self.globals.get(*i as usize).copied())
    }
}

fn wasm(e: wasmparser::BinaryReaderError) -> Error {
    Error::Module(e.to_string())
}

struct Types<'a> {
    types: &'a [SubType],
    alone: &'a [bool],
    /// The definitions being read, to stop at a recursive one.
    visiting: RefCell<Vec<u32>>,
}

fn sub<'a>(types: &Types<'a>, i: u32) -> Result<&'a SubType> {
    types
        .types
        .get(i as usize)
        .ok_or_else(|| Error::Module(format!("type index {i} out of range")))
}

fn func(types: &Types<'_>, i: u32) -> Result<FuncType> {
    match &sub(types, i)?.composite_type.inner {
        CompositeInnerType::Func(f) => Ok(FuncType {
            params: f
                .params()
                .iter()
                .map(|v| val(types, *v))
                .collect::<Result<_>>()?,
            results: f
                .results()
                .iter()
                .map(|v| val(types, *v))
                .collect::<Result<_>>()?,
        }),
        _ => Err(Error::Module(format!("type {i} is not a function type"))),
    }
}

fn val(types: &Types<'_>, v: wasmparser::ValType) -> Result<ValType> {
    Ok(match v {
        wasmparser::ValType::I32 => ValType::I32,
        wasmparser::ValType::I64 => ValType::I64,
        wasmparser::ValType::F32 => ValType::F32,
        wasmparser::ValType::F64 => ValType::F64,
        wasmparser::ValType::V128 => return Err(Error::Module("`v128` is not in the ABI".into())),
        wasmparser::ValType::Ref(r) => {
            let heap = match r.heap_type() {
                HeapType::Abstract {
                    ty: AbstractHeapType::Extern,
                    shared: false,
                } => Heap::Extern,
                HeapType::Abstract {
                    ty: AbstractHeapType::Any,
                    shared: false,
                } => Heap::Any,
                HeapType::Concrete(i) => {
                    let i = i
                        .as_module_index()
                        .ok_or_else(|| Error::Module("a non-module type index".into()))?;
                    Heap::Def(Box::new(def(types, i)?))
                }
                h => {
                    return Err(Error::Module(format!(
                        "the heap type `{h:?}` is not in the ABI"
                    )))
                }
            };
            ValType::Ref(RefType {
                nullable: r.is_nullable(),
                heap,
            })
        }
    })
}

fn def(types: &Types<'_>, i: u32) -> Result<Def> {
    if types.visiting.borrow().contains(&i) {
        return Err(Error::Module(format!(
            "type {i} is recursive, which the ABI can't name"
        )));
    }
    types.visiting.borrow_mut().push(i);
    let d = def_at(types, i);
    types.visiting.borrow_mut().pop();
    d
}

fn def_at(types: &Types<'_>, i: u32) -> Result<Def> {
    let t = sub(types, i)?;
    if !types.alone[i as usize] || !t.is_final || !t.supertype_idxs.is_empty() {
        return Err(Error::Module(format!(
            "type {i} must be a final singleton rec group to match the bridge's"
        )));
    }
    let storage = |f: &wasmparser::FieldType| -> Result<Storage> {
        Ok(match f.element_type {
            StorageType::I8 => Storage::I8,
            StorageType::I16 => Storage::I16,
            StorageType::Val(v) => Storage::Val(val(types, v)?),
        })
    };
    match &t.composite_type.inner {
        CompositeInnerType::Array(a) if a.0.mutable => Ok(Def::Array(storage(&a.0)?)),
        CompositeInnerType::Struct(s) if s.fields.iter().all(|f| !f.mutable) => Ok(Def::Struct(
            s.fields.iter().map(storage).collect::<Result<_>>()?,
        )),
        _ => Err(Error::Module(format!(
            "type {i} is not an ABI type: arrays are `(mut E)` and struct fields immutable"
        ))),
    }
}
