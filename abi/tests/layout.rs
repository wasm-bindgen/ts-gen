//! Wire layouts: the layouts IR.md states, checked directly, and a golden
//! rendering of every IR.md example under each test profile
//! (`tests/layout/<profile>.txt`, rewritten with `BLESS=1`).

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::Path as FsPath;

use ts_gen_abi::ir::*;
use ts_gen_abi::key::{Keyed, Role};
use ts_gen_abi::layout::*;
use ts_gen_abi::profile::Profile;
use ts_gen_abi::resolve::Context;
use ts_gen_abi::text;

fn profile(src: &str) -> Profile {
    Profile::parse(src).unwrap()
}

const PROFILES: &[(&str, &str)] = &[
    ("linear32", "all-linear"),
    ("linear64", "all-linear\nmemory = 64"),
    ("gc", "all-gc"),
    ("gc-host", "all-gc\nstr = host\nmemory = 32"),
    ("host", "all-host"),
    (
        "mixed",
        "all-linear\nmemory = 64\nvec = gc\nstr = gc+wtf16\nhandles = gc",
    ),
];

fn parse(src: &str) -> Ir {
    let src = if src.trim_start().starts_with("ir ") {
        src.to_string()
    } else {
        format!("ir 1;\n{src}")
    };
    text::parse(&src).unwrap_or_else(|e| panic!("{e}\n{src}"))
}

fn member<'a>(keys: &'a [Keyed<'a>], key: &str) -> &'a Keyed<'a> {
    keys.iter()
        .find(|k| k.key == key)
        .unwrap_or_else(|| panic!("no member {key}"))
}

fn sig(ir: &Ir, p: &Profile, key: &str) -> String {
    let l = Layouts::new(ir, p);
    let keys = l.keys.members();
    l.member(member(&keys, key), &[])
        .unwrap()
        .core()
        .to_string()
}

fn ty(src: &str) -> Type {
    let ir = parse(&format!("f(x: {src});"));
    match &ir.items[0] {
        Decl::Member(m) => m.params[0].ty.clone(),
        _ => unreachable!(),
    }
}

fn wire_of(ir: &Ir, p: &Profile, t: &str, flow: Flow) -> String {
    let l = Layouts::new(ir, p);
    let r = l.repr(&ty(t), &Context::default(), flow).unwrap();
    let w: Vec<String> = l.wire(&r).unwrap().iter().map(|v| v.to_string()).collect();
    format!("({})", w.join(", "))
}

const BODY: &str = r#"
class ReadableStream {}
enum Body {
  Text(str);
  Bytes(u8[]);
  Stream(ReadableStream);
}
f(b: Body);
"#;

#[test]
fn body_enum_slots() {
    let ir = parse(BODY);
    let lin = profile("all-linear");
    assert_eq!(sig(&ir, &lin, "f(Body)"), "(i32, i32, i32, externref)");
    let l = Layouts::new(&ir, &lin);
    let r = l
        .repr(&Type::named(["Body"]), &Context::default(), Flow::ToJs)
        .unwrap();
    let Kind::Enum { layout, .. } = &r.kind else {
        panic!()
    };
    let arms: Vec<(&str, i64, Vec<usize>)> = layout
        .arms
        .iter()
        .map(|a| (a.name.as_str(), a.index, a.slots.clone()))
        .collect();
    assert_eq!(
        arms,
        [
            ("Text", 1, vec![0, 1]),
            ("Bytes", 2, vec![0, 1]),
            ("Stream", 3, vec![2]),
        ]
    );
    // GC: slots are nullable so unused ones can be null. A WTF-16 `str`
    // and a `u8[]` are distinct arrays; a UTF-8 `str` shares the `u8[]`'s.
    assert_eq!(
        sig(&ir, &profile("all-gc"), "f(Body)"),
        "(i32, (ref null (array (mut i16))), (ref null (array (mut i8))), externref)"
    );
    assert_eq!(
        sig(&ir, &profile("all-gc\nstr = gc+utf8"), "f(Body)"),
        "(i32, (ref null (array (mut i8))), externref)"
    );
}

#[test]
fn first_fit_never_needs_more_slots_than_positional() {
    let ir = parse(
        r#"
class C {}
enum E {
  A(u32, C);
  B(C);
  D(C, C, f64);
  G(f64, u32);
}
f(e: E);
"#,
    );
    // A: i32 0, extern 1. B: extern 1. D: extern 1, extern 2, f64 3.
    // G: f64 3, i32 0.
    assert_eq!(
        sig(&ir, &profile("all-linear"), "f(E)"),
        "(i32, i32, externref, externref, f64)"
    );
}

#[test]
fn err_and_its_children_share_the_tag_wire() {
    let ir = parse(
        r#"
enum KvError extends err {
  @js(DOMException[name = "NotFoundError"]) NotFound(Str);
}
class Kv {
  get(key: str): str throws KvError;
}
"#,
    );
    for (_, p) in PROFILES {
        let p = profile(p);
        let l = Layouts::new(&ir, &p);
        let cx = Context::default();
        assert_eq!(
            l.tag(&Path::new(["KvError"]), &cx).unwrap(),
            [ValType::I32, ValType::EXTERNREF]
        );
        assert_eq!(
            l.tag(&Path::err(), &cx).unwrap(),
            [ValType::I32, ValType::EXTERNREF]
        );
        let e = l
            .enum_of(&Path::new(["KvError"]), &cx, Flow::ToLang)
            .unwrap();
        let arms: Vec<(&str, i64)> = e.arms.iter().map(|a| (a.name.as_str(), a.index)).collect();
        assert_eq!(arms, [("TypeError", 1), ("Error", -1), ("NotFound", 2)]);
        let keys = l.keys.members();
        let s = l.member(member(&keys, "Kv#get(str)"), &[]).unwrap();
        assert_eq!(s.throws.unwrap().path, "KvError");
    }
}

#[test]
fn combined_bases_keep_their_stated_indices() {
    let ir = parse(
        r#"
enum DateUnit { @js("day") Day; }
enum TimeUnit { @js("hour") Hour = 2; @js("minute") Minute; }
enum Unit extends TimeUnit, DateUnit { @js("week") Week; }
"#,
    );
    let p = profile("all-gc");
    let l = Layouts::new(&ir, &p);
    let e = l
        .enum_of(&Path::new(["Unit"]), &Context::default(), Flow::ToLang)
        .unwrap();
    let arms: Vec<(&str, i64)> = e.arms.iter().map(|a| (a.name.as_str(), a.index)).collect();
    assert_eq!(arms, [("Hour", 2), ("Minute", 3), ("Day", 1), ("Week", 4)]);
}

#[test]
fn nullable_encodings() {
    let ir = parse("enum Method { @js(\"GET\") Get; }\nenum Flags: u32 { Read = 1; }\nclass C {}");
    let lin = profile("all-linear");
    let cases = [
        ("bool?", "(i32)"),
        ("u8?", "(i32)"),
        ("i16?", "(i32)"),
        ("i53?", "(i64)"),
        ("u32?", "(i32, i32)"),
        ("i64?", "(i32, i64)"),
        ("f16?", "(i32, f32)"),
        ("f64?", "(i32, f64)"),
        ("Method?", "(i32)"),
        ("Flags", "(i32)"),
        ("Flags?", "(i32, i32)"),
        ("str?", "(i32, i32)"),
        ("C?", "(externref)"),
        ("(u32, bool)?", "(i32, i32, i32)"),
        ("str??", "(i32, i32, i32)"),
        ("Str??", "(externref)"),
    ];
    for (t, w) in cases {
        assert_eq!(wire_of(&ir, &lin, t, Flow::ToJs), w, "{t}");
    }
    let l = Layouts::new(&ir, &lin);
    let r = l.repr(&ty("u8?"), &Context::default(), Flow::ToJs).unwrap();
    assert_eq!(
        r.kind,
        Kind::NullScalar(Scalar::U8, ScalarNull::Niche(i32::MIN as i64))
    );
    let r = l
        .repr(&ty("bool?"), &Context::default(), Flow::ToJs)
        .unwrap();
    assert_eq!(r.kind, Kind::NullScalar(Scalar::Bool, ScalarNull::Niche(2)));
    let r = l
        .repr(&ty("u53?"), &Context::default(), Flow::ToJs)
        .unwrap();
    assert_eq!(
        r.kind,
        Kind::NullScalar(Scalar::U53, ScalarNull::Niche(i64::MIN))
    );
    let gc = profile("all-gc");
    assert_eq!(
        wire_of(&ir, &gc, "str?", Flow::ToJs),
        "((ref null (array (mut i16))))"
    );
    assert_eq!(
        wire_of(&ir, &gc, "str", Flow::ToJs),
        "((ref (array (mut i16))))"
    );
}

#[test]
fn tuples_flatten_in_direct_positions() {
    let ir = parse("f(p: (u32, (f64, bool)));");
    assert_eq!(
        sig(&ir, &profile("all-linear"), "f((u32,(f64,bool)))"),
        "(i32, f64, i32)"
    );
}

#[test]
fn linear_array_splits_memory_and_table() {
    let ir = parse("class Request {}");
    let lin = profile("all-linear");
    let l = Layouts::new(&ir, &lin);
    let r = l
        .repr(
            &ty("arr<(u32, str, Request)>"),
            &Context::default(),
            Flow::ToJs,
        )
        .unwrap();
    assert_eq!(
        l.wire(&r).unwrap(),
        [ValType::I32, ValType::I32, ValType::I32]
    );
    let Kind::Array { elem, .. } = &r.kind else {
        panic!()
    };
    let e = l.elem(elem).unwrap();
    assert_eq!((e.size, e.align), (12, 4));
    let fields: Vec<(u32, FieldKind)> = e
        .fields
        .iter()
        .map(|f| (f.offset, f.kind.clone()))
        .collect();
    assert_eq!(
        fields,
        [
            (0, FieldKind::Scalar(Scalar::U32)),
            (4, FieldKind::Ptr),
            (8, FieldKind::Len)
        ]
    );
    assert_eq!(e.refs, [ValType::EXTERNREF]);
    assert_eq!(l.alloc(&r).unwrap(), Some(Alloc { size: 12, align: 4 }));

    // Packed, unaligned: a nullable scalar is `(u8 present, T)`.
    let r = l
        .repr(&ty("arr<(u8, f64?)>"), &Context::default(), Flow::ToJs)
        .unwrap();
    let Kind::Array { elem, .. } = &r.kind else {
        panic!()
    };
    let e = l.elem(elem).unwrap();
    let fields: Vec<(u32, FieldKind)> = e
        .fields
        .iter()
        .map(|f| (f.offset, f.kind.clone()))
        .collect();
    assert_eq!(
        fields,
        [
            (0, FieldKind::Scalar(Scalar::U8)),
            (1, FieldKind::Present),
            (2, FieldKind::Scalar(Scalar::F64)),
        ]
    );
    assert_eq!((e.size, e.align), (10, 8));

    // References only: `(base, len)`; memory64 widens `len` but not `base`.
    assert_eq!(wire_of(&ir, &lin, "arr<Request>", Flow::ToJs), "(i32, i32)");
    let m64 = profile("all-linear\nmemory = 64");
    assert_eq!(wire_of(&ir, &m64, "arr<Request>", Flow::ToJs), "(i32, i64)");
    assert_eq!(
        wire_of(&ir, &m64, "arr<(u32, Request)>", Flow::ToJs),
        "(i64, i32, i64)"
    );
    assert_eq!(wire_of(&ir, &m64, "str", Flow::ToJs), "(i64, i64)");
}

#[test]
fn gc_array_elements() {
    let ir = parse("class Request {}");
    let gc = profile("all-gc");
    assert_eq!(
        wire_of(&ir, &gc, "arr<(u32, str)>", Flow::ToJs),
        "((ref (array (mut (ref null (struct (field i32) (field (ref (array (mut i16))))))))))"
    );
    assert_eq!(
        wire_of(&ir, &gc, "arr<u8[]>", Flow::ToJs),
        "((ref (array (mut (ref null (array (mut i8)))))))"
    );
    assert_eq!(
        wire_of(&ir, &gc, "arr<Request>?", Flow::ToJs),
        "((ref null (array (mut externref))))"
    );
    assert_eq!(
        wire_of(&ir, &gc, "f16[]", Flow::ToJs),
        "((ref (array (mut i16))))"
    );
    assert_eq!(
        wire_of(&ir, &gc, "u32[]", Flow::ToJs),
        "((ref (array (mut i32))))"
    );
}

#[test]
fn reference_wire_types() {
    let ir = parse(
        r#"
class Request {}
class Response {}
class Cache {
  put(request: Request, response: Response): future<unit>;
}
class Map<K, V> {
  get(key: K): V?;
  set(key: K, value: V);
}
export class Counter {
  new();
  increment(by: u32): future<u32>;
  get count(): u32;
  static create(): Counter;
}
class EventTarget {
  addEventListener(type: str, listener: fn(Event));
}
class Event {}
"#,
    );
    let lin = profile("all-linear");
    let gc = profile("all-gc");
    for p in [&lin, &gc] {
        assert_eq!(
            sig(&ir, p, "Cache#put(Request,Response)"),
            "(externref, externref, externref) -> (externref)"
        );
    }
    // Erased `T`: `externref` in linear, `anyref` with GC handles.
    assert_eq!(
        sig(&ir, &lin, "Map#set(K,V)"),
        "(externref, externref, externref)"
    );
    assert_eq!(sig(&ir, &gc, "Map#set(K,V)"), "(externref, anyref, anyref)");
    // Exported class instance members take `(this, handle, args…)`.
    assert_eq!(
        sig(&ir, &lin, "Counter#increment(u32)"),
        "(externref, i32, i32) -> (externref)"
    );
    assert_eq!(
        sig(&ir, &gc, "Counter#increment(u32)"),
        "(externref, anyref, i32) -> (externref)"
    );
    assert_eq!(sig(&ir, &lin, "Counter.new()"), "(externref) -> (i32)");
    assert_eq!(sig(&ir, &lin, "Counter.create()"), "() -> (i32)");
    assert_eq!(
        sig(
            &ir,
            &profile("all-linear\nmemory = 64"),
            "Counter#get:count()"
        ),
        "(externref, i64) -> (i32)"
    );
    // A callback the language provides is its handle.
    assert_eq!(
        sig(&ir, &lin, "EventTarget#addEventListener(str,fn(Event))"),
        "(externref, i32, i32, i32)"
    );
    assert_eq!(
        sig(&ir, &gc, "EventTarget#addEventListener(str,fn(Event))"),
        "(externref, (ref (array (mut i16))), anyref)"
    );
}

#[test]
fn host_carriers_are_externref() {
    let ir = parse(
        r#"
class Map<K, V> {
  set(key: K, value: V);
}
export class Counter {
  increment(by: u32): u32;
}
class EventTarget {
  addEventListener(type: str, listener: fn(Event));
}
class Event {}
f(bytes: u8[], xs: arr<(u32, str)>, into: mut f64[]): u8[]?;
"#,
    );
    let host = profile("all-host");
    assert_eq!(
        sig(&ir, &host, "Map#set(K,V)"),
        "(externref, externref, externref)"
    );
    assert_eq!(
        sig(&ir, &host, "Counter#increment(u32)"),
        "(externref, externref, i32) -> (i32)"
    );
    assert_eq!(
        sig(&ir, &host, "EventTarget#addEventListener(str,fn(Event))"),
        "(externref, externref, externref)"
    );
    assert_eq!(
        sig(&ir, &host, "f(u8[],arr<(u32,str)>,mut f64[])"),
        "(externref, externref, externref) -> (externref)"
    );
    // Host sequences inside a linear `arr<T>` are table entries.
    let mixed = profile("all-linear\nvec = host");
    let l = Layouts::new(&ir, &mixed);
    let r = l
        .repr(&ty("arr<u8[]>"), &Context::default(), Flow::ToJs)
        .unwrap();
    let Kind::Array { elem, .. } = &r.kind else {
        panic!()
    };
    let e = l.elem(elem).unwrap();
    assert!(e.fields.is_empty());
    assert_eq!(e.refs, vec![ValType::EXTERNREF]);
}

#[test]
fn mono_variants_substitute_their_arguments() {
    let ir = parse(
        r#"
class Map<K, V> {
  get(key: K): V?;
}
class Array<T> {
  map<U>(f: fn(T) -> U): arr<U>;
}
"#,
    );
    let lin = profile("all-linear");
    let l = Layouts::new(&ir, &lin);
    let keys = l.keys.members();
    let s = l
        .member(
            member(&keys, "Map#get(K)"),
            &[Type::Str, Type::Scalar(Scalar::F64)],
        )
        .unwrap();
    assert_eq!(s.core().to_string(), "(externref, i32, i32) -> (i32, f64)");
    assert_eq!(s.results[0].repr.ty, "f64?");
    let s = l
        .member(
            member(&keys, "Array#map(fn(T)->U)"),
            &[Type::Scalar(Scalar::F64), Type::StrBox],
        )
        .unwrap();
    assert_eq!(s.params[1].repr.ty, "fn(f64)->Str");
    assert_eq!(s.results[0].repr.ty, "arr<Str>");
    assert_eq!(s.core().to_string(), "(externref, i32) -> (i32, i32)");
    let Kind::Fn { provided, .. } = &s.params[1].repr.kind else {
        panic!()
    };
    assert!(provided.is_some());
    let t = l.fn_op(&s.params[1].repr).unwrap();
    assert_eq!(t.core().to_string(), "(i32, f64) -> (externref)");
    assert!(l.member(member(&keys, "Map#get(K)"), &[Type::Str]).is_err());
}

#[test]
fn associated_types_take_their_row() {
    let ir = parse(
        r#"
enum Unit { @js("day") Day; @js("hour") Hour; }
enum DateUnit { @js("day") Day; }
class Plural { new(); }
interface Options<U in (Unit, DateUnit)> {
  type P = match U { Unit => str, DateUnit => Plural };
  get p(): P;
}
"#,
    );
    let lin = profile("all-linear");
    let l = Layouts::new(&ir, &lin);
    let keys = l.keys.members();
    let k = member(&keys, "Options#get:p()");
    // Erased, the first row: the bound's union.
    assert_eq!(l.member(k, &[]).unwrap().results[0].repr.ty, "str");
    let date = Type::Named(TypeRef {
        path: Path::new(["DateUnit"]),
        args: Vec::new(),
    });
    let s = l.member(k, &[date]).unwrap();
    assert_eq!(s.results[0].repr.ty, "Plural");
}

#[test]
fn kotlin_erased_example() {
    let ir = parse("class Map<K, V> { set(key: K, value: V); get(key: K): V?; }");
    let gc = profile("all-gc");
    let l = Layouts::new(&ir, &gc);
    let cx = Context::default();
    assert_eq!(
        l.box_(&ty("(u32, bool)"), &cx).unwrap().core().to_string(),
        "(i32, i32) -> (externref)"
    );
    assert_eq!(
        l.unbox(&ty("(u32, bool)"), &cx).unwrap().core().to_string(),
        "(externref) -> (i32, i32)"
    );
    assert_eq!(sig(&ir, &gc, "Map#set(K,V)"), "(externref, anyref, anyref)");
    assert_eq!(
        sig(&ir, &gc, "Map#get(K)"),
        "(externref, anyref) -> (anyref)"
    );
}

#[test]
fn end_to_end_example() {
    let ir = parse(
        r#"
enum FetchError extends err {
  @js(DOMException[name = "AbortError"]) Aborted(Str);
}
class Request {
  get url(): str;
}
class Response {
  new(body: str);
}
interface RequestInit {}
class Env {}
class ExecutionContext {}
@js(
  let c = AbortController.new(),
  args = (input, init? { signal = c.signal }),
  cancel = c.abort,
)
fetch(input: Request, init: RequestInit?): future<Response, FetchError>;
export namespace default {
  fetch(request: Request, env: Env, ctx: ExecutionContext): future<Response>;
}
"#,
    );
    let lin = profile("all-linear");
    let gc = profile("all-gc");
    assert_eq!(
        sig(&ir, &lin, "Request#get:url()"),
        "(externref) -> (i32, i32)"
    );
    assert_eq!(
        sig(&ir, &gc, "Request#get:url()"),
        "(externref) -> ((ref (array (mut i16))))"
    );
    assert_eq!(
        sig(&ir, &lin, "Response.new(str)"),
        "(i32, i32) -> (externref)"
    );
    assert_eq!(
        sig(&ir, &gc, "Response.new(str)"),
        "((ref (array (mut i16)))) -> (externref)"
    );
    for p in [&lin, &gc] {
        assert_eq!(
            sig(&ir, p, "fetch(Request,RequestInit?)"),
            "(externref, externref) -> (externref)"
        );
        assert_eq!(
            sig(&ir, p, "default.fetch(Request,Env,ExecutionContext)"),
            "(externref, externref, externref) -> (externref)"
        );
        let l = Layouts::new(&ir, p);
        let cx = Context::default();
        assert_eq!(
            l.future_take(&Type::named(["Response"]), &cx)
                .unwrap()
                .core()
                .to_string(),
            "(externref) -> (externref)"
        );
        let e = l.future_error(&Path::new(["FetchError"]), &cx).unwrap();
        assert_eq!(e.core().to_string(), "(externref) -> (i32, externref)");
        let Kind::Enum { layout, .. } = &e.results[0].repr.kind else {
            panic!()
        };
        let arms: Vec<(&str, i64)> = layout
            .arms
            .iter()
            .map(|a| (a.name.as_str(), a.index))
            .collect();
        assert_eq!(arms, [("TypeError", 1), ("Error", -1), ("Aborted", 2)]);
    }
}

#[test]
fn protocol_operations() {
    let ir = parse("class C {}");
    let lin = profile("all-linear");
    let m64 = profile("all-linear\nmemory = 64");
    let gc = profile("all-gc");
    let op = |p: &Profile, m: Option<&str>, n: &str| {
        Layouts::new(&ir, p)
            .fixed_op(m, n)
            .map(|f| f.to_string())
            .unwrap_or_else(|| "none".into())
    };
    assert_eq!(
        op(&lin, None, "tw:realloc"),
        "(i32, i32, i32, i32) -> (i32)"
    );
    assert_eq!(
        op(&m64, None, "tw:realloc"),
        "(i64, i64, i64, i64) -> (i64)"
    );
    assert_eq!(op(&m64, None, "tw:free"), "(i64, i64, i64)");
    assert_eq!(op(&gc, None, "tw:realloc"), "none");
    assert_eq!(op(&m64, None, "tw:table/alloc"), "(i32) -> (i32)");
    assert_eq!(op(&lin, None, "tw:loop/tick"), "(i32, i32) -> (i32)");
    assert_eq!(op(&lin, None, "tw:loop/next_sleep"), "(i32) -> (i64)");
    assert_eq!(
        op(&lin, Some("tw:future"), "poll"),
        "(externref, i32) -> (i32)"
    );
    assert_eq!(
        op(&lin, Some("tw:future"), "new"),
        "() -> (externref, externref)"
    );
    assert_eq!(op(&lin, Some("tw:instanceof"), "C"), "(externref) -> (i32)");
    assert_eq!(op(&lin, Some("tw:arr"), "len"), "(externref) -> (i32)");
    let l = Layouts::new(&ir, &lin);
    assert_eq!(
        l.terminated(),
        Global {
            ty: ValType::I32,
            mutable: false
        }
    );
    assert_eq!(
        Layouts::new(&ir, &m64).terminated(),
        Global {
            ty: ValType::I64,
            mutable: false
        }
    );
    assert_eq!(
        Layouts::new(&ir, &gc).terminated(),
        Global {
            ty: ValType::I32,
            mutable: true
        }
    );
    assert_eq!(l.handle_op().unwrap().to_string(), "(i32)");
    assert_eq!(
        Layouts::new(&ir, &gc).handle_op().unwrap().to_string(),
        "(anyref)"
    );
    assert_eq!(
        l.arr_get(&ty("str"), &Context::default())
            .unwrap()
            .core()
            .to_string(),
        "(externref, i32) -> (i32, i32)"
    );
    assert_eq!(
        l.future_resolve(&Type::Unit, &Context::default())
            .unwrap()
            .core()
            .to_string(),
        "(externref)"
    );
}

#[test]
fn invalid_layouts() {
    let ir = parse("enum Rec { A(arr<Rec>); }\nenum Bad { A = 0; }");
    let lin = profile("all-linear");
    let gc = profile("all-gc");
    let l = Layouts::new(&ir, &lin);
    let cx = Context::default();
    let err = |l: &Layouts, t: &str| l.repr(&ty(t), &cx, Flow::ToJs).unwrap_err().to_string();
    assert!(err(&l, "Rec").contains("recursive"));
    assert!(err(&l, "Bad").contains("isn't from 1"));
    assert!(err(&l, "bool[]").contains("typed-array"));
    assert!(err(&l, "Missing").contains("names nothing"));
    let g = Layouts::new(&ir, &gc);
    assert!(err(&g, "*u8[]").contains("linear memory"));
    assert!(l
        .elem(&l.repr(&ty("*u8[]"), &cx, Flow::ToJs).unwrap())
        .is_err());
}

// The golden rendering.

struct Example {
    /// The enclosing IR.md heading, which keeps the goldens stable when the
    /// spec's lines move.
    at: String,
    src: String,
}

fn examples() -> Vec<Example> {
    let spec =
        std::fs::read_to_string(FsPath::new(env!("CARGO_MANIFEST_DIR")).join("../IR.md")).unwrap();
    let mut out = Vec::new();
    let mut open: Option<Example> = None;
    let mut heading = String::new();
    for line in spec.lines() {
        let fence = line.trim_start().strip_prefix("```").map(str::trim);
        match (&mut open, fence) {
            (None, Some(info)) => {
                let mut words = info.split_whitespace();
                if words.next() == Some("ts") && !words.any(|w| w == "invalid") {
                    open = Some(Example {
                        at: heading.clone(),
                        src: String::new(),
                    });
                }
            }
            (None, None) if line.starts_with("## ") || line.starts_with("### ") => {
                heading = line.trim_start_matches('#').trim().to_string();
            }
            (Some(_), Some(_)) => out.push(open.take().unwrap()),
            (Some(e), None) => {
                e.src.push_str(line);
                e.src.push('\n');
            }
            (None, None) => {}
        }
    }
    out
}

/// Declares every class an example uses but leaves out, so the rest of it
/// lays out. A name used as an error type is declared as an enum.
fn complete(src: &str, p: &Profile) -> Ir {
    let mut added: Vec<(String, bool)> = Vec::new();
    loop {
        let mut full = src.to_string();
        for (name, is_enum) in &added {
            if *is_enum {
                full.push_str(&format!("enum {name} extends err {{}}\n"));
            } else {
                full.push_str(&format!("class {name} {{}}\n"));
            }
        }
        let ir = parse(&full);
        let l = Layouts::new(&ir, p);
        let mut errors: Vec<Error> = l
            .keys
            .members()
            .iter()
            .filter_map(|k| l.member(k, &[]).err())
            .collect();
        let mut declared = Vec::new();
        enums(&ir.items, &Context::default(), &mut declared);
        for (name, cx) in declared {
            if let Err(e) = l.repr(&Type::named([name]), &cx, Flow::ToLang) {
                errors.push(e);
            }
        }
        let mut changed = false;
        for e in errors {
            match e {
                Error::Unknown(name) if text_is_ident(&name) => {
                    added.push((name, false));
                    changed = true;
                    break;
                }
                Error::Invalid(m) => {
                    if let Some(a) = added
                        .iter_mut()
                        .find(|(n, is_enum)| !is_enum && m == format!("`{n}` isn't an enum"))
                    {
                        a.1 = true;
                        changed = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !changed {
            return ir;
        }
    }
}

/// Every declared enum, with the context it's declared in.
fn enums<'a>(items: &'a [Decl], cx: &Context<'a>, out: &mut Vec<(&'a str, Context<'a>)>) {
    for d in items {
        match d {
            Decl::Enum(e) => out.push((&e.name, cx.clone())),
            Decl::Namespace(n) => {
                let mut inner = cx.clone();
                inner.namespace.push(&n.name);
                enums(&n.items, &inner, out);
            }
            Decl::Module(m) => {
                let inner = Context {
                    module: Some(&m.spec),
                    ..Context::default()
                };
                enums(&m.items, &inner, out);
            }
            _ => {}
        }
    }
}

fn text_is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[derive(Default)]
struct Seen {
    out: String,
    arrays: BTreeSet<String>,
    enums: BTreeSet<String>,
    fns: BTreeSet<String>,
    futures: BTreeSet<String>,
}

fn walk(l: &Layouts, r: &Repr, seen: &mut Seen) {
    match &r.kind {
        Kind::Array { elem, carrier, .. } => {
            walk(l, elem, seen);
            if seen.arrays.insert(r.ty.clone()) {
                let line = match carrier {
                    ts_gen_abi::profile::Carrier::Linear => {
                        let e = l.elem(elem).unwrap();
                        let fields: Vec<String> = e
                            .fields
                            .iter()
                            .map(|f| format!("{}@{}", field(&f.kind), f.offset))
                            .collect();
                        let refs: Vec<String> = e.refs.iter().map(ToString::to_string).collect();
                        format!(
                            "memory {} align {} [{}] table [{}]",
                            e.size,
                            e.align,
                            fields.join(" "),
                            refs.join(" ")
                        )
                    }
                    ts_gen_abi::profile::Carrier::Gc => {
                        format!("gc {}", l.gc_elem(elem).unwrap())
                    }
                    ts_gen_abi::profile::Carrier::Host => "host externref".into(),
                };
                let _ = writeln!(seen.out, "  elem {}: {line}", r.ty);
            }
        }
        Kind::Tuple { elems, .. } => elems.iter().for_each(|e| walk(l, e, seen)),
        Kind::Enum { layout, .. } => {
            for a in &layout.arms {
                a.payload.iter().for_each(|p| walk(l, p, seen));
            }
            if seen.enums.insert(layout.path.clone()) {
                let shape = match &layout.shape {
                    Shape::Index => "index".to_string(),
                    Shape::Carrier(c) => format!("carrier {}", c.name()),
                    Shape::Payload { slots } => {
                        let s: Vec<String> = slots.iter().map(ToString::to_string).collect();
                        format!("slots ({})", s.join(", "))
                    }
                };
                let arms: Vec<String> = layout
                    .arms
                    .iter()
                    .map(|a| {
                        if a.slots.is_empty() {
                            format!("{}={}", a.name, a.index)
                        } else {
                            let s: Vec<String> = a.slots.iter().map(ToString::to_string).collect();
                            format!("{}={}[{}]", a.name, a.index, s.join(","))
                        }
                    })
                    .collect();
                let _ = writeln!(
                    seen.out,
                    "  enum {}: {shape}; {}",
                    layout.path,
                    arms.join(" ")
                );
            }
        }
        Kind::Host { of, .. } => match of {
            Host::Box(inner) => walk(l, inner, seen),
            Host::Future { value, error } => {
                walk(l, value, seen);
                let e = Repr {
                    ty: error.path.clone(),
                    kind: Kind::Enum {
                        layout: error.clone(),
                        nullable: false,
                    },
                    js_null: false,
                };
                walk(l, &e, seen);
                if seen.futures.insert(r.ty.clone()) {
                    let _ = writeln!(
                        seen.out,
                        "  future {}: take {} error {}",
                        r.ty,
                        fmt_wire(&l.wire(value).unwrap()),
                        fmt_wire(&l.wire(&e).unwrap())
                    );
                }
            }
            _ => {}
        },
        Kind::Fn { sig, provided, .. } => {
            sig.params.iter().for_each(|p| walk(l, p, seen));
            walk(l, &sig.result, seen);
            let op = if provided.is_some() {
                "trampoline"
            } else {
                "call"
            };
            if seen.fns.insert(format!("{op} {}", r.ty)) {
                let s = l.fn_op(r).unwrap();
                let _ = writeln!(seen.out, "  {op} {}: {}", r.ty, s.core());
            }
        }
        _ => {}
    }
}

fn fmt_wire(w: &[ValType]) -> String {
    let w: Vec<String> = w.iter().map(ToString::to_string).collect();
    format!("({})", w.join(", "))
}

fn field(k: &FieldKind) -> String {
    match k {
        FieldKind::Scalar(s) => s.name().into(),
        FieldKind::Present => "present".into(),
        FieldKind::Arm => "arm".into(),
        FieldKind::Slot(v) => format!("slot:{v}"),
        FieldKind::Ptr => "ptr".into(),
        FieldKind::Len => "len".into(),
        FieldKind::Base => "base".into(),
        FieldKind::Handle => "handle".into(),
    }
}

/// What the spec examples leave out: every type form, in both flows.
const FIXTURE: &str = r#"
class Event {}
class Request {}
export class Counter {
  new();
}
enum Method {
  @js("GET") Get;
  @js("POST") Post;
  Latest = .Post;
}
enum Flags: u8 {
  Read = 1;
  Write = 2;
}
enum Body {
  Text(str);
  Bytes(u8[]);
  Stream(Request);
}
enum Value {
  Num(f64);
  Flag(bool);
  Pair(u32, Request);
  @fallback Other(Any);
}
class Fixture<T> {
  scalars(a: bool, b: i8, c: u16, d: i53, e: u64, f: f16, g: f64): u32;
  nullables(a: bool?, b: u8?, c: i53?, d: u32?, e: f32?, f: Method?, g: Flags?, h: Body?): str?;
  sequences(a: u8[], b: mut f32[], c: i64[]?, d: arr<u8>, e: arr<str>?, f: arr<Request>): arr<(u32, str)>;
  nested(a: arr<arr<(u32, str)>>, b: arr<Body>, c: arr<(Flags?, Method, f64?)>, d: arr<Counter>, e: arr<fn(Event)>, f: arr<Value>);
  boxes(a: Str, b: U32, c: Vec<u8>, d: Arr<str>, e: Tuple<str, u16>, f: Any?): Arr<Request>;
  callbacks(a: fn(Event, u32) -> str, b: fn(str?) -> Body throws err): fn(Request) -> u32;
  handles(a: Counter, b: Counter?): arr<Counter>;
  erased(a: T, b: T?, c: arr<T>): fn(T) -> T;
  undef(a: str??): Request??;
  view(a: *u8[], b: *mut f64[]);
}
export takeCallback(cb: fn(Event)): fn(u32) -> str;
"#;

fn render(p: &Profile) -> String {
    let mut out = String::new();
    let mut all = examples();
    all.push(Example {
        at: "fixture".into(),
        src: FIXTURE.into(),
    });
    for e in all {
        let ir = complete(&e.src, p);
        let l = Layouts::new(&ir, p);
        let _ = writeln!(out, "## {}", e.at);
        let mut seen = Seen::default();
        for k in l.keys.members() {
            let role = match &k.role {
                Role::Import(m) => format!("import {m}"),
                Role::Export => "export".into(),
            };
            match l.member(&k, &[]) {
                Ok(s) => {
                    let throws = s
                        .throws
                        .as_ref()
                        .map(|t| format!(" throws {}", t.path))
                        .unwrap_or_default();
                    let _ = writeln!(seen.out, "{role} {}: {}{throws}", k.key, s.core());
                    for pos in s.params.iter().chain(&s.results) {
                        walk(&l, &pos.repr, &mut seen);
                    }
                    if let Some(t) = &s.throws {
                        let e = Repr {
                            ty: t.path.clone(),
                            kind: Kind::Enum {
                                layout: t.clone(),
                                nullable: false,
                            },
                            js_null: false,
                        };
                        walk(&l, &e, &mut seen);
                    }
                }
                Err(err) => {
                    let _ = writeln!(seen.out, "{role} {}: error: {err}", k.key);
                }
            }
        }
        let mut declared = Vec::new();
        enums(&ir.items, &Context::default(), &mut declared);
        for (name, cx) in declared {
            match l.repr(&Type::named([name]), &cx, Flow::ToLang) {
                Ok(r) => {
                    let _ = writeln!(
                        seen.out,
                        "enum {}: {}",
                        r.ty,
                        fmt_wire(&l.wire(&r).unwrap())
                    );
                    walk(&l, &r, &mut seen);
                }
                Err(err) => {
                    let _ = writeln!(seen.out, "enum {name}: error: {err}");
                }
            }
        }
        out.push_str(&seen.out);
        out.push('\n');
    }
    out
}

#[test]
fn golden_spec_layouts() {
    let dir = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("tests/layout");
    let bless = std::env::var_os("BLESS").is_some();
    let mut failures = Vec::new();
    for (name, src) in PROFILES {
        let p = profile(src);
        let mut got = format!("# profile: {}\n\n", src.replace('\n', ", "));
        got.push_str(&render(&p));
        let path = dir.join(format!("{name}.txt"));
        if bless {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_default();
        if want != got {
            failures.push(format!(
                "{} differs (rerun with BLESS=1 to update):\n{got}",
                path.display()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn unboxing_a_handle_or_function_recovers_the_handle() {
    let ir = parse("export class Counter { new(); }");
    let cx = Context::default();
    for (p, h) in [("all-linear", "i32"), ("all-gc", "anyref")] {
        let prof = profile(p);
        let l = Layouts::new(&ir, &prof);
        for t in ["Counter", "fn(u32)->str", "fn(u32)?"] {
            assert_eq!(
                l.unbox(&ty(t), &cx).unwrap().core().to_string(),
                format!("(externref) -> ({h})"),
                "{p} {t}"
            );
            assert_eq!(
                l.box_(&ty(t), &cx).unwrap().core().to_string(),
                format!("({h}) -> (externref)"),
                "{p} {t}"
            );
        }
    }
}
