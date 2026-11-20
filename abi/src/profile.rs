//! The profile (IR.md §12): the settings the bridge generator and the
//! language generator share. The IR is unchanged by it.
//!
//! The text form is a list of entries separated by newlines or commas, with
//! `#` comments. A preset (`all-linear`, `all-gc`, `all-host`) may come first; every
//! other entry is `key = value`:
//!
//! ```text
//! all-gc
//! memory = 32
//! str = host
//! rename.tw:terminated = __instance_terminated
//! ```
//!
//! With no preset the base is `all-linear`.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carrier {
    Linear,
    Gc,
    /// `externref`: the JS value itself (a typed array, a JS array, or a
    /// language object externalized with `extern.convert_any`), for
    /// languages whose interop can't name GC types.
    Host,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Wtf16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrCarrier {
    Linear(Encoding),
    Gc(Encoding),
    /// `externref` JS strings, used natively through `wasm:js-string`.
    Host,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryWidth {
    M32,
    M64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eh {
    /// `try_table` / `exnref`.
    Modern,
    /// `try` / `catch` / `rethrow`.
    Legacy,
}

/// How core functions between the language and the bridge return (§12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Results {
    Multi,
    /// Slot 0 is the result, and the rest go through the spill, for
    /// languages that can't declare multi-value functions.
    Single,
}

/// An optional box's none in Wasm (§12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxNone {
    /// `ref.null`: the bridge folds both JS empties into it, and writes the
    /// node's default for it.
    Null,
    /// The unit box, `undefined`, with empties kept faithfully.
    Unit,
}

/// How `throws` errors cross between the language and the bridge (§15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Throws {
    /// A tag per error enum, `("tw:tag", E)`.
    Tag,
    /// JS exceptions in, `("tw:err", "set:E")` out, for languages that
    /// can't import tags.
    Js,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// `T[]`.
    pub vec: Carrier,
    /// `arr<T>`.
    pub array: Carrier,
    pub str: StrCarrier,
    pub handles: Carrier,
    /// The linear memory, defined and exported by the language. GC doesn't
    /// preclude one (for `*T[]`).
    pub memory: Option<MemoryWidth>,
    pub eh: Eh,
    pub validation: bool,
    /// A `@js(null) T?` rejects `undefined` from JS (§10).
    pub strict_null: bool,
    pub box_none: BoxNone,
    /// `tw:terminated`, checked on every entry.
    pub termination_flag: bool,
    /// Exported-class finalizers (linear handles only).
    pub finalizers: bool,
    pub results: Results,
    pub throws: Throws,
    /// Protocol name overrides, default name → name.
    pub renames: BTreeMap<String, String>,
}

impl Default for Profile {
    fn default() -> Profile {
        Profile::all_linear()
    }
}

/// A profile text error, at a 1-based entry line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "profile line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for Error {}

const PRESETS: [&str; 3] = ["all-linear", "all-gc", "all-host"];

impl Profile {
    /// Every category linear, memory32: the Rust preset.
    pub fn all_linear() -> Profile {
        Profile {
            vec: Carrier::Linear,
            array: Carrier::Linear,
            str: StrCarrier::Linear(Encoding::Utf8),
            handles: Carrier::Linear,
            memory: Some(MemoryWidth::M32),
            eh: Eh::Modern,
            validation: true,
            strict_null: false,
            box_none: BoxNone::Null,
            termination_flag: false,
            finalizers: true,
            results: Results::Multi,
            throws: Throws::Tag,
            renames: BTreeMap::new(),
        }
    }

    /// Every category GC, with no linear memory.
    pub fn all_gc() -> Profile {
        Profile {
            vec: Carrier::Gc,
            array: Carrier::Gc,
            str: StrCarrier::Gc(Encoding::Wtf16),
            handles: Carrier::Gc,
            memory: None,
            ..Profile::all_linear()
        }
    }

    /// Every category on `externref`, with no linear memory: languages
    /// whose interop has no GC types (Kotlin/Wasm).
    pub fn all_host() -> Profile {
        Profile {
            vec: Carrier::Host,
            array: Carrier::Host,
            str: StrCarrier::Host,
            handles: Carrier::Host,
            memory: None,
            finalizers: false,
            ..Profile::all_linear()
        }
    }

    pub fn preset(name: &str) -> Option<Profile> {
        match name {
            "all-linear" => Some(Profile::all_linear()),
            "all-gc" => Some(Profile::all_gc()),
            "all-host" => Some(Profile::all_host()),
            _ => None,
        }
    }

    /// A protocol name after renames: `name(protocol::TERMINATED)`.
    pub fn name<'a>(&'a self, default: &'a str) -> &'a str {
        self.renames.get(default).map_or(default, String::as_str)
    }

    /// Whether any category is carried in linear memory.
    pub fn uses_linear(&self) -> bool {
        self.vec == Carrier::Linear
            || self.array == Carrier::Linear
            || matches!(self.str, StrCarrier::Linear(_))
            || self.handles == Carrier::Linear
    }

    /// Consistency: linear categories (including linear handles, which are
    /// pointers) need a memory.
    pub fn check(&self) -> Result<(), String> {
        if self.uses_linear() && self.memory.is_none() {
            return Err("a linear category needs `memory = 32` or `memory = 64`".into());
        }
        Ok(())
    }

    pub fn parse(src: &str) -> Result<Profile, Error> {
        let mut entries = Vec::new();
        for (i, line) in src.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("");
            for e in line.split(',') {
                let e = e.trim();
                if !e.is_empty() {
                    entries.push((i + 1, e));
                }
            }
        }
        let mut p = Profile::all_linear();
        for (n, &(line, e)) in entries.iter().enumerate() {
            let err = |message: String| Error { line, message };
            let Some((key, value)) = e.split_once('=') else {
                match Profile::preset(e) {
                    Some(preset) if n == 0 => {
                        p = preset;
                        continue;
                    }
                    Some(_) => return Err(err(format!("the preset `{e}` must come first"))),
                    None => {
                        return Err(err(format!(
                            "expected `key = value` or a preset ({}), found `{e}`",
                            PRESETS.join(", ")
                        )))
                    }
                }
            };
            let (key, value) = (key.trim(), value.trim());
            if let Some(default) = key.strip_prefix("rename.") {
                if default.is_empty() || value.is_empty() {
                    return Err(err("`rename.<name> = <new name>`".into()));
                }
                p.renames.insert(default.into(), value.into());
                continue;
            }
            let bad = || err(format!("`{value}` is not a valid `{key}`"));
            match key {
                "vec" => p.vec = carrier(value).ok_or_else(bad)?,
                "array" => p.array = carrier(value).ok_or_else(bad)?,
                "handles" => p.handles = carrier(value).ok_or_else(bad)?,
                "str" => p.str = str_carrier(value).ok_or_else(bad)?,
                "memory" => {
                    p.memory = match value {
                        "none" => None,
                        "32" => Some(MemoryWidth::M32),
                        "64" => Some(MemoryWidth::M64),
                        _ => return Err(bad()),
                    }
                }
                "eh" => {
                    p.eh = match value {
                        "modern" => Eh::Modern,
                        "legacy" => Eh::Legacy,
                        _ => return Err(bad()),
                    }
                }
                "validation" => p.validation = switch(value).ok_or_else(bad)?,
                "strict-null" => p.strict_null = switch(value).ok_or_else(bad)?,
                "box-none" => {
                    p.box_none = match value {
                        "null" => BoxNone::Null,
                        "unit" => BoxNone::Unit,
                        _ => return Err(bad()),
                    }
                }
                "termination" => p.termination_flag = switch(value).ok_or_else(bad)?,
                "finalizers" => p.finalizers = switch(value).ok_or_else(bad)?,
                "results" => {
                    p.results = match value {
                        "multi" => Results::Multi,
                        "single" => Results::Single,
                        _ => return Err(bad()),
                    }
                }
                "throws" => {
                    p.throws = match value {
                        "tag" => Throws::Tag,
                        "js" => Throws::Js,
                        _ => return Err(bad()),
                    }
                }
                _ => return Err(err(format!("unknown setting `{key}`"))),
            }
        }
        let last = entries.last().map_or(1, |e| e.0);
        p.check().map_err(|message| Error {
            line: last,
            message,
        })?;
        Ok(p)
    }
}

/// Every setting, one per line, in the text form [`Profile::parse`] reads.
impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let carrier = |c: Carrier| match c {
            Carrier::Linear => "linear",
            Carrier::Gc => "gc",
            Carrier::Host => "host",
        };
        let on = |b: bool| if b { "on" } else { "off" };
        writeln!(f, "vec = {}", carrier(self.vec))?;
        writeln!(f, "array = {}", carrier(self.array))?;
        let enc = |e: Encoding| match e {
            Encoding::Utf8 => "utf8",
            Encoding::Wtf16 => "wtf16",
        };
        match self.str {
            StrCarrier::Linear(e) => writeln!(f, "str = linear+{}", enc(e))?,
            StrCarrier::Gc(e) => writeln!(f, "str = gc+{}", enc(e))?,
            StrCarrier::Host => writeln!(f, "str = host")?,
        }
        writeln!(f, "handles = {}", carrier(self.handles))?;
        let memory = match self.memory {
            None => "none",
            Some(MemoryWidth::M32) => "32",
            Some(MemoryWidth::M64) => "64",
        };
        writeln!(f, "memory = {memory}")?;
        let eh = match self.eh {
            Eh::Modern => "modern",
            Eh::Legacy => "legacy",
        };
        writeln!(f, "eh = {eh}")?;
        writeln!(f, "validation = {}", on(self.validation))?;
        writeln!(f, "strict-null = {}", on(self.strict_null))?;
        let box_none = match self.box_none {
            BoxNone::Null => "null",
            BoxNone::Unit => "unit",
        };
        writeln!(f, "box-none = {box_none}")?;
        writeln!(f, "termination = {}", on(self.termination_flag))?;
        writeln!(f, "finalizers = {}", on(self.finalizers))?;
        let results = match self.results {
            Results::Multi => "multi",
            Results::Single => "single",
        };
        writeln!(f, "results = {results}")?;
        let throws = match self.throws {
            Throws::Tag => "tag",
            Throws::Js => "js",
        };
        writeln!(f, "throws = {throws}")?;
        for (from, to) in &self.renames {
            writeln!(f, "rename.{from} = {to}")?;
        }
        Ok(())
    }
}

fn carrier(v: &str) -> Option<Carrier> {
    match v {
        "linear" => Some(Carrier::Linear),
        "gc" => Some(Carrier::Gc),
        "host" => Some(Carrier::Host),
        _ => None,
    }
}

/// `linear`, `gc` (UTF-8), `linear+wtf16`, `gc+utf8`, ..., `host`.
fn str_carrier(v: &str) -> Option<StrCarrier> {
    if v == "host" {
        return Some(StrCarrier::Host);
    }
    // GC strings default to WTF-16, which `wasm:js-string` reads directly.
    let (c, e) = v
        .split_once('+')
        .unwrap_or((v, if v == "gc" { "wtf16" } else { "utf8" }));
    let e = match e {
        "utf8" => Encoding::Utf8,
        "wtf16" => Encoding::Wtf16,
        _ => return None,
    };
    match carrier(c)? {
        Carrier::Linear => Some(StrCarrier::Linear(e)),
        Carrier::Gc => Some(StrCarrier::Gc(e)),
        Carrier::Host => None,
    }
}

fn switch(v: &str) -> Option<bool> {
    match v {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}
