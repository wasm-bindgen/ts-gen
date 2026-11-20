//! Type aliases (IR.md §3): keys spell through them but for a back-edge,
//! validation keeps recursion behind a type constructor, and layouts
//! reach a function back-edge lazily.

use ts_gen_abi::ir::*;
use ts_gen_abi::key::Keys;
use ts_gen_abi::layout::{Flow, Kind, Layouts};
use ts_gen_abi::profile::Profile;
use ts_gen_abi::resolve::Context;
use ts_gen_abi::text;
use ts_gen_abi::validate;

fn parse(src: &str) -> Ir {
    let src = format!("ir 1;\n{src}");
    text::parse(&src).unwrap_or_else(|e| panic!("{e}\n{src}"))
}

fn keys(src: &str) -> Vec<String> {
    let ir = parse(src);
    let keys = Keys::new(&ir);
    keys.members().into_iter().map(|k| k.key).collect()
}

fn errors(src: &str) -> Vec<String> {
    let ir = parse(src);
    match validate::validate(&ir) {
        Ok(()) => Vec::new(),
        Err(es) => es.iter().map(|e| e.to_string()).collect(),
    }
}

const SRC: &str = r#"class Event {}
type Handler = fn(Event);
type Pair<A, B = str> = (A, B);
type Next<T> = fn(Next<T>, T);
type Tree = fn(arr<Tree>) -> Tree?;
namespace Ns {
  class Inner {}
  type Wrap<T> = arr<T>;
  type Inners = Wrap<Inner>;
}
module "m" {
  class Thing {}
  type Things = arr<Thing>;
  export { Thing, Things };
}
on(h: Handler);
pair(p: Pair<f64>, q: Pair<f64, Event>);
next(n: Next<f64>);
tree(t: Tree);
inners(i: Ns.Inners, w: Ns.Wrap<Ns.Inner>);
things(t: "m".Things);
"#;

#[test]
fn aliases_round_trip() {
    let ir = parse(SRC);
    assert_eq!(parse(text::print(&ir).trim_start_matches("ir 1;\n")), ir);
    assert!(text::print(&ir).contains("type Pair<A, B = str> = (A, B);"));
    assert_eq!(errors(SRC), Vec::<String>::new());
}

#[test]
fn keys_spell_the_expansion() {
    assert_eq!(
        keys(SRC),
        [
            "on(fn(Event))",
            "pair((f64,str),(f64,Event))",
            "next(fn(Next<f64>,f64))",
            "tree(fn(arr<Tree>)->Tree?)",
            "inners(arr<Ns.Inner>,arr<Ns.Inner>)",
            "things(arr<\"m\".Thing>)",
        ]
    );
}

#[test]
fn a_back_edge_lays_out_afresh() {
    let ir = parse(SRC);
    let p = Profile::parse("all-gc").unwrap();
    let l = Layouts::new(&ir, &p);
    let t = Type::named(["Next"]);
    let t = Type::Named(TypeRef {
        args: vec![Type::Scalar(Scalar::F64)],
        ..match t {
            Type::Named(r) => r,
            _ => unreachable!(),
        }
    });
    let r = l.repr(&t, &Context::default(), Flow::ToLang).unwrap();
    assert_eq!(r.ty, "fn(Next<f64>,f64)");
    let Kind::Fn { sig, .. } = &r.kind else {
        panic!("{r:?}")
    };
    let back = &sig.params[0];
    assert_eq!(back.ty, "fn(Next<f64>,f64)");
    assert!(matches!(
        back.kind,
        Kind::Rec {
            flow: Flow::ToJs,
            ..
        }
    ));
    let again = l.rec(back).unwrap();
    assert!(matches!(
        again.kind,
        Kind::Fn {
            provided: Some(_),
            ..
        }
    ));
    assert_eq!(l.wire(back).unwrap(), l.wire(&again).unwrap());
}

#[test]
fn recursion_lays_out_or_is_invalid() {
    for src in [
        "type List = arr<List>;",
        "type A = (i32, A?);",
        "type A = future<A>;",
        "type A = fn(A)?;",
        "type A = fn(B);\ntype B = arr<A>;",
    ] {
        let es = errors(src);
        assert!(
            es.iter().any(|e| e.contains("isn't a function type")),
            "{src}: {es:?}"
        );
    }
    let es = errors("type Next<T> = fn(Next<T>, T);\nclass D<T> { run(n: Next<T>); }");
    assert!(
        es.iter()
            .any(|e| e.contains("recurses over a type parameter")),
        "{es:?}"
    );
    let src = "type Next<T> = fn(Next<T>, T);\ntype P<T> = (T, Next<f64>);\nclass D<T> { run(n: Next<f64>, p: P<T>); }";
    assert_eq!(errors(src), Vec::<String>::new());
}

#[test]
fn recursion_needs_a_constructor() {
    for src in [
        "type A = A;",
        "type A = A?;",
        "type A = B;\ntype B = A;",
        "type Id<T> = T;\ntype A = Id<A>;",
    ] {
        let es = errors(src);
        assert!(
            es.iter()
                .any(|e| e.contains("reaches itself without a type constructor")),
            "{src}: {es:?}"
        );
    }
    for src in [
        "type A = fn(A);",
        "type A = fn(A?) -> arr<A>;",
        "type W<T> = fn(T);\ntype A = W<A>;",
        "type A = fn(B);\ntype B = fn(arr<A>);",
    ] {
        assert_eq!(errors(src), Vec::<String>::new(), "{src}");
    }
}

#[test]
fn arguments_take_defaults() {
    let es = errors("type P<A, B = str> = (A, B);\nf(p: P);");
    assert!(
        es.iter()
            .any(|e| e.contains("takes 1 to 2 type arguments, not 0")),
        "{es:?}"
    );
    let es = errors("type P<A, B = str> = (A, B);\nclass C {}\nf(p: P<str, str, str>);");
    assert!(es.iter().any(|e| e.contains("not 3")), "{es:?}");
    let es = errors("class X {}\ntype X = str;");
    assert!(es.iter().any(|e| e.contains("declared twice")), "{es:?}");
}
