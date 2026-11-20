//! Conflict renaming: CONVENTIONS.md's variant names, recased to camelCase.

use ts_gen_abi::ir::*;
use ts_gen_abi::naming::{self, Slot};
use ts_gen_abi::text;

/// A variant from `name: type` parameters, `...name: type` rest parameters
/// and `name = "lit"` / `name = null` hidden constants, comma separated.
fn sig(src: &str) -> Vec<Slot<'static>> {
    if src.is_empty() {
        return Vec::new();
    }
    src.split(", ")
        .map(|p| {
            if let Some((name, value)) = p.split_once(" = ") {
                let value = match value {
                    "null" => Literal::Null,
                    v if v.starts_with('"') => Literal::Str(v.trim_matches('"').into()),
                    v => Literal::Int(v.parse().unwrap()),
                };
                return Slot::Const {
                    name: leak(name),
                    value: Box::leak(Box::new(value)),
                };
            }
            let (name, ty) = p.split_once(": ").unwrap();
            let (name, rest) = match name.strip_prefix("...") {
                Some(n) => (n, true),
                None => (name, false),
            };
            let ir = text::parse(&format!("ir 1;\nf(x: {ty});\n")).unwrap();
            let Decl::Member(m) = &ir.items[0] else {
                unreachable!()
            };
            Slot::Param {
                name: leak(name),
                ty: Box::leak(Box::new(m.params[0].ty.clone())),
                rest,
            }
        })
        .collect()
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn names(base: &str, variants: &[&str]) -> Vec<String> {
    let sigs: Vec<_> = variants.iter().map(|v| sig(v)).collect();
    naming::variant_names(base, &sigs)
}

#[test]
fn a_lone_variant_keeps_its_name() {
    assert_eq!(names("send", &["body: str"]), ["send"]);
    assert_eq!(names("get", &["key: str, type = \"json\""]), ["getJson"]);
}

#[test]
fn optional_truncation() {
    assert_eq!(
        names(
            "f",
            &["a: str", "a: str, b: f64", "a: str, b: f64?, c: bool"]
        ),
        ["f", "fWithB", "fWithBAndC"]
    );
}

#[test]
fn union_parameters_name_by_type() {
    assert_eq!(
        names("send", &["body: str", "body: ArrayBuffer"]),
        ["send", "sendWithArrayBuffer"]
    );
}

#[test]
fn variants_name_what_they_add() {
    assert_eq!(
        names(
            "until",
            &[
                "other: PlainDate",
                "other: str",
                "other: PlainDate, options: DifferenceOptions",
                "other: str, options: DifferenceOptions",
            ]
        ),
        [
            "until",
            "untilWithStr",
            "untilWithOptions",
            "untilWithStrAndOptions"
        ]
    );
}

#[test]
fn the_same_addition_twice_is_named_in_full() {
    assert_eq!(
        names(
            "compare",
            &[
                "one: A, two: A",
                "one: A, two: B",
                "one: B, two: A",
                "one: B, two: B"
            ]
        ),
        [
            "compare",
            "compareWithAAndB",
            "compareWithBAndA",
            "compareWithBAndB"
        ]
    );
}

#[test]
fn overloads_with_other_parameter_names() {
    assert_eq!(
        names(
            "put",
            &[
                "key: str, value: str",
                "key: str, value: str, options: PutOptions",
                "entries: Record",
                "entries: Record, options: PutOptions",
            ]
        ),
        [
            "put",
            "putWithOptions",
            "putWithEntries",
            "putWithEntriesAndOptions"
        ]
    );
}

#[test]
fn mixed_overload_optional_and_union() {
    assert_eq!(
        names(
            "show",
            &[
                "",
                "value: str",
                "value: f64",
                "value: str, opts: ShowOpts",
                "value: f64, opts: ShowOpts",
            ]
        ),
        [
            "show",
            "showWithStr",
            "showWithF64",
            "showWithStrAndOpts",
            "showWithF64AndOpts"
        ]
    );
}

#[test]
fn a_base_reading_with_continues_with_and() {
    assert_eq!(
        names(
            "with",
            &["fields: Fields", "fields: Fields, options: Options"]
        ),
        ["with", "withAndOptions"]
    );
    assert_eq!(
        names("setWith", &["a: str", "a: str, b: str"]),
        ["setWith", "setWithAndB"]
    );
    assert_eq!(
        names("width", &["a: str", "a: str, b: str"]),
        ["width", "widthWithB"]
    );
}

#[test]
fn null_constants_name_their_position() {
    // `set signal(value: AbortSignal)` beside `@js(args = (null))`.
    assert_eq!(
        names("signal", &["value: AbortSignal", "value = null"]),
        ["signal", "signalWithNull"]
    );
    assert_eq!(
        names("f", &["a: A, b: B", "a = null, b = null"]),
        ["f", "fWithANullAndBNull"]
    );
    // `f(a?: T, b?: U | null, c?: V)`.
    assert_eq!(
        names(
            "f",
            &[
                "",
                "a: T",
                "a: T?, b: U",
                "a: T?, b = null",
                "a: T?, b: U?, c: V",
                "a: T?, b = null, c: V",
            ]
        ),
        [
            "f",
            "fWithA",
            "fWithAAndB",
            "fWithAAndNull",
            "fWithAAndBAndC",
            "fWithAAndNullAndC"
        ]
    );
}

/// workers-types' `KVNamespace.get`: string constants name their arms, and
/// no arm takes the bare name from the literal-free overload.
#[test]
fn kv_get_literal_cohort() {
    assert_eq!(
        names(
            "get",
            &[
                "key: str",
                "key: str, type = \"text\"",
                "key: str, type = \"json\"",
                "key: str, type = \"arrayBuffer\"",
                "key: str, type = \"stream\"",
                "key: arr<str>, type = \"text\"",
                "key: arr<str>, type = \"json\"",
            ]
        ),
        [
            "get",
            "getText",
            "getJson",
            "getArrayBuffer",
            "getStream",
            "getTextWithArray",
            "getJsonWithArray"
        ]
    );
}

/// The Rust generator names a type as its signatures spell it.
#[test]
fn a_generator_vocabulary() {
    let rust_word = |t: &Type| match t {
        Type::Array(_) => "Slice".to_string(),
        Type::Any => "JsValue".to_string(),
        t => naming::type_word(t),
    };
    let sigs: Vec<_> = [
        "key: str, type = \"text\"",
        "key: arr<str>, type = \"text\"",
    ]
    .iter()
    .map(|v| sig(v))
    .collect();
    assert_eq!(
        naming::variant_names_with("get", &sigs, &rust_word),
        ["getText", "getTextWithSlice"]
    );
}

/// workers-types' `CompressionStream` constructor over
/// `"gzip" | "deflate" | "deflate-raw"`.
#[test]
fn constructor_literals() {
    assert_eq!(
        names(
            "new",
            &[
                "format = \"gzip\"",
                "format = \"deflate\"",
                "format = \"deflate-raw\""
            ]
        ),
        ["newGzip", "newDeflate", "newDeflateRaw"]
    );
}

/// Temporal's `getTimeZoneTransition(direction: "next" | "previous")`
/// beside `(direction: TransitionOptions)`.
#[test]
fn literal_arms_beside_a_typed_overload() {
    assert_eq!(
        names(
            "getTimeZoneTransition",
            &[
                "direction: TransitionOptions",
                "direction = \"next\"",
                "direction = \"previous\"",
            ]
        ),
        [
            "getTimeZoneTransition",
            "getTimeZoneTransitionNext",
            "getTimeZoneTransitionPrevious"
        ]
    );
}

#[test]
fn colliding_type_words_take_the_parameter_name() {
    assert_eq!(
        names(
            "f",
            &["a: str", "a: Map<str,u8>", "a: \"node:buffer\".Map, b: u8"]
        ),
        ["f", "fWithMap", "fWithNodeBufferMapAndB"]
    );
    // Two siblings that differ only in type arguments: the second takes
    // its parameter name, then a number.
    assert_eq!(
        names("f", &["a: str", "a: Map<str,u8>", "a: Map<str,f64>"]),
        ["f", "fWithMap", "fWithAMap"]
    );
    assert_eq!(
        names(
            "f",
            &[
                "a: str",
                "a: Map<str,u8>",
                "a: Map<str,f64>",
                "a: Map<str,i8>"
            ]
        ),
        ["f", "fWithMap", "fWithAMap", "fWithAMap2"]
    );
}

#[test]
fn rest_parameters_trail() {
    assert_eq!(
        names(
            "log",
            &["...data: arr<Any>", "label: str, ...data: arr<Any>"]
        ),
        ["log", "logWithLabel"]
    );
}

#[test]
fn pascal_words() {
    assert_eq!(naming::pascal("flagKey"), "FlagKey");
    assert_eq!(naming::pascal("deflate-raw"), "DeflateRaw");
    assert_eq!(naming::pascal("Set-Cookie"), "SetCookie");
    assert_eq!(naming::pascal("node:buffer"), "NodeBuffer");
    assert_eq!(naming::pascal("x_y"), "XY");
    assert_eq!(
        names("f", &["a = \"\"", "a = -1", "a = 2"]),
        ["fEmpty", "fNeg1", "f2"]
    );
}
