//! External type mapping: resolve imported types to Rust paths.
//!
//! When an imported type can't be resolved by parsing its source file,
//! the external map provides the Rust path to use instead.
//!
//! ## CLI format
//!
//! ```text
//! --external "node:*=node_sys::*"              # wildcard module mapping
//! --external "Blob=::web_sys::Blob"            # explicit type mapping
//! --external "node:buffer=node_buffer_sys"     # specific module mapping
//! --external "Intl.Locale=my_intl::Locale"     # qualified type mapping
//! --external "Intl.*=my_intl::*"               # wildcard namespace mapping
//! ```
//!
//! Multiple mappings can be comma-separated or specified with multiple flags.

use std::collections::HashMap;

/// A resolved Rust path for an external type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustPath {
    /// The full Rust path (e.g. `::web_sys::Blob`, `node_sys::buffer::Blob`)
    pub path: String,
}

/// Maps TypeScript module specifiers and type names to Rust paths.
#[derive(Clone, Debug, Default)]
pub struct ExternalMap {
    /// Explicit type mappings from `--external`: `"Blob" → "::web_sys::Blob"`,
    /// or by qualified path, `"Intl.Locale" → "my_intl::Locale"`.
    type_map: HashMap<String, String>,
    /// Module mappings: `"node:buffer" → "node_buffer_sys"`
    module_map: HashMap<String, String>,
    /// Wildcard module mappings: `"node:" → "node_sys"`
    /// Stored as (prefix, rust_crate) pairs.
    wildcard_map: Vec<(String, String)>,
    /// Wildcard namespace mappings: `"Intl." → "my_intl"`, longest prefix
    /// first. They map qualified type paths only.
    namespace_wildcard_map: Vec<(String, String)>,
    /// Built-in defaults (web platform types → `::web_sys::...`, the
    /// `js_sys` names whose Rust spelling differs from TS, and the lib's
    /// `Intl.<Name>` types → `::js_sys::Intl::<Name>`). Checked last,
    /// so any user-supplied mapping (explicit or module-based) wins.
    /// Populated by `new()`; cleared in test contexts where bare
    /// behaviour is needed.
    default_type_map: HashMap<String, String>,
}

/// `js_sys` classes whose Rust name is not the TS name, so the
/// `use js_sys::*` glob cannot supply them under the TS spelling.
pub const JS_SYS_RENAMES: &[(&str, &str)] = &[("URIError", "::js_sys::UriError")];

/// The `js_sys::Intl` types, which TypeScript's lib declares as members of
/// the global `Intl` namespace: `Intl.<Name>` defaults to
/// `::js_sys::Intl::<Name>`. Like the renames, these need nothing beyond
/// js-sys and survive `--no-web-sys`. js-sys's string enums
/// (`Intl::LocaleMatcher`, …) are left out: TypeScript spells those as
/// literal unions, which lower on their own.
pub const JS_SYS_INTL: &[&str] = &[
    "Collator",
    "CollatorOptions",
    "DateTimeFormat",
    "DateTimeFormatOptions",
    "DateTimeFormatPart",
    "DateTimeRangeFormatPart",
    "DisplayNames",
    "DisplayNamesOptions",
    "Duration",
    "DurationFormat",
    "DurationFormatOptions",
    "DurationFormatPart",
    "ListFormat",
    "ListFormatOptions",
    "ListFormatPart",
    "Locale",
    "LocaleMatcherOptions",
    "NumberFormat",
    "NumberFormatOptions",
    "NumberFormatPart",
    "NumberRangeFormatPart",
    "PluralRules",
    "PluralRulesOptions",
    "RelativeTimeFormat",
    "RelativeTimeFormatOptions",
    "RelativeTimeFormatPart",
    "ResolvedCollatorOptions",
    "ResolvedDateTimeFormatOptions",
    "ResolvedDisplayNamesOptions",
    "ResolvedDurationFormatOptions",
    "ResolvedListFormatOptions",
    "ResolvedNumberFormatOptions",
    "ResolvedPluralRulesOptions",
    "ResolvedRelativeTimeFormatOptions",
    "ResolvedSegmenterOptions",
    "SegmentData",
    "Segmenter",
    "SegmenterOptions",
    "Segments",
    "TextInfo",
    "WeekInfo",
];

/// Default mappings for the web platform types declared in the TS lib.
/// Anything in this list resolves to its `web_sys::` equivalent unless the
/// user provides an explicit `--external` override (which wins via
/// `add_mapping` overwriting the entry).
pub const WEB_SYS_DEFAULTS: &[(&str, &str)] = &[
    ("AbortController", "::web_sys::AbortController"),
    ("AbortSignal", "::web_sys::AbortSignal"),
    ("Blob", "::web_sys::Blob"),
    ("Crypto", "::web_sys::Crypto"),
    ("CryptoKey", "::web_sys::CryptoKey"),
    ("DOMException", "::web_sys::DomException"),
    ("Event", "::web_sys::Event"),
    ("EventTarget", "::web_sys::EventTarget"),
    ("File", "::web_sys::File"),
    ("FormData", "::web_sys::FormData"),
    ("Headers", "::web_sys::Headers"),
    ("ReadableStream", "::web_sys::ReadableStream"),
    ("Request", "::web_sys::Request"),
    ("Response", "::web_sys::Response"),
    ("SubtleCrypto", "::web_sys::SubtleCrypto"),
    ("TextDecoder", "::web_sys::TextDecoder"),
    ("TextEncoder", "::web_sys::TextEncoder"),
    ("TransformStream", "::web_sys::TransformStream"),
    ("URL", "::web_sys::Url"),
    ("URLSearchParams", "::web_sys::UrlSearchParams"),
    ("WebSocket", "::web_sys::WebSocket"),
    ("Worker", "::web_sys::Worker"),
    ("WritableStream", "::web_sys::WritableStream"),
];

impl ExternalMap {
    pub fn new() -> Self {
        let mut map = Self::default();
        for &(name, path) in WEB_SYS_DEFAULTS.iter().chain(JS_SYS_RENAMES) {
            map.default_type_map
                .insert(name.to_string(), path.to_string());
        }
        for name in JS_SYS_INTL {
            map.default_type_map
                .insert(format!("Intl.{name}"), format!("::js_sys::Intl::{name}"));
        }
        map
    }

    /// Construct a map with no defaults — for tests that need to verify
    /// the user-added precedence in isolation.
    pub fn empty_for_test() -> Self {
        Self::default()
    }

    /// Drop the built-in web platform defaults. Used by the
    /// `--no-web-sys` CLI flag for environments that don't link
    /// `web_sys` (Node-only runtimes, custom JS hosts, …). The `js_sys`
    /// renames and `Intl` types stay: they need nothing beyond js-sys.
    pub fn clear_defaults(&mut self) {
        for &(name, _) in WEB_SYS_DEFAULTS {
            self.default_type_map.remove(name);
        }
    }

    /// Parse a CLI external mapping string.
    ///
    /// Format: `"LHS=RHS"` where:
    /// - `"Blob=::web_sys::Blob"` → explicit type map
    /// - `"node:buffer=node_buffer_sys"` → module map
    /// - `"node:*=node_sys::*"` → wildcard module map
    /// - `"Intl.Locale=my_intl::Locale"` → explicit type map, by qualified path
    /// - `"Intl.*=my_intl::*"` → wildcard namespace map
    pub fn add_mapping(&mut self, mapping: &str) {
        let Some((lhs, rhs)) = mapping.split_once('=') else {
            return;
        };
        let lhs = lhs.trim();
        let rhs = rhs.trim();

        let is_module = |lhs: &str| lhs.contains(':') || lhs.contains('/');
        if lhs.ends_with('*') && rhs.ends_with('*') {
            let prefix = lhs.trim_end_matches('*');
            let rust_prefix = rhs.trim_end_matches('*').trim_end_matches("::");
            // A namespace path ("Intl.*") or a module prefix ("node:*").
            let wildcards = if prefix.ends_with('.') && !is_module(prefix) {
                &mut self.namespace_wildcard_map
            } else {
                &mut self.wildcard_map
            };
            wildcards.push((prefix.to_string(), rust_prefix.to_string()));
            // Sort by prefix length descending so longer (more specific) prefixes match first
            wildcards.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
        } else if is_module(lhs) {
            // Module mapping: "node:buffer=node_buffer_sys"
            self.module_map.insert(lhs.to_string(), rhs.to_string());
        } else {
            // Explicit type: "Blob=::web_sys::Blob"
            self.type_map.insert(lhs.to_string(), rhs.to_string());
        }
    }

    /// Parse multiple mappings from a comma-separated string.
    pub fn add_mappings(&mut self, mappings: &str) {
        for mapping in mappings.split(',') {
            let mapping = mapping.trim();
            if !mapping.is_empty() {
                self.add_mapping(mapping);
            }
        }
    }

    /// Resolve a type name imported from a module to a Rust path.
    ///
    /// Returns `None` if no mapping exists (caller should fall back to JsValue).
    pub fn resolve(&self, type_name: &str, from_module: &str) -> Option<RustPath> {
        // 1. User-supplied explicit type map (highest priority)
        if let Some(rust_path) = self.type_map.get(type_name) {
            return Some(RustPath {
                path: rust_path.clone(),
            });
        }

        // 2. Specific module map
        if let Some(rust_crate) = self.module_map.get(from_module) {
            return Some(RustPath {
                path: format!("{rust_crate}::{type_name}"),
            });
        }

        // 3. Wildcard module map
        for (prefix, rust_crate) in &self.wildcard_map {
            if from_module.starts_with(prefix) {
                let module_suffix = &from_module[prefix.len()..];
                let rust_module = module_suffix.replace('/', "::");
                if rust_module.is_empty() {
                    return Some(RustPath {
                        path: format!("{rust_crate}::{type_name}"),
                    });
                } else {
                    return Some(RustPath {
                        path: format!("{rust_crate}::{rust_module}::{type_name}"),
                    });
                }
            }
        }

        // 4. Built-in default (web_sys) — last resort.
        self.default_type_map
            .get(type_name)
            .map(|rust_path| RustPath {
                path: rust_path.clone(),
            })
    }

    /// Resolve a type name without a known module (explicit type map only).
    pub fn resolve_type(&self, type_name: &str) -> Option<RustPath> {
        self.type_map
            .get(type_name)
            .or_else(|| self.default_type_map.get(type_name))
            .map(|rust_path| RustPath {
                path: rust_path.clone(),
            })
    }

    /// Resolve a qualified type path (`Intl.Locale`) that names no
    /// declaration: an explicit mapping of the path, then the longest
    /// wildcard namespace mapping covering it (`Intl.*=my_intl::*` maps
    /// `Intl.Locale` to `my_intl::Locale`, and a nested `Intl.A.B` to
    /// `my_intl::A::B`), then the built-in defaults.
    pub fn resolve_qualified(&self, path: &str) -> Option<RustPath> {
        if let Some(rust_path) = self.type_map.get(path) {
            return Some(RustPath {
                path: rust_path.clone(),
            });
        }
        for (prefix, rust_prefix) in &self.namespace_wildcard_map {
            if let Some(rest) = path.strip_prefix(prefix.as_str()) {
                return Some(RustPath {
                    path: format!("{rust_prefix}::{}", rest.replace('.', "::")),
                });
            }
        }
        self.default_type_map.get(path).map(|rust_path| RustPath {
            path: rust_path.clone(),
        })
    }

    /// Check if any mappings have been configured.
    pub fn is_empty(&self) -> bool {
        self.type_map.is_empty()
            && self.module_map.is_empty()
            && self.wildcard_map.is_empty()
            && self.namespace_wildcard_map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explicit_type_mapping() {
        let mut map = ExternalMap::new();
        map.add_mapping("Blob=::web_sys::Blob");

        let result = map.resolve("Blob", "node:buffer");
        assert_eq!(result.unwrap().path, "::web_sys::Blob");

        // Unknown type returns None
        assert!(map.resolve("Unknown", "node:buffer").is_none());
    }

    #[test]
    fn test_module_mapping() {
        let mut map = ExternalMap::new();
        map.add_mapping("node:buffer=node_buffer_sys");

        let result = map.resolve("Blob", "node:buffer");
        assert_eq!(result.unwrap().path, "node_buffer_sys::Blob");

        // Different module not mapped
        assert!(map.resolve("Foo", "node:http").is_none());
    }

    #[test]
    fn test_wildcard_mapping() {
        let mut map = ExternalMap::new();
        map.add_mapping("node:*=node_sys::*");

        let result = map.resolve("Blob", "node:buffer");
        assert_eq!(result.unwrap().path, "node_sys::buffer::Blob");

        let result2 = map.resolve("Server", "node:http");
        assert_eq!(result2.unwrap().path, "node_sys::http::Server");
    }

    #[test]
    fn test_explicit_overrides_wildcard() {
        let mut map = ExternalMap::new();
        map.add_mapping("node:*=node_sys::*");
        map.add_mapping("Blob=::web_sys::Blob");

        // Explicit wins
        let result = map.resolve("Blob", "node:buffer");
        assert_eq!(result.unwrap().path, "::web_sys::Blob");

        // Wildcard for non-explicit types
        let result2 = map.resolve("Buffer", "node:buffer");
        assert_eq!(result2.unwrap().path, "node_sys::buffer::Buffer");
    }

    #[test]
    fn test_comma_separated() {
        let mut map = ExternalMap::new();
        map.add_mappings("Blob=::web_sys::Blob, node:*=node_sys::*");

        assert_eq!(
            map.resolve("Blob", "node:buffer").unwrap().path,
            "::web_sys::Blob"
        );
        assert_eq!(
            map.resolve("Server", "node:http").unwrap().path,
            "node_sys::http::Server"
        );
    }

    #[test]
    fn test_web_sys_default_resolves_known_types() {
        let map = ExternalMap::new();
        assert_eq!(
            map.resolve("Blob", "irrelevant").unwrap().path,
            "::web_sys::Blob"
        );
        assert_eq!(
            map.resolve_type("Headers").unwrap().path,
            "::web_sys::Headers"
        );
        assert!(map.resolve_type("NotABuiltin").is_none());
    }

    #[test]
    fn test_user_explicit_overrides_default() {
        let mut map = ExternalMap::new();
        map.add_mapping("Blob=my_crate::CustomBlob");
        assert_eq!(
            map.resolve("Blob", "any").unwrap().path,
            "my_crate::CustomBlob"
        );
    }

    #[test]
    fn test_user_module_map_overrides_default() {
        // A user mapping `node:buffer=node_buffer_sys` should win over
        // the web_sys default for `Blob`, since the user clearly meant
        // to source `Blob` from `node:buffer`.
        let mut map = ExternalMap::new();
        map.add_mapping("node:buffer=node_buffer_sys");
        assert_eq!(
            map.resolve("Blob", "node:buffer").unwrap().path,
            "node_buffer_sys::Blob"
        );
    }

    #[test]
    fn test_clear_defaults_disables_web_sys() {
        let mut map = ExternalMap::new();
        map.clear_defaults();
        assert!(map.resolve("Blob", "any").is_none());
        assert!(map.resolve_type("Headers").is_none());
        assert_eq!(
            map.resolve_type("URIError").unwrap().path,
            "::js_sys::UriError"
        );
        assert_eq!(
            map.resolve_qualified("Intl.Locale").unwrap().path,
            "::js_sys::Intl::Locale"
        );
    }

    #[test]
    fn test_qualified_defaults_and_overrides() {
        let mut map = ExternalMap::new();
        assert_eq!(
            map.resolve_qualified("Intl.DateTimeFormatOptions")
                .unwrap()
                .path,
            "::js_sys::Intl::DateTimeFormatOptions"
        );
        // Only the qualified path maps: a bare `Locale` is another name.
        assert!(map.resolve_type("Locale").is_none());
        assert!(map.resolve_qualified("Intl.Missing").is_none());

        map.add_mapping("Intl.DateTimeFormatOptions=my::Opts");
        assert_eq!(
            map.resolve_qualified("Intl.DateTimeFormatOptions")
                .unwrap()
                .path,
            "my::Opts"
        );
        assert_eq!(
            map.resolve_qualified("Intl.Locale").unwrap().path,
            "::js_sys::Intl::Locale"
        );
    }

    #[test]
    fn test_namespace_wildcard() {
        let mut map = ExternalMap::new();
        map.add_mapping("Intl.*=my_intl::*");
        map.add_mapping("Intl.Locale=my::Locale");
        // Explicit beats the wildcard, which beats the defaults.
        assert_eq!(
            map.resolve_qualified("Intl.Locale").unwrap().path,
            "my::Locale"
        );
        assert_eq!(
            map.resolve_qualified("Intl.DateTimeFormatOptions")
                .unwrap()
                .path,
            "my_intl::DateTimeFormatOptions"
        );
        assert_eq!(
            map.resolve_qualified("Intl.A.B").unwrap().path,
            "my_intl::A::B"
        );
        assert!(map.resolve_qualified("Other.Locale").is_none());
        // A namespace wildcard isn't a module prefix.
        assert!(map.resolve("Locale", "Intl.x").is_none());
    }

    #[test]
    fn test_subpath_wildcard() {
        let mut map = ExternalMap::new();
        map.add_mapping("node:*=node_sys::*");

        let result = map.resolve("ReadableStream", "node:stream/web");
        assert_eq!(
            result.unwrap().path,
            "node_sys::stream::web::ReadableStream"
        );
    }
}
