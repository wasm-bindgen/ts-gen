//! Every `ts` example in IR.md reads, prints, reads back equal, and
//! validates, so the spec, the parser and the validator can't drift.
//! Examples are fragments, so names they don't declare are opaque. A fence
//! marked ```` ```ts invalid ```` is a deliberately invalid example, and must
//! be rejected by the parser or the validator. One marked
//! ```` ```ts builtin ```` shows a built-in declaration, which only the IR
//! itself may declare, so it isn't validated.

use std::path::Path;

use ts_gen_abi::text;
use ts_gen_abi::validate::{self, Options};

struct Example {
    line: usize,
    invalid: bool,
    builtin: bool,
    src: String,
}

fn examples() -> Vec<Example> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../IR.md");
    let spec = std::fs::read_to_string(&path).unwrap();
    let mut out = Vec::new();
    let mut open: Option<Example> = None;
    for (i, line) in spec.lines().enumerate() {
        match &mut open {
            None => {
                let info = line.trim_start().strip_prefix("```").map(str::trim);
                let mut words = info.into_iter().flat_map(str::split_whitespace);
                if words.next() == Some("ts") {
                    let words: Vec<&str> = words.collect();
                    open = Some(Example {
                        line: i + 1,
                        invalid: words.contains(&"invalid"),
                        builtin: words.contains(&"builtin"),
                        src: String::new(),
                    });
                }
            }
            Some(e) if line.trim_start().starts_with("```") => {
                out.push(std::mem::replace(
                    e,
                    Example {
                        line: 0,
                        invalid: false,
                        builtin: false,
                        src: String::new(),
                    },
                ));
                open = None;
            }
            Some(e) => {
                e.src.push_str(line);
                e.src.push('\n');
            }
        }
    }
    assert!(out.len() > 10, "found only {} examples", out.len());
    out
}

fn with_header(src: &str) -> String {
    if src.trim_start().starts_with("ir ") {
        src.into()
    } else {
        format!("ir 1;\n{src}")
    }
}

#[test]
fn spec_examples_round_trip() {
    let mut failures = Vec::new();
    for e in examples() {
        let src = with_header(&e.src);
        let parsed = text::parse(&src);
        let fragment = Options {
            allow_unresolved: true,
            ..Default::default()
        };
        if e.invalid {
            if let Ok(ir) = parsed {
                if validate::validate_with(&ir, fragment).is_ok() {
                    failures.push(format!("IR.md:{}: an invalid example validated", e.line));
                }
            }
            continue;
        }
        let ir = match parsed {
            Ok(ir) => ir,
            Err(err) => {
                failures.push(format!("IR.md:{}: {err}\n{src}", e.line));
                continue;
            }
        };
        if let Err(errs) = validate::validate_with(&ir, fragment).or_else(|errs| {
            if e.builtin {
                Ok(())
            } else {
                Err(errs)
            }
        }) {
            let errs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
            failures.push(format!(
                "IR.md:{}: doesn't validate:\n{}\n{src}",
                e.line,
                errs.join("\n")
            ));
        }
        let printed = text::print(&ir);
        match text::parse(&printed) {
            Ok(back) if back == ir => {}
            Ok(_) => failures.push(format!(
                "IR.md:{}: printing changed the IR\n{printed}",
                e.line
            )),
            Err(err) => failures.push(format!(
                "IR.md:{}: printed text doesn't read back: {err}\n{printed}",
                e.line
            )),
        }
        let again = text::print(&text::parse(&printed).unwrap_or_default());
        if again != printed {
            failures.push(format!(
                "IR.md:{}: print isn't a fixed point\n{printed}\n---\n{again}",
                e.line
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
