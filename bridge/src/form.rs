//! The glue wire and memory forms of a value.
//!
//! The JS glue can't read GC objects, so the bridge module translates every
//! GC carrier into a linear form in its own scratch memory and table, and
//! the glue only ever converts linear forms: in the language's memory
//! ([`Space::Lang`]) or the bridge's scratch ([`Space::Scratch`]). A
//! value's **glue wire** ([`Forms::jsw`]) is its wire with each GC carrier
//! replaced by its scratch descriptor; without GC inside it's the wire
//! itself.
//!
//! The **memory form** of a `T[]` element ([`Forms::elem`]) stores the
//! element's glue wire values one by one ([`Store`]). In the language's
//! space it's exactly the linear layout of IR.md §11. In scratch, an enum
//! with a GC payload gives each arm its own region instead of shared slots,
//! because a shared GC slot has a different linear form per arm.

use ts_gen_abi::ir::Scalar;
use ts_gen_abi::layout::{carrier, EnumLayout, Kind, Repr, ScalarNull, Shape, ValType};
use ts_gen_abi::profile::{Carrier, Encoding, MemoryWidth, Profile, StrCarrier};

use crate::{unsupported, Result};

/// A glue wire value: what JS sees of a Wasm value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Jv {
    I32,
    I64,
    F32,
    F64,
    Ext,
}

impl Jv {
    pub fn of(v: &ValType) -> Jv {
        match v {
            ValType::I32 => Jv::I32,
            ValType::I64 => Jv::I64,
            ValType::F32 => Jv::F32,
            ValType::F64 => Jv::F64,
            ValType::Ref(_) => Jv::Ext,
        }
    }

    pub fn wat(self) -> &'static str {
        match self {
            Jv::I32 => "i32",
            Jv::I64 => "i64",
            Jv::F32 => "f32",
            Jv::F64 => "f64",
            Jv::Ext => "externref",
        }
    }

    /// The JS zero of the value: an unused slot.
    pub fn zero(self) -> &'static str {
        match self {
            Jv::I64 => "0n",
            Jv::Ext => "null",
            _ => "0",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    Lang,
    Scratch,
}

impl Space {
    pub fn of(c: Carrier) -> Space {
        match c {
            Carrier::Linear => Space::Lang,
            Carrier::Gc => Space::Scratch,
            Carrier::Host => unreachable!("a host value crosses as itself"),
        }
    }

    pub fn js(self) -> &'static str {
        match self {
            Space::Lang => "$L",
            Space::Scratch => "$S",
        }
    }
}

/// A number in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Num {
    I8,
    U8,
    I16,
    U16,
    I32,
    I64,
    F16,
    F32,
    F64,
}

impl Num {
    pub fn size(self) -> u32 {
        match self {
            Num::I8 | Num::U8 => 1,
            Num::I16 | Num::U16 | Num::F16 => 2,
            Num::I32 | Num::F32 => 4,
            Num::I64 | Num::F64 => 8,
        }
    }

    pub fn of_scalar(s: Scalar) -> Num {
        match s {
            Scalar::Bool | Scalar::U8 => Num::U8,
            Scalar::I8 => Num::I8,
            Scalar::I16 => Num::I16,
            Scalar::U16 => Num::U16,
            Scalar::I32 | Scalar::U32 => Num::I32,
            Scalar::I53 | Scalar::U53 | Scalar::I64 | Scalar::U64 => Num::I64,
            Scalar::F16 => Num::F16,
            Scalar::F32 => Num::F32,
            Scalar::F64 => Num::F64,
        }
    }

    pub fn of_jv(j: Jv) -> Num {
        match j {
            Jv::I32 => Num::I32,
            Jv::I64 => Num::I64,
            Jv::F32 => Num::F32,
            Jv::F64 => Num::F64,
            Jv::Ext => unreachable!("references never sit in memory"),
        }
    }

    /// The `DataView` accessor suffix.
    pub fn dv(self) -> &'static str {
        match self {
            Num::I8 => "Int8",
            Num::U8 => "Uint8",
            Num::I16 => "Int16",
            Num::U16 => "Uint16",
            Num::I32 => "Int32",
            Num::I64 => "BigInt64",
            Num::F16 => "Float16",
            Num::F32 => "Float32",
            Num::F64 => "Float64",
        }
    }
}

/// How one glue wire value of an element sits in memory or the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Store {
    Num {
        off: u32,
        num: Num,
    },
    /// A niche-encoded nullable scalar, as `(u8 present, num)`.
    Niche {
        off: u32,
        num: Num,
        niche: i64,
    },
    /// An `i32` present flag, as a `u8`.
    Flag {
        off: u32,
    },
    /// The `idx`th table slot of the element.
    Ref {
        idx: u32,
    },
    /// A GC value in a linear `T[]` (§10): the `idx`th table slot holds it
    /// externalized, and the bridge module converts it to and from its `n`
    /// glue wire values in scratch. `conv` indexes [`Elem::convs`].
    Conv {
        idx: u32,
        n: u32,
        conv: u32,
    },
    /// A payload enum with GC payloads in a linear `T[]`: the `i32` arm at
    /// `off`, then the joined slots, of which only the arm's own hold values.
    /// Its glue wire is the arm, then every arm's payload in turn.
    Arm {
        off: u32,
        /// Every slot's location, zeroed before an arm's are written.
        slots: Vec<Store>,
        arms: Vec<ArmStores>,
    },
}

/// One arm of a [`Store::Arm`]: its payload's glue wire values, each with
/// its zero for the other arms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmStores {
    pub index: i64,
    pub values: Vec<(Store, Vec<Jv>)>,
}

/// A `T[]` element's memory form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elem {
    pub size: u32,
    pub align: u32,
    pub refs: u32,
    /// One per glue wire value of the element, except that a
    /// [`Store::Conv`] covers all of its value's.
    pub stores: Vec<Store>,
    /// The GC values [`Store::Conv`] converts.
    pub convs: Vec<Repr>,
}

pub struct Forms {
    pub wide: bool,
    /// Erased `T` is `anyref` under GC handles (§13).
    pub erased_gc: bool,
}

impl Forms {
    pub fn new(profile: &Profile) -> Forms {
        Forms {
            wide: profile.memory == Some(MemoryWidth::M64),
            erased_gc: profile.handles == Carrier::Gc,
        }
    }

    /// The pointer of a space.
    pub fn ptr(&self, s: Space) -> Jv {
        match s {
            Space::Lang if self.wide => Jv::I64,
            _ => Jv::I32,
        }
    }

    /// Whether a GC carrier sits anywhere inside `r`, so that the bridge
    /// module has to translate it.
    pub fn has_gc(&self, r: &Repr) -> bool {
        match &r.kind {
            Kind::Str { carrier, .. } => matches!(carrier, StrCarrier::Gc(_)),
            Kind::Vec { carrier, .. } => *carrier == Carrier::Gc,
            // A linear `T[]`'s GC elements sit in the language's table,
            // converted element by element; a host one's are JS values.
            Kind::Array { carrier, .. } => *carrier == Carrier::Gc,
            Kind::Tuple { elems, .. } => elems.iter().any(|r| self.has_gc(r)),
            Kind::Enum { layout, .. } => self.enum_has_gc(layout),
            // A GC handle is `anyref`, which crosses JS externalized.
            Kind::Handle { carrier, .. } => *carrier == Carrier::Gc,
            Kind::Fn { provided, .. } | Kind::Rec { provided, .. } => {
                *provided == Some(Carrier::Gc)
            }
            Kind::Erased { .. } => self.erased_gc,
            _ => false,
        }
    }

    pub fn enum_has_gc(&self, e: &EnumLayout) -> bool {
        e.arms
            .iter()
            .any(|a| a.payload.iter().any(|r| self.has_gc(r)))
    }

    /// `r`'s glue wire.
    pub fn jsw(&self, r: &Repr) -> Result<Vec<Jv>> {
        let mut out = Vec::new();
        self.walk(r, &mut out)?;
        Ok(out)
    }

    fn seq(&self, s: Space, out: &mut Vec<Jv>) {
        out.push(self.ptr(s));
        out.push(self.ptr(s));
    }

    fn walk(&self, r: &Repr, out: &mut Vec<Jv>) -> Result<()> {
        match &r.kind {
            Kind::Unit | Kind::Never => {}
            Kind::Scalar(s) | Kind::NullScalar(s, ScalarNull::Niche(_)) => out.push(scalar_jv(*s)),
            Kind::NullScalar(s, ScalarNull::Present) => {
                out.push(Jv::I32);
                out.push(scalar_jv(*s));
            }
            Kind::Str { carrier, .. } => match carrier {
                StrCarrier::Linear(_) => self.seq(Space::Lang, out),
                StrCarrier::Gc(Encoding::Utf8) => self.seq(Space::Scratch, out),
                // `wasm:js-string` converts WTF-16 directly.
                StrCarrier::Gc(Encoding::Wtf16) | StrCarrier::Host => out.push(Jv::Ext),
            },
            Kind::Vec {
                carrier: Carrier::Host,
                ..
            }
            | Kind::Array {
                carrier: Carrier::Host,
                ..
            } => out.push(Jv::Ext),
            Kind::Vec { carrier, .. } => self.seq(Space::of(*carrier), out),
            Kind::Array { elem, carrier, .. } => {
                let s = Space::of(*carrier);
                let e = self.elem(elem, s)?;
                if e.size > 0 {
                    out.push(self.ptr(s));
                }
                if e.refs > 0 {
                    out.push(Jv::I32);
                }
                out.push(self.ptr(s));
            }
            Kind::Tuple { elems, nullable } => {
                if *nullable {
                    out.push(Jv::I32);
                }
                for e in elems {
                    self.walk(e, out)?;
                }
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Index => out.push(Jv::I32),
                Shape::Carrier(c) => {
                    if *nullable {
                        out.push(Jv::I32);
                    }
                    out.push(scalar_jv(*c));
                }
                Shape::Payload { slots } => {
                    out.push(Jv::I32);
                    if self.enum_has_gc(layout) {
                        for a in &layout.arms {
                            for p in &a.payload {
                                self.walk(p, out)?;
                            }
                        }
                    } else {
                        out.extend(slots.iter().map(Jv::of));
                    }
                }
            },
            Kind::Host { .. } => out.push(Jv::Ext),
            Kind::View { .. } => {
                out.push(self.ptr(Space::Lang));
                out.push(self.ptr(Space::Lang));
            }
            Kind::Handle { carrier, .. }
            | Kind::Fn {
                provided: Some(carrier),
                ..
            }
            | Kind::Rec {
                provided: Some(carrier),
                ..
            } => out.push(match carrier {
                Carrier::Linear => self.ptr(Space::Lang),
                Carrier::Gc | Carrier::Host => Jv::Ext,
            }),
            Kind::Fn { provided: None, .. } | Kind::Rec { provided: None, .. } => out.push(Jv::Ext),
            Kind::Erased { .. } => out.push(Jv::Ext),
        }
        Ok(())
    }

    /// The memory form of an element of a `T[]` in space `s`.
    pub fn elem(&self, r: &Repr, s: Space) -> Result<Elem> {
        let mut e = Elem {
            size: 0,
            align: 1,
            refs: 0,
            stores: Vec::new(),
            convs: Vec::new(),
        };
        match r.kind {
            // A GC array of scalars holds them natively, `f16` included.
            Kind::Scalar(sc) if s == Space::Scratch => Forms::num(&mut e, Num::of_scalar(sc)),
            _ => self.place(r, &mut e, s)?,
        }
        Ok(e)
    }

    fn num(e: &mut Elem, num: Num) {
        e.stores.push(Store::Num { off: e.size, num });
        e.size += num.size();
        e.align = e.align.max(num.size());
    }

    fn flag(e: &mut Elem) {
        e.stores.push(Store::Flag { off: e.size });
        e.size += 1;
    }

    fn place(&self, r: &Repr, e: &mut Elem, sp: Space) -> Result<()> {
        // The bridge module can't convert `f16`, so scratch holds its `f32`.
        let scalar = |s: Scalar| match (Num::of_scalar(s), sp) {
            (Num::F16, Space::Scratch) => Num::F32,
            (n, _) => n,
        };
        match &r.kind {
            Kind::Str {
                carrier: StrCarrier::Gc(_),
                ..
            }
            | Kind::Vec {
                carrier: Carrier::Gc,
                ..
            }
            | Kind::Array {
                carrier: Carrier::Gc,
                ..
            } if sp == Space::Lang => {
                e.stores.push(Store::Conv {
                    idx: e.refs,
                    n: self.jsw(r)?.len() as u32,
                    conv: e.convs.len() as u32,
                });
                e.convs.push(r.clone());
                e.refs += 1;
            }
            Kind::Enum { layout, .. } if sp == Space::Lang && self.enum_has_gc(layout) => {
                self.place_gc_enum(r, layout, e)?;
            }
            Kind::Scalar(s) => Forms::num(e, scalar(*s)),
            Kind::NullScalar(s, ScalarNull::Niche(n)) => {
                let num = Num::of_scalar(*s);
                e.stores.push(Store::Niche {
                    off: e.size,
                    num,
                    niche: *n,
                });
                e.size += 1 + num.size();
                e.align = e.align.max(num.size());
            }
            Kind::NullScalar(s, ScalarNull::Present) => {
                Forms::flag(e);
                Forms::num(e, scalar(*s));
            }
            Kind::Tuple { elems, nullable } => {
                if *nullable {
                    Forms::flag(e);
                }
                for t in elems {
                    self.place(t, e, sp)?;
                }
            }
            Kind::Enum { layout, nullable } => match &layout.shape {
                Shape::Index => Forms::num(e, Num::I32),
                Shape::Carrier(c) => {
                    if *nullable {
                        Forms::flag(e);
                    }
                    Forms::num(e, scalar(*c));
                }
                Shape::Payload { .. } if self.enum_has_gc(layout) => {
                    Forms::num(e, Num::I32);
                    for a in &layout.arms {
                        for p in &a.payload {
                            self.place(p, e, sp)?;
                        }
                    }
                }
                Shape::Payload { slots } => {
                    Forms::num(e, Num::I32);
                    for v in slots {
                        match Jv::of(v) {
                            Jv::Ext => Forms::reference(e),
                            j => Forms::num(e, Num::of_jv(j)),
                        }
                    }
                }
            },
            Kind::Unit => {}
            Kind::View { .. } => return unsupported("`*T[]` is legal only in import parameters"),
            _ => {
                for j in self.jsw(r)? {
                    match j {
                        Jv::Ext => Forms::reference(e),
                        j => Forms::num(e, Num::of_jv(j)),
                    }
                }
            }
        }
        Ok(())
    }

    /// The joined form of [`ts_gen_abi::layout::Layouts::elem`]: the arm,
    /// numeric slots in memory, reference slots in the table, with GC
    /// carriers externalized there and converted by the bridge module.
    fn place_gc_enum(&self, r: &Repr, layout: &EnumLayout, e: &mut Elem) -> Result<()> {
        let Shape::Payload { slots } = &layout.shape else {
            unreachable!("GC payloads make a payload enum")
        };
        let off = e.size;
        e.size += 4;
        e.align = e.align.max(4);
        let mut at = Vec::new();
        for v in slots {
            let st = match Jv::of(v) {
                Jv::Ext => {
                    e.refs += 1;
                    Store::Ref { idx: e.refs - 1 }
                }
                j => {
                    let num = Num::of_jv(j);
                    let st = Store::Num { off: e.size, num };
                    e.size += num.size();
                    e.align = e.align.max(num.size());
                    st
                }
            };
            at.push(st);
        }
        let mut arms = Vec::new();
        for a in &layout.arms {
            let mut values = Vec::new();
            let mut wire = a.slots.iter();
            for p in &a.payload {
                let jsw = self.jsw(p)?;
                let carrier = matches!(
                    p.kind,
                    Kind::Str {
                        carrier: StrCarrier::Gc(_),
                        ..
                    } | Kind::Vec {
                        carrier: Carrier::Gc,
                        ..
                    } | Kind::Array {
                        carrier: Carrier::Gc,
                        ..
                    }
                );
                if carrier {
                    let slot = *wire.next().expect("a GC carrier's slot");
                    let Store::Ref { idx } = at[slot] else {
                        unreachable!("a GC carrier's slot is a reference")
                    };
                    values.push((
                        Store::Conv {
                            idx,
                            n: jsw.len() as u32,
                            conv: e.convs.len() as u32,
                        },
                        jsw,
                    ));
                    e.convs.push(p.clone());
                    continue;
                }
                if matches!(p.kind, Kind::Tuple { .. } | Kind::Enum { .. }) && self.has_gc(p) {
                    return unsupported(format!(
                        "`{}`: the payload `{}` mixes GC and other values inside a linear `T[]`",
                        r.ty, p.ty
                    ));
                }
                for j in jsw {
                    let slot = *wire.next().expect("a slot per wire value");
                    values.push((at[slot].clone(), vec![j]));
                }
            }
            arms.push(ArmStores {
                index: a.index,
                values,
            });
        }
        e.stores.push(Store::Arm {
            off,
            slots: at,
            arms,
        });
        Ok(())
    }

    fn reference(e: &mut Elem) {
        e.stores.push(Store::Ref { idx: e.refs });
        e.refs += 1;
    }
}

pub fn scalar_jv(s: Scalar) -> Jv {
    Jv::of(&carrier(s))
}
