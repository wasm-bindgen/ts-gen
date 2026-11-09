//! The lowered JSON: golden output of `tests/json/sample.ir` under the
//! Kotlin and GC profiles (`tests/json/sample.<profile>.json`, rewritten
//! with `BLESS=1`), and well-formedness.

use std::path::Path;

use ts_gen_abi::json::{self, Json};
use ts_gen_abi::profile::Profile;
use ts_gen_abi::{text, validate};

fn sample() -> ts_gen_abi::ir::Ir {
    let src = std::fs::read_to_string(dir().join("sample.ir")).unwrap();
    let ir = text::parse(&src).unwrap();
    validate::validate(&ir).unwrap();
    ir
}

fn dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/json")
}

/// A strict JSON reader, enough to check the output parses.
fn check(s: &str) {
    fn ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && b" \n\r\t".contains(&b[*i]) {
            *i += 1;
        }
    }
    fn value(b: &[u8], i: &mut usize) {
        ws(b, i);
        match b[*i] {
            b'{' | b'[' => {
                let close = if b[*i] == b'{' { b'}' } else { b']' };
                let obj = close == b'}';
                *i += 1;
                ws(b, i);
                if b[*i] == close {
                    *i += 1;
                    return;
                }
                loop {
                    if obj {
                        ws(b, i);
                        assert_eq!(b[*i], b'"', "key at {i}");
                        value(b, i);
                        ws(b, i);
                        assert_eq!(b[*i], b':', "colon at {i}");
                        *i += 1;
                    }
                    value(b, i);
                    ws(b, i);
                    match b[*i] {
                        b',' => *i += 1,
                        c if c == close => {
                            *i += 1;
                            return;
                        }
                        c => panic!("unexpected {:?} at {i}", c as char),
                    }
                }
            }
            b'"' => {
                *i += 1;
                while b[*i] != b'"' {
                    if b[*i] == b'\\' {
                        *i += 1;
                    }
                    assert!(b[*i] >= 0x20, "control character at {i}");
                    *i += 1;
                }
                *i += 1;
            }
            _ => {
                let start = *i;
                while *i < b.len() && !b",]} \n".contains(&b[*i]) {
                    *i += 1;
                }
                let t = std::str::from_utf8(&b[start..*i]).unwrap();
                assert!(
                    ["true", "false", "null"].contains(&t) || t.parse::<i64>().is_ok(),
                    "bad literal {t}"
                );
            }
        }
    }
    let b = s.as_bytes();
    let mut i = 0;
    value(b, &mut i);
    ws(b, &mut i);
    assert_eq!(i, b.len(), "trailing input");
}

#[test]
fn golden() {
    let ir = sample();
    for (name, profile) in [
        ("all-host", "all-host"),
        ("all-gc", "all-gc"),
        // Kotlin: `core.results` is slot 0, the rest in `core.spill`.
        ("kotlin", "all-host\nresults = single\nthrows = js"),
    ] {
        let p = Profile::parse(profile).unwrap();
        let out = json::lower(&ir, &p).pretty();
        check(&out);
        assert!(!out.contains("\"error\": \""), "a member failed to lay out");
        let file = dir().join(format!("sample.{name}.json"));
        if std::env::var_os("BLESS").is_some() {
            std::fs::write(&file, &out).unwrap();
        } else {
            let want = std::fs::read_to_string(&file).unwrap_or_default();
            assert!(
                out == want,
                "{} differs; rerun with BLESS=1",
                file.display()
            );
        }
    }
}

#[test]
fn escapes_and_layout_errors() {
    let s = Json::Arr(vec![
        Json::Str("a\"b\\c\n\u{1}".into()),
        Json::Int(-1),
        Json::Null,
    ])
    .pretty();
    assert_eq!(s, "[\"a\\\"b\\\\c\\n\\u0001\", -1, null]\n");
    check(&s);
    // A linear-only position under a profile without memory.
    let ir = text::parse("ir 1;\nf(s: str);").unwrap();
    let p = Profile::parse("all-gc, str = linear, memory = 32").unwrap();
    let out = json::lower(&ir, &p).pretty();
    check(&out);
    assert!(!out.contains("\"error\""));
    let mut p = Profile::all_host();
    p.str = ts_gen_abi::profile::StrCarrier::Linear(ts_gen_abi::profile::Encoding::Utf8);
    let out = json::lower(&ir, &p).pretty();
    assert!(out.contains("\"error\": \"a linear `str` needs a linear memory\""));
}

#[test]
fn libs() {
    let lib = text::parse(
        "ir 1;\nlib std;\n\nclass Date {\n  getTime(): f64;\n}\n\nenum Order {\n  Less;\n  More;\n}\n",
    )
    .unwrap();
    let ir = text::parse(
        "ir 1;\nwith std;\n\ninterface Event {\n  get timeStamp(): Date;\n  sort(o: Order);\n}\n",
    )
    .unwrap();
    let out = json::lower_with(&ir, &[&lib], &Profile::all_host()).pretty();
    check(&out);
    assert!(!out.contains("\"error\""), "{out}");
    assert!(out.contains("\"with\": [\"std\"]"));
    assert!(out.contains("\"name\": \"std\""));
    assert!(out.contains("\"path\": \"Date\""));
    // The lib's members are its own bindings', not this IR's.
    assert!(!out.contains("Date#getTime()"));
    assert!(out.contains("Event#get:timeStamp()"));
}

#[test]
fn variants() {
    let ir = text::parse(
        "ir 1;\n\ninterface Iterable<T> {}\n\nclass Bytes {\n  static from(elements: Iterable<F64>): Bytes;\n  static from<T>(elements: Iterable<T>): Bytes;\n  at(index: u32): f64?;\n  at(index: i32): f64?;\n  get size(): u32;\n}\n",
    )
    .unwrap();
    let out = json::lower(&ir, &Profile::all_host()).pretty();
    check(&out);
    let variant = |key: &str| {
        let at = out.find(&format!("\"key\": \"{key}\"")).unwrap();
        let rest = &out[at..];
        let v = &rest[rest.find("\"variant\": ").unwrap() + 11..];
        v[..v.find(['\n', ',']).unwrap()].to_string()
    };
    assert_eq!(variant("Bytes.from(Iterable<F64>)"), "\"from\"");
    assert_eq!(variant("Bytes.from(Iterable<T>)"), "\"fromWithIterableT\"");
    assert_eq!(variant("Bytes#at(u32)"), "\"at\"");
    assert_eq!(variant("Bytes#at(i32)"), "\"atWithI32\"");
    assert_eq!(variant("Bytes#get:size()"), "\"size\"");
}

#[test]
fn box_nested_warns_under_box_none_null() {
    let ir = text::parse("ir 1;\nf(v: Str??): Str??;\ng(v: Str?);").unwrap();
    let out = json::lower(&ir, &Profile::default()).pretty();
    check(&out);
    let w = "a `Str??` box is `Str?` under `box-none = null`: its two nones are one";
    assert_eq!(out.matches(w).count(), 1);
    let p = Profile::parse("box-none = unit").unwrap();
    assert!(!json::lower(&ir, &p).pretty().contains("\"warnings\""));
}
