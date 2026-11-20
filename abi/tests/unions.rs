//! `union` parameters (IR.md §7): the flag round-trips, validation keeps
//! it to discriminable payload enums on import parameters, and each arm
//! derives a variant key making the same JS call.

use ts_gen_abi::ir::*;
use ts_gen_abi::key::Keys;
use ts_gen_abi::text;
use ts_gen_abi::union;
use ts_gen_abi::validate;

fn parse(src: &str) -> Ir {
    let src = format!("ir 1;\n{src}");
    text::parse(&src).unwrap_or_else(|e| panic!("{e}\n{src}"))
}

fn errors(src: &str) -> Vec<String> {
    match validate::validate(&parse(src)) {
        Ok(()) => Vec::new(),
        Err(es) => es.iter().map(|e| e.to_string()).collect(),
    }
}

const ENUMS: &str = "class Stream {}
enum Body { Text(str); Num(f64); Stream(Stream); }
enum Id { Name(str); Num(u32); }
";

#[test]
fn round_trips() {
    let src = format!(
        "ir 1;\n{ENUMS}class C {{\n  f(a: union Body, b: union Id?, c: @js(null) union Id?, d: union Id??, union: str): u32;\n}}\n"
    );
    let ir = text::parse(&src).unwrap();
    let m = match &ir.items[3] {
        Decl::Class(c) => &c.members[0],
        _ => unreachable!(),
    };
    let flags: Vec<bool> = m.params.iter().map(|p| p.union).collect();
    assert_eq!(flags, [true, true, true, true, false]);
    let printed = text::print(&ir);
    assert!(
        printed.contains("f(a: union Body, b: union Id?, c: @js(null) union Id?, d: union Id??, union: str): u32;"),
        "{printed}"
    );
    assert_eq!(text::print(&text::parse(&printed).unwrap()), printed);
}

#[test]
fn variants() {
    let ir = parse(&format!(
        "{ENUMS}class C {{\n  f(a: union Id, n: u32, b: union Id?): u32;\n  set v(x: union Body);\n}}\n"
    ));
    let keys = Keys::new(&ir);
    let all = |name: &str| {
        let k = keys
            .members()
            .into_iter()
            .find(|k| k.key.contains(name))
            .unwrap();
        union::variants(&keys, &k)
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        all("#f("),
        [
            "C#f(str,u32,Id?)",
            "C#f(u32,u32,Id?)",
            "C#f(Id,u32,str?)",
            "C#f(str,u32,str?)",
            "C#f(u32,u32,str?)",
            "C#f(Id,u32,u32?)",
            "C#f(str,u32,u32?)",
            "C#f(u32,u32,u32?)",
        ]
    );
    assert_eq!(
        all("set:v"),
        ["C#set:v(str)", "C#set:v(f64)", "C#set:v(Stream)"]
    );
}

#[test]
fn invalid() {
    let cases = [
        ("class C { f(x: union str); }", "`x`: `union` takes a payload enum"),
        (
            "enum M { A; B; }\nclass C { f(x: union M); }",
            "`M` has no payload arm; a literal-only enum is passed as a value",
        ),
        (
            "enum N { A(u8); B(f64); }\nclass C { f(x: union N); }",
            "`N`'s arms `A` and `B` can't be told apart (`number`)",
        ),
        (
            "enum O { A(Ia); B(Ib); }\ninterface Ia {}\ninterface Ib {}\nclass C { f(x: union O); }",
            "can't be told apart (an object)",
        ),
        (
            "enum Id { Name(str); Num(u32); }\nexport class C { f(x: union Id); }",
            "`union` applies to an import's parameter; `x` comes from JS",
        ),
        (
            "enum Id { Name(str); Num(u32); }\nclass C {\n  f(x: union Id);\n  @js(name = \"g\") f(x: str);\n}",
            "`C#f(str)` is an arm variant of `C#f(Id)`'s `union`, but a different JS call",
        ),
    ];
    for (src, want) in cases {
        let es = errors(src);
        assert!(es.iter().any(|e| e.contains(want)), "{src}\n{es:?}");
    }
    // A real overload making the same call is a valid stand-in.
    assert_eq!(
        errors("enum Id { Name(str); Num(u32); }\nclass C {\n  f(x: union Id);\n  f(y: str);\n}"),
        Vec::<String>::new()
    );
    // Fallbacks and nested enums aren't compared.
    assert_eq!(
        errors(&format!("{ENUMS}enum W {{ Id(Id); Flag(bool); @fallback Other(Any); }}\nclass C {{ f(x: union W); }}")),
        Vec::<String>::new()
    );
}
