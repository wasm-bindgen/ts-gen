//! JS name → Rust name conversion utilities.

use convert_case::{Case, Casing};

/// Convert a TS module specifier (`"cloudflare:email"`, `"node:url"`,
/// `"some/sub/path"`) into the snake_case Rust identifier we use to
/// wrap that module's declarations in `pub mod <ident> { ... }`.
///
/// Strips any protocol prefix (everything before the last `:`), then
/// replaces `/` with `_` and `*` with `star`, and snake-cases the result.
/// Codegen and qualification both go through here so they stay in sync.
pub fn module_specifier_to_ident(specifier: &str) -> String {
    let short = specifier
        .rsplit_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(specifier);
    to_snake_case(&short.replace('/', "_").replace('*', "star"))
}

/// Convert a JS identifier to a Rust snake_case name (for functions, methods, variables).
///
/// Does NOT escape Rust keywords — that's handled by `make_ident` at the
/// codegen boundary using `r#` raw identifiers, so that name composition
/// (prefixes like `try_`, `set_`, suffixes like `_with_foo`) works correctly.
/// Convert a JS identifier to Rust `snake_case`.
///
/// Departs from `convert_case::Case::Snake` in one place: digits
/// don't introduce a word boundary when they trail an alphabetic
/// run. So `Float32Array` → `float32_array` (not `float_32_array`),
/// `Int8Array` → `int8_array`, `Uint8ClampedArray` →
/// `uint8_clamped_array`. This matches the historical wasm-bindgen /
/// `js_sys` naming convention for typed arrays and produces more
/// readable identifiers in general.
///
/// We keep the digit-as-boundary behavior in the *other* direction
/// — a digit followed by a letter still introduces a boundary
/// (`HTML5Element` → `html5_element` would still split the `5e`,
/// matching the standard convention).
pub fn to_snake_case(name: &str) -> String {
    // First pass through `convert_case::Case::Snake` to do the
    // standard splits (camelCase boundaries, runs of caps, etc.).
    let standard = name.to_case(Case::Snake);

    // Then merge any `<word>_<digits>(_|$)` back into `<word><digits>`.
    // The trailing context (`_` or end) ensures we only merge digits
    // that originally trailed a letter run, not standalone numeric
    // segments.
    let chars: Vec<char> = standard.chars().collect();
    let mut out = String::with_capacity(standard.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Look for `<letter>_<digits>` and elide the `_`.
        if c.is_alphabetic() && chars.get(i + 1) == Some(&'_') {
            let mut j = i + 2;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            // Only merge when the digit run is followed by a word
            // boundary (`_` or end) — otherwise it's a separate
            // numeric segment we want to keep separated.
            let merged_ok = j > i + 2 && (j == chars.len() || chars[j] == '_');
            if merged_ok {
                out.push(c);
                out.extend(&chars[i + 2..j]);
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Convert a JS identifier to a Rust PascalCase name (for types, enums).
pub fn to_pascal_case(name: &str) -> String {
    name.to_case(Case::Pascal)
}

/// Convert a string literal to a PascalCase enum variant name.
/// Handles things like `"v8"` → `"V8"`, `"text"` → `"Text"`.
pub fn to_enum_variant(s: &str) -> String {
    // Special-case: if the string is all lowercase/digits, just PascalCase it
    let pascal = s.to_case(Case::Pascal);
    if pascal.is_empty() {
        "Empty".to_string()
    } else {
        pascal
    }
}

/// snake_case identifier fragment for a number literal: `1`, `0_5`, `neg_1`.
pub(crate) fn number_literal_snake(n: f64) -> String {
    let abs = n.abs().to_string().replace('.', "_");
    if n < 0.0 {
        format!("neg_{abs}")
    } else {
        abs
    }
}

/// PascalCase enum variant for a number literal: `N1`, `N0_5`, `Neg1`.
pub(crate) fn number_literal_pascal(n: f64) -> String {
    let abs = n.abs().to_string().replace('.', "_");
    if n < 0.0 {
        format!("Neg{abs}")
    } else {
        format!("N{abs}")
    }
}

/// Deduplicate names in-place by appending `_2`, `_3`, etc. to collisions.
///
/// Takes a slice of `(name, setter)` pairs where `setter` is a closure that
/// updates the name on the original item. This avoids coupling to specific
/// enum variant types.
pub fn dedup_names(names: &mut [String]) {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for name in names.iter_mut() {
        let count = counts.entry(name.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            *name = format!("{name}_{count}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snake_case() {
        assert_eq!(to_snake_case("getUserById"), "get_user_by_id");
        assert_eq!(to_snake_case("HTMLElement"), "html_element");
        assert_eq!(to_snake_case("send"), "send");
    }

    #[test]
    fn test_snake_case_keeps_trailing_digits_with_word() {
        // Digits trailing a letter run stay attached. This matches
        // the historical wasm-bindgen typed-array naming convention.
        assert_eq!(to_snake_case("Float32Array"), "float32_array");
        assert_eq!(to_snake_case("Int8Array"), "int8_array");
        assert_eq!(to_snake_case("Uint8ClampedArray"), "uint8_clamped_array");
        assert_eq!(to_snake_case("BigInt64Array"), "big_int64_array");
        assert_eq!(to_snake_case("Float64"), "float64");
    }

    #[test]
    fn test_number_literal_names() {
        assert_eq!(number_literal_snake(1.0), "1");
        assert_eq!(number_literal_snake(0.5), "0_5");
        assert_eq!(number_literal_snake(-1.0), "neg_1");
        assert_eq!(number_literal_pascal(1.0), "N1");
        assert_eq!(number_literal_pascal(0.5), "N0_5");
        assert_eq!(number_literal_pascal(-1.0), "Neg1");
    }

    #[test]
    fn test_snake_case_non_ascii() {
        assert_eq!(to_snake_case("déjà"), "déjà");
        assert_eq!(to_snake_case("voilàLà2"), "voilà_là2");
    }

    #[test]
    fn test_pascal_case() {
        assert_eq!(to_pascal_case("readableStream"), "ReadableStream");
        assert_eq!(to_pascal_case("my_type"), "MyType");
    }

    #[test]
    fn test_keywords_not_escaped() {
        // Keywords are NOT escaped by to_snake_case — make_ident handles
        // them with r# at the codegen boundary, after name composition.
        assert_eq!(to_snake_case("type"), "type");
        assert_eq!(to_snake_case("match"), "match");
        assert_eq!(to_snake_case("return"), "return");
        assert_eq!(to_snake_case("raw"), "raw");
    }

    #[test]
    fn test_enum_variant() {
        assert_eq!(to_enum_variant("text"), "Text");
        assert_eq!(to_enum_variant("bytes"), "Bytes");
        assert_eq!(to_enum_variant("json"), "Json");
    }

    #[test]
    fn test_dedup_names_no_collision() {
        let mut names = vec!["Foo".to_string(), "Bar".to_string(), "Baz".to_string()];
        dedup_names(&mut names);
        assert_eq!(names, &["Foo", "Bar", "Baz"]);
    }

    #[test]
    fn test_dedup_names_collision() {
        // "text-plain" and "textPlain" both produce "TextPlain"
        let mut names = vec![
            "TextPlain".to_string(),
            "TextPlain".to_string(),
            "Other".to_string(),
        ];
        dedup_names(&mut names);
        assert_eq!(names, &["TextPlain", "TextPlain_2", "Other"]);
    }

    #[test]
    fn test_dedup_names_triple_collision() {
        let mut names = vec!["A".to_string(), "A".to_string(), "A".to_string()];
        dedup_names(&mut names);
        assert_eq!(names, &["A", "A_2", "A_3"]);
    }
}
