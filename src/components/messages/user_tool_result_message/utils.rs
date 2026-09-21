//! Maps to: CC `components/messages/UserToolResultMessage/utils.tsx`.
//! Rust keeps these line/segment DTOs at the UserToolResultMessage boundary so
//! per-tool UI ports can return main-screen render data without a separate
//! aggregate renderer layer.

use crate::components::structured_diff::color_diff::SyntaxHighlightTheme;
use iocraft::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolRenderTone {
    Normal,
    Success,
    Warning,
    Error,
    Inactive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolRenderBackground {
    DiffAdded,
    DiffRemoved,
    DiffAddedWord,
    DiffRemovedWord,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRenderSegment {
    pub text: String,
    pub foreground: Option<Color>,
    pub background: Option<ToolRenderBackground>,
    pub dim: bool,
    pub bold: bool,
}

impl ToolRenderSegment {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            foreground: None,
            background: None,
            dim: false,
            bold: false,
        }
    }

    pub fn with_background(mut self, background: ToolRenderBackground) -> Self {
        self.background = Some(background);
        self
    }

    pub fn with_dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }

    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRenderLine {
    pub text: String,
    pub tone: ToolRenderTone,
    pub background: Option<ToolRenderBackground>,
    pub dim: bool,
    pub segments: Vec<ToolRenderSegment>,
}

impl ToolRenderLine {
    pub fn new(text: impl Into<String>, tone: ToolRenderTone) -> Self {
        Self {
            text: text.into(),
            tone,
            background: None,
            dim: false,
            segments: Vec::new(),
        }
    }

    pub fn with_background(mut self, background: ToolRenderBackground) -> Self {
        self.background = Some(background);
        self
    }

    pub fn with_dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }

    pub fn with_segments(mut self, segments: Vec<ToolRenderSegment>) -> Self {
        self.segments = segments;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolRenderOptions {
    pub verbose: bool,
    pub is_transcript_mode: bool,
    pub terminal_width: usize,
    pub syntax_highlighting: bool,
    pub syntax_theme: SyntaxHighlightTheme,
}

impl Default for ToolRenderOptions {
    fn default() -> Self {
        Self {
            verbose: false,
            is_transcript_mode: false,
            terminal_width: 80,
            syntax_highlighting: true,
            syntax_theme: SyntaxHighlightTheme::Dark,
        }
    }
}

impl ToolRenderOptions {
    pub fn show_full(self) -> bool {
        self.verbose || self.is_transcript_mode
    }
}

// ─── Shared tool-result parsing/formatting helpers ───────────────────────
// Deliberate deviation: CC inlines these small helpers inside each tool's
// UI.tsx; the Rust ports share one copy at the UserToolResultMessage
// boundary. `format_file_size` here is the spaced "1.5 KB" variant used by
// tool-result rows (distinct from `utils::format::format_file_size`).

pub(crate) fn format_file_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else if size >= 10.0 {
        format!("{size:.0} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

pub(crate) fn pretty_json_for_display(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| json_stringify_for_display(value))
}

pub(crate) fn first_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|value| value.as_str())
            .map(String::from)
    })
}

pub(crate) fn first_bool(value: &serde_json::Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|value| value.as_bool()))
}

pub(crate) fn first_i64(value: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|value| value.as_i64()))
}

pub(crate) fn json_stringify_for_display(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

pub(crate) fn value_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(block_content_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(_) => block_content_text(value),
        _ => String::new(),
    }
}

pub(crate) fn block_content_text(block: &serde_json::Value) -> String {
    if let Some(text) = first_string(block, &["text", "thinking", "content"]) {
        return text;
    }
    match block.get("content") {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(block_content_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Some(value) => value_to_text(value),
        None => String::new(),
    }
}

#[allow(dead_code)]
pub(crate) fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}
