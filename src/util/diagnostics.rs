//! Diagnostic messages for unsupported or partially-supported TS constructs.

use std::fmt::Write;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Source location for a diagnostic.
#[derive(Clone, Debug)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file.display(), self.line, self.col)
    }
}

/// A diagnostic emitted during parsing or code generation.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
    /// Optional source location.
    pub location: Option<SourceLocation>,
    /// Optional: the TS source text that triggered this diagnostic.
    pub source_text: Option<String>,
    /// Optional: the source line with the offending span underlined.
    pub frame: Option<CodeFrame>,
    pub help: Option<String>,
}

/// A single source line with a byte range of it to underline.
#[derive(Clone, Debug)]
pub struct CodeFrame {
    pub line: u32,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticLevel {
    /// A type could not be resolved — the output may be incorrect.
    Error,
    /// Something was skipped or simplified.
    Warning,
    /// Informational — the tool made a decision the user should know about.
    Info,
}

/// An input file registered with [`DiagnosticCollector::set_file`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileId(u32);

/// A byte range in a source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug)]
struct SourceFile {
    path: PathBuf,
    source: String,
    /// Byte offset of each line start, for O(log n) line lookup.
    line_offsets: Vec<u32>,
}

impl SourceFile {
    fn new(path: &Path, source: &str) -> Self {
        let mut line_offsets = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_offsets.push((i + 1) as u32);
            }
        }
        SourceFile {
            path: path.to_path_buf(),
            source: source.to_string(),
            line_offsets,
        }
    }

    /// The 1-indexed line and column of a byte offset.
    fn line_col(&self, offset: u32) -> (u32, u32) {
        let line_idx = match self.line_offsets.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line = (line_idx as u32) + 1;
        let col = offset - self.line_offsets[line_idx] + 1;
        (line, col)
    }

    fn location(&self, offset: u32) -> SourceLocation {
        let (line, col) = self.line_col(offset);
        SourceLocation {
            file: self.path.clone(),
            line,
            col,
        }
    }

    /// The frame is clamped to the line containing `start`.
    fn frame(&self, start: u32, end: u32, label: Option<String>) -> CodeFrame {
        let (line, _) = self.line_col(start);
        let line_start = self.line_offsets[line as usize - 1] as usize;
        let line_end = self.source[line_start..]
            .find('\n')
            .map_or(self.source.len(), |i| line_start + i);
        let text = self.source[line_start..line_end].trim_end_matches('\r');
        let start = (start as usize - line_start).min(text.len());
        let end = (end as usize)
            .saturating_sub(line_start)
            .clamp(start, text.len());
        CodeFrame {
            line,
            text: text.to_string(),
            start,
            end,
            label,
        }
    }
}

/// Collects diagnostics during a parse/codegen session.
///
/// Call `set_file` before processing each file. `warn_at`/`error_at` accept
/// byte offsets in it and compute line:col from the source. Every file's
/// source is kept, so a pass that runs once all files are parsed can point
/// into any of them with `warn_span_in`.
#[derive(Clone, Debug, Default)]
pub struct DiagnosticCollector {
    pub diagnostics: Vec<Diagnostic>,
    files: Vec<SourceFile>,
    /// The file currently being processed.
    current: Option<FileId>,
}

impl DiagnosticCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a file and its source text, and make it current for
    /// subsequent diagnostics.
    pub fn set_file(&mut self, path: &Path, source: &str) -> FileId {
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile::new(path, source));
        self.current = Some(id);
        id
    }

    /// Emit a warning with optional line/column.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            location: None,
            source_text: None,
            frame: None,
            help: None,
        });
    }

    /// Emit a warning at a byte offset in the current file.
    pub fn warn_at(&mut self, message: impl Into<String>, offset: u32) {
        let location = self.location(offset);
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            location,
            source_text: None,
            frame: None,
            help: None,
        });
    }

    pub fn warn_with_source(&mut self, message: impl Into<String>, source: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            location: None,
            source_text: Some(source.into()),
            frame: None,
            help: None,
        });
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Info,
            message: message.into(),
            location: None,
            source_text: None,
            frame: None,
            help: None,
        });
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            message: message.into(),
            location: None,
            source_text: None,
            frame: None,
            help: None,
        });
    }

    /// Emit an error at a byte offset in the current file.
    pub fn error_at(&mut self, message: impl Into<String>, offset: u32) {
        let location = self.location(offset);
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            message: message.into(),
            location,
            source_text: None,
            frame: None,
            help: None,
        });
    }

    /// Emit a warning at `start..end` in the current file, rendered with a
    /// code frame underlining the span.
    pub fn warn_span(
        &mut self,
        message: impl Into<String>,
        start: u32,
        end: u32,
        label: Option<String>,
        help: Option<String>,
    ) {
        self.warn_span_in(self.current, message, Span { start, end }, label, help);
    }

    /// Emit a warning at `span` in `file`, rendered with a code frame
    /// underlining the span.
    pub fn warn_span_in(
        &mut self,
        file: Option<FileId>,
        message: impl Into<String>,
        span: Span,
        label: Option<String>,
        help: Option<String>,
    ) {
        let file = file.map(|id| &self.files[id.0 as usize]);
        let location = file.map(|f| f.location(span.start));
        let frame = file.map(|f| f.frame(span.start, span.end, label));
        self.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            message: message.into(),
            location,
            source_text: None,
            frame,
            help,
        });
    }

    fn location(&self, offset: u32) -> Option<SourceLocation> {
        let file = &self.files[self.current?.0 as usize];
        Some(file.location(offset))
    }

    /// Print all diagnostics to stderr, deduplicating identical messages.
    /// Colour is used when stderr is a terminal and `NO_COLOR` is unset.
    pub fn emit(&self) {
        let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        eprint!("{}", self.render(color));
    }

    /// The text `emit` prints, with ANSI colour when `color` is set.
    pub fn render(&self, color: bool) -> String {
        let paint = |code: &str, text: &str| {
            if color && !text.is_empty() {
                format!("\x1b[{code}m{text}\x1b[0m")
            } else {
                text.to_string()
            }
        };
        let mut out = String::new();
        let mut seen = std::collections::HashSet::new();
        for diag in &self.diagnostics {
            let location = diag.location.as_ref().map(|l| l.to_string());
            let key = format!("{:?}:{location:?}:{}", diag.level, diag.message);
            if !seen.insert(key) {
                continue;
            }
            let (prefix, accent) = match diag.level {
                DiagnosticLevel::Error => ("error", "1;31"),
                DiagnosticLevel::Warning => ("warning", "1;33"),
                DiagnosticLevel::Info => ("info", "1;36"),
            };
            let _ = write!(out, "[ts-gen {}]: ", paint(accent, prefix));
            if let Some(loc) = location {
                let _ = write!(out, "{loc}: ");
            }
            let _ = writeln!(out, "{}", diag.message);
            if let Some(ref src) = diag.source_text {
                let _ = writeln!(out, "  source: {src}");
            }
            let gutter_width = diag.frame.as_ref().map_or(1, |f| f.line.to_string().len());
            let pad = " ".repeat(gutter_width);
            if let Some(ref frame) = diag.frame {
                let bar = paint("1;34", "|");
                let (before, rest) = frame.text.split_at(frame.start);
                let (span, after) = rest.split_at(frame.end - frame.start);
                // Tabs are kept so the arrow lines up with the source as rendered.
                let indent: String = before
                    .chars()
                    .map(|c| if c == '\t' { '\t' } else { ' ' })
                    .collect();
                let arrow = "^".repeat(span.chars().count().max(1));
                let label = frame
                    .label
                    .as_deref()
                    .map_or(String::new(), |l| format!(" {l}"));
                let _ = writeln!(out, "{pad} {bar}");
                let _ = writeln!(
                    out,
                    "{} {bar} {before}{}{after}",
                    paint("1;34", &frame.line.to_string()),
                    paint(accent, span),
                );
                let _ = writeln!(
                    out,
                    "{pad} {bar} {indent}{}",
                    paint(accent, &format!("{arrow}{label}"))
                );
            }
            if let Some(ref help) = diag.help {
                let _ = writeln!(out, "{pad} = {}: {help}", paint("1", "help"));
            }
        }
        out
    }

    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.level == DiagnosticLevel::Warning)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.level == DiagnosticLevel::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collector(source: &str) -> DiagnosticCollector {
        let mut diag = DiagnosticCollector::new();
        diag.set_file(Path::new("a.d.ts"), source);
        diag
    }

    #[test]
    fn frame_underlines_span_with_label_and_help() {
        let mut diag = collector("let a;\n\tfoo bar;\n");
        diag.warn_span(
            "Bad",
            12,
            15,
            Some("here".to_string()),
            Some("fix it".to_string()),
        );
        assert_eq!(
            diag.render(false),
            "\
[ts-gen warning]: a.d.ts:2:6: Bad
  |
2 | \tfoo bar;
  | \t    ^^^ here
  = help: fix it
"
        );
    }

    #[test]
    fn frame_clamps_multiline_span_to_first_line() {
        let mut diag = collector("ab\ncd\n");
        diag.warn_span("Bad", 1, 5, None, None);
        assert_eq!(
            diag.render(false),
            "[ts-gen warning]: a.d.ts:1:2: Bad\n  |\n1 | ab\n  |  ^\n"
        );
    }

    #[test]
    fn frame_colours_span_and_arrow() {
        let mut diag = collector("ab\n");
        diag.warn_span("Bad", 1, 2, None, None);
        let out = diag.render(true);
        assert!(out.contains("a\x1b[1;33mb\x1b[0m"));
        assert!(out.contains("\x1b[1;33m^\x1b[0m"));
    }

    #[test]
    fn dedup_keeps_same_message_at_distinct_locations() {
        let mut diag = collector("a\nb\n");
        diag.warn_at("Bad", 0);
        diag.warn_at("Bad", 0);
        diag.warn_at("Bad", 2);
        assert_eq!(diag.render(false).lines().count(), 2);
    }
}
