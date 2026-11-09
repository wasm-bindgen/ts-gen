//! Enum helpers (IR.md §8).
//!
//! [`flatten`] resolves an enum's hierarchy into one arm list: stated or
//! implicit indices from 1, `@fallback` = -1, base arms first, aliases resolved
//! through bases. It also gives the child-first discrimination order and
//! the widen/narrow (`From`/`TryFrom`) tables against every base. The
//! bridge and every language generator read enums through it, so they agree
//! on indices and orders.

use std::fmt;
use std::sync::OnceLock;

use crate::ir::*;
use crate::resolve::{Context, Location, Symbols, Target, TypeDecl};
use crate::text::Error;

/// The index of a `@fallback` arm, wherever it's declared.
pub const FALLBACK: i64 = -1;

/// The built-in `err` (IR.md §15):
///
/// ```ts
/// enum err {
///   @js(TypeError | RangeError) TypeError(Str);
///   @fallback Error(Str);
/// }
/// ```
pub fn builtin_err() -> &'static Enum {
    static ERR_ENUM: OnceLock<Enum> = OnceLock::new();
    ERR_ENUM.get_or_init(|| {
        let class = |name: &str| ClassPred {
            class: Path::new([name]),
            props: Vec::new(),
        };
        Enum {
            meta: Meta::default(),
            name: ERR.into(),
            carrier: None,
            extends: Vec::new(),
            members: Vec::new(),
            arms: vec![
                Arm {
                    meta: Meta {
                        attrs: Attrs {
                            js: vec![JsEntry::Class(vec![
                                class("TypeError"),
                                class("RangeError"),
                            ])],
                            ..Attrs::default()
                        },
                        ..Meta::default()
                    },
                    name: "TypeError".into(),
                    payload: vec![Type::StrBox],
                    value: None,
                },
                Arm {
                    meta: Meta {
                        attrs: Attrs {
                            fallback: true,
                            ..Attrs::default()
                        },
                        ..Meta::default()
                    },
                    name: "Error".into(),
                    payload: vec![Type::StrBox],
                    value: None,
                },
            ],
        }
    })
}

/// An enum declaration and where it's declared; `loc` is `None` for the
/// built-in `err`.
#[derive(Clone, Debug)]
pub struct EnumRef<'a> {
    pub loc: Option<Location>,
    pub decl: &'a Enum,
}

impl EnumRef<'_> {
    fn same(&self, other: &EnumRef<'_>) -> bool {
        std::ptr::eq(self.decl, other.decl)
    }

    /// The context paths inside the declaration resolve in.
    fn context(&self) -> Context<'_> {
        match &self.loc {
            Some(loc) => Context {
                module: loc.module.as_deref(),
                namespace: loc.path[..loc.path.len() - 1]
                    .iter()
                    .map(String::as_str)
                    .collect(),
                ..Context::default()
            },
            None => Context::default(),
        }
    }
}

impl fmt::Display for EnumRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.loc {
            Some(loc) => f.write_str(&crate::key::location(loc)),
            None => f.write_str(ERR),
        }
    }
}

/// How an enum crosses (IR.md §8, "Wire shapes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// All unit arms, no carrier: an `i32` index.
    Unit,
    /// `enum E: <int>`: the carrier, an open set.
    Carrier(Scalar),
    /// Any payload arm: `(i32 arm, slots…)`.
    Payload,
}

/// One arm of a flattened hierarchy.
#[derive(Clone, Debug)]
pub struct FlatArm<'a> {
    pub arm: &'a Arm,
    /// The enum that declares the arm.
    pub owner: EnumRef<'a>,
    /// The index, [`FALLBACK`], or the carrier's numeric value.
    pub value: i64,
}

impl FlatArm<'_> {
    pub fn is_fallback(&self) -> bool {
        self.arm.meta.attrs.fallback
    }
}

/// An enum's flattened hierarchy.
#[derive(Clone, Debug)]
pub struct Flat<'a> {
    pub this: EnumRef<'a>,
    pub shape: Shape,
    /// Index order: base arms first (the bases of `extends A, B` in order,
    /// each once), then the enum's own. Aliases are left out. A base's
    /// arms keep their values, and its `@fallback` keeps its declared
    /// position. Slot assignment uses this order too.
    pub arms: Vec<FlatArm<'a>>,
    /// Each alias arm (own or inherited), with the index into
    /// [`Flat::arms`] it names.
    pub aliases: Vec<(&'a Arm, usize)>,
    /// Discrimination order, as indices into [`Flat::arms`]: own arms,
    /// then each base's order, then the `@fallback`.
    pub discrimination: Vec<usize>,
    /// How many of [`Flat::discrimination`]'s first entries are the own
    /// arms' tier.
    pub own_tier: usize,
    /// Each arm's place within its tier (IR.md §8), by [`Flat::arms`]
    /// index: 0 literal units, 1 nominal, 2 structural, 3 `typeof`, 4 `Any`.
    pub ranks: Vec<u8>,
    /// Every transitive base, nearest first, each once, with the index into
    /// [`Flat::arms`] of each of the base's own flattened arms.
    pub bases: Vec<(EnumRef<'a>, Vec<usize>)>,
}

/// A widen/narrow table between an enum and one of its bases.
#[derive(Clone, Debug)]
pub struct Conversion<'a> {
    pub base: EnumRef<'a>,
    /// `From`: for each base arm, the child arm it widens to. Base indices
    /// keep their values in the child, so the wire value doesn't change.
    pub widen: Vec<usize>,
    /// `TryFrom`: for each child arm, the base arm it narrows to; `None`
    /// fails.
    pub narrow: Vec<Option<usize>>,
}

impl<'a> Flat<'a> {
    pub fn arm(&self, name: &str) -> Option<&FlatArm<'a>> {
        self.arms.iter().find(|a| a.arm.name == name).or_else(|| {
            let (_, i) = self.aliases.iter().find(|(a, _)| a.name == name)?;
            Some(&self.arms[*i])
        })
    }

    pub fn fallback(&self) -> Option<&FlatArm<'a>> {
        self.arms.iter().find(|a| a.is_fallback())
    }

    pub fn conversions(&self) -> Vec<Conversion<'a>> {
        self.bases
            .iter()
            .map(|(base, of)| {
                let mut narrow = vec![None; self.arms.len()];
                for (b, &c) in of.iter().enumerate() {
                    narrow[c] = Some(b);
                }
                Conversion {
                    base: base.clone(),
                    widen: of.clone(),
                    narrow,
                }
            })
            .collect()
    }
}

/// Resolves `path`, written in `cx`, to an enum.
pub fn resolve<'a>(symbols: &'a Symbols<'a>, path: &Path, cx: &Context<'_>) -> Option<EnumRef<'a>> {
    match symbols.resolve(path, cx)? {
        Target::Decl(loc, TypeDecl::Enum(e)) => Some(EnumRef {
            loc: Some(loc.clone()),
            decl: e,
        }),
        Target::Err => Some(EnumRef {
            loc: None,
            decl: builtin_err(),
        }),
        _ => None,
    }
}

/// Flattens the enum `e`, declared at `loc` (`None` for `err`). Errors are
/// the hierarchy's validation errors: unknown or non-enum bases, cycles,
/// duplicate arm names, more than one `@fallback`, a `@fallback` that isn't
/// its enum's last arm, index or value collisions, stated indices out of
/// range or on a `@fallback`, carrier mismatches, and unresolved aliases.
pub fn flatten<'a>(symbols: &'a Symbols<'a>, this: EnumRef<'a>) -> Result<Flat<'a>, Vec<Error>> {
    let mut stack = Vec::new();
    let mut errors = Vec::new();
    let flat = Flattener {
        symbols,
        errors: &mut errors,
    }
    .flatten(this, &mut stack);
    match flat {
        Some(f) if errors.is_empty() => Ok(f),
        _ => Err(errors),
    }
}

struct Flattener<'s, 'a> {
    symbols: &'a Symbols<'a>,
    errors: &'s mut Vec<Error>,
}

impl<'a> Flattener<'_, 'a> {
    fn err(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(Error::new(span, message));
    }

    /// Where an arm's test runs within its discrimination tier (IR.md §8):
    /// literals, then nominal tests (`instanceof`, `Array.isArray`, function
    /// and thenable), then structural ones (interfaces and nested enums),
    /// then `typeof` primitives, then `Any`. A nominal test never matches
    /// what an earlier structural one would have caught by mistake.
    fn test_rank(&self, a: &FlatArm<'a>) -> u8 {
        let t = match a.arm.payload.as_slice() {
            [] => return 0,
            // Several fields are a tuple.
            [_, _, ..] => return 1,
            [t] => t,
        };
        let t = match t {
            Type::Nullable(t, _) => t,
            t => t,
        };
        match t {
            Type::Vec(_)
            | Type::VecBox(_)
            | Type::MutVec(_)
            | Type::Array(_)
            | Type::ArrBox(_)
            | Type::Tuple(_)
            | Type::TupleBox(_)
            | Type::Fn(_)
            | Type::Future(..) => 1,
            Type::Named(r) => match self.symbols.resolve(&r.path, &a.owner.context()) {
                Some(Target::Decl(loc, TypeDecl::Class(_)))
                    if loc.module.is_none() && loc.path == [crate::layout::SYMBOL] =>
                {
                    3
                }
                Some(Target::Decl(_, TypeDecl::Class(c))) if c.kind == ClassKind::Class => 1,
                Some(Target::TypeParam | Target::Assoc) => 4,
                _ => 2,
            },
            Type::Any => 4,
            _ => 3,
        }
    }

    fn flatten(&mut self, this: EnumRef<'a>, stack: &mut Vec<EnumRef<'a>>) -> Option<Flat<'a>> {
        let e = this.decl;
        if stack.iter().any(|s| s.same(&this)) {
            self.err(e.meta.span, format!("`{this}` extends itself"));
            return None;
        }
        stack.push(this.clone());
        let mut bases: Vec<Flat<'a>> = Vec::new();
        let cx = this.context();
        for p in &e.extends {
            match resolve(self.symbols, p, &cx) {
                Some(b) => {
                    if let Some(f) = self.flatten(b, stack) {
                        bases.push(f);
                    }
                }
                None => {
                    let what = match self.symbols.resolve(p, &cx) {
                        Some(_) => "isn't an enum",
                        None => "isn't declared",
                    };
                    self.err(
                        e.meta.span,
                        format!(
                            "`{}` extends `{}`, which {what}",
                            e.name,
                            p.segments.join(".")
                        ),
                    );
                }
            }
        }
        stack.pop();

        let base_carriers: Vec<Option<Scalar>> = bases
            .iter()
            .map(|b| match b.shape {
                Shape::Carrier(c) => Some(c),
                _ => None,
            })
            .collect();
        if base_carriers.windows(2).any(|w| w[0] != w[1]) {
            self.err(
                e.meta.span,
                format!("`{}` extends enums with different carriers", e.name),
            );
        }
        let carrier = e.carrier.or(base_carriers.first().copied().flatten());

        let mut arms: Vec<FlatArm<'a>> = Vec::new();
        let mut aliases: Vec<(&'a Arm, usize)> = Vec::new();
        let mut flat_bases: Vec<(EnumRef<'a>, Vec<usize>)> = Vec::new();
        let mut discrimination = Vec::new();
        let mut base_fallback = None;
        for b in &bases {
            let mut of = Vec::with_capacity(b.arms.len());
            for a in &b.arms {
                let i = match arms.iter().position(|x| std::ptr::eq(x.arm, a.arm)) {
                    Some(i) => i,
                    None => {
                        arms.push(a.clone());
                        arms.len() - 1
                    }
                };
                of.push(i);
            }
            for (alias, i) in &b.aliases {
                if !aliases.iter().any(|(x, _)| std::ptr::eq(*x, *alias)) {
                    aliases.push((alias, of[*i]));
                }
            }
            for &d in &b.discrimination {
                let i = of[d];
                if arms[i].is_fallback() {
                    base_fallback = Some(i);
                } else if !discrimination.contains(&i) {
                    discrimination.push(i);
                }
            }
            let entry = (b.this.clone(), of.clone());
            if !flat_bases.iter().any(|(x, _)| x.same(&entry.0)) {
                flat_bases.push(entry);
            }
            for (bb, bof) in &b.bases {
                if !flat_bases.iter().any(|(x, _)| x.same(bb)) {
                    flat_bases.push((bb.clone(), bof.iter().map(|&k| of[k]).collect()));
                }
            }
        }
        let inherited = arms.len();

        let mut next = arms
            .iter()
            .map(|a| a.value)
            .filter(|&v| v > 0)
            .max()
            .unwrap_or(0)
            + 1;
        let own: Vec<&'a Arm> = e.arms.iter().collect();
        let last_real = own
            .iter()
            .rposition(|a| !matches!(a.value, Some(ArmValue::Alias(_))));
        let mut own_disc = Vec::new();
        let mut own_fallback = None;
        for (k, a) in own.iter().enumerate() {
            if let Some(ArmValue::Alias(_)) = a.value {
                continue;
            }
            if a.meta.attrs.fallback && Some(k) != last_real {
                self.err(
                    a.meta.span,
                    format!("`@fallback` arm `{}` must be the last arm", a.name),
                );
            }
            let value = if a.meta.attrs.fallback {
                if let (Some(ArmValue::Int(v)), None) = (&a.value, carrier) {
                    self.err(
                        a.meta.span,
                        format!("`@fallback` arm `{}` can't state the index {v}", a.name),
                    );
                }
                FALLBACK
            } else {
                match (&a.value, carrier) {
                    (Some(ArmValue::Int(v)), Some(_)) => *v,
                    (Some(ArmValue::Int(v)), None) => {
                        if !(1..=i32::MAX as i64).contains(v) {
                            self.err(
                                a.meta.span,
                                format!(
                                    "arm `{}` states the index {v}, which isn't from 1 to {}",
                                    a.name,
                                    i32::MAX
                                ),
                            );
                        }
                        next = next.max(*v + 1);
                        *v
                    }
                    (None, Some(c)) => {
                        self.err(
                            a.meta.span,
                            format!(
                                "arm `{}` of a `{}` carrier enum needs a numeric value",
                                a.name,
                                c.name()
                            ),
                        );
                        0
                    }
                    _ => {
                        let v = next;
                        next += 1;
                        v
                    }
                }
            };
            arms.push(FlatArm {
                arm: a,
                owner: this.clone(),
                value,
            });
            let i = arms.len() - 1;
            if a.meta.attrs.fallback {
                own_fallback = Some(i);
            } else {
                own_disc.push(i);
            }
        }

        // Unique names, one fallback, no index or value collisions.
        let mut fallbacks = 0;
        for (i, a) in arms.iter().enumerate() {
            let span = if i >= inherited {
                a.arm.meta.span
            } else {
                e.meta.span
            };
            if arms[..i].iter().any(|x| x.arm.name == a.arm.name) {
                self.err(
                    span,
                    format!("arm name `{}` appears twice in `{this}`", a.arm.name),
                );
            }
            if a.is_fallback() {
                fallbacks += 1;
                if fallbacks == 2 {
                    self.err(span, format!("`{this}` has more than one `@fallback`"));
                }
                continue;
            }
            if let Some(x) = arms[..i]
                .iter()
                .find(|x| !x.is_fallback() && x.value == a.value)
            {
                self.err(
                    span,
                    format!(
                        "arm `{}` of `{}` has index {}, which `{}` of `{}` already has",
                        a.arm.name, a.owner, a.value, x.arm.name, x.owner
                    ),
                );
            }
        }
        if let Some(c) = carrier {
            for a in &arms[inherited..] {
                if !a.is_fallback() && !fits(c, a.value) {
                    self.err(
                        a.arm.meta.span,
                        format!(
                            "arm `{}` value {} doesn't fit `{}`",
                            a.arm.name,
                            a.value,
                            c.name()
                        ),
                    );
                }
            }
        }

        for a in own.iter() {
            let Some(ArmValue::Alias(target)) = &a.value else {
                continue;
            };
            if arms.iter().any(|x| x.arm.name == a.name)
                || aliases.iter().any(|(x, _)| x.name == a.name)
            {
                self.err(
                    a.meta.span,
                    format!("arm name `{}` appears twice in `{this}`", a.name),
                );
            }
            if !a.payload.is_empty() || a.meta.attrs != Attrs::default() {
                self.err(
                    a.meta.span,
                    format!("alias `{}` can't have a payload or attributes", a.name),
                );
            }
            match self.alias_target(&own, &arms, &aliases, target) {
                Some(i) => aliases.push((a, i)),
                None => self.err(
                    a.meta.span,
                    format!(
                        "alias `{}` names `.{target}`, which isn't an arm of `{this}`",
                        a.name
                    ),
                ),
            }
        }

        own_disc.sort_by_key(|&i| self.test_rank(&arms[i]));
        discrimination.sort_by_key(|&i| self.test_rank(&arms[i]));
        let own_tier = own_disc.len();
        let ranks = arms.iter().map(|a| self.test_rank(a)).collect();
        let mut order = own_disc;
        order.extend(discrimination);
        order.extend(own_fallback.or(base_fallback));

        let shape = if let Some(c) = carrier {
            Shape::Carrier(c)
        } else if arms.iter().any(|a| !a.arm.payload.is_empty()) {
            Shape::Payload
        } else {
            Shape::Unit
        };
        Some(Flat {
            this,
            shape,
            arms,
            aliases,
            discrimination: order,
            own_tier,
            ranks,
            bases: flat_bases,
        })
    }

    /// An alias resolves to a real arm, through other aliases.
    fn alias_target(
        &self,
        own: &[&'a Arm],
        arms: &[FlatArm<'a>],
        aliases: &[(&'a Arm, usize)],
        target: &str,
    ) -> Option<usize> {
        let mut name = target;
        for _ in 0..=own.len() {
            if let Some(i) = arms.iter().position(|a| a.arm.name == name) {
                return Some(i);
            }
            if let Some((_, i)) = aliases.iter().find(|(a, _)| a.name == name) {
                return Some(*i);
            }
            match own.iter().find(|a| a.name == name)?.value.as_ref()? {
                ArmValue::Alias(next) => name = next,
                ArmValue::Int(_) => return None,
            }
        }
        None
    }
}

/// Whether `v` is in range for the integer carrier `c`.
pub fn fits(c: Scalar, v: i64) -> bool {
    let (lo, hi): (i128, i128) = match c {
        Scalar::I8 => (i8::MIN.into(), i8::MAX.into()),
        Scalar::U8 => (0, u8::MAX.into()),
        Scalar::I16 => (i16::MIN.into(), i16::MAX.into()),
        Scalar::U16 => (0, u16::MAX.into()),
        Scalar::I32 => (i32::MIN.into(), i32::MAX.into()),
        Scalar::U32 => (0, u32::MAX.into()),
        Scalar::I64 => (i64::MIN.into(), i64::MAX.into()),
        Scalar::U64 => (0, u64::MAX.into()),
        _ => return false,
    };
    (lo..=hi).contains(&i128::from(v))
}
