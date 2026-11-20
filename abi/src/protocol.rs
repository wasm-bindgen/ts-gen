//! The `tw` protocol names (IR.md §19).
//!
//! Imports the bridge provides are `(module, name)` pairs: the module is
//! `tw:<group>`, and the name is the member, or for typed operations the
//! subject type or path in canonical spelling ([`crate::key`]). Exports the
//! language provides are single names: typed operations join the op and
//! the subject with `:` (`tw:drop:Counter`), grouped ones join with `/`
//! (`tw:loop/tick`).
//!
//! These are the defaults. A [`crate::profile::Profile`] can rename any of
//! them ([`crate::profile::Profile::name`]).

/// The import module field of ambient globals (§5).
pub const GLOBAL: &str = "tw:js/global";

/// User keys never start with this, so they can't collide with protocol
/// names.
pub const PREFIX: &str = "tw:";

pub const INSTANCEOF: &str = "tw:instanceof";
pub const DESTROY: &str = "tw:destroy";
pub const CALL: &str = "tw:call";
pub const BOX: &str = "tw:box";
pub const UNBOX: &str = "tw:unbox";
pub const UNBOX_UNCHECKED: &str = "tw:unboxUnchecked";
pub const TRY_UNBOX: &str = "tw:tryUnbox";
pub const NEW: &str = "tw:new";
pub const ARR: &str = "tw:arr";
/// `(externref) -> i32` presence tests of a box, with an empty name: a `T?`
/// box is none on either empty, and a `T??` box's outer none is `undefined`
/// (§13).
pub const IS_NULLISH: &str = "tw:isNullish";
pub const IS_UNDEFINED: &str = "tw:isUndefined";
/// `() -> externref` constants, with an empty name: the unit box (a box's
/// none) and the `Bool` boxes. The table's constant range holds the same
/// values (§13).
pub const UNIT: &str = "tw:unit";
pub const TRUE: &str = "tw:true";
pub const FALSE: &str = "tw:false";
pub const TAG: &str = "tw:tag";
pub const FUTURE: &str = "tw:future";
pub const LOOP: &str = "tw:loop";
/// `results = single` (§12): spilled result slots, popped by value type
/// (`("tw:ret", "i32")`) or pushed (`("tw:ret", "push:i32")`).
pub const RET: &str = "tw:ret";
/// `throws = js` (§15): `("tw:err", E)` reads a caught JS exception as `E`'s
/// arm and slots; `("tw:err", "set:E")` sets the error an export returns.
pub const ERR: &str = "tw:err";

pub const REALLOC: &str = "tw:realloc";
pub const FREE: &str = "tw:free";
pub const TABLE_ALLOC: &str = "tw:table/alloc";
pub const TABLE_FREE: &str = "tw:table/free";
pub const MEMORY: &str = "memory";
pub const TABLE: &str = "tw:table";
/// The table's index ranges, `0..stack_base..const_base..heap_base..`, as
/// immutable `i32` globals exported beside the table by whichever side
/// defines it (§14).
pub const TABLE_STACK_BASE: &str = "tw:table/stack_base";
pub const TABLE_CONST_BASE: &str = "tw:table/const_base";
pub const TABLE_HEAP_BASE: &str = "tw:table/heap_base";
pub const TERMINATED: &str = "tw:terminated";
pub const LOOP_TICK: &str = "tw:loop/tick";
pub const LOOP_NEXT_SLEEP: &str = "tw:loop/next_sleep";
pub const FUTURE_WAKE: &str = "tw:future/wake";

/// The bases a bridge-defined table uses.
pub const DEFAULT_STACK_BASE: u32 = 1;
pub const DEFAULT_CONST_BASE: u32 = 128;
pub const DEFAULT_HEAP_BASE: u32 = 132;

/// The constants the bridge stores from `const_base`, in slot order.
pub const TABLE_CONSTANTS: [&str; 4] = ["undefined", "null", "true", "false"];

/// The budget the bridge passes to `tw:loop/tick` (§16).
pub const TICK_BUDGET: u32 = 64;

/// A bridge import.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Import {
    pub module: String,
    pub name: String,
}

impl Import {
    pub fn new(module: &str, name: impl Into<String>) -> Import {
        Import {
            module: module.into(),
            name: name.into(),
        }
    }
}

/// `("tw:instanceof", path)`: `(externref) -> i32`, classes only.
pub fn instanceof(class: &str) -> Import {
    Import::new(INSTANCEOF, class)
}

/// `("tw:destroy", F)` or `("tw:destroy", C)`: `(handle)`.
pub fn destroy(subject: &str) -> Import {
    Import::new(DESTROY, subject)
}

/// `("tw:call", F)`: call a JS function.
pub fn call(f: &str) -> Import {
    Import::new(CALL, f)
}

pub fn box_(t: &str) -> Import {
    Import::new(BOX, t)
}

pub fn unbox(t: &str) -> Import {
    Import::new(UNBOX, t)
}

/// Constructs an exported class through JS, named by its constructor's key.
pub fn new(ctor: &str) -> Import {
    Import::new(NEW, ctor)
}

pub fn try_unbox(t: &str) -> Import {
    Import::new(TRY_UNBOX, t)
}

pub fn unbox_unchecked(t: &str) -> Import {
    Import::new(UNBOX_UNCHECKED, t)
}

pub fn arr_len() -> Import {
    Import::new(ARR, "len")
}

pub fn arr_get(t: &str) -> Import {
    Import::new(ARR, format!("get<{t}>"))
}

/// The tag of an enum used in `throws`; `err` for the built-in.
pub fn tag(e: &str) -> Import {
    Import::new(TAG, e)
}

/// The spill value types, by their `tw:ret` names.
pub const SPILL_TYPES: [&str; 5] = ["i32", "i64", "f32", "f64", "externref"];

pub const RET_PUSH: &str = "push:";
pub const ERR_SET: &str = "set:";

/// `("tw:ret", vt)`: pops the next spilled slot of an import's results.
pub fn ret_pop(vt: &str) -> Import {
    Import::new(RET, vt)
}

/// `("tw:ret", "push:vt")`: pushes the next slot of an export's results.
pub fn ret_push(vt: &str) -> Import {
    Import::new(RET, format!("{RET_PUSH}{vt}"))
}

/// `("tw:err", E)`: `(externref exn) -> i32 arm`, with the arm's slots
/// after it.
pub fn err_read(e: &str) -> Import {
    Import::new(ERR, e)
}

/// `("tw:err", "set:E")`: `(i32 arm, slots...)`.
pub fn err_set(e: &str) -> Import {
    Import::new(ERR, format!("{ERR_SET}{e}"))
}

pub fn future_poll() -> Import {
    Import::new(FUTURE, "poll")
}

pub fn future_take(t: &str) -> Import {
    Import::new(FUTURE, format!("take<{t}>"))
}

pub fn future_error(e: &str) -> Import {
    Import::new(FUTURE, format!("error<{e}>"))
}

pub fn future_drop() -> Import {
    Import::new(FUTURE, "drop")
}

pub fn future_new() -> Import {
    Import::new(FUTURE, "new")
}

pub fn future_resolve(t: &str) -> Import {
    Import::new(FUTURE, format!("resolve<{t}>"))
}

pub fn future_reject(e: &str) -> Import {
    Import::new(FUTURE, format!("reject<{e}>"))
}

pub fn loop_work_available() -> Import {
    Import::new(LOOP, "work_available")
}

pub fn loop_drop() -> Import {
    Import::new(LOOP, "drop")
}

/// `tw:call:F`: the trampoline of a callback type the language provides.
pub fn call_export(f: &str) -> String {
    format!("{CALL}:{f}")
}

/// `tw:drop:F` / `tw:drop:C`: linear-profile finalizers.
pub fn drop_export(subject: &str) -> String {
    format!("tw:drop:{subject}")
}

/// `("tw:future", "poll")` results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Poll {
    Pending = 0,
    Resolved = 1,
    Rejected = 2,
}

/// `tw:terminated` states.
pub const LIVE: i32 = 0;
pub const TERMINATED_STATE: i32 = 1;
