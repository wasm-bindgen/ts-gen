//! Extract JSDoc comments from the oxc comment list.
//!
//! oxc stores all comments in a flat sorted `Vec<Comment>` on `Program`.
//! Each `Comment` has an `attached_to` field — the byte offset of the token
//! the comment is leading. We match JSDoc (`/** ... */`) comments to AST
//! nodes by comparing `comment.attached_to` with `node.span.start`.

use oxc_ast::ast::Comment;

/// Structured JSDoc data extracted alongside the rendered doc comment.
///
/// Currently carries `@throws` types for callable members; `for_span` keeps
/// returning a bare `Option<String>` so non-callable callers don't have to
/// touch this struct.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JsDocInfo {
    /// Type names mentioned across all `@throws {T}` lines, deduped while
    /// preserving source order. Each entry is the raw identifier as written
    /// in the JSDoc (e.g. `"TypeError"`, `"ImagesError"`).
    ///
    /// Use [`Self::throws`] to convert into the standard pipeline
    /// representation ([`crate::ir::Throws`]) that codegen consumes.
    ///
    /// Recognized forms:
    /// * `@throws {TypeError} when foo` — single type
    /// * `@throws {TypeError | RangeError} when bar` — union
    /// * `@throws {@link ImagesError} if upload fails` — JSDoc link form
    /// * `@throws {never}` — declares the callable never throws (sets
    ///   [`Self::nothrow`]; the name itself is *not* added to this list)
    ///
    /// Pure-prose `@throws Sentence describing condition.` entries with no
    /// `{T}` annotation are silently ignored.
    pub throws: Vec<String>,

    /// `true` when an `@throws {never}` annotation was seen. Combined with
    /// an empty `throws` list this maps to [`crate::ir::Throws::Never`];
    /// if other named types also appeared, the `never` is ignored and the
    /// other types win (see [`Self::throws`]).
    pub nothrow: bool,
}

impl JsDocInfo {
    /// Convert the structured `@throws` info into a [`crate::ir::Throws`]
    /// for the IR.
    ///
    /// * No `@throws` info at all → [`Throws::None`]
    /// * `@throws {never}` alone → [`Throws::Never`]
    /// * Single named type → [`Throws::Type`] with `TypeRef::Named`
    /// * Multiple names → [`Throws::Type`] with `TypeRef::Union(...)`
    ///
    /// `@throws {never | OtherError}` resolves to `Throws::Type(OtherError)`
    /// — the `never` is dropped during parsing and only the residual
    /// type(s) drive codegen. This matches the principle that `T | never`
    /// is just `T` in the TypeScript type system.
    ///
    /// [`Throws::None`]: crate::ir::Throws::None
    /// [`Throws::Type`]: crate::ir::Throws::Type
    /// [`Throws::Never`]: crate::ir::Throws::Never
    pub fn throws(&self) -> crate::ir::Throws {
        use crate::ir::{Throws, TypeRef};
        match self.throws.len() {
            0 if self.nothrow => Throws::Never,
            0 => Throws::None,
            1 => Throws::Type(TypeRef::ident(self.throws[0].clone())),
            _ => Throws::Type(TypeRef::Union(
                self.throws.iter().map(TypeRef::ident).collect(),
            )),
        }
    }
}

/// Provides JSDoc lookup by span position.
pub struct DocComments<'a> {
    comments: &'a [Comment],
    source: &'a str,
}

impl<'a> DocComments<'a> {
    pub fn new(comments: &'a [Comment], source: &'a str) -> Self {
        Self { comments, source }
    }

    /// Find the JSDoc comment attached to the node starting at `span_start`.
    ///
    /// Returns the cleaned doc text (leading `*` and whitespace stripped per line),
    /// or `None` if no JSDoc is attached.
    pub fn for_span(&self, span_start: u32) -> Option<String> {
        self.info_for_span(span_start).map(|(doc, _)| doc)
    }

    /// Like [`for_span`] but also returns structured JSDoc info (e.g. `@throws`
    /// types). Callable converters that build `MethodMember` / `FunctionDecl`
    /// / `ConstructorMember` use this to capture throws annotations.
    ///
    /// [`for_span`]: Self::for_span
    pub fn info_for_span(&self, span_start: u32) -> Option<(String, JsDocInfo)> {
        // Find the last JSDoc comment attached to this position.
        // (There could be multiple leading comments; we want the JSDoc one closest to the node.)
        let jsdoc = self
            .comments
            .iter()
            .rev()
            .find(|c| c.attached_to == span_start && c.is_jsdoc())?;

        let content_span = jsdoc.content_span();
        let raw = &self.source[content_span.start as usize..content_span.end as usize];

        Some(clean_jsdoc_with_info(raw))
    }
}

/// Clean raw JSDoc content (between `/**` and `*/`) and convert to Rust doc conventions.
///
/// - Strips leading `*` and whitespace per line
/// - Converts `@param name - desc` → `* \`name\` - desc`
/// - Converts `@returns desc` → `Returns: desc`
/// - Converts `@example` blocks into fenced ` ```js ` code blocks
/// - Removes empty leading/trailing lines
#[cfg(test)]
fn clean_jsdoc(raw: &str) -> String {
    clean_jsdoc_with_info(raw).0
}

/// Like [`clean_jsdoc`] but also collects structured JSDoc info from the
/// content (currently `@throws` type names).
fn clean_jsdoc_with_info(raw: &str) -> (String, JsDocInfo) {
    let lines: Vec<&str> = raw.lines().collect();
    let mut cleaned: Vec<&str> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        // Strip leading `* ` or `*`
        let stripped = if let Some(rest) = trimmed.strip_prefix("* ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix('*') {
            rest
        } else {
            trimmed
        };
        cleaned.push(stripped);
    }

    // Remove empty leading and trailing lines
    while cleaned.first().is_some_and(|l| l.is_empty()) {
        cleaned.remove(0);
    }
    while cleaned.last().is_some_and(|l| l.is_empty()) {
        cleaned.pop();
    }

    convert_jsdoc_tags(&cleaned)
}

/// Convert JSDoc tags in cleaned lines to Rust doc conventions.
///
/// Collects description lines, `@param` entries, `@returns`, `@throws`, and
/// `@example` blocks, then re-emits them in idiomatic Rust doc order. Returns
/// the rendered doc plus structured info pulled from `@throws`.
fn convert_jsdoc_tags(lines: &[&str]) -> (String, JsDocInfo) {
    let mut description: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    let mut returns: Option<String> = None;
    let mut throws_lines: Vec<String> = Vec::new();
    let mut examples: Vec<Vec<String>> = Vec::new();
    let mut info = JsDocInfo::default();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        if let Some(rest) = line.strip_prefix("@param ") {
            // @param name - description  or  @param name description
            params.push(format_param(rest));
        } else if let Some(rest) = line
            .strip_prefix("@returns ")
            .or_else(|| line.strip_prefix("@return "))
        {
            returns = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("@throws ") {
            // Capture both the structured type info (for `info.throws`) and a
            // human-readable line for the Errors section in the rendered doc.
            //
            // `@throws {never}` lines never contribute to the rendered
            // `## Errors` section — by definition there are no errors to
            // document. The structured `nothrow` flag still gets set so
            // codegen can drop the fallible variants.
            let parsed = parse_throws_tag(rest);
            for ty in parsed.types {
                if !info.throws.contains(&ty) {
                    info.throws.push(ty);
                }
            }
            if parsed.nothrow {
                info.nothrow = true;
            } else {
                throws_lines.push(parsed.prose);
            }
        } else if line == "@example" {
            // Collect all lines until the next tag or end
            let mut code_lines = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].starts_with('@') {
                code_lines.push(lines[i].to_string());
                i += 1;
            }
            // Trim empty leading/trailing lines from example
            while code_lines.first().is_some_and(|l| l.is_empty()) {
                code_lines.remove(0);
            }
            while code_lines.last().is_some_and(|l| l.is_empty()) {
                code_lines.pop();
            }
            if !code_lines.is_empty() {
                examples.push(code_lines);
            }
            continue; // don't increment i again
        } else if line.starts_with('@') {
            // Unknown tag — pass through as-is
            description.push(line.to_string());
        } else {
            description.push(line.to_string());
        }

        i += 1;
    }

    // Build the output
    let mut out: Vec<String> = Vec::new();

    // Description
    out.extend(description);

    // Argument bullets
    if !params.is_empty() {
        if !out.is_empty() && !out.last().is_none_or(|l| l.is_empty()) {
            out.push(String::new());
        }
        for p in &params {
            out.push(p.clone());
        }
    }

    // Returns line
    if let Some(ret) = &returns {
        if !out.is_empty() && !out.last().is_none_or(|l| l.is_empty()) {
            out.push(String::new());
        }
        out.push(format!("Returns: {ret}"));
    }

    // Errors section — surfaces `@throws` lines so the rendered doc still
    // captures them even though the structured info is what drives codegen.
    if !throws_lines.is_empty() {
        if !out.is_empty() && !out.last().is_none_or(|l| l.is_empty()) {
            out.push(String::new());
        }
        out.push("## Errors".to_string());
        out.push(String::new());
        for line in &throws_lines {
            out.push(format!("* {line}"));
        }
    }

    // Examples
    for example in &examples {
        if !out.is_empty() && !out.last().is_none_or(|l| l.is_empty()) {
            out.push(String::new());
        }
        out.push("## Example".to_string());
        out.push(String::new());
        out.push("```js".to_string());
        for line in example {
            out.push(line.clone());
        }
        out.push("```".to_string());
    }

    // Trim trailing empty lines
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }

    (out.join("\n"), info)
}

/// Result of parsing one `@throws` tag's contents.
struct ParsedThrows {
    /// Named identifiers extracted from `{...}`, in source order. Empty if
    /// the tag was pure prose, only listed primitives, or only `never`.
    types: Vec<String>,
    /// `true` when the tag contained `never` and *no* other non-primitive
    /// type. `@throws {never | TypeError}` does *not* set this — the
    /// resulting throws is just `TypeError` (the `never` is absorbed).
    nothrow: bool,
    /// Human-readable line for the rendered Errors section.
    prose: String,
}

/// Parse the contents of an `@throws` tag.
///
/// Per the JSDoc spec, structured types live inside curly braces:
///
/// * `{TypeError} when foo` — single type
/// * `{TypeError | RangeError} when bar` — union
/// * `{@link ImagesError} if upload fails` — JSDoc link form (linked
///   identifier is taken as the type)
/// * `{never}` — declares the callable never throws (sets `nothrow`)
/// * `If the X does not exist, an error will be thrown.` — pure prose, no type
///
/// Primitive type names (`string`/`number`/etc.) inside `{...}` are *not*
/// added to `types` — they'd widen the LUB to `JsValue` anyway and aren't
/// resolvable to a Rust error type. `never` is its own special case: it
/// sets `nothrow` instead of contributing a type.
fn parse_throws_tag(rest: &str) -> ParsedThrows {
    let trimmed = rest.trim();

    if let Some(stripped) = trimmed.strip_prefix('{') {
        if let Some(end) = stripped.find('}') {
            let inner = stripped[..end].trim();
            let after = stripped[end + 1..].trim();

            // Handle `{@link Foo}` — keep just the linked name.
            let inner = inner.strip_prefix("@link ").map(str::trim).unwrap_or(inner);

            let mut names: Vec<String> = Vec::new();
            let mut saw_never = false;
            for raw in inner.split('|').map(str::trim) {
                if raw.is_empty() {
                    continue;
                }
                if raw == "never" {
                    saw_never = true;
                    continue;
                }
                if is_primitive_type_name(raw) {
                    continue;
                }
                names.push(raw.to_string());
            }

            // `nothrow` only fires when `never` was the sole non-empty arm.
            // `@throws {never | TypeError}` is just `@throws {TypeError}` —
            // `T | never` collapses to `T` in TS, and we mirror that.
            let nothrow = saw_never && names.is_empty();

            // Build a prose line for the rendered Errors section. Use the
            // raw inner text (with `{@link X}` collapsed to `X`) so unions
            // and link forms read naturally.
            let prose = if after.is_empty() {
                format!("`{inner}`")
            } else {
                format!("`{inner}` — {after}")
            };
            return ParsedThrows {
                types: names,
                nothrow,
                prose,
            };
        }
    }

    // No braces, or unmatched `{`: treat as pure prose with no structured type.
    ParsedThrows {
        types: Vec::new(),
        nothrow: false,
        prose: trimmed.to_string(),
    }
}

/// Names of TypeScript primitive types that should not be promoted to a
/// throws "type" — they widen the LUB to `JsValue` anyway and aren't
/// resolvable to a Rust error type.
fn is_primitive_type_name(s: &str) -> bool {
    matches!(
        s,
        "string"
            | "number"
            | "bigint"
            | "boolean"
            | "undefined"
            | "null"
            | "void"
            | "any"
            | "unknown"
            | "object"
            | "symbol"
            | "never"
    )
}

/// Format a `@param` rest string into a Rust-style argument list item.
///
/// Input forms:
/// - `name - description`
/// - `name description`
/// - `{type} name - description` (type is stripped)
///
/// Output: `* \`name\` - description`
fn format_param(rest: &str) -> String {
    let rest = rest.trim();

    // Strip optional JSDoc type annotation `{...}`
    let rest = if rest.starts_with('{') {
        if let Some(end) = rest.find('}') {
            rest[end + 1..].trim()
        } else {
            rest
        }
    } else {
        rest
    };

    // Split into name and description
    if let Some((name, desc)) = rest.split_once(" - ") {
        format!("* `{}` - {}", name.trim(), desc.trim())
    } else if let Some((name, desc)) = rest.split_once(' ') {
        format!("* `{}` - {}", name.trim(), desc.trim())
    } else {
        format!("* `{rest}`")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_single_line() {
        assert_eq!(
            clean_jsdoc(" A simple description "),
            "A simple description"
        );
    }

    #[test]
    fn test_clean_multi_line() {
        let raw = "\n * First line\n * Second line\n ";
        assert_eq!(clean_jsdoc(raw), "First line\nSecond line");
    }

    #[test]
    fn test_param_conversion() {
        let raw = "\n * Does a thing.\n * @param x - the value\n * @returns the result\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Does a thing.\n\n* `x` - the value\n\nReturns: the result"
        );
    }

    #[test]
    fn test_param_without_dash() {
        let raw = "\n * Hello.\n * @param source Source code to parse\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Hello.\n\n* `source` - Source code to parse"
        );
    }

    #[test]
    fn test_multiple_params() {
        let raw = "\n * Parse it.\n * @param source Source code\n * @param name Optional name\n * @returns The parsed result.\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Parse it.\n\n* `source` - Source code\n* `name` - Optional name\n\nReturns: The parsed result."
        );
    }

    #[test]
    fn test_example_block() {
        let raw = "\n * Do something.\n * @example\n * const x = foo();\n * console.log(x);\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Do something.\n\n## Example\n\n```js\nconst x = foo();\nconsole.log(x);\n```"
        );
    }

    #[test]
    fn test_multiple_examples() {
        let raw = "\n * Thing.\n * @example\n * foo();\n * @example\n * bar();\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Thing.\n\n## Example\n\n```js\nfoo();\n```\n\n## Example\n\n```js\nbar();\n```"
        );
    }

    #[test]
    fn test_param_with_jsdoc_type() {
        assert_eq!(
            format_param("{string} name - the name"),
            "* `name` - the name"
        );
    }

    #[test]
    fn test_description_only() {
        let raw = "\n * Just a description with `inline code`.\n ";
        assert_eq!(clean_jsdoc(raw), "Just a description with `inline code`.");
    }

    #[test]
    fn test_example_between_tags() {
        let raw = "\n * Desc.\n * @example\n * code();\n * @returns result\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Desc.\n\nReturns: result\n\n## Example\n\n```js\ncode();\n```"
        );
    }

    fn parse(raw: &str) -> (String, JsDocInfo) {
        clean_jsdoc_with_info(raw)
    }

    #[test]
    fn test_throws_single_braced_type() {
        let raw = "\n * Does X.\n * @throws {TypeError} when foo is bad\n ";
        let (doc, info) = parse(raw);
        assert_eq!(info.throws, vec!["TypeError"]);
        // The Errors section appears in the rendered doc with the raw type
        // and prose preserved.
        assert!(doc.contains("## Errors"));
        assert!(doc.contains("`TypeError` — when foo is bad"));
    }

    #[test]
    fn test_throws_union() {
        let raw = "\n * @throws {TypeError | RangeError} on bad input\n ";
        let (_doc, info) = parse(raw);
        assert_eq!(info.throws, vec!["TypeError", "RangeError"]);
    }

    #[test]
    fn test_throws_link_form() {
        let raw = "\n * @throws {@link ImagesError} if upload fails\n ";
        let (doc, info) = parse(raw);
        assert_eq!(info.throws, vec!["ImagesError"]);
        // The rendered doc uses the linked name without the `@link` marker.
        assert!(doc.contains("`ImagesError` — if upload fails"));
    }

    #[test]
    fn test_throws_multiple_lines_dedup_preserves_order() {
        // Multiple `@throws` lines accumulate into the same union, with
        // duplicates collapsed but order preserved across lines.
        let raw = "
                 * @throws {NotFoundError} if not found
                 * @throws {BadRequestError} if invalid
                 * @throws {NotFoundError} again
                 ";
        let (_doc, info) = parse(raw);
        assert_eq!(info.throws, vec!["NotFoundError", "BadRequestError"]);
    }

    #[test]
    fn test_throws_pure_prose_has_no_types() {
        let raw = "\n * @throws If the resource does not exist, an error is thrown.\n ";
        let (doc, info) = parse(raw);
        assert!(info.throws.is_empty());
        // Prose still surfaces in the rendered Errors section.
        assert!(doc.contains("## Errors"));
        assert!(doc.contains("If the resource does not exist"));
    }

    #[test]
    fn test_throws_primitives_filtered() {
        // Primitive types in the union don't make it into `info.throws`
        // since they aren't useful as a Rust error type.
        let raw = "\n * @throws {TypeError | string} bad input\n ";
        let (_doc, info) = parse(raw);
        assert_eq!(info.throws, vec!["TypeError"]);
        assert!(!info.nothrow);
    }

    #[test]
    fn test_throws_unmatched_brace_falls_back_to_prose() {
        // If `}` is missing we don't attempt structured parsing.
        let raw = "\n * @throws {TypeError if oops\n ";
        let (_doc, info) = parse(raw);
        assert!(info.throws.is_empty());
        assert!(!info.nothrow);
    }

    #[test]
    fn test_no_throws_yields_empty_info() {
        let raw = "\n * Just a description.\n ";
        let (_doc, info) = parse(raw);
        assert!(info.throws.is_empty());
        assert!(!info.nothrow);
    }

    #[test]
    fn test_throws_never_sets_nothrow() {
        // `@throws {never}` is the explicit "this never throws" annotation —
        // it sets `nothrow` and contributes no named type.
        let raw = "\n * Always succeeds.\n * @throws {never}\n ";
        let (_doc, info) = parse(raw);
        assert!(info.throws.is_empty());
        assert!(info.nothrow);
        assert_eq!(info.throws(), crate::ir::Throws::Never);
    }

    #[test]
    fn test_throws_never_omits_errors_section() {
        // `@throws {never}` is a "negative" annotation — it documents the
        // absence of failure modes. Surfacing it under an `## Errors`
        // heading would read backwards, so the rendered doc has no
        // Errors section at all.
        let raw = "\n * Always succeeds.\n * @throws {never} guaranteed by construction\n ";
        let (doc, info) = parse(raw);
        assert!(info.throws.is_empty());
        assert!(info.nothrow);
        assert!(!doc.contains("## Errors"));
        assert!(!doc.contains("never"));
    }

    #[test]
    fn test_throws_never_alongside_typed_throws_keeps_errors_section() {
        // If a callable has *both* `@throws {never}` and `@throws {T}`
        // lines (a degenerate but possible JSDoc), the typed one still
        // surfaces in the Errors section. The `never` line is dropped
        // from the prose, but only the never line — typed throws win.
        let raw = "
            * @throws {never} this branch never fires
            * @throws {TypeError} when foo is bad
            ";
        let (doc, info) = parse(raw);
        assert_eq!(info.throws, vec!["TypeError"]);
        assert!(info.nothrow); // we still saw the never marker
        assert!(doc.contains("## Errors"));
        assert!(doc.contains("`TypeError`"));
        assert!(!doc.contains("this branch never fires"));
    }

    #[test]
    fn test_throws_never_in_union_is_absorbed() {
        // `T | never` is just `T` in the TS type system — we mirror that
        // here. The `never` arm is silently dropped and the residual type
        // wins, with `nothrow` staying false.
        let raw = "\n * @throws {never | TypeError} sometimes\n ";
        let (_doc, info) = parse(raw);
        assert_eq!(info.throws, vec!["TypeError"]);
        assert!(!info.nothrow);
        assert_eq!(
            info.throws(),
            crate::ir::Throws::Type(crate::ir::TypeRef::ident("TypeError"))
        );
    }

    #[test]
    fn test_throws_never_only_after_primitives_filtered() {
        // `{never | string}` filters `string` and leaves `never` alone —
        // since no real types remain, this is treated as nothrow.
        let raw = "\n * @throws {never | string} dead code\n ";
        let (_doc, info) = parse(raw);
        assert!(info.throws.is_empty());
        assert!(info.nothrow);
    }
}
