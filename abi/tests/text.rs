use std::path::Path;

use ts_gen_abi::ir::{ClassKind, Head, JsType, Literal, MemberKind, Named, Type};
use ts_gen_abi::{text, OwnerRef};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn sample_is_a_fixed_point() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/sample.ir");
    let src = read(&path);
    let ir = text::parse(&src).unwrap();
    let out = text::emit(&ir);
    if out != src {
        let line = src
            .lines()
            .zip(out.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(src.lines().count().min(out.lines().count()));
        panic!(
            "emit differs from the sample at line {}:\n  sample: {:?}\n  emit:   {:?}\n\n{out}",
            line + 1,
            src.lines().nth(line),
            out.lines().nth(line)
        );
    }
}

#[test]
fn sample_reads_as_written() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/sample.ir");
    let ir = text::parse(&read(&path)).unwrap();

    let request = ir.class(None, "Request").unwrap();
    assert_eq!(
        request.doc.as_deref(),
        Some("An HTTP request.\n\n  Indented, and \"quoted\" \\ text.")
    );
    let ctor = &request.members[1];
    assert_eq!(
        (ctor.kind, ctor.name.as_str(), ctor.js.as_str()),
        (MemberKind::Constructor, "new_with_request", "Request")
    );
    assert_eq!(ctor.result, named(None, "Request"));
    let wait = request
        .members
        .iter()
        .find(|m| m.js == "waitUntil")
        .unwrap();
    assert_eq!(
        (wait.kind, wait.name.as_str(), &wait.result),
        (MemberKind::Method, "wait_until", &Type::Void)
    );
    let cf = request.members.iter().find(|m| m.js == "cf").unwrap();
    assert_eq!(
        cf.result,
        Type::Js(JsType {
            head: Head::Any,
            null: false,
            undefined: true
        })
    );
    let redirect = request.members.iter().find(|m| m.js == "redirect").unwrap();
    assert!(redirect.throws);
    assert_eq!(redirect.kind, MemberKind::StaticMethod);

    let log = &ir.class(None, "Console").unwrap().members[0];
    assert!(log.variadic);

    let conn = ir.class(None, "Conn").unwrap();
    assert_eq!(conn.aliases, ["Connection", "Db.Connection"]);
    assert_eq!(conn.members[0].aliases, ["open2", "reopen"]);

    let workers = Some("cloudflare:workers");
    let durable = ir.class(workers, "DurableObject").unwrap();
    assert_eq!(durable.params, ["Env"]);
    assert_eq!(durable.members[1].params[0].ty, named(None, "Request"));
    assert_eq!(
        durable.members[0].result,
        Type::js(Head::Named(Named {
            name: owner(workers, "DurableObject"),
            args: vec![JsType {
                head: Head::Var("Env".into()),
                null: false,
                undefined: false
            }],
        }))
    );
    let body = ir.kind(workers, "Body").unwrap();
    assert_eq!(
        body.arms[0].ty.head,
        Head::Named(Named {
            name: owner(workers, "Request"),
            args: vec![]
        })
    );
    let connect = ir
        .functions
        .iter()
        .find(|f| f.member.name == "connect")
        .unwrap();
    assert_eq!(connect.module.as_deref(), workers);
    assert_eq!(connect.member.aliases, ["open"]);
    assert_eq!(
        connect.member.result,
        named(Some("cloudflare:sockets"), "Socket")
    );

    let headers = ir.class(None, "Headers").unwrap();
    let flags = headers.members.iter().find(|m| m.js == "flags").unwrap();
    assert_eq!(flags.result, named(None, "util.Flags"));

    let fetch = &ir.provides[0];
    assert_eq!(
        (fetch.name.as_str(), fetch.js.as_slice(), fetch.is_async),
        (
            "fetch",
            ["default".to_string(), "fetch".into()].as_slice(),
            true
        )
    );
    assert_eq!(ir.provides[1].name, "default.scheduled");

    assert_eq!(ir.class(None, "Body").unwrap().kind, ClassKind::Interface);
    assert_eq!(
        ir.class(None, "WebAssembly").unwrap().kind,
        ClassKind::Namespace
    );
    assert_eq!(request.kind, ClassKind::Class);
    let counter = ir.class(None, "Counter").unwrap();
    let clone = request.members.iter().find(|m| m.js == "clone").unwrap();
    assert!(clone.throws && !clone.nothrow);
    let heads: Vec<&Head> = clone.errors.iter().map(|e| &e.head).collect();
    let error = |path| {
        Head::Named(Named {
            name: owner(None, path),
            args: vec![],
        })
    };
    assert_eq!(heads, [&error("TypeError"), &error("RangeError")]);
    let json = request.members.iter().find(|m| m.js == "json").unwrap();
    assert!(!json.throws);
    assert_eq!(
        json.errors,
        [
            JsType {
                head: error("SyntaxError"),
                null: false,
                undefined: false
            },
            JsType {
                head: Head::Empty,
                null: true,
                undefined: false
            }
        ]
    );
    let url = request.members.iter().find(|m| m.js == "url").unwrap();
    assert!(url.nothrow && !url.throws && url.errors.is_empty());

    let stream = ir.class(None, "ReadableStream").unwrap();
    let pipe = stream
        .members
        .iter()
        .find(|m| m.js == "pipeThrough")
        .unwrap();
    assert_eq!(pipe.type_params, ["T"]);
    assert_eq!(
        pipe.result,
        Type::js(Head::Named(Named {
            name: owner(None, "ReadableStream"),
            args: vec![JsType {
                head: Head::Var("T".into()),
                null: false,
                undefined: false,
            }],
        }))
    );
    let clone = &ir
        .functions
        .iter()
        .find(|f| f.member.js == "structuredClone")
        .unwrap()
        .member;
    assert_eq!(clone.type_params, ["T"]);
    assert_eq!(clone.result, Type::js(Head::Var("T".into())));
    let socket = ir.class(Some("cloudflare:sockets"), "Socket").unwrap();
    assert_eq!(
        socket.members[0].module.as_deref(),
        Some("cloudflare:workers")
    );
    assert_eq!(
        (
            counter.members[0].js.as_str(),
            counter.members[1].js.as_str()
        ),
        ("Counter", "Counters.Counter")
    );
    let method = ir.kind(None, "MethodKind").unwrap();
    assert_eq!((method.path.as_str(), method.fallback), ("Method", true));
    assert!(!ir.kind(None, "Thing").unwrap().fallback);
    assert_eq!(method.arms[0].doc.as_deref(), Some("Fetches."));
    assert_eq!(method.arms[1].aliases, ["Create", "Submit"]);
    let arms: Vec<(&str, &Head, bool, bool)> = method
        .arms
        .iter()
        .map(|a| (a.name.as_str(), &a.ty.head, a.ty.null, a.ty.undefined))
        .collect();
    let lit = |s: &str| Head::Literal(Literal::String(s.into()));
    assert_eq!(
        arms,
        [
            ("Get", &lit("GET"), false, false),
            ("Post", &lit("POST"), false, false),
            ("Zero", &Head::Literal(Literal::Int(0)), false, false),
            ("MinusOne", &Head::Literal(Literal::Int(-1)), false, false),
            ("Other", &Head::String, false, false),
        ]
    );
    let of = &counter.members[counter.members.len() - 2];
    assert_eq!(of.cohort.as_deref(), Some("__of"));
    let reset = counter.members.last().unwrap();
    let Type::Js(JsType {
        head: Head::Named(promise),
        ..
    }) = &reset.result
    else {
        panic!("{reset:?}");
    };
    assert_eq!(
        promise.args[0],
        JsType {
            head: Head::Empty,
            null: false,
            undefined: true
        }
    );
    let init = ir.class(None, "RequestInit").unwrap();
    assert!(init.members[1].optional && !init.members[0].optional);

    let local2 = ir.class(None, "Local2").unwrap();
    assert_eq!(local2.path, "Local");
    assert_eq!(
        (local2.members[0].js.as_str(), &local2.members[0].result),
        ("Local", &named(None, "Local2"))
    );
    assert_eq!(local2.members[1].result, named(None, "Local"));
    let local = ir.class(None, "Local").unwrap();
    assert_eq!(local.members[0].result, named(None, "Local2"));
    assert_eq!(local.members[1].result, named(None, "M_2.T"));
    assert_eq!(ir.class(None, "M_2.T").unwrap().path, "M.T");
    let nothing = headers.members.iter().find(|m| m.js == "nothing").unwrap();
    assert_eq!(
        nothing.result,
        Type::Js(JsType {
            head: Head::Empty,
            null: true,
            undefined: false
        })
    );
}

#[test]
fn comments_carry_no_meaning() {
    let plain = "ir 1;\nclass A {\n  f(x: f64): String;\n}\n";
    let commented = "// header\nir 1; // version\n// before\nclass A { // open\n  //// not a doc\n  f(x: f64): String; // trailing\n}\n";
    assert_eq!(text::parse(commented).unwrap(), text::parse(plain).unwrap());
}

#[test]
fn declarations_may_come_in_any_order() {
    let ir = text::parse(
        "ir 1;\nfunction f(): K;\nkind K {\n  A = A;\n}\nprovide p();\nclass A { g(): B; }\nclass B {}\n",
    )
    .unwrap();
    assert_eq!(ir.functions[0].member.result, named(None, "K"));
    let classes: Vec<&str> = ir.classes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(classes, ["A", "B"]);
    let again = text::parse(&text::emit(&ir)).unwrap();
    assert_eq!(again, ir);
}

fn error(src: &str) -> String {
    match text::parse(src) {
        Ok(ir) => panic!("parsed {src:?} as {ir:?}"),
        Err(e) => e,
    }
}

#[test]
fn rejects_a_wasm_type_with_an_empty() {
    let e = error("ir 1;\nclass A {\n  get x: f64 | Undefined;\n}\n");
    assert!(e.starts_with("3:10: "), "{e}");
    assert!(e.contains("Number | Undefined"), "{e}");
    let e = error("ir 1;\nfunction f(x: Null | bool);\n");
    assert!(e.starts_with("2:22: "), "{e}");
}

#[test]
fn rejects_unknown_types() {
    let e = error("ir 1;\nfunction f(x: string);\n");
    assert_eq!(e, "2:15: unknown type `string`");
    let e = error("ir 1;\nfunction f(x: Promise<f64>);\n");
    assert!(e.starts_with("2:23: type arguments are JS types"), "{e}");
    let e = error("ir 1;\nfunction f(x: String | Number);\n");
    assert!(e.starts_with("2:24: a union has one arm"), "{e}");
}

#[test]
fn rejects_duplicate_names() {
    let e = error("ir 1;\nclass A {\n  get x: f64;\n  set x(value: f64);\n}\n");
    assert_eq!(e, "4:3: duplicate member `x` in `A`");
    let e = error("ir 1;\nclass A {\n  f();\n  g();\n}\nalias A.f = A.g;\n");
    assert_eq!(e, "6:1: duplicate member `f` in `A`");
    let e = error("ir 1;\nclass A {}\nkind A {}\n");
    assert_eq!(e, "3:1: `A` is declared twice");
    let e = error("ir 1;\nclass B as A {}\nclass A {}\n");
    assert_eq!(e, "3:1: `A` is declared twice");
    assert!(text::parse("ir 1;\nclass A as A2 {}\nclass A {}\n").is_ok());
    let e = error("ir 1;\nkind K {\n  A = String;\n  B = Number alias A;\n}\n");
    assert_eq!(e, "4:3: duplicate arm `A` in `K`");
    let e = error("ir 1;\nkind K {\n  A = String alias B, B;\n}\n");
    assert_eq!(e, "3:3: duplicate arm `B` in `K`");
    let e = error("ir 1;\nclass A<T> {\n  f<T>(x: T);\n}\n");
    assert!(e.starts_with("3:5: duplicate type parameter `T`"), "{e}");
    let e = error("ir 1;\nfunction f(): f64 throws A | A;\n");
    assert!(e.starts_with("2:30: duplicate exception type"), "{e}");
    let ok = "ir 1;\nclass A {}\nmodule \"m\" {\n  class A {}\n}\n";
    assert!(text::parse(ok).is_ok());
}

#[test]
fn rejects_a_missing_or_other_version() {
    assert_eq!(
        error("class A {}\n"),
        "1:1: expected the `ir <version>;` header"
    );
    assert!(error("ir 2;\n").starts_with("1:4: format version 2"));
}

#[test]
fn rejects_misplaced_syntax() {
    assert!(error("ir 1;\n/// Doc.\n").starts_with("3:1: expected a declaration"));
    assert!(error("ir 1;\nclass A {\n  /// Doc.\n}\n").starts_with("4:1: a doc comment"));
    assert!(error("ir 1;\nfunction f(...xs: Array<Any>, y: f64);\n").starts_with("2:31: a rest"));
    assert!(error("ir 1;\nfunction f(...xs: Any);\n").starts_with("2:19: a rest"));
    assert!(error("ir 1;\nmodule \"m\" {\n  provide f();\n}\n").starts_with("3:3: `provide`"));
    assert!(error("ir 1;\nalias B = A;\n").starts_with("2:1: `A` names no"));
    assert!(error("ir 1;\nfunction f(x: void);\n").starts_with("2:15: `void` is only"));
    assert!(error("ir 1;\nkind K {\n  A = f64;\n}\n").starts_with("3:7: a kind's arms"));
    assert!(
        error("ir 1;\nkind K {\n  A = String | Null;\n}\n").starts_with("3:7: a kind's arms admit")
    );
    assert!(error("ir 1;\nkind K {\n  A = Undefined;\n}\n").starts_with("3:7: a kind's arms admit"));
    assert!(error("ir 1;\nfunction f(x: \"m);\n").starts_with("2:15: unterminated string"));
}

fn owner(module: Option<&str>, path: &str) -> OwnerRef {
    OwnerRef {
        module: module.map(String::from),
        path: path.into(),
    }
}

fn named(module: Option<&str>, path: &str) -> Type {
    Type::js(Head::Named(Named {
        name: owner(module, path),
        args: vec![],
    }))
}
