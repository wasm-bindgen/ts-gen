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
    /// Use [`Self::throws_typeref`] to convert into the standard `TypeRef`
    /// representation that the rest of the pipeline understands.
    ///
    /// Recognized forms:
    /// * `@throws {TypeError} when foo` — single type
    /// * `@throws {TypeError | RangeError} when bar` — union
    /// * `@throws {@link ImagesError} if upload fails` — JSDoc link form
    ///
    /// Pure-prose `@throws Sentence describing condition.` entries with no
    /// `{T}` annotation are silently ignored.
    pub throws: Vec<String>,
}

impl JsDocInfo {
    /// Lift the `@throws` name list into a `TypeRef`:
    ///
    /// * Empty → `None`
    /// * Single name → `TypeRef::Named(...)`
    /// * Multiple → `TypeRef::Union(...)` of named entries
    ///
    /// The returned `TypeRef` is just a regular type from the pipeline's
    /// perspective — codegen's union-LUB rules apply uniformly to it.
    pub fn throws_typeref(&self) -> Option<crate::ir::TypeRef> {
        match self.throws.len() {
            0 => None,
            1 => Some(crate::ir::TypeRef::Named(self.throws[0].clone())),
            _ => Some(crate::ir::TypeRef::Union(
                self.throws
                    .iter()
                    .map(|n| crate::ir::TypeRef::Named(n.clone()))
                    .collect(),
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
/// - Converts `@param name - desc` → `# Arguments` section with `* \`name\` - desc`
/// - Converts `@returns desc` → `# Returns` section
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
            let (types, prose) = parse_throws_tag(rest);
            for ty in types {
                if !info.throws.contains(&ty) {
                    info.throws.push(ty);
                }
            }
            throws_lines.push(prose);
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

    // Arguments section
    if !params.is_empty() {
        // Add blank line separator if we have preceding content
        if !out.is_empty() && !out.last().is_none_or(|l| l.is_empty()) {
            out.push(String::new());
        }
        out.push("## Arguments".to_string());
        out.push(String::new());
        for p in &params {
            out.push(p.clone());
        }
    }

    // Returns section
    if let Some(ret) = &returns {
        if !out.is_empty() && !out.last().is_none_or(|l| l.is_empty()) {
            out.push(String::new());
        }
        out.push("## Returns".to_string());
        out.push(String::new());
        out.push(ret.clone());
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

/// Parse the contents of an `@throws` tag.
///
/// Returns `(type_names, prose)` where:
/// * `type_names` is the list of identifiers found between `{...}` (preserving
///   source order). Empty if the tag is pure prose with no braced type.
/// * `prose` is a short human-readable line for the rendered Errors section.
///
/// Per the JSDoc spec, structured types live inside curly braces:
///
/// * `{TypeError} when foo` — single type
/// * `{TypeError | RangeError} when bar` — union
/// * `{@link ImagesError} if upload fails` — JSDoc link form (linked
///   identifier is taken as the type)
/// * `If the X does not exist, an error will be thrown.` — pure prose, no type
///
/// Primitive type names (`string`/`number`/etc.) inside `{...}` are *not*
/// added to `type_names` — they'd widen the LUB to `JsValue` anyway and
/// aren't resolvable to a Rust error type.
fn parse_throws_tag(rest: &str) -> (Vec<String>, String) {
    let trimmed = rest.trim();

    if let Some(stripped) = trimmed.strip_prefix('{') {
        if let Some(end) = stripped.find('}') {
            let inner = stripped[..end].trim();
            let after = stripped[end + 1..].trim();

            // Handle `{@link Foo}` — keep just the linked name.
            let inner = inner.strip_prefix("@link ").map(str::trim).unwrap_or(inner);

            let names: Vec<String> = inner
                .split('|')
                .map(str::trim)
                .filter(|s| !s.is_empty() && !is_primitive_type_name(s))
                .map(String::from)
                .collect();

            // Build a prose line for the rendered Errors section. Use the
            // raw inner text (with `{@link X}` collapsed to `X`) so unions
            // and link forms read naturally.
            let prose = if after.is_empty() {
                format!("`{inner}`")
            } else {
                format!("`{inner}` — {after}")
            };
            return (names, prose);
        }
    }

    // No braces, or unmatched `{`: treat as pure prose with no structured type.
    (Vec::new(), trimmed.to_string())
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
            "Does a thing.\n\n## Arguments\n\n* `x` - the value\n\n## Returns\n\nthe result"
        );
    }

    #[test]
    fn test_param_without_dash() {
        let raw = "\n * Hello.\n * @param source Source code to parse\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Hello.\n\n## Arguments\n\n* `source` - Source code to parse"
        );
    }

    #[test]
    fn test_multiple_params() {
        let raw = "\n * Parse it.\n * @param source Source code\n * @param name Optional name\n * @returns The parsed result.\n ";
        assert_eq!(
            clean_jsdoc(raw),
            "Parse it.\n\n## Arguments\n\n* `source` - Source code\n* `name` - Optional name\n\n## Returns\n\nThe parsed result."
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
            "Desc.\n\n## Returns\n\nresult\n\n## Example\n\n```js\ncode();\n```"
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
    }

    #[test]
    fn test_throws_unmatched_brace_falls_back_to_prose() {
        // If `}` is missing we don't attempt structured parsing.
        let raw = "\n * @throws {TypeError if oops\n ";
        let (_doc, info) = parse(raw);
        assert!(info.throws.is_empty());
    }

    #[test]
    fn test_no_throws_yields_empty_info() {
        let raw = "\n * Just a description.\n ";
        let (_doc, info) = parse(raw);
        assert!(info.throws.is_empty());
    }
}
