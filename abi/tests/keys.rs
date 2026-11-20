//! Golden keys and protocol names, from IR.md §3, §5, §6, §13, §17–§20.

use ts_gen_abi::ir::*;
use ts_gen_abi::key::{import_module, Keys, Role};
use ts_gen_abi::protocol::{self, Import};
use ts_gen_abi::resolve::Context;
use ts_gen_abi::text;

const SRC: &str = r#"ir 1;
class Request {
  get url(): str;
}
class Response extends Body {
  new(body: str);
  get status(): u16;
}
class Headers {
  new();
  get(name: str): str?;
  set(name: str, value: str);
  static from(init: arr<(str, str)>): Headers;
}
class Map<K, V> {
  get(key: K): V?;
  set(key: K, value: V);
}
class Array<T> {
  map<U>(f: fn(T) -> U): arr<U>;
}
class Cache {
  put(request: Request, response: Response): future<unit>;
}
class KVNamespace {
  @js(args = (key, "text"))
  getText(key: str): future<str?>;
}
interface RequestInit {
  @js(object) new();
  set signal(value: AbortSignal);
  @js(args = (null)) set signalWithNull();
}
class Buffer {
  static concat(list: Buffer): Buffer;
}
foo();
namespace foo {
  baz();
  get value(): u53;
}
get version(): str;
fetch(input: Request, init: RequestInit?): future<Response, FetchError>;
module "cloudflare:workers" {
  class DurableObject {
    new(ctx: DurableObjectState, env: Env);
    get ctx(): DurableObjectState;
  }
  class Hidden {
    static make(): Hidden;
    new();
    read(): u8;
  }
  export { DurableObject, DurableObject as default };
}
module "node:buffer" {
  class Buffer {
    static concat(list: Buffer): Buffer;
    static global(list: "tw:none".Missing): Buffer;
  }
  export { Buffer };
}
class Uses {
  f(a: "node:buffer".Buffer, b: Buffer, c: "cloudflare:workers".DurableObject);
}
namespace outer {
  class Inner {}
  namespace deeper {
    g(a: Inner, b: outer.Inner, c: Request);
  }
}
export namespace default {
  fetch(request: Request, env: Env, ctx: ExecutionContext): future<Response>;
}
export version(): str;
export class Counter extends DurableObject {
  new(ctx: DurableObjectState, env: Env);
  increment(by: u32): future<u32>;
  get count(): u32;
  static create(): Counter;
}
"#;

fn ir() -> Ir {
    text::parse(SRC).unwrap()
}

#[test]
fn member_keys() {
    let ir = ir();
    let keys = Keys::new(&ir);
    let got: Vec<(String, String)> = keys
        .members()
        .into_iter()
        .map(|k| {
            let role = match k.role {
                Role::Import(m) => m,
                Role::Export => "export".into(),
            };
            (role, k.key)
        })
        .collect();
    let g = protocol::GLOBAL;
    let cf = "cloudflare:workers";
    let nb = "node:buffer";
    let want: Vec<(&str, &str)> = vec![
        (g, "Request#get:url()"),
        (g, "Response.new(str)"),
        (g, "Response#get:status()"),
        (g, "Headers.new()"),
        (g, "Headers#get(str)"),
        (g, "Headers#set(str,str)"),
        (g, "Headers.from(arr<(str,str)>)"),
        (g, "Map#get(K)"),
        (g, "Map#set(K,V)"),
        (g, "Array#map(fn(T)->U)"),
        (g, "Cache#put(Request,Response)"),
        (g, "KVNamespace#getText(str)"),
        (g, "RequestInit.new()"),
        (g, "RequestInit#set:signal(AbortSignal)"),
        (g, "RequestInit#set:signalWithNull()"),
        (g, "Buffer.concat(Buffer)"),
        (g, "foo()"),
        (g, "foo.baz()"),
        (g, "foo.get:value()"),
        (g, "get:version()"),
        (g, "fetch(Request,RequestInit?)"),
        (cf, "DurableObject.new(DurableObjectState,Env)"),
        (cf, "DurableObject#get:ctx()"),
        (cf, "Hidden#read()"),
        (nb, "Buffer.concat(\"node:buffer\".Buffer)"),
        (nb, "Buffer.global(\"tw:none\".Missing)"),
        (
            g,
            "Uses#f(\"node:buffer\".Buffer,Buffer,\"cloudflare:workers\".DurableObject)",
        ),
        (g, "outer.deeper.g(outer.Inner,outer.Inner,Request)"),
        ("export", "default.fetch(Request,Env,ExecutionContext)"),
        ("export", "version()"),
        ("export", "Counter.new(DurableObjectState,Env)"),
        ("export", "Counter#increment(u32)"),
        ("export", "Counter#get:count()"),
        ("export", "Counter.create()"),
    ];
    let want: Vec<(String, String)> = want
        .into_iter()
        .map(|(a, b)| (a.into(), b.into()))
        .collect();
    assert_eq!(got, want);
}

fn class<'a>(ir: &'a Ir, name: &str) -> &'a Class {
    ir.items
        .iter()
        .find_map(|d| match d {
            Decl::Class(c) if c.name == name => Some(c),
            _ => None,
        })
        .unwrap()
}

#[test]
fn mono_keys_put_class_arguments_first() {
    let ir = ir();
    let keys = Keys::new(&ir);
    let map = class(&ir, "Map");
    let cx = Context::default().with_tparams(&map.tparams);
    assert_eq!(
        keys.member(
            &["Map"],
            true,
            &map.members[0],
            &[Type::StrBox, Type::Scalar(Scalar::F64)],
            &cx
        ),
        "Map#get<Str,f64>(K)"
    );
    let array = class(&ir, "Array");
    let m = &array.members[0];
    let cx = Context::default()
        .with_tparams(&array.tparams)
        .with_tparams(&m.tparams);
    assert_eq!(
        keys.member(
            &["Array"],
            true,
            m,
            &[Type::Scalar(Scalar::F64), Type::StrBox],
            &cx
        ),
        "Array#map<f64,Str>(fn(T)->U)"
    );
}

fn canon(src: &str) -> String {
    let ir = text::parse(&format!("ir 1;\nf(a: {src});\n{}", "class Buffer {}")).unwrap();
    let Decl::Member(m) = &ir.items[0] else {
        panic!()
    };
    Keys::new(&ir).ty(&m.params[0].ty, &Context::default())
}

#[test]
fn canonical_spelling() {
    let cases = [
        ("Map<Str, f64>", "Map<Str,f64>"),
        ("arr<(u32, str)>", "arr<(u32,str)>"),
        ("fn(f64, u32) -> Str", "fn(f64,u32)->Str"),
        ("*mut u8[]", "*mut u8[]"),
        ("*u8[]", "*u8[]"),
        ("*(u8?)[]", "*(u8?)[]"),
        ("u8[]", "u8[]"),
        ("u8[]?", "u8[]?"),
        ("arr<u8[]>", "arr<u8[]>"),
        ("(u8[])[]", "u8[][]"),
        ("mut u8[]", "mut u8[]"),
        ("mut u8[]?", "mut u8[]?"),
        (
            "fn(Event) -> unit throws KvError",
            "fn(Event)->unit throws KvError",
        ),
        ("fn(Event) throws", "fn(Event)->unit throws err"),
        ("fn(Event)", "fn(Event)"),
        ("fn(Event) -> unit", "fn(Event)"),
        ("fn(Event)?", "fn(Event)?"),
        ("fn(str, ...arr<f64>)", "fn(str,...arr<f64>)"),
        ("fn(...arr<Any?>)", "fn(...arr<Any?>)"),
        ("(fn(Event) -> str)?", "(fn(Event)->str)?"),
        ("fn(Event) -> str?", "fn(Event)->str?"),
        ("fn() -> (fn(u8)) throws", "fn()->(fn(u8)) throws err"),
        ("(f64,)", "(f64,)"),
        ("(u32, (f64, bool))", "(u32,(f64,bool))"),
        ("arr<str?>", "arr<str?>"),
        ("arr<str>?", "arr<str>?"),
        ("future<Response>", "future<Response>"),
        ("future<Response, err>", "future<Response>"),
        (
            "future<Response, FetchError>",
            "future<Response,FetchError>",
        ),
        ("Tuple<str, u16>", "Tuple<str,u16>"),
        ("Vec<u8>", "Vec<u8>"),
        ("Arr<U32>", "Arr<U32>"),
        ("str??", "str??"),
        ("arr<@js(null) f64?>", "arr<f64?>"),
        ("Any?", "Any?"),
        ("\"node:buffer\".Buffer", "\"node:buffer\".Buffer"),
        ("Buffer", "Buffer"),
        ("\"Str\"", "Str"),
        ("ns.\"content-type\"", "ns.\"content-type\""),
    ];
    for (src, want) in cases {
        assert_eq!(canon(src), want, "for {src}");
        let read = text::parse_type(want).unwrap_or_else(|e| panic!("{want}: {e}"));
        let ir = text::parse("ir 1;\nclass Buffer {}").unwrap();
        assert_eq!(
            Keys::new(&ir).ty(&read, &Context::default()),
            want,
            "canonical spelling reads back for {src}"
        );
    }
}

#[test]
fn parse_type_rejects_trailing_input() {
    let e = text::parse_type("str str").unwrap_err();
    assert_eq!(
        e.to_string(),
        "1:5: expected the end of the input, found `str`"
    );
    assert!(text::parse_type("").is_err());
}

#[test]
fn import_modules() {
    assert_eq!(import_module(None), "tw:js/global");
    assert_eq!(import_module(Some("node:buffer")), "node:buffer");
}

#[test]
fn protocol_names() {
    let i = |m: &str, n: &str| Import::new(m, n);
    assert_eq!(
        protocol::instanceof("Response"),
        i("tw:instanceof", "Response")
    );
    assert_eq!(protocol::destroy("fn(Event)"), i("tw:destroy", "fn(Event)"));
    assert_eq!(protocol::destroy("Counter"), i("tw:destroy", "Counter"));
    assert_eq!(protocol::call("fn(Event)"), i("tw:call", "fn(Event)"));
    assert_eq!(protocol::box_("Str"), i("tw:box", "Str"));
    assert_eq!(protocol::box_("(u32,bool)"), i("tw:box", "(u32,bool)"));
    assert_eq!(protocol::unbox("(u32,bool)"), i("tw:unbox", "(u32,bool)"));
    assert_eq!(
        protocol::unbox_unchecked("Str"),
        i("tw:unboxUnchecked", "Str")
    );
    assert_eq!(protocol::arr_len(), i("tw:arr", "len"));
    assert_eq!(protocol::arr_get("str"), i("tw:arr", "get<str>"));
    assert_eq!(protocol::tag("KvError"), i("tw:tag", "KvError"));
    assert_eq!(protocol::tag(ERR), i("tw:tag", "err"));
    assert_eq!(protocol::future_poll(), i("tw:future", "poll"));
    assert_eq!(
        protocol::future_take("Response"),
        i("tw:future", "take<Response>")
    );
    assert_eq!(
        protocol::future_error("FetchError"),
        i("tw:future", "error<FetchError>")
    );
    assert_eq!(protocol::future_drop(), i("tw:future", "drop"));
    assert_eq!(protocol::future_new(), i("tw:future", "new"));
    assert_eq!(
        protocol::future_resolve("u32"),
        i("tw:future", "resolve<u32>")
    );
    assert_eq!(
        protocol::future_reject("err"),
        i("tw:future", "reject<err>")
    );
    assert_eq!(
        protocol::loop_work_available(),
        i("tw:loop", "work_available")
    );
    assert_eq!(protocol::loop_drop(), i("tw:loop", "drop"));

    assert_eq!(protocol::call_export("fn(Event)"), "tw:call:fn(Event)");
    assert_eq!(protocol::drop_export("fn(Event)"), "tw:drop:fn(Event)");
    assert_eq!(protocol::drop_export("Counter"), "tw:drop:Counter");
    let exports = [
        protocol::REALLOC,
        protocol::FREE,
        protocol::TABLE_ALLOC,
        protocol::TABLE_FREE,
        protocol::MEMORY,
        protocol::TABLE,
        protocol::TERMINATED,
        protocol::LOOP_TICK,
        protocol::LOOP_NEXT_SLEEP,
        protocol::FUTURE_WAKE,
    ];
    assert_eq!(
        exports,
        [
            "tw:realloc",
            "tw:free",
            "tw:table/alloc",
            "tw:table/free",
            "memory",
            "tw:table",
            "tw:terminated",
            "tw:loop/tick",
            "tw:loop/next_sleep",
            "tw:future/wake",
        ]
    );
    assert_eq!(protocol::Poll::Rejected as i32, 2);
    assert_eq!(protocol::TICK_BUDGET, 64);
}

#[test]
fn protocol_subjects_use_canonical_spelling() {
    let ir = text::parse(
        "ir 1;\nmodule \"node:buffer\" {\n  class Buffer {}\n  export { Buffer };\n}\nf(a: \"node:buffer\".Buffer, b: fn(Event) -> unit throws KvError);\n",
    )
    .unwrap();
    let keys = Keys::new(&ir);
    let Decl::Member(m) = &ir.items[1] else {
        panic!()
    };
    let cx = Context::default();
    assert_eq!(
        protocol::instanceof(&keys.ty(&m.params[0].ty, &cx)),
        Import::new("tw:instanceof", "\"node:buffer\".Buffer")
    );
    assert_eq!(
        protocol::call_export(&keys.ty(&m.params[1].ty, &cx)),
        "tw:call:fn(Event)->unit throws KvError"
    );
}

#[test]
fn enum_statics_key_as_statics() {
    let src = "ir 1;\n\nenum Color: i32 {\n  Red = 1;\n  Green = 2;\n  static parse(s: str): Color throws;\n  static get all(): arr<Color>;\n}\n";
    let ir = text::parse(src).unwrap();
    assert_eq!(text::print(&ir), src);
    ts_gen_abi::validate::validate(&ir).unwrap();
    let keys: Vec<String> = Keys::new(&ir)
        .members()
        .into_iter()
        .map(|k| k.key)
        .collect();
    assert_eq!(keys, ["Color.parse(str)", "Color.get:all()"]);
}

#[test]
fn paths_resolve_lexically_and_from_the_root() {
    let src = r#"ir 1;
interface Inner {}
namespace Shadow {
  interface Inner {}
  near(a: Inner, b: .Inner);
  namespace Deep {
    deep(a: Inner, b: Shadow.Inner, c: .Inner, d: .Shadow.Inner);
  }
}
module "m" {
  interface Inner {}
  namespace N {
    interface Inner {
      inner(a: Inner, b: .Inner, c: "m".Inner, d: "m".N.Inner, e: m.Inner);
    }
  }
}
"#;
    let ir = text::parse(src).unwrap();
    assert_eq!(text::parse(&text::print(&ir)).unwrap(), ir);
    assert!(
        text::print(&ir).contains("deep(a: Inner, b: Shadow.Inner, c: .Inner, d: .Shadow.Inner)")
    );
    let keys: Vec<String> = Keys::new(&ir)
        .members()
        .into_iter()
        .map(|k| k.key)
        .collect();
    assert_eq!(
        keys,
        [
            "Shadow.near(Shadow.Inner,Inner)",
            "Shadow.Deep.deep(Shadow.Inner,Shadow.Inner,Inner,Shadow.Inner)",
            "N.Inner#inner(\"m\".N.Inner,Inner,\"m\".Inner,\"m\".N.Inner,m.Inner)",
        ]
    );
}
