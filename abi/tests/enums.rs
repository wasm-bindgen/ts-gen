//! Enum helpers: indices, orders and conversions over a hierarchy.

use ts_gen_abi::enums::{self, EnumRef, Flat, Shape, FALLBACK};
use ts_gen_abi::ir::*;
use ts_gen_abi::resolve::{Context, Symbols};
use ts_gen_abi::text;

fn parse(src: &str) -> Ir {
    text::parse(&format!("ir 1;\n{src}")).unwrap()
}

fn flat<'a>(symbols: &'a Symbols<'a>, name: &str) -> Flat<'a> {
    let path = Path::new(name.split('.'));
    let e = enums::resolve(symbols, &path, &Context::default()).unwrap();
    enums::flatten(symbols, e).unwrap_or_else(|es| panic!("{es:?}"))
}

fn arms(f: &Flat<'_>) -> Vec<(String, i64)> {
    f.arms
        .iter()
        .map(|a| (a.arm.name.clone(), a.value))
        .collect()
}

fn order<'a>(f: &'a Flat<'_>) -> Vec<&'a str> {
    f.discrimination
        .iter()
        .map(|&i| f.arms[i].arm.name.as_str())
        .collect()
}

fn pairs(v: &[(&str, i64)]) -> Vec<(String, i64)> {
    v.iter().map(|(n, i)| (n.to_string(), *i)).collect()
}

#[test]
fn implicit_indices_start_at_one() {
    let ir = parse(
        "enum Method { @js(\"GET\") Get; @js(\"POST\") Post; Latest = .Post; @fallback Unknown; }",
    );
    let s = Symbols::new(&ir);
    let f = flat(&s, "Method");
    assert_eq!(f.shape, Shape::Unit);
    assert_eq!(
        arms(&f),
        pairs(&[("Get", 1), ("Post", 2), ("Unknown", FALLBACK)])
    );
    assert_eq!(f.arm("Latest").unwrap().value, 2);
    assert_eq!(f.fallback().unwrap().arm.name, "Unknown");
}

#[test]
fn stated_indices_let_bases_combine() {
    let ir = parse(
        r#"
enum DateUnit { @js("year") Year; @js("month") Month; @js("day") Day; }
enum TimeUnit { @js("hour") Hour = 4; @js("minute") Minute; @js("second") Second; }
enum Unit extends DateUnit, TimeUnit { @js("week") Week; }
enum Gap { @js("a") A = 7; @js("b") B = 3; @js("c") C; @fallback Other(Str); }
"#,
    );
    let s = Symbols::new(&ir);
    let f = flat(&s, "Unit");
    assert_eq!(
        arms(&f),
        pairs(&[
            ("Year", 1),
            ("Month", 2),
            ("Day", 3),
            ("Hour", 4),
            ("Minute", 5),
            ("Second", 6),
            ("Week", 7),
        ])
    );
    assert_eq!(order(&f)[0], "Week");
    let widen = &f.conversions()[1];
    assert_eq!(widen.widen, [3, 4, 5]);
    let f = flat(&s, "Gap");
    assert_eq!(
        arms(&f),
        pairs(&[("A", 7), ("B", 3), ("C", 8), ("Other", FALLBACK)])
    );
}

#[test]
fn carriers_keep_their_values() {
    let ir = parse("enum Flags: u32 { Read = 1; Write = 2; ReadWrite = 3; All = .ReadWrite; }\nenum More extends Flags { Exec = 4; }");
    let s = Symbols::new(&ir);
    let f = flat(&s, "More");
    assert_eq!(f.shape, Shape::Carrier(Scalar::U32));
    assert_eq!(
        arms(&f),
        pairs(&[("Read", 1), ("Write", 2), ("ReadWrite", 3), ("Exec", 4)])
    );
    assert_eq!(f.arm("All").unwrap().value, 3);
}

#[test]
fn payload_shape() {
    let ir = parse("enum Body { Text(str); Bytes(u8[]); Stream(ReadableStream); }");
    let s = Symbols::new(&ir);
    let f = flat(&s, "Body");
    assert_eq!(f.shape, Shape::Payload);
    assert_eq!(arms(&f), pairs(&[("Text", 1), ("Bytes", 2), ("Stream", 3)]));
}

/// IR.md §8: `KvError` is `TypeError = 1`, `NotFound = 2`, `Error = -1`,
/// discriminated `NotFound`, `TypeError`, `Error`.
#[test]
fn kv_error_extends_err() {
    let ir = parse(
        "enum KvError extends err { @js(DOMException[name = \"NotFoundError\"]) NotFound(Str); }",
    );
    let s = Symbols::new(&ir);
    let f = flat(&s, "KvError");
    assert_eq!(
        arms(&f),
        pairs(&[("TypeError", 1), ("Error", FALLBACK), ("NotFound", 2)])
    );
    assert_eq!(order(&f), ["NotFound", "TypeError", "Error"]);
    assert_eq!(f.bases.len(), 1);
    assert_eq!(f.bases[0].0.to_string(), "err");
}

#[test]
fn the_builtin_err() {
    let ir = parse("");
    let s = Symbols::new(&ir);
    let f = flat(&s, "err");
    assert_eq!(arms(&f), pairs(&[("TypeError", 1), ("Error", FALLBACK)]));
    assert_eq!(order(&f), ["TypeError", "Error"]);
    assert!(f.bases.is_empty());
}

/// A child's arms are tested first, then each base's in turn, then the
/// `@fallback`; indices stay base first.
#[test]
fn deep_hierarchies() {
    let ir = parse(
        "enum A extends err { @js(SyntaxError) Syntax(Str); }\n\
         enum B extends A { @js(DOMException[name = \"AbortError\"]) Aborted(Str); Halt = .Aborted; }\n\
         namespace n { enum C extends B { @js(DOMException) Dom(Str); } }",
    );
    let s = Symbols::new(&ir);
    let f = flat(&s, "n.C");
    assert_eq!(
        arms(&f),
        pairs(&[
            ("TypeError", 1),
            ("Error", FALLBACK),
            ("Syntax", 2),
            ("Aborted", 3),
            ("Dom", 4)
        ])
    );
    assert_eq!(
        order(&f),
        ["Dom", "Aborted", "Syntax", "TypeError", "Error"]
    );
    assert_eq!(f.arm("Halt").unwrap().arm.name, "Aborted");
    let bases: Vec<String> = f.bases.iter().map(|(b, _)| b.to_string()).collect();
    assert_eq!(bases, ["B", "A", "err"]);
}

/// A base reached twice is included once.
#[test]
fn diamonds() {
    let ir = parse(
        "enum A extends err { @js(SyntaxError) Syntax(Str); }\n\
         enum B extends A {}\n\
         enum C extends A, B { @js(EvalError) Eval(Str); }",
    );
    let s = Symbols::new(&ir);
    let f = flat(&s, "C");
    assert_eq!(
        arms(&f),
        pairs(&[
            ("TypeError", 1),
            ("Error", FALLBACK),
            ("Syntax", 2),
            ("Eval", 3)
        ])
    );
    assert_eq!(order(&f), ["Eval", "Syntax", "TypeError", "Error"]);
}

#[test]
fn conversions() {
    let ir = parse(
        "enum A extends err { @js(SyntaxError) Syntax(Str); }\n\
         enum B extends A { @js(EvalError) Eval(Str); }",
    );
    let s = Symbols::new(&ir);
    let f = flat(&s, "B");
    let cs = f.conversions();
    assert_eq!(cs.len(), 2);
    let to_a = &cs[0];
    assert_eq!(to_a.base.to_string(), "A");
    // `A`'s arms: TypeError, Error, Syntax; `B`'s add Eval.
    assert_eq!(to_a.widen, [0, 1, 2]);
    assert_eq!(to_a.narrow, [Some(0), Some(1), Some(2), None]);
    let to_err = &cs[1];
    assert_eq!(to_err.base.to_string(), "err");
    assert_eq!(to_err.widen, [0, 1]);
    assert_eq!(to_err.narrow, [Some(0), Some(1), None, None]);
}

#[test]
fn enums_in_modules() {
    let ir = parse("module \"m\" { enum E { @js(\"a\") A; } enum F extends E { @js(\"b\") B; } }");
    let s = Symbols::new(&ir);
    let path = Path::in_module("m", ["F"]);
    let e: EnumRef<'_> = enums::resolve(&s, &path, &Context::default()).unwrap();
    let f = enums::flatten(&s, e).unwrap();
    assert_eq!(f.this.to_string(), "\"m\".F");
    assert_eq!(arms(&f), pairs(&[("A", 1), ("B", 2)]));
}

#[test]
fn hierarchy_errors() {
    let ir = parse(
        "enum A { @js(\"a\") X; }\nenum B { @js(\"b\") Y; }\nenum C extends A, B { @js(\"c\") Z; }",
    );
    let s = Symbols::new(&ir);
    let e = enums::resolve(&s, &Path::new(["C"]), &Context::default()).unwrap();
    let errs = enums::flatten(&s, e).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert!(errs[0].message.contains("index 1"), "{errs:?}");
}

#[test]
fn carrier_ranges() {
    assert!(enums::fits(Scalar::U8, 255));
    assert!(!enums::fits(Scalar::U8, 256));
    assert!(!enums::fits(Scalar::U32, -1));
    assert!(enums::fits(Scalar::I8, -128));
    assert!(enums::fits(Scalar::I64, i64::MIN));
}

#[test]
fn class_tests_come_before_interface_and_primitive_tests() {
    let ir = parse(
        "interface ArrayBufferView {}\nclass Blob {}\nclass ReadableStream {}\ninterface Init {}\n\
         enum BodyInit { Str(str); ArrayBufferView(ArrayBufferView); Blob(Blob); Bytes(u8[]); \
         @js(\"none\") None; Any(Any); Stream(ReadableStream); Pair(u32, str); }\n\
         enum More extends BodyInit { Init(Init); Num(f64); Other(Blob); }",
    );
    let s = Symbols::new(&ir);
    let f = flat(&s, "BodyInit");
    assert_eq!(
        order(&f),
        [
            "None",
            "Blob",
            "Bytes",
            "Stream",
            "Pair",
            "ArrayBufferView",
            "Str",
            "Any"
        ]
    );
    // Indices keep declaration order.
    assert_eq!(f.arm("Str").unwrap().value, 1);
    // Each tier is ranked on its own: the child's arms, then the base's.
    let f = flat(&s, "More");
    assert_eq!(
        order(&f),
        [
            "Other",
            "Init",
            "Num",
            "None",
            "Blob",
            "Bytes",
            "Stream",
            "Pair",
            "ArrayBufferView",
            "Str",
            "Any"
        ]
    );
}
