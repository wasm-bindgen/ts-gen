use std::path::Path as FsPath;

use ts_gen_abi::ir::*;
use ts_gen_abi::text;

fn sample() -> String {
    let path = FsPath::new(env!("CARGO_MANIFEST_DIR")).join("testdata/sample.ir");
    std::fs::read_to_string(path).unwrap()
}

fn parse(src: &str) -> Ir {
    text::parse(src).unwrap_or_else(|e| panic!("{e}\n{src}"))
}

#[test]
fn sample_is_a_fixed_point() {
    let src = sample();
    let out = text::print(&parse(&src));
    if out != src {
        let line = src
            .lines()
            .zip(out.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(src.lines().count().min(out.lines().count()));
        panic!(
            "print differs from the sample at line {}:\n  sample: {:?}\n  print:  {:?}\n\n{out}",
            line + 1,
            src.lines().nth(line),
            out.lines().nth(line)
        );
    }
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

fn method<'a>(c: &'a Class, name: &str) -> &'a Member {
    c.members
        .iter()
        .find(|m| m.kind == MemberKind::Method(name.into()))
        .unwrap()
}

#[test]
fn sample_reads_as_written() {
    let ir = parse(&sample());
    let request = class(&ir, "Request");
    assert_eq!(
        request.meta.doc.as_deref(),
        Some("An HTTP request.\n\n  Indented, and \"quoted\" \\ text.")
    );
    assert_eq!(request.extends[1].path, Path::new(["content-source"]));
    assert_eq!(
        request.members[1].kind,
        MemberKind::New(Some("withRequest".into()))
    );
    assert_eq!(
        request.members[3].kind,
        MemberKind::Get("content-type".into())
    );
    assert!(request.members[5].is_static);
    assert_eq!(method(request, "clone").throws, Some(Path::err()));
    assert_eq!(
        method(request, "iterator").meta.attrs.js,
        [JsEntry::Symbol("iterator".into())]
    );

    let map = class(&ir, "Map");
    let m = method(map, "map");
    assert_eq!(
        m.tparams[1].bound,
        Some(Bound::In(vec![Type::StrBox, Type::Scalar(Scalar::F64)]))
    );

    let types = class(&ir, "Types");
    let f = &method(types, "functions").params;
    assert_eq!(
        f[1].ty,
        Type::Nullable(
            Box::new(Type::Fn(Box::new(FnType {
                params: vec![Type::named(["Event"])],
                rest: false,
                result: Type::Unit,
                throws: None,
            }))),
            false
        )
    );
    let Type::Fn(d) = &f[3].ty else { panic!() };
    assert_eq!(d.result, Type::Nullable(Box::new(Type::Str), false));
    let p = &method(types, "pointers").params;
    assert_eq!(
        p[2].ty,
        Type::Array(Box::new(Type::View {
            mutable: false,
            elem: Box::new(Type::Scalar(Scalar::U8))
        }))
    );
    let s = &method(types, "sequences").params;
    assert_eq!(s[4].ty, Type::Tuple(vec![Type::Scalar(Scalar::F64)]));
    assert_eq!(
        s[6].ty,
        Type::Nullable(
            Box::new(Type::MutVec(Box::new(Type::Scalar(Scalar::U8)))),
            false
        )
    );
    let q = &method(types, "quoted").params;
    assert_eq!(
        q[0].ty,
        Type::Named(TypeRef {
            path: Path::in_module("node:buffer", ["Buffer"]),
            args: Vec::new(),
        })
    );
    assert_eq!(q[1].ty, Type::named(["Str"]));
    assert_eq!(
        method(types, "futures").result,
        Type::Future(Box::new(Type::Unit), Path::err())
    );

    let target = class(&ir, "EventTarget");
    let add = &method(target, "addEventListener").params;
    assert!(add[1].once && !matches!(add[1].ty, Type::Nullable(_, true)));
    assert!(add[2].once && matches!(add[2].ty, Type::Nullable(_, true)));
    assert_eq!(
        method(target, "padded").meta.attrs.js,
        [JsEntry::Args(vec![
            Telem::Param("a".into()),
            Telem::Undefined,
            Telem::Lit(Literal::Int(1)),
            Telem::Lit(Literal::Bool(true)),
            Telem::Lit(Literal::Bool(false)),
            Telem::Lit(Literal::Int(-2)),
            Telem::Lit(Literal::Str("x".into())),
        ])]
    );

    let enums: Vec<&Enum> = ir
        .items
        .iter()
        .filter_map(|d| match d {
            Decl::Enum(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(enums[0].arms[2].value, Some(ArmValue::Alias("Post".into())));
    assert_eq!(
        enums[0].arms[2].meta.doc.as_deref(),
        Some("The latest method.")
    );
    assert!(enums[0].arms[3].meta.attrs.fallback);
    assert_eq!(enums[1].carrier, Some(Scalar::I32));
    assert_eq!(enums[1].arms[1].value, Some(ArmValue::Int(-1)));
    assert_eq!(enums[2].arms[2].payload.len(), 2);
    assert_eq!(enums[3].extends, [Path::err(), Path::new(["ns", "Base"])]);
    assert_eq!(
        enums[3].arms[0].meta.attrs.js,
        [JsEntry::Class(vec![
            ClassPred {
                class: Path::new(["DOMException"]),
                props: vec![
                    ("name".into(), Literal::Str("NotFoundError".into())),
                    ("code".into(), Literal::Int(8)),
                ],
            },
            ClassPred {
                class: Path::new(["TypeError"]),
                props: vec![],
            },
        ])]
    );

    let fetch = ir
        .items
        .iter()
        .find_map(|d| match d {
            Decl::Member(m) if m.kind == MemberKind::Method("fetch".into()) => Some(m),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        fetch.meta.attrs.js[2],
        JsEntry::Cancel(Expr::Ref {
            path: vec!["$result".into(), "cancel".into()],
            call: None
        })
    );
}

#[test]
fn equivalent_spellings_read_equal() {
    let pairs = [
        ("f(): unit;", "f();"),
        ("f(): future<u8, err>;", "f(): future<u8>;"),
        ("f() throws err;", "f() throws;"),
        ("f(a: fn(E) -> unit);", "f(a: fn(E));"),
        ("f(a: ((u8)));", "f(a: u8);"),
        (
            "@js(name = \"x\") @js(null) get y(): u8?;",
            "@js(name = \"x\") get y(): @js(null) u8?;",
        ),
        ("@js(null) f(): future<u8?>;", "f(): future<@js(null) u8?>;"),
        ("f(@js(null) a: u8?);", "f(a: @js(null) u8?);"),
        (
            "f(a: arr<@js(null) u8?>, b: (@js(null) u8?)?, c: str??);",
            "f(a: arr<@js(null) u8?>, b: (@js(null) u8?)?, c: str??);",
        ),
        ("f(a: *(u8)[]);", "f(a: *u8[]);"),
        ("f(a: mut (u8)[]);", "f(a: mut u8[]);"),
        ("f(a: (u8)[]);", "f(a: u8[]);"),
    ];
    for (a, b) in pairs {
        assert_eq!(
            parse(&format!("ir 1;\n{a}")),
            parse(&format!("ir 1;\n{b}")),
            "{a} vs {b}"
        );
    }
}

#[test]
fn rest_parameters_print() {
    for t in [
        "fn(...arr<Any?>)",
        "fn(str, ...Arr<f64>) -> u8",
        "fn(...arr<f64>)?",
    ] {
        assert_eq!(text::print_type(&text::parse_type(t).unwrap()), t);
    }
    assert_ne!(
        text::parse_type("fn(...arr<f64>)").unwrap(),
        text::parse_type("fn(arr<f64>)").unwrap()
    );
}

#[test]
fn defaults_and_undefined_print() {
    let src = "ir 1;\n\nclass P<A, B: Q<A> = Q<A>, C in (u8, str) = str> {\n  all<T = unit>(v: Unit): (unit, T);\n}\n";
    assert_eq!(text::print(&parse(src)), src);
}

#[test]
fn contextual_keywords_are_names() {
    let ir = parse(
        "ir 1;\nclass class {\n  get(name: str): str;\n  set(a: str, b: str);\n  get get(): u8;\n  static static(): unit;\n  static();\n  export(): unit;\n}\nmodule(): unit;\nnamespace(): unit;\n",
    );
    let c = class(&ir, "class");
    assert_eq!(c.members[0].kind, MemberKind::Method("get".into()));
    assert_eq!(c.members[1].kind, MemberKind::Method("set".into()));
    assert_eq!(c.members[2].kind, MemberKind::Get("get".into()));
    assert!(c.members[3].is_static);
    assert_eq!(c.members[4].kind, MemberKind::Method("static".into()));
    assert!(!c.members[4].is_static);
    assert_eq!(c.members[5].kind, MemberKind::Method("export".into()));
    assert!(
        matches!(&ir.items[1], Decl::Member(m) if m.kind == MemberKind::Method("module".into()))
    );
    let printed = text::print(&ir);
    assert_eq!(parse(&printed), ir);
}

#[test]
fn errors_are_positioned() {
    let cases = [
        ("", "1:1: expected the `ir 1;` header"),
        ("ir 2;", "1:4: IR version 2; this reader reads 1"),
        ("ir 1", "1:5: expected `;`, found the end of the file"),
        (
            "ir 1;\nclass A {\n  f(): ;\n}",
            "3:8: expected a type, found `;`",
        ),
        (
            "ir 1;\nf(a: ());",
            "2:6: `()` is not a type; tuples have at least one element",
        ),
        ("ir 1;\nf(a: \"x);", "2:6: unterminated string"),
        ("ir 1;\nf(a: \"\\q\");", "2:7: unknown escape"),
        ("ir 1;\nf(a: \"\\u{zz}\");", "2:7: bad `\\u{...}` escape"),
        ("ir 1;\n  f(a: u8) ~", "2:12: unexpected character `~`"),
        ("ir 1;\n@foo f();", "2:1: unknown attribute `@foo`"),
        (
            "ir 1;\nf(@fallback a: u8);",
            "2:3: `@fallback` is not allowed on a parameter",
        ),
        (
            "ir 1;\nf(@js(name = \"a\") a: u8);",
            "2:3: only `@js(null)` and `@js(unshared)` are allowed on a parameter",
        ),
        (
            "ir 1;\nenum E: f64 { A; }",
            "2:9: `f64` is not an integer carrier",
        ),
        (
            "ir 1;\nenum E { A(); }",
            "2:11: an empty payload; write a unit arm without `()`",
        ),
        (
            "ir 1;\nclass A { get x(a: u8): u8; }",
            "2:17: a getter takes no parameters",
        ),
        (
            "ir 1;\nclass A { set x(a: u8, b: u8); }",
            "2:24: a setter takes at most one parameter",
        ),
        (
            "ir 1;\nexport interface I {}",
            "2:1: an interface can't be exported",
        ),
        ("ir 1;\nf(a: arr);", "2:6: `arr` needs type arguments"),
        ("ir 1;\nf(a: *u8);", "2:9: `*` applies to a whole `T[]`"),
        (
            "ir 1;\nf(a: mut u8?);",
            "2:12: `mut` applies to a whole `T[]`",
        ),
        ("ir 1;\nf(a: Map<>);", "2:10: an empty type argument list"),
        (
            "ir 1;\nf(@js(null) a: Str);",
            "2:3: `@js(null)` applies to a `T?`, not `Str`",
        ),
        (
            "ir 1;\nclass A { @js(null) get s(): Str; }",
            "2:11: `@js(null)` applies to a `T?`, not `Str`",
        ),
        (
            "ir 1;\n@js(null) class A {}",
            "2:1: `@js(null)` goes on a member or a `T?`",
        ),
        ("ir 1;\n@js() f();", "2:1: empty `@js()`"),
        (
            "ir 1;\n@js(null)\n/// late\nf();",
            "3:1: a doc comment must come before the attributes",
        ),
        ("ir 1;\nf(a: u8 b: u8);", "2:9: expected `)`, found `b`"),
        (
            "ir 1;\nf(a: fn(...arr<u8>, u8));",
            "2:21: a rest parameter `...arr<T>` must be the last parameter",
        ),
        (
            "ir 1;\nenum E: u8 { A = 99999999999999999999; }",
            "2:18: integer literal out of range",
        ),
        (
            "ir 1;\nmodule \"m\" { export {}; }",
            "2:14: an empty export list",
        ),
        (
            "ir 1;\ninterface I<U in (A)> { type P = match U {}; }",
            "2:42: an empty `match`",
        ),
    ];
    for (src, want) in cases {
        let got = text::parse(src).map(|_| ()).unwrap_err().to_string();
        assert_eq!(got, want, "for {src:?}");
    }
}

#[test]
fn associated_types() {
    let src = "ir 1;\n\ninterface RoundingOptions<Units in (Unit, DateUnit)> {\n  type PluralUnit = match Units { Unit => PluralUnit_, DateUnit => PluralDateUnit };\n  get smallestUnit(): PluralUnit?;\n}\n";
    let ir = parse(src);
    let c = class(&ir, "RoundingOptions");
    let a = &c.assoc[0];
    assert_eq!((a.name.as_str(), a.param.as_str()), ("PluralUnit", "Units"));
    let named = |n: &str| {
        Type::Named(TypeRef {
            path: Path::new([n]),
            args: Vec::new(),
        })
    };
    assert_eq!(a.row(&named("DateUnit")), Some(&named("PluralDateUnit")));
    assert_eq!(text::print(&ir), src);
    // `type` stays a member name.
    let ir = parse("ir 1;\nclass A { type(): str; get type(): str; }");
    assert!(class(&ir, "A").assoc.is_empty());
}
