//! The link step: renaming a language module's imports to their keys.

use ts_gen_bridge::link::{link, read_links};

fn imports(wasm: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ImportSection(r) = p.unwrap() {
            for i in r.into_imports() {
                let i = i.unwrap();
                out.push((i.module.to_string(), i.name.to_string()));
            }
        }
    }
    out
}

const MODULE: &str = r#"(module
  (import "js_code" "app.flat.Request_get_url" (func $url (param externref) (result externref)))
  (import "js_code" "kotlin.println" (func (param externref)))
  (import "js_code" "app.flat.dropped" (func (param i32)))
  (func (export "f") (param externref) (result externref) (call $url (local.get 0)))
  (@custom "name-ish" "kept")
)"#;

#[test]
fn renames_listed_imports_only() {
    let wasm = wat::parse_str(MODULE).unwrap();
    let links = read_links(
        "js_code\tapp.flat.Request_get_url\ttw:js/global\tRequest#get:url()\n\njs_code\tapp.flat.gone\ttw:js/global\tgone()\n",
    )
    .unwrap();
    let out = link(&wasm, &links).unwrap();
    wasmparser::Validator::new().validate_all(&out).unwrap();
    assert_eq!(
        imports(&out),
        [
            ("tw:js/global".into(), "Request#get:url()".into()),
            ("js_code".into(), "kotlin.println".into()),
            ("js_code".into(), "app.flat.dropped".into()),
        ]
    );
    // Everything but the import names is unchanged.
    let back =
        read_links("tw:js/global\tRequest#get:url()\tjs_code\tapp.flat.Request_get_url\n").unwrap();
    assert_eq!(link(&out, &back).unwrap(), wasm);
}

#[test]
fn rejects_malformed_links() {
    let e = read_links("js_code\tonly-two\n").unwrap_err().to_string();
    assert!(
        e.contains("line 1: expected 4 tab-separated fields, got 2"),
        "{e}"
    );
    let e = read_links("js_code\ta\tm\tx\njs_code\ta\tm\ty\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("line 2"), "{e}");
    assert!(read_links("js_code\ta\tm\tx\njs_code\ta\tm\tx\n").is_ok());
}

#[test]
fn ignores_embedder_imports_it_cannot_type() {
    // Kotlin's own js_code imports take its GC structs.
    let lang = wat::parse_str(
        r#"(module
  (import "js_code" "kotlin.internal" (func (param structref)))
  (import "tw:js/global" "f()" (func))
)"#,
    )
    .unwrap();
    let ir = ts_gen_abi::text::parse("ir 1;\nf();\n").unwrap();
    let p = ts_gen_abi::profile::Profile::all_host();
    ts_gen_bridge::generate(&ir, &p, &lang).unwrap();
    let bad = wat::parse_str(
        r#"(module
  (import "tw:js/global" "f()" (func (param structref)))
)"#,
    )
    .unwrap();
    let e = ts_gen_bridge::generate(&ir, &p, &bad)
        .err()
        .unwrap()
        .to_string();
    assert!(
        e.contains("(\"tw:js/global\", \"f()\"): language module: the heap type"),
        "{e}"
    );
}
