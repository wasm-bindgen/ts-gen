//! Static validation: one valid and one invalid case per rule.

use ts_gen_abi::text;
use ts_gen_abi::validate::{self, Options};

fn check(src: &str) -> Result<(), Vec<String>> {
    let src = format!("ir 1;\n{src}");
    let ir = text::parse(&src).unwrap_or_else(|e| panic!("{e}\n{src}"));
    validate::validate(&ir).map_err(|es| es.iter().map(|e| e.to_string()).collect())
}

#[track_caller]
fn ok(src: &str) {
    if let Err(es) = check(src) {
        panic!("expected valid:\n{src}\n{}", es.join("\n"));
    }
}

#[track_caller]
fn bad(src: &str, needle: &str) {
    match check(src) {
        Ok(()) => panic!("expected an error containing {needle:?}:\n{src}"),
        Err(es) => assert!(
            es.iter().any(|e| e.contains(needle)),
            "no error contains {needle:?}:\n{src}\n{}",
            es.join("\n")
        ),
    }
}

#[test]
fn errors_are_positioned() {
    let es = check("class A {}\nclass B {\n  f(x: Missing);\n}\n").unwrap_err();
    assert_eq!(es, ["4:5: `Missing` isn't declared"]);
}

#[test]
fn builtin_names_are_escaped() {
    ok("@js(name = \"Str\") class Str_ {}");
    bad("class Str {}", "`Str` is a built-in type name");
    bad(
        "enum err { @fallback E(Str); }",
        "`err` is a built-in type name",
    );
    bad("namespace arr {}", "built-in type name");
    bad("class A<Any> {}", "`Any` is a built-in type name");
}

#[test]
fn lexical_paths_dont_start_with_a_builtin() {
    // The text can't spell it (`str.X` reads as `str`), so build it.
    let mut ir = text::parse("ir 1;\nnamespace N { interface X {} }\nf(x: N.X);\n").unwrap();
    let ts_gen_abi::ir::Decl::Member(m) = &mut ir.items[1] else {
        panic!()
    };
    let ts_gen_abi::ir::Type::Named(r) = &mut m.params[0].ty else {
        panic!()
    };
    r.path.segments[0] = "str".into();
    let es: Vec<String> = validate::validate(&ir)
        .unwrap_err()
        .iter()
        .map(|e| e.to_string())
        .collect();
    assert!(
        es.iter()
            .any(|e| e.contains("starts with the built-in `str`")),
        "{es:?}"
    );
}

#[test]
fn renames_are_one_segment() {
    ok("@js(name = \"List\") class List_ {}");
    bad(
        "@js(name = \"a.b\") class A {}",
        "must be one non-empty segment",
    );
    bad(
        "@js(object) class A {}",
        "only `@js(name = ...)` applies to a declaration",
    );
}

#[test]
fn type_names_are_unique_in_a_scope() {
    ok("class A {}\nnamespace A { f(); }\nnamespace n { class A {} }");
    bad(
        "class A {}\nenum A { @js(\"a\") X; }",
        "`A` is declared twice",
    );
}

#[test]
fn paths_resolve() {
    ok("class A {}\nnamespace n { class B { f(a: A): B; } }");
    ok("module \"m\" { class C {} export { C }; }\nclass D { f(c: \"m\".C); }");
    bad("class D { f(c: C); }", "`C` isn't declared");
    bad("class D { f(): n.D; }", "`n.D` isn't declared");
}

#[test]
fn type_arguments_match() {
    ok("class Map<K, V> { get(k: K): V?; }\nclass A { f(m: Map<str, u8>); }");
    bad(
        "class Map<K, V> {}\nclass A { f(m: Map<str>); }",
        "takes 2 type arguments, not 1",
    );
    bad("class A<T> { f(t: T<u8>); }", "takes 0 type arguments");
}

#[test]
fn bounds() {
    ok("class B {}\nclass A<T: B, U: Any, W in (Str, f64)> {}");
    bad(
        "class A<T: str> {}",
        "bounds by a class, an interface or `Any`",
    );
    bad(
        "enum E { @js(\"a\") X; }\nclass A<T: E> {}",
        "`T` bounds `E` by an enum",
    );
}

#[test]
fn unit_goes_anywhere_but_an_array() {
    ok("f(): unit;\ng(): future<unit>;\nh(cb: fn(u8) -> unit throws);");
    ok("class Pair<A, B> {}\nf(x: unit, p: Pair<unit, Str>, t: (unit, u8), u: Unit?): Unit;");
    bad("f(): unit?;", "`unit?` is invalid");
    bad("f(): arr<unit>;", "an array element can't be `unit`");
}

#[test]
fn type_parameter_defaults() {
    ok("class Fetcher<T = unit> {}\nclass P<A, B: Fetcher<A> = Fetcher<A>> { all<T = Any?>(): T; }");
    ok("class B<T in (u8, str) = str> {}");
    bad(
        "class B<T in (u8, str) = bool> {}",
        "default `bool` isn't in its bound",
    );
    bad(
        "class B<T = U, U = u8> {}",
        "names `U`, which isn't declared before it",
    );
    bad("class B<T = T> {}", "names `T`");
    bad("class B<T = Missing> {}", "`Missing` isn't declared");
}

#[test]
fn never_is_only_a_result() {
    ok("f(): never throws;\ng(): never;\nh(cb: fn(u8) -> never);");
    bad("f(x: never);", "`never` is only a result");
    bad("f(): never?;", "`never?` is invalid");
    ok("f(): future<never>;\ng(cb: fn() -> future<never, err>);");
    bad("f(): arr<never>;", "`never` is only a result");
}

#[test]
fn nullable_once() {
    ok("f(x: str?);");
    ok("f(x: (str?)?);");
    bad("f(x: str???);", "`?` nests once");
}

#[test]
fn vec_elements_are_typed_array_types() {
    ok("f(a: u8[], b: Vec<f64>, c: i64[], d: f16[]);");
    bad(
        "f(a: bool[]);",
        "`bool[]`: the element must be a typed-array type",
    );
    bad("f(a: i53[]);", "the element must be a typed-array type");
    bad("f(a: Vec<str>);", "the element must be a typed-array type");
}

#[test]
fn mut_vec_is_only_a_parameter() {
    ok("f(a: mut u8[], b: mut u8[]?);\nexport g(a: mut f32[]);");
    bad("f(): mut u8[];", "is only a parameter type");
    bad("f(a: arr<mut u8[]>);", "is only a parameter type");
    bad("f(a: mut str[]);", "the element must be a typed-array type");
}

#[test]
fn views() {
    ok("class Crypto {\n  getRandomValues(@js(unshared) buf: *mut u8[]);\n}");
    ok("@js(args = (b, offset, data)) writeBuffer(b: Any, offset: u53, data: *u8[]);");
    bad("f(p: *u8[]?);", "`?` is invalid on a view");
    bad("export f(p: *u8[]);", "is only an import parameter type");
    bad("f(): *u8[];", "is only an import parameter type");
    bad("f(cb: fn(*u8[]));", "is only an import parameter type");
    bad("f(p: arr<*u8[]>);", "is only an import parameter type");
    bad(
        "f(p: *bool[]);",
        "`*bool[]`: the element must be a typed-array type; use `arr<bool>`",
    );
    bad(
        "f(@js(unshared) p: u8[]);",
        "`@js(unshared)` applies to a view",
    );
    bad(
        "@js(cancel = p.length) f(p: *u8[]): future<unit>;",
        "the view `p` is used only in `@js(args)`",
    );
}

#[test]
fn sequences() {
    ok("f(a: arr<str>, b: arr<(u32, str)>, c: Arr<bool>, d: arr<u8[]>);");
    bad("f(a: u8[][]);", "use `arr<u8[]>`");
    bad(
        "f(a: str[]);",
        "`str[]`: the element must be a typed-array type; use `arr<str>`",
    );
    bad("f(a: (u32, str)[]);", "use `arr<(u32, str)>`");
    bad("f(a: Vec<bool>);", "use `Arr<bool>`");
}

#[test]
fn enum_literals() {
    ok("enum Method { @js(\"GET\") Get; @js(\"POST\") Post; Latest = .Post; @fallback Unknown; }");
    bad(
        "enum Method { @js(\"GET\") Get; Post; }",
        "unit arm `Post` needs its JS literal",
    );
    bad(
        "enum M { @js(\"a\") A; @js(\"a\") B; }",
        "both have the literal \"a\"",
    );
    bad(
        "enum M { @js(\"a\") @js(\"b\") A; }",
        "more than one literal",
    );
    bad(
        "enum M { @js(\"a\") A(str); }",
        "only a unit arm that isn't `@fallback` has a literal",
    );
    bad(
        "enum M { @js(\"a\") A; @js(name = \"x\") B; }",
        "takes only a literal or an error class match",
    );
}

#[test]
fn enum_carriers() {
    ok("enum Flags: u32 { Read = 1; Write = 2; ReadWrite = 3; }");
    ok("enum M { @js(\"a\") A = 3; @js(\"b\") B; }");
    bad("enum M { @js(\"a\") A = 0; }", "isn't from 1");
    bad(
        "enum M { @js(\"a\") A = 2; @js(\"b\") B = 2; }",
        "already has",
    );
    bad(
        "enum M { @js(\"a\") A; @fallback U = 2; }",
        "can't state the index 2",
    );
    bad("enum Flags: u8 { Read; }", "needs a numeric value");
    bad("enum Flags: u8 { Big = 256; }", "doesn't fit `u8`");
    bad("enum Flags: u8 { A = 1; B = 1; }", "already has");
    bad("enum Flags: u8 { A(str) = 1; }", "can't have a payload");
    bad("enum Flags: u8 { @js(\"a\") A = 1; }", "has no literal");
    bad(
        "enum Flags: u8 { A = 1; @fallback B = 2; }",
        "has no `@fallback`",
    );
}

#[test]
fn enum_fallbacks() {
    ok("enum Body { Text(str); @fallback Other(Any); }");
    bad(
        "enum M { @fallback A(Any); B(str); }",
        "must be the last arm",
    );
    bad("enum M { @fallback A(Any, str); }", "at most one payload");
    bad(
        "enum A { @js(\"a\") X; @fallback F; }\nenum B extends A { @fallback G; }",
        "more than one `@fallback`",
    );
}

#[test]
fn enum_hierarchies() {
    ok("enum KvError extends err { @js(DOMException[name = \"NotFoundError\"]) NotFound(Str); }");
    bad(
        "enum E extends Missing { @js(\"a\") A; }",
        "`Missing`, which isn't declared",
    );
    bad(
        "class C {}\nenum E extends C { @js(\"a\") A; }",
        "isn't an enum",
    );
    bad(
        "enum A extends B { @js(\"a\") X; }\nenum B extends A { @js(\"b\") Y; }",
        "extends itself",
    );
    bad(
        "enum A { @js(\"a\") X; }\nenum B extends A { @js(\"b\") X; }",
        "arm name `X` appears twice",
    );
    bad(
        "enum A { @js(\"a\") X; }\nenum B { @js(\"b\") Y; }\nenum C extends A, B { @js(\"c\") Z; }",
        "has index 1, which `X` of `A` already has",
    );
    ok("enum A { @js(\"a\") X; }\nenum B { @js(\"b\") Y = 2; }\nenum C extends A, B { @js(\"c\") Z; }");
    bad(
        "enum A: u8 { X = 1; }\nenum B { @js(\"b\") Y; }\nenum C extends A, B {}",
        "different carriers",
    );
    bad("enum E {}", "`E` has no arms");
}

#[test]
fn enum_aliases() {
    ok("enum A { @js(\"a\") X; }\nenum B extends A { Y = .X; Z = .Y; }");
    bad("enum A { @js(\"a\") X; Y = .Missing; }", "names `.Missing`");
    bad("enum A { @js(\"a\") X; Y = .Z; Z = .Y; }", "names `.Z`");
    bad("enum A { @js(\"a\") X; X = .X; }", "appears twice");
}

#[test]
fn error_class_matches_need_a_payload() {
    ok("enum E extends err { @js(TypeError) Bad(Str); }");
    bad("enum E { @js(TypeError) Bad; }", "needs a `(Str)` payload");
}

#[test]
fn throws_enums() {
    ok("enum KvError extends err { @js(DOMException) NotFound(Str); }\nf() throws KvError;\ng(): future<u8, KvError>;\nh(cb: fn() -> unit throws KvError);\ni() throws;");
    ok("enum Own { @js(TypeError) T(Str); @fallback Other(Str); }\nf() throws Own;");
    bad(
        "enum E { @js(TypeError) T(Str); }\nf() throws E;",
        "must end in a `@fallback`",
    );
    bad(
        "enum E extends err { Code(u32); }\nf() throws E;",
        "only `(Str)` arms",
    );
    bad(
        "enum E extends err { Code(str); }\ng(): future<u8, E>;",
        "only `(Str)` arms",
    );
    bad("class C {}\nf() throws C;", "isn't an enum");
    bad("f() throws Missing;", "isn't declared");
}

#[test]
fn member_forms() {
    ok("get version(): str;\nset version(value: str);\nclass A { new(); static make(): A; get x(): u8; }");
    bad("new();", "a constructor must be inside a class");
    bad("static f();", "`static` applies to class members");
    bad(
        "class A { export f(); }",
        "`export` marks top-level members",
    );
    bad(
        "class A { get x(): unit; }",
        "getter `get x` needs a result",
    );
    bad("class A { set x(); }", "must give its one value");
    ok("class A { @js(args = (null)) set xWithNull(); }");
}

#[test]
fn nested_optionals() {
    ok("f(a: str??, b: Str??, c: arr<@js(null) f64?>, d: fn(@js(null) Str?)): Request??;\nclass Request {}");
    bad("f(a: str???);", "`?` nests once");
    bad(
        "f(a: (@js(null) str?)?);",
        "`@js(null)` doesn't apply to `T??`",
    );
    bad(
        "f(a: @js(null) str??);",
        "`@js(null)` doesn't apply to `T??`",
    );
    bad("f(a: unit??);", "`unit?` is invalid");
}

#[test]
fn parameters() {
    ok("f(a: str, @js(null) b: Str?, @once cb: fn(u8));");
    bad("f(a: str, a: u8);", "parameter `a` is declared twice");
    bad("export f(@js(null) b: Str?);", "has no effect");
    bad(
        "f(@once cb: u8);",
        "`@once` applies to a callback parameter",
    );
    bad("export f(@once cb: fn(u8));", "comes from JS");
    ok("f(@once cb: fn(u8)?);");
    ok("f(cb: fn(str, ...arr<f64>));\nf(cb: fn(...Arr<Any>));");
    bad(
        "f(cb: fn(...u8));",
        "a rest parameter is an `arr<T>` or `Arr<T>`",
    );
    bad(
        "f(cb: fn(...u8[]));",
        "a rest parameter is an `arr<T>` or `Arr<T>`",
    );
}

#[test]
fn attributes_belong_somewhere() {
    bad("@fallback f();", "`@fallback` goes on an enum arm");
    bad("@once f();", "`@once` goes inline on a callback parameter");
    bad("@once class A {}", "`@once` goes inline");
    bad("enum M { @once @js(\"a\") A; }", "`@once` goes inline");
    bad(
        "@js(\"x\") f();",
        "literals and error class matches go on enum arms",
    );
}

#[test]
fn js_member_entries() {
    ok("class A {\n  @js(null) get s(): Str?;\n  @js(null) f(): future<Str?>;\n}");
    ok("interface I { @js(object) new(); }");
    bad(
        "interface I { @js(object) new(a: u8); }",
        "a constructor with no parameters",
    );
    bad(
        "class A { @js(object) f(); }",
        "a constructor with no parameters",
    );
    ok("class A { @js(index) at(i: u32): Str?; @js(index) setAt(i: u32, v: Str); }");
    bad(
        "class A { @js(index) get x(): u8; }",
        "`@js(index)` applies to a method",
    );
    ok("class A { @js(symbol = iterator) iterator(): Any; }");
    bad(
        "class A { @js(symbol = nope) f(): Any; }",
        "isn't a well-known symbol",
    );
    ok("class A { @js(symbol = toStringTag) get toStringTag(): str; @js(symbol = hasInstance) static hasInstance(v: Any?): bool; }");
    bad(
        "class A { @js(symbol = iterator) new(); }",
        "applies to a method, getter or setter",
    );
    bad(
        "class A { @js(name = \"x\", symbol = toStringTag) get tag(): str; }",
        "it excludes `name`",
    );
    bad(
        "class A { @js(this = t) on(t: Any); }",
        "isn't supported yet",
    );
    bad(
        "class A { @js(name = \"x\", name = \"y\") f(); }",
        "more than one `@js(name ...)`",
    );
}

#[test]
fn incoming_templates() {
    ok("class Mode { static get FAST(): str; }\nexport namespace u {\n  @js(args = (x, o? { mode = Mode.FAST, tag = t, n = 1 })) f(x: u8, o: Any?, t: Any?);\n  @js(args = (Mode.FAST)) g();\n}");
    bad(
        "class C { new(); get s(): Any; }\nexport namespace u { @js(args = (C.new())) f(); }",
        "`C.new()` in `@js(args)` of export `f` can't be matched",
    );
    bad(
        "export namespace u { @js(args = (o? { tag = t.x })) f(o: Any?, t: Any); }",
        "`t.x` in `@js(args)` of export `f` can't be matched",
    );
    bad(
        "class C { new(); }\nexport namespace u { @js(let c = C.new()) f(); }",
        "`let c` on `f` binds nothing",
    );
    bad(
        "export get version(): str;",
        "`get version` can't be a module export",
    );
    bad("export set version(v: str);", "can't be a module export");
    ok("export namespace u { get version(): str; }");
}

#[test]
fn args_templates() {
    ok("class Kv {\n  @js(args = (key, \"text\"))\n  getText(key: str): future<str?>;\n  @js(args = (b, undefined, a, 1, true, null))\n  f(a: str, b: u8);\n}");
    bad("@js(args = (a, a)) f(a: str);", "appears more than once");
    bad("@js(args = (b)) f(a: str);", "`b` isn't a parameter");
    ok("@js(args = (\"x\", ...rest)) log(rest: arr<Any>);");
    bad(
        "@js(args = (...rest, 1)) log(rest: arr<Any>);",
        "must be the last element",
    );
    bad(
        "@js(args = (...rest)) log(rest: Any);",
        "spreads a sequence parameter",
    );
    bad(
        "@js(args = (...nope)) log(rest: arr<Any>);",
        "has no parameter `nope`",
    );
    bad(
        "@js(args = (a), args = (a)) f(a: u8);",
        "more than one `@js(args ...)`",
    );
}

#[test]
fn fills_and_expressions() {
    let prelude = "class AbortSignal {}\nclass AbortController {\n  new();\n  get signal(): AbortSignal;\n  abort();\n}\ninterface RequestInit { set signal(v: AbortSignal); }\nclass Request {}\nclass Response {}\n";
    ok(&format!(
        "{prelude}@js(let c = AbortController.new(), args = (input, init? {{ signal = c.signal }}), cancel = c.abort)\nfetch(input: Request, init: RequestInit?): future<Response>;"
    ));
    ok(&format!(
        "{prelude}class Task {{ cancel(); }}\n@js(cancel = $result.cancel)\nrun(): future<Task>;"
    ));
    bad(
        &format!("{prelude}@js(args = (init? {{ signal = 1 }}))\nfetch(init: RequestInit);"),
        "fills a nullable parameter",
    );
    bad(
        &format!(
            "{prelude}@js(args = (init? {{ signal = 1, signal = 2 }}))\nfetch(init: RequestInit?);"
        ),
        "sets `signal` twice",
    );
    bad(
        &format!("{prelude}@js(let c = AbortController.new(), cancel = c.stop)\nfetch(): future<Response>;"),
        "`AbortController` has no member `stop`",
    );
    bad(
        &format!("{prelude}@js(let c = AbortController.make())\nf();"),
        "has no static member `make`",
    );
    bad(
        &format!("{prelude}@js(let c = Nope.new())\nf();"),
        "`Nope.new` names nothing in scope",
    );
    bad(
        &format!("{prelude}@js(cancel = $result.cancel)\nf(): u8;"),
        "needs a `future` result",
    );
    bad(
        &format!("{prelude}@js(let c = $result)\nf();"),
        "`$result` is only in scope",
    );
    bad(
        &format!("{prelude}@js(let a = AbortController.new())\nf(a: u8);"),
        "`let a` is already a name",
    );
}

#[test]
fn inherited_members_type_check() {
    ok("class Base { get x(): Base; }\nclass Kid extends Base {}\n@js(args = (k.x.x)) f(k: Kid);");
}

#[test]
fn colliding_keys() {
    ok("class Kv { get(key: str): str; get(key: u8): str; }");
    bad(
        "class Kv { get(key: str): str; get(name: str): u8; }",
        "the key `Kv#get(str)` is declared twice",
    );
    bad(
        "f(a: str);\nf(b: str);",
        "the key `f(str)` is declared twice",
    );
    ok("module \"a\" { f(a: str); export { f }; }\nmodule \"b\" { f(a: str); export { f }; }");
}

#[test]
fn modules() {
    ok("module \"m\" { class A {} export { A, A as default }; }");
    bad(
        "module \"m\" { export { A }; }",
        "exports `A`, which it doesn't declare",
    );
    bad(
        "module \"m\" { class A {} export { A, A }; }",
        "exports `A` twice",
    );
    bad("module \"m\" { export f(); }", "can't export");
    bad("module \"m\" { export class C {} }", "can't export");
    bad("module \"m\" { export namespace n {} }", "can't export");
}

#[test]
fn export_classes() {
    let prelude = "class DurableObjectState {}\nclass DurableObject { new(); }\n";
    ok(&format!(
        "{prelude}export class Counter extends DurableObject {{\n  @js(super = (ctx))\n  new(ctx: DurableObjectState);\n  static create(): Counter;\n}}"
    ));
    bad(
        "class A {}\nclass B {}\nexport class C extends A, B {}",
        "may extend one imported class",
    );
    bad(
        "interface I {}\nexport class C extends I {}",
        "may extend only an imported class",
    );
    bad(
        "export class A {}\nexport class C extends A {}",
        "may extend only an imported class",
    );
    // A handle of a class with an imported base and no live wrapper is a
    // bridge `TypeError` at runtime, not a validation error.
    ok("class Base { new(a: u8); }\nexport class C extends Base { new(); static create(): C; }");
    ok("class Base { new(a: u8); }\nexport class C extends Base { new(); }\nf(c: C);");
    ok("class Base { new(a: u8); }\nexport class C extends Base { @js(super = (1)) new(); }\nexport g(c: C);");
    bad(
        "class A { @js(super = (1)) new(); }",
        "an exported class's constructor",
    );
    bad(
        "export class A { @js(super = (b)) new(a: u8); }",
        "`b` isn't a parameter",
    );
}

#[test]
fn the_end_to_end_example_validates_in_full() {
    ok("enum FetchError extends err {\n  @js(DOMException[name = \"AbortError\"]) Aborted(Str);\n}\nclass AbortSignal {}\nclass AbortController { new(); get signal(): AbortSignal; abort(); }\ninterface RequestInit { set signal(v: AbortSignal); }\nclass Env {}\nclass ExecutionContext {}\nclass Request {\n  get url(): str;\n}\nclass Response {\n  new(body: str);\n}\n@js(\n  let c = AbortController.new(),\n  args = (input, init? { signal = c.signal }),\n  cancel = c.abort,\n)\nfetch(input: Request, init: RequestInit?): future<Response, FetchError>;\nexport namespace default {\n  fetch(request: Request, env: Env, ctx: ExecutionContext): future<Response>;\n}\n");
}

#[test]
fn unresolved_names_are_allowed_in_fragments() {
    let ir = text::parse("ir 1;\nclass A extends Body { f(e: Event): Response throws KvError; }\nenum E extends Base { @js(\"a\") A; }\n@js(let c = AbortController.new()) g();\n").unwrap();
    assert!(validate::validate(&ir).is_err());
    validate::validate_with(
        &ir,
        Options {
            allow_unresolved: true,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn with_resolves_against_supplied_libs() {
    let lib = text::parse(
        "ir 1;\nlib std;\n\nclass Date {\n  getTime(): f64;\n}\n\ninterface Map<K, V> {}\n",
    )
    .unwrap();
    assert_eq!(
        text::print(&lib),
        "ir 1;\nlib std;\n\nclass Date {\n  getTime(): f64;\n}\n\ninterface Map<K, V> {}\n"
    );
    let src = "ir 1;\nwith std;\n\nclass Event {\n  get timeStamp(): Date;\n  get tags(): Map<Str, Date>;\n}\n";
    let ir = text::parse(src).unwrap();
    assert_eq!(text::print(&ir), src);
    let libs = std::slice::from_ref(&lib);
    let with = |ir| {
        validate::validate_with(
            ir,
            Options {
                libs,
                ..Default::default()
            },
        )
        .map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>())
    };
    with(&ir).unwrap();
    // Without the lib, or without naming it, its declarations don't resolve.
    let es = validate::validate(&ir).unwrap_err();
    assert!(es[0]
        .to_string()
        .contains("`with std;` needs the lib IR `std`"));
    let unnamed = text::parse(&src.replace("with std;\n", "")).unwrap();
    assert!(with(&unnamed).unwrap_err()[0].contains("`Date` isn't declared"));
    // The lib's arity holds, and the IR's own declarations shadow the lib's.
    let arity = text::parse(&src.replace("Map<Str, Date>", "Map<Date>")).unwrap();
    assert!(with(&arity).unwrap_err()[0].contains("takes 2 type arguments"));
    let own = text::parse(
        &format!("{src}\ninterface Map<V> {{}}\n").replace("Map<Str, Date>", "Map<Date>"),
    )
    .unwrap();
    with(&own).unwrap();
    // A member named `with` isn't a header.
    ok("with(x: f64);");
}

#[test]
fn module_declarations_need_a_js_value() {
    ok("module \"m\" { class A { f(); } export { A }; }");
    ok("module \"m\" { interface H { f(); @js(object) new(); } namespace n { class B {} } export { n }; }");
    bad(
        "module \"m\" { class H { f(); } }",
        "the class `H` in module \"m\" has no JS value: export it, or declare it as an interface",
    );
    bad(
        "module \"m\" { interface H { static make(); } }",
        "`H.make` in module \"m\" has no JS value, since `H` isn't exported",
    );
    bad(
        "module \"m\" { interface H { new(); } }",
        "`H.new` in module",
    );
    bad(
        "module \"m\" { namespace n { class B {} } }",
        "the class `n.B` in module",
    );
    bad(
        "module \"m\" { f(); }",
        "`f` in module \"m\" has no JS value, since it isn't exported",
    );
    ok("module \"m\" { @js(name = \"g\") f(); export { g }; }");
}

#[test]
fn associated_types_cover_their_bound() {
    let decls = "enum A { @js(\"a\") A; }\nenum B { @js(\"b\") B; }\nenum PA { @js(\"as\") As; }\nenum PB { @js(\"bs\") Bs; }\n";
    let with = |class: &str| format!("{decls}{class}");
    ok(&with(
        "interface I<U in (A, B)> {\n  type P = match U { A => PA, B => PB };\n  get p(): P?;\n  set p(value: P);\n}",
    ));
    bad(
        &with("interface I<U in (A, B)> { type P = match U { A => PA }; }"),
        "`I.P` needs one row per element of `U`'s bound, in order: A, B",
    );
    bad(
        &with("interface I<U in (A, B)> { type P = match U { B => PB, A => PA }; }"),
        "one row per element",
    );
    bad(
        &with("interface I<U: A> { type P = match U { A => PA }; }"),
        "`I.P` matches `U`, which has no `in (..)` bound",
    );
    bad(
        &with("interface I<U in (A)> { type P = match V { A => PA }; }"),
        "`I.P` matches `V`, which isn't a type parameter of `I`",
    );
    bad(
        &with("interface I<U in (A)> { type P = match U { A => Missing }; }"),
        "`Missing` isn't declared",
    );
    bad(
        &with("interface I<U in (A)> { type U = match U { A => PA }; }"),
        "`I.U` is declared twice",
    );
    bad(
        &with("interface I<U in (A)> {}\nf(): P;"),
        "`P` isn't declared",
    );
}
