//! JS name → Rust name conversion utilities.

use std::collections::HashSet;

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
/// Rust module path for a JS module specifier, one snake_case segment per
/// `:` or `/` component: `node:fs/promises` → `node::fs::promises`.
/// Components without an identifier character, such as the `*` of
/// `assets:*`, are dropped.
pub fn module_path(specifier: &str) -> Vec<String> {
    specifier
        .split([':', '/'])
        .map(|component| {
            let component: String = component
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
                .collect();
            to_snake_case(&component)
        })
        .filter(|segment| !segment.is_empty())
        .collect()
}

pub fn to_snake_case(name: &str) -> String {
    if let Some(words) = symbol_words(name) {
        return words.join("_");
    }
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

/// The Rust identifier a JS name binds under: its `snake_case` form with
/// the characters a Rust identifier can't hold removed (`$data` → `data`,
/// `@cf/model` → `cfmodel`). Keywords and a leading digit are left to
/// `make_ident`, after prefixes like `set_` are composed.
pub(crate) fn snake_ident(js_name: &str) -> String {
    ident_chars(&to_snake_case(js_name))
}

/// The characters of `name` a Rust identifier can hold.
pub(crate) fn ident_chars(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Convert a JS identifier to a Rust PascalCase name (for types, enums).
pub fn to_pascal_case(name: &str) -> String {
    match symbol_words(name) {
        Some(words) => words.join("_").to_case(Case::Pascal),
        None => name.to_case(Case::Pascal),
    }
}

/// Spell out the symbols of a name with no alphanumeric characters, which
/// would otherwise sanitise to nothing: `$$` → `["dollar", "dollar"]`, so
/// `$` and `$$` get usable, distinct names.
pub(crate) fn symbol_words(name: &str) -> Option<Vec<&'static str>> {
    if name.chars().any(char::is_alphanumeric) {
        return None;
    }
    let words: Vec<_> = name.chars().filter_map(symbol_word).collect();
    (!words.is_empty()).then_some(words)
}

fn symbol_word(c: char) -> Option<&'static str> {
    Some(match c {
        '$' => "dollar",
        '_' => "underscore",
        '@' => "at",
        '#' => "hash",
        '%' => "percent",
        '&' => "amp",
        '*' => "star",
        '+' => "plus",
        '-' => "minus",
        '.' => "dot",
        '/' => "slash",
        ':' => "colon",
        '<' => "lt",
        '=' => "eq",
        '>' => "gt",
        '?' => "question",
        '!' => "bang",
        '^' => "caret",
        '|' => "pipe",
        '~' => "tilde",
        _ => return None,
    })
}

/// Convert a string literal to a PascalCase enum variant name.
/// Handles things like `"v8"` → `"V8"`, `"text"` → `"Text"`.
pub fn to_enum_variant(s: &str) -> String {
    let pascal = to_pascal_case(s);
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
pub fn dedup_names(names: &mut [String]) {
    let mut scope = LocalScope::default();
    for name in names.iter_mut() {
        *name = scope.declare(name, Suffix::Snake);
    }
}

/// How a claimed name is respelled when its candidate is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suffix {
    /// `Foo_`, `Foo_2`, …: a declared type moved aside from a prelude
    /// name, re-exported under its own name.
    Trailing,
    /// `foo_2`, `foo_3`, …: a further binding sharing a snake_case ident.
    Snake,
    /// `Foo2`, `Foo3`, …: a further synthesised PascalCase type.
    Pascal,
    /// `a2`, `a3`, …: a further parameter. wasm-bindgen splits each
    /// parameter `a` into `a_1` … `a_4` in the code it generates.
    Param,
}

/// The names claimed in one Rust scope.
///
/// Every identifier ts-gen invents, whether a type moved aside from the
/// `js_sys` glob, an overload or `try_` sibling inside an extern block, a
/// synthesised interface or dynamic-union enum, an enum variant or a
/// static, resolves collisions the same way: the first free candidate
/// against the scope's claimed set. Only the fallback spelling depends on
/// what kind of name is being claimed, which is the [`Suffix`].
#[derive(Debug, Default, Clone)]
pub struct LocalScope {
    claimed: HashSet<String>,
}

impl LocalScope {
    /// A scope with `names` already claimed.
    pub fn with_claimed<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            claimed: names.into_iter().map(Into::into).collect(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.claimed.contains(name)
    }

    /// Claim `name` exactly as spelled. Returns whether it was free.
    pub fn claim(&mut self, name: impl Into<String>) -> bool {
        self.claimed.insert(name.into())
    }

    /// Give a claimed name back, for a declaration that was named before
    /// it turned out not to be emitted.
    pub fn release(&mut self, name: &str) {
        self.claimed.remove(name);
    }

    /// Claim `candidate`, or the first free respelling of it under
    /// `suffix`, and return the name claimed.
    pub fn declare(&mut self, candidate: &str, suffix: Suffix) -> String {
        if self.claim(candidate) {
            return candidate.to_string();
        }
        if suffix == Suffix::Trailing {
            let trailing = format!("{candidate}_");
            if self.claim(trailing.clone()) {
                return trailing;
            }
        }
        for n in 2u32.. {
            let name = match suffix {
                Suffix::Trailing | Suffix::Snake => format!("{candidate}_{n}"),
                Suffix::Pascal | Suffix::Param => format!("{candidate}{n}"),
            };
            if self.claim(name.clone()) {
                return name;
            }
        }
        unreachable!("every scope has a free numbered name")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_scope_respells_by_suffix() {
        let mut scope = LocalScope::with_claimed(["Map", "Map_", "foo", "Kind", "Kind2"]);
        assert_eq!(scope.declare("Set", Suffix::Trailing), "Set");
        assert_eq!(scope.declare("Map", Suffix::Trailing), "Map_2");
        assert_eq!(scope.declare("foo", Suffix::Snake), "foo_2");
        assert_eq!(scope.declare("foo", Suffix::Snake), "foo_3");
        assert_eq!(scope.declare("Kind", Suffix::Pascal), "Kind3");
        assert!(scope.contains("Kind3"));
        assert!(!scope.claim("foo_3"));
    }

    #[test]
    fn module_path_splits_scheme_and_subpath() {
        let path = |spec: &str| module_path(spec).join("::");
        assert_eq!(path("node:fs"), "node::fs");
        assert_eq!(path("node:fs/promises"), "node::fs::promises");
        assert_eq!(path("cloudflare:email"), "cloudflare::email");
        assert_eq!(path("node:http2"), "node::http2");
        assert_eq!(path("es-module-lexer"), "es_module_lexer");
        assert_eq!(path("@scope/pkg/sub"), "scope::pkg::sub");
        assert_eq!(path("assets:*"), "assets");
        assert_eq!(path("worker_threads"), "worker_threads");
    }

    #[test]
    fn dedup_names_never_lands_on_a_later_name() {
        let mut names: Vec<String> = ["A", "A", "A_2"].map(String::from).into();
        dedup_names(&mut names);
        assert_eq!(names, ["A", "A_2", "A_2_2"]);
    }

    #[test]
    fn symbol_only_names_are_spelled_out() {
        assert_eq!(to_snake_case("$"), "dollar");
        assert_eq!(to_snake_case("$$"), "dollar_dollar");
        assert_eq!(to_snake_case("_"), "underscore");
        assert_eq!(to_snake_case("$foo"), "$foo");
        assert_eq!(to_pascal_case("$_"), "DollarUnderscore");
        assert_eq!(to_enum_variant("$"), "Dollar");
        assert_eq!(to_enum_variant("_"), "Underscore");
        assert_eq!(to_enum_variant(""), "Empty");
        assert_eq!(to_snake_case("☃"), "☃");
    }

    #[test]
    fn snake_ident_strips_invalid_characters() {
        assert_eq!(snake_ident("$data"), "data");
        assert_eq!(snake_ident("@cf/model"), "cfmodel");
        assert_eq!(snake_ident("fooBar"), "foo_bar");
        assert_eq!(snake_ident("$$"), "dollar_dollar");
    }

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
