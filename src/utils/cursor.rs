//! Maps to: CC `utils/Cursor.ts`.
//! Immutable terminal cursor/text editing model. Source offsets retain UTF-16
//! units; `offset()` projects to UTF-8 for native rendering. Only
//! source movement operations consult shared Intl grapheme/word boundaries.

use crate::utils::intl::{get_grapheme_segmenter, get_word_segmenter};
use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use self::kill_ring::{
    KillDirection, get_last_kill, push_to_kill_ring, record_yank, update_yank_len, yank_pop,
};

pub mod kill_ring {
    //! Maps to the process-global kill-ring helpers in CC `utils/Cursor.ts`.

    use std::sync::{Mutex, OnceLock};

    const KILL_RING_MAX_SIZE: usize = 10;

    #[derive(Default)]
    struct KillRing {
        entries: Vec<String>,
        index: usize,
        last_action_was_kill: bool,
        last_action_was_yank: bool,
        last_yank_start: usize,
        last_yank_len: usize,
    }

    static KILL_RING: OnceLock<Mutex<KillRing>> = OnceLock::new();

    fn ring() -> &'static Mutex<KillRing> {
        KILL_RING.get_or_init(|| Mutex::new(KillRing::default()))
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum KillDirection {
        Append,
        Prepend,
    }

    /// Maps to: `pushToKillRing(text, direction)`.
    pub fn push_to_kill_ring(text: impl Into<String>, direction: KillDirection) {
        let text = text.into();
        if text.is_empty() {
            return;
        }

        if let Ok(mut ring) = ring().lock() {
            if ring.last_action_was_kill && !ring.entries.is_empty() {
                match direction {
                    KillDirection::Append => ring.entries[0].push_str(&text),
                    KillDirection::Prepend => {
                        ring.entries[0] = format!("{}{}", text, ring.entries[0]);
                    }
                }
            } else {
                ring.entries.insert(0, text);
                if ring.entries.len() > KILL_RING_MAX_SIZE {
                    ring.entries.pop();
                }
            }
            ring.index = 0;
            ring.last_action_was_kill = true;
            ring.last_action_was_yank = false;
        }
    }

    /// Maps to: `getLastKill()`.
    pub fn get_last_kill() -> String {
        ring()
            .lock()
            .ok()
            .and_then(|ring| ring.entries.first().cloned())
            .unwrap_or_default()
    }

    /// Maps to: `resetKillAccumulation()`.
    pub fn reset_kill_accumulation() {
        if let Ok(mut ring) = ring().lock() {
            ring.last_action_was_kill = false;
        }
    }

    /// Maps to: `recordYank(start, length)`. Both values use source UTF-16 units.
    pub fn record_yank(start: usize, len: usize) {
        if let Ok(mut ring) = ring().lock() {
            ring.last_yank_start = start;
            ring.last_yank_len = len;
            ring.last_action_was_yank = true;
            ring.index = 0;
        }
    }

    /// Maps to: `yankPop()`.
    pub fn yank_pop() -> Option<(String, usize, usize)> {
        let mut ring = ring().lock().ok()?;
        if !ring.last_action_was_yank || ring.entries.len() <= 1 {
            return None;
        }
        ring.index = (ring.index + 1) % ring.entries.len();
        let text = ring.entries[ring.index].clone();
        Some((text, ring.last_yank_start, ring.last_yank_len))
    }

    /// Maps to: `updateYankLength(length)`.
    pub fn update_yank_len(len: usize) {
        if let Ok(mut ring) = ring().lock() {
            ring.last_yank_len = len;
        }
    }

    /// Maps to: `resetYankState()`.
    pub fn reset_yank_state() {
        if let Ok(mut ring) = ring().lock() {
            ring.last_action_was_yank = false;
        }
    }

    #[cfg(test)]
    pub fn clear_kill_ring_for_tests() {
        if let Ok(mut ring) = ring().lock() {
            *ring = KillRing::default();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    text: String,
    columns: usize,
    offset: usize,
    source_offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KillEdit {
    pub cursor: Cursor,
    pub killed: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedLine {
    pub before: String,
    pub cursor: String,
    pub after: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WrappedLine {
    start: usize,
    end: usize,
    width: usize,
}

impl Cursor {
    /// Maps to: `Cursor.fromText(text, columns, offset)`.
    pub fn from_text(text: impl Into<String>, columns: usize, offset: usize) -> Self {
        // CC normalizes via MeasuredText before all cursor operations.
        let text = text.into().nfc().collect::<String>();
        let offset = clamp_byte_offset(&text, offset);
        let source_offset = text[..offset].encode_utf16().count();
        Self {
            text,
            source_offset,
            // CC reserves one terminal column for the cursor cell.
            columns: columns.saturating_sub(1).max(1),
            offset,
        }
    }

    /// Maps to: CC `Cursor.ts:162-174` fromText + `MeasuredText` NFC constructor.
    /// JS UTF-16 offset is applied *after* NFC, as a number; normalizing a prefix
    /// would change the source cursor position when text composes across a seam.
    /// A valid insertion can leave the numeric offset inside a surrogate pair
    /// after NFC. Preserve it; only the native byte view projects to scalar start.
    pub fn from_text_utf16(text: impl Into<String>, columns: usize, offset: usize) -> Self {
        let text = text.into().nfc().collect::<String>();
        let source_offset = offset.min(text.encode_utf16().count());
        let offset = utf16_offset_to_byte(&text, source_offset);
        Self {
            text,
            columns: columns.saturating_sub(1).max(1),
            offset,
            source_offset,
        }
    }

    /// UTF-16 projection for source callers; ordinary `offset()` stays UTF-8.
    pub fn offset_utf16(&self) -> usize {
        self.source_offset
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn is_at_start(&self) -> bool {
        self.source_offset == 0
    }

    pub fn is_at_end(&self) -> bool {
        self.source_offset >= self.text.encode_utf16().count()
    }

    /// Maps to: `cursor.left()`, including Image chip hopping.
    pub fn left(&self) -> Self {
        if self.is_at_start() {
            return self.clone();
        }
        if let Some((start, _end)) = self.image_ref_ending_at(self.offset) {
            return self.with_offset(start);
        }
        let boundary = grapheme_boundaries(&self.text)
            .into_iter()
            .take_while(|byte| self.text[..*byte].encode_utf16().count() < self.source_offset)
            .last()
            .unwrap_or(0);
        self.with_offset(boundary)
    }

    /// Maps to: `cursor.right()`, including Image chip hopping.
    pub fn right(&self) -> Self {
        if self.is_at_end() {
            return self.clone();
        }
        if let Some((_start, end)) = self.image_ref_starting_at(self.offset) {
            return self.with_offset(end);
        }
        let boundary = grapheme_boundaries(&self.text)
            .into_iter()
            .find(|byte| self.text[..*byte].encode_utf16().count() > self.source_offset)
            .unwrap_or(self.text.len());
        self.with_offset(boundary)
    }

    /// Maps to: `cursor.startOfLine()` (visual wrapped line).
    pub fn start_of_line(&self) -> Self {
        let lines = self.wrapped_lines();
        let (line_idx, col) = self.line_col_with_lines(&lines);
        if col == 0 && line_idx > 0 {
            self.with_offset(lines[line_idx - 1].start)
        } else {
            self.with_offset(lines[line_idx].start)
        }
    }

    /// Maps to: `cursor.endOfLine()` (visual wrapped line).
    pub fn end_of_line(&self) -> Self {
        let lines = self.wrapped_lines();
        let (line_idx, _) = self.line_col_with_lines(&lines);
        self.with_offset(lines[line_idx].end)
    }

    /// Maps to: `cursor.up()`.
    pub fn up(&self) -> Self {
        let lines = self.wrapped_lines();
        let (line_idx, col) = self.line_col_with_lines(&lines);
        if line_idx == 0 {
            self.clone()
        } else {
            self.with_offset(offset_for_column(&self.text, lines[line_idx - 1], col))
        }
    }

    /// Maps to: `cursor.down()`.
    pub fn down(&self) -> Self {
        let lines = self.wrapped_lines();
        let (line_idx, col) = self.line_col_with_lines(&lines);
        if line_idx + 1 >= lines.len() {
            self.clone()
        } else {
            self.with_offset(offset_for_column(&self.text, lines[line_idx + 1], col))
        }
    }

    /// Maps to: `cursor.upLogicalLine()`.
    pub fn up_logical_line(&self) -> Self {
        let current_start = self.find_logical_line_start(self.offset);
        if current_start == 0 {
            return self.with_offset(0);
        }

        let target_column = display_width(&self.text[current_start..self.offset]);
        let prev_end = current_start.saturating_sub(1);
        let prev_start = self.find_logical_line_start(prev_end);
        self.with_offset(offset_for_display_column(
            &self.text,
            prev_start,
            prev_end,
            target_column,
        ))
    }

    /// Maps to: `cursor.downLogicalLine()`.
    pub fn down_logical_line(&self) -> Self {
        let current_start = self.find_logical_line_start(self.offset);
        let current_end = self.find_logical_line_end(self.offset);
        if current_end >= self.text.len() {
            return self.with_offset(self.text.len());
        }

        let target_column = display_width(&self.text[current_start..self.offset]);
        let next_start = current_end + 1;
        let next_end = self.find_logical_line_end(next_start);
        self.with_offset(offset_for_display_column(
            &self.text,
            next_start,
            next_end,
            target_column,
        ))
    }

    /// Maps to: `cursor.prevWord()` using Unicode word segmentation.
    pub fn prev_word(&self) -> Self {
        if self.is_at_start() {
            return self.clone();
        }
        self.with_offset(prev_word_start(&self.text, self.source_offset))
    }

    /// Maps to: `cursor.nextWord()` using Unicode word segmentation.
    pub fn next_word(&self) -> Self {
        if self.is_at_end() {
            return self.clone();
        }
        self.with_offset(next_word_start(&self.text, self.source_offset))
    }

    /// Maps to: CC `Cursor.ts:845-859` modifyText.
    pub fn modify_text(&self, end: &Self, insert: &str) -> Self {
        // Execute JS slice arithmetic in UTF-16 before projecting to String.
        // If the final edit creates an unpaired surrogate, String necessarily
        // renders U+FFFD; its original code unit is not losslessly retained.
        let units: Vec<_> = self.text.encode_utf16().collect();
        let mut edited = units[..self.source_offset].to_vec();
        edited.extend(insert.encode_utf16());
        edited.extend_from_slice(&units[end.source_offset.min(units.len())..]);
        let text = String::from_utf16_lossy(&edited);
        let offset = self.offset_utf16() + insert.nfc().collect::<String>().encode_utf16().count();
        Self::from_text_utf16(text, self.columns + 1, offset)
    }

    /// Maps to: CC `Cursor.ts:861-864` insert.
    pub fn insert(&self, insert: &str) -> Self {
        self.modify_text(self, insert)
    }

    /// Maps to: `cursor.del()`.
    pub fn del(&self) -> Self {
        if self.is_at_end() {
            return self.clone();
        }
        self.modify_text(&self.right(), "")
    }

    /// Maps to: `cursor.backspace()`.
    pub fn backspace(&self) -> Self {
        if self.is_at_start() {
            return self.clone();
        }
        self.left().modify_text(self, "")
    }

    /// Maps to: `cursor.deleteTokenBefore() ?? cursor.backspace()` consumers.
    pub fn delete_token_before(&self) -> Option<Self> {
        // Cursor at Image chip start is the "selected" state — delete chip forward.
        if let Some((_start, end)) = self.image_ref_starting_at(self.offset) {
            let end = if self.text[end..].starts_with(' ') {
                next_boundary(&self.text, end)
            } else {
                end
            };
            return Some(self.replace_range(self.offset, end, ""));
        }

        if self.is_at_start() {
            return None;
        }

        // Only trigger token deletion at token boundary.
        if self
            .text
            .get(self.offset..)
            .and_then(|s| s.chars().next())
            .is_some_and(|ch| !ch.is_whitespace())
        {
            return None;
        }

        let before = &self.text[..self.offset];
        let captures = token_before_re().captures(before)?;
        let whole = captures.get(0)?;
        let leading_len = captures.get(1).map(|m| m.as_str().len()).unwrap_or(0);
        let start = whole.start() + leading_len;
        Some(self.replace_range(start, self.offset, ""))
    }

    /// Maps to: `cursor.deleteToLineStart()`.
    pub fn delete_to_line_start(&self) -> KillEdit {
        if self.source_offset > 0
            && self.text.encode_utf16().nth(self.source_offset - 1) == Some(u16::from(b'\n'))
        {
            return KillEdit {
                cursor: self.left().modify_text(self, ""),
                killed: "\n".into(),
            };
        }

        let start = self.start_of_line();
        let units: Vec<_> = self.text.encode_utf16().collect();
        let killed = String::from_utf16_lossy(&units[start.source_offset..self.source_offset]);
        KillEdit {
            cursor: start.modify_text(self, ""),
            killed,
        }
    }

    /// Maps to: `cursor.deleteToLineEnd()`.
    pub fn delete_to_line_end(&self) -> KillEdit {
        let end = if self.text.encode_utf16().nth(self.source_offset) == Some(u16::from(b'\n')) {
            next_boundary(&self.text, self.offset)
        } else {
            self.end_of_line().offset
        };
        let end = self.with_offset(end);
        let units: Vec<_> = self.text.encode_utf16().collect();
        let killed = String::from_utf16_lossy(&units[self.source_offset..end.source_offset]);
        KillEdit {
            cursor: self.modify_text(&end, ""),
            killed,
        }
    }

    /// Maps to: `cursor.deleteWordBefore()`.
    pub fn delete_word_before(&self) -> KillEdit {
        if self.is_at_start() {
            return KillEdit {
                cursor: self.clone(),
                killed: String::new(),
            };
        }
        let start = self.snap_out_of_image_ref(self.prev_word().offset, SnapToward::Start);
        let start_cursor = self.with_offset(start);
        let units: Vec<_> = self.text.encode_utf16().collect();
        let killed =
            String::from_utf16_lossy(&units[start_cursor.source_offset..self.source_offset]);
        KillEdit {
            cursor: start_cursor.modify_text(self, ""),
            killed,
        }
    }

    /// Maps to: `cursor.deleteWordAfter()`.
    pub fn delete_word_after(&self) -> Self {
        if self.is_at_end() {
            return self.clone();
        }
        let end = self.snap_out_of_image_ref(self.next_word().offset, SnapToward::End);
        self.modify_text(&self.with_offset(end), "")
    }

    /// Maps to: CC `hooks/useTextInput.ts:198-207` yank (native Cursor adapter).
    pub fn yank(&self) -> Self {
        let text = get_last_kill();
        if text.is_empty() {
            return self.clone();
        }
        let start = self.offset_utf16();
        let cursor = self.insert(&text);
        record_yank(start, text.encode_utf16().count());
        cursor
    }

    /// Maps to: CC `hooks/useTextInput.ts:209-222` handleYankPop (native adapter).
    pub fn yank_pop(&self) -> Self {
        let Some((replacement, start, len)) = yank_pop() else {
            return self.clone();
        };
        // Maps to: CC hooks/useTextInput.ts:209-222 handleYankPop. The ring
        // records raw UTF-16 lengths; only Cursor::from_text_utf16 applies NFC.
        let units: Vec<_> = self.text.encode_utf16().collect();
        let mut edited = units[..start.min(units.len())].to_vec();
        edited.extend(replacement.encode_utf16());
        edited.extend_from_slice(&units[start.saturating_add(len).min(units.len())..]);
        let text = String::from_utf16_lossy(&edited);
        let replacement_len = replacement.encode_utf16().count();
        update_yank_len(replacement_len);
        Self::from_text_utf16(text, self.columns + 1, start + replacement_len)
    }

    pub fn kill_to_line_start(&self) -> Self {
        let edit = self.delete_to_line_start();
        push_to_kill_ring(edit.killed, KillDirection::Prepend);
        edit.cursor
    }

    pub fn kill_to_line_end(&self) -> Self {
        let edit = self.delete_to_line_end();
        push_to_kill_ring(edit.killed, KillDirection::Append);
        edit.cursor
    }

    pub fn kill_word_before(&self) -> Self {
        let edit = self.delete_word_before();
        push_to_kill_ring(edit.killed, KillDirection::Prepend);
        edit.cursor
    }

    pub fn get_position(&self) -> CursorPosition {
        let lines = self.wrapped_lines();
        let (line, column) = self.line_col_with_lines(&lines);
        CursorPosition { line, column }
    }

    pub fn get_viewport_start_line(&self, max_visible_lines: Option<usize>) -> usize {
        let lines = self.wrapped_lines();
        let (cursor_line, _) = self.line_col_with_lines(&lines);
        viewport_range(lines.len(), cursor_line, max_visible_lines).0
    }

    pub fn get_viewport_char_offset(&self, max_visible_lines: Option<usize>) -> usize {
        let lines = self.wrapped_lines();
        let start = self.get_viewport_start_line(max_visible_lines);
        if start == 0 {
            0
        } else {
            lines.get(start).map(|l| l.start).unwrap_or(0)
        }
    }

    pub fn get_viewport_char_end(&self, max_visible_lines: Option<usize>) -> usize {
        let lines = self.wrapped_lines();
        let start = self.get_viewport_start_line(max_visible_lines);
        let Some(max_visible) = max_visible_lines.filter(|n| *n > 0) else {
            return self.text.len();
        };
        let end_line = (start + max_visible).min(lines.len());
        if end_line >= lines.len() {
            self.text.len()
        } else {
            lines[end_line].start
        }
    }

    /// Render with a synthetic cursor cell. Maps to `cursor.render(...)` in CC,
    /// but returns structured line segments so iocraft can style the cursor cell.
    pub fn render_lines(&self, max_visible_lines: Option<usize>) -> Vec<RenderedLine> {
        let lines = self.wrapped_lines();
        let (cursor_line, _) = self.line_col_with_lines(&lines);
        let (start_line, end_line) = viewport_range(lines.len(), cursor_line, max_visible_lines);

        lines[start_line..end_line]
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let line_idx = start_line + idx;
                let raw = &self.text[line.start..line.end];
                if line_idx != cursor_line {
                    return RenderedLine {
                        before: raw.trim_end().to_string(),
                        cursor: String::new(),
                        after: String::new(),
                    };
                }

                let cursor = self.offset.clamp(line.start, line.end);
                let before = self.text[line.start..cursor].to_string();
                if cursor < line.end {
                    let next = next_boundary(&self.text, cursor);
                    RenderedLine {
                        before,
                        cursor: self.text[cursor..next].to_string(),
                        after: self.text[next..line.end].trim_end().to_string(),
                    }
                } else {
                    RenderedLine {
                        before,
                        cursor: " ".to_string(),
                        after: String::new(),
                    }
                }
            })
            .collect()
    }

    fn with_offset(&self, offset: usize) -> Self {
        Self::from_text(self.text.clone(), self.columns + 1, offset)
    }

    fn replace_range(&self, start: usize, end: usize, replacement: &str) -> Self {
        self.with_offset(start)
            .modify_text(&self.with_offset(end), replacement)
    }

    fn wrapped_lines(&self) -> Vec<WrappedLine> {
        wrap_lines(&self.text, self.columns)
    }

    fn line_col_with_lines(&self, lines: &[WrappedLine]) -> (usize, usize) {
        for (idx, line) in lines.iter().enumerate() {
            if self.offset < line.start || self.offset > line.end {
                continue;
            }
            // At a soft-wrap boundary, CC reports the cursor on the next line.
            if self.offset == line.end
                && idx + 1 < lines.len()
                && lines[idx + 1].start == line.end
                && line.start != line.end
            {
                continue;
            }
            let col = display_width(&self.text[line.start..self.offset]);
            return (idx, col.min(line.width));
        }
        let last = lines.len().saturating_sub(1);
        (last, lines.get(last).map(|l| l.width).unwrap_or_default())
    }

    fn find_logical_line_start(&self, from_offset: usize) -> usize {
        let from_offset = clamp_cursor(&self.text, from_offset);
        self.text[..from_offset]
            .rfind('\n')
            .map(|idx| idx + 1)
            .unwrap_or(0)
    }

    fn find_logical_line_end(&self, from_offset: usize) -> usize {
        let from_offset = clamp_cursor(&self.text, from_offset);
        self.text[from_offset..]
            .find('\n')
            .map(|idx| from_offset + idx)
            .unwrap_or(self.text.len())
    }

    fn image_ref_ending_at(&self, offset: usize) -> Option<(usize, usize)> {
        let prefix = self.text.get(..offset)?;
        image_ref_re()
            .find_iter(prefix)
            .find_map(|m| (m.end() == offset).then_some((m.start(), m.end())))
    }

    fn image_ref_starting_at(&self, offset: usize) -> Option<(usize, usize)> {
        let suffix = self.text.get(offset..)?;
        image_ref_re()
            .find(suffix)
            .and_then(|m| (m.start() == 0).then_some((offset, offset + m.end())))
    }

    fn snap_out_of_image_ref(&self, offset: usize, toward: SnapToward) -> usize {
        for m in image_ref_re().find_iter(&self.text) {
            if offset > m.start() && offset < m.end() {
                return match toward {
                    SnapToward::Start => m.start(),
                    SnapToward::End => m.end(),
                };
            }
        }
        offset
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapToward {
    Start,
    End,
}

fn viewport_range(total: usize, cursor_line: usize, max_visible: Option<usize>) -> (usize, usize) {
    let Some(max_visible) = max_visible.filter(|n| *n > 0) else {
        return (0, total);
    };
    if total <= max_visible {
        return (0, total);
    }
    let half = max_visible / 2;
    let mut start = cursor_line.saturating_sub(half);
    let end = (start + max_visible).min(total);
    if end - start < max_visible {
        start = end.saturating_sub(max_visible);
    }
    (start, end)
}

fn wrap_lines(text: &str, columns: usize) -> Vec<WrappedLine> {
    if text.is_empty() {
        return vec![WrappedLine {
            start: 0,
            end: 0,
            width: 0,
        }];
    }

    let columns = columns.max(1);
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut width = 0usize;

    for (idx, grapheme) in UnicodeSegmentation::grapheme_indices(text, true) {
        if grapheme == "\n" {
            out.push(WrappedLine {
                start,
                end: idx,
                width,
            });
            start = idx + grapheme.len();
            width = 0;
            continue;
        }

        let g_width = grapheme_width(grapheme);
        if width > 0 && width + g_width > columns {
            out.push(WrappedLine {
                start,
                end: idx,
                width,
            });
            start = idx;
            width = 0;
        }
        width += g_width;
    }

    out.push(WrappedLine {
        start,
        end: text.len(),
        width,
    });
    out
}

fn offset_for_column(text: &str, line: WrappedLine, target_col: usize) -> usize {
    offset_for_display_column(text, line.start, line.end, target_col)
}

fn offset_for_display_column(text: &str, start: usize, end: usize, target_col: usize) -> usize {
    if start >= end || target_col == 0 {
        return start;
    }

    let mut width = 0usize;
    for (rel, grapheme) in UnicodeSegmentation::grapheme_indices(&text[start..end], true) {
        let idx = start + rel;
        let next_width = width + grapheme_width(grapheme);
        if next_width > target_col {
            return idx;
        }
        width = next_width;
    }
    end
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn grapheme_width(s: &str) -> usize {
    UnicodeWidthStr::width(s).max(1)
}

/// Native UTF-16 → UTF-8 index carrier. It does not normalize text or segment it.
pub fn utf16_offset_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units + character.len_utf16() > offset {
            return byte;
        }
        units += character.len_utf16();
    }
    text.len()
}

fn clamp_byte_offset(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

pub fn clamp_cursor(text: &str, cursor: usize) -> usize {
    snap_to_grapheme_boundary(text, clamp_byte_offset(text, cursor))
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries = get_grapheme_segmenter()
        .segment(text)
        .into_iter()
        .map(|segment| segment.index)
        .collect::<Vec<_>>();
    if boundaries.first().copied() != Some(0) {
        boundaries.insert(0, 0);
    }
    if boundaries.last().copied() != Some(text.len()) {
        boundaries.push(text.len());
    }
    boundaries
}

fn snap_to_grapheme_boundary(text: &str, cursor: usize) -> usize {
    if cursor == 0 || cursor >= text.len() {
        return cursor.min(text.len());
    }
    grapheme_boundaries(text)
        .into_iter()
        .take_while(|boundary| *boundary <= cursor)
        .last()
        .unwrap_or(0)
}

#[allow(dead_code)]
fn prev_boundary(text: &str, cursor: usize) -> usize {
    let cursor = clamp_byte_offset(text, cursor);
    if cursor == 0 {
        return 0;
    }
    grapheme_boundaries(text)
        .into_iter()
        .take_while(|boundary| *boundary < cursor)
        .last()
        .unwrap_or(0)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    let cursor = clamp_byte_offset(text, cursor);
    if cursor >= text.len() {
        return text.len();
    }
    grapheme_boundaries(text)
        .into_iter()
        .find(|boundary| *boundary > cursor)
        .unwrap_or(text.len())
}

fn word_ranges(text: &str) -> Vec<(usize, usize)> {
    // Maps to: CC Cursor.ts:1173-1188 MeasuredText.getWordBoundaries,
    // consumed by nextWord/prevWord. The native adapter only projects offsets.
    get_word_segmenter()
        .segment(text)
        .into_iter()
        .filter(|segment| segment.is_word_like.unwrap_or(false))
        .map(|segment| (segment.index, segment.index + segment.segment.len()))
        .collect()
}

fn prev_word_start(text: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let mut candidate = 0;
    for (start, end) in word_ranges(text) {
        let start_units = text[..start].encode_utf16().count();
        let end_units = text[..end].encode_utf16().count();
        if start_units < cursor {
            if cursor > start_units && cursor <= end_units {
                return start;
            }
            candidate = start;
        } else {
            break;
        }
    }
    candidate
}

fn next_word_start(text: &str, cursor: usize) -> usize {
    if cursor >= text.encode_utf16().count() {
        return text.len();
    }
    for (start, _) in word_ranges(text) {
        if text[..start].encode_utf16().count() > cursor {
            return start;
        }
    }
    text.len()
}

fn image_ref_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[Image #\d+\]").expect("valid image ref regex"))
}

fn token_before_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(^|\s)\[(Pasted text #\d+(?: \+\d+ lines)?|Image #\d+|\.\.\.Truncated text #\d+ \+\d+ lines\.\.\.)\]$",
        )
        .expect("valid token ref regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kill_ring::clear_kill_ring_for_tests;

    #[test]
    fn ctrl_u_kill_to_line_start() {
        clear_kill_ring_for_tests();
        let c = Cursor::from_text("hello world", 80, 6).kill_to_line_start();
        assert_eq!(c.text(), "world");
        assert_eq!(c.offset(), 0);
    }

    #[test]
    fn ctrl_k_kill_to_line_end() {
        clear_kill_ring_for_tests();
        let c = Cursor::from_text("hello world", 80, 6).kill_to_line_end();
        assert_eq!(c.text(), "hello ");
        assert_eq!(c.offset(), 6);
    }

    #[test]
    fn word_navigation_uses_utf8_boundaries() {
        let text = "你好 world";
        let c = Cursor::from_text(text, 80, text.len()).prev_word();
        assert_eq!(&text[c.offset()..], "world");
    }

    #[test]
    fn wraps_by_display_width() {
        let c = Cursor::from_text("abcd", 4, 4); // content width = 3
        let lines = c.render_lines(None);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn logical_line_fallback_moves_between_paragraph_lines() {
        let text = "abc\ndef";
        let c = Cursor::from_text(text, 80, text.len()).up_logical_line();
        assert_eq!(c.offset(), 3);
        let c = c.down_logical_line();
        assert_eq!(c.offset(), text.len());
    }

    #[test]
    fn backspace_deletes_pasted_text_token() {
        let text = "see [Pasted text #1 +2 lines]";
        let c = Cursor::from_text(text, 80, text.len())
            .delete_token_before()
            .expect("token");
        assert_eq!(c.text(), "see ");
    }

    #[test]
    fn grapheme_movement_keeps_emoji_family_together() {
        let text = "a👨‍👩‍👧‍👦b";
        let c = Cursor::from_text(text, 80, text.len()).left().left();
        assert_eq!(&text[c.offset()..], "👨‍👩‍👧‍👦b");
    }
    #[test]
    fn word_movement_and_deletion_match_bun_oracle_with_accepted_icu78_differences() {
        // Bun source run: all previous oracle offsets retained, plus raw
        // input length. Cursor.ts nextWord/prevWord/deleteWordBefore/After;
        // .test/system-icu-0912/bun-oracle.json replaces the Node77 oracle.
        // UTF-16 expected indices remain in their original units.
        // User-accepted 2026-09-13: colon word editing and four Khmer
        // grapheme steps use fixed native ICU78 results; all other rows
        // remain Bun results. Original oracle is retained in research/.
        type Operation<'a> = (
            usize,
            usize,
            usize,
            usize,
            &'a str,
            &'a str,
            usize,
            &'a str,
            usize,
            usize,
        );
        let cases: &[(&str, &[Operation<'_>])] = &[
            ("", &[(0, 0, 0, 0, "", "", 0, "", 0, 0)]),
            (
                "你好 世界",
                &[
                    (0, 3, 0, 0, "你好 世界", "", 0, "世界", 0, 1),
                    (1, 3, 0, 0, "好 世界", "你", 1, "你世界", 0, 2),
                    (2, 3, 0, 0, " 世界", "你好", 2, "你好世界", 1, 3),
                    (3, 5, 0, 0, "世界", "你好 ", 3, "你好 ", 2, 4),
                    (4, 5, 3, 3, "你好 界", "世", 4, "你好 世", 3, 5),
                    (5, 5, 3, 3, "你好 ", "世界", 5, "你好 世界", 4, 5),
                ],
            ),
            (
                "你好世界",
                &[
                    (0, 2, 0, 0, "你好世界", "", 0, "世界", 0, 1),
                    (1, 2, 0, 0, "好世界", "你", 1, "你世界", 0, 2),
                    (2, 4, 0, 0, "世界", "你好", 2, "你好", 1, 3),
                    (3, 4, 2, 2, "你好界", "世", 3, "你好世", 2, 4),
                    (4, 4, 2, 2, "你好", "世界", 4, "你好世界", 3, 4),
                ],
            ),
            (
                "私は学生です",
                &[
                    (0, 1, 0, 0, "私は学生です", "", 0, "は学生です", 0, 1),
                    (1, 2, 0, 0, "は学生です", "私", 1, "私学生です", 0, 2),
                    (2, 4, 1, 1, "私学生です", "は", 2, "私はです", 1, 3),
                    (3, 4, 2, 2, "私は生です", "学", 3, "私は学です", 2, 4),
                    (4, 6, 2, 2, "私はです", "学生", 4, "私は学生", 3, 5),
                    (5, 6, 4, 4, "私は学生す", "で", 5, "私は学生で", 4, 6),
                    (6, 6, 4, 4, "私は学生", "です", 6, "私は学生です", 5, 6),
                ],
            ),
            (
                "こんにちは世界",
                &[
                    (0, 5, 0, 0, "こんにちは世界", "", 0, "世界", 0, 1),
                    (1, 5, 0, 0, "んにちは世界", "こ", 1, "こ世界", 0, 2),
                    (2, 5, 0, 0, "にちは世界", "こん", 2, "こん世界", 1, 3),
                    (3, 5, 0, 0, "ちは世界", "こんに", 3, "こんに世界", 2, 4),
                    (4, 5, 0, 0, "は世界", "こんにち", 4, "こんにち世界", 3, 5),
                    (5, 7, 0, 0, "世界", "こんにちは", 5, "こんにちは", 4, 6),
                    (6, 7, 5, 5, "こんにちは界", "世", 6, "こんにちは世", 5, 7),
                    (7, 7, 5, 5, "こんにちは", "世界", 7, "こんにちは世界", 6, 7),
                ],
            ),
            (
                "안녕하세요 세계",
                &[
                    (0, 6, 0, 0, "안녕하세요 세계", "", 0, "세계", 0, 1),
                    (1, 6, 0, 0, "녕하세요 세계", "안", 1, "안세계", 0, 2),
                    (2, 6, 0, 0, "하세요 세계", "안녕", 2, "안녕세계", 1, 3),
                    (3, 6, 0, 0, "세요 세계", "안녕하", 3, "안녕하세계", 2, 4),
                    (4, 6, 0, 0, "요 세계", "안녕하세", 4, "안녕하세세계", 3, 5),
                    (5, 6, 0, 0, " 세계", "안녕하세요", 5, "안녕하세요세계", 4, 6),
                    (6, 8, 0, 0, "세계", "안녕하세요 ", 6, "안녕하세요 ", 5, 7),
                    (7, 8, 6, 6, "안녕하세요 계", "세", 7, "안녕하세요 세", 6, 8),
                    (
                        8,
                        8,
                        6,
                        6,
                        "안녕하세요 ",
                        "세계",
                        8,
                        "안녕하세요 세계",
                        7,
                        8,
                    ),
                ],
            ),
            (
                "alpha-beta",
                &[
                    (0, 6, 0, 0, "alpha-beta", "", 0, "beta", 0, 1),
                    (1, 6, 0, 0, "lpha-beta", "a", 1, "abeta", 0, 2),
                    (2, 6, 0, 0, "pha-beta", "al", 2, "albeta", 1, 3),
                    (3, 6, 0, 0, "ha-beta", "alp", 3, "alpbeta", 2, 4),
                    (4, 6, 0, 0, "a-beta", "alph", 4, "alphbeta", 3, 5),
                    (5, 6, 0, 0, "-beta", "alpha", 5, "alphabeta", 4, 6),
                    (6, 10, 0, 0, "beta", "alpha-", 6, "alpha-", 5, 7),
                    (7, 10, 6, 6, "alpha-eta", "b", 7, "alpha-b", 6, 8),
                    (8, 10, 6, 6, "alpha-ta", "be", 8, "alpha-be", 7, 9),
                    (9, 10, 6, 6, "alpha-a", "bet", 9, "alpha-bet", 8, 10),
                    (10, 10, 6, 6, "alpha-", "beta", 10, "alpha-beta", 9, 10),
                ],
            ),
            // Accepted ICU78: one word, so next/previous target 11/0.
            // Bun/ICU74 splits at the colon (next 6, previous 6 in world).
            (
                "hello:world",
                &[
                    (0, 11, 0, 0, "hello:world", "", 0, "", 0, 1),
                    (1, 11, 0, 0, "ello:world", "h", 1, "h", 0, 2),
                    (2, 11, 0, 0, "llo:world", "he", 2, "he", 1, 3),
                    (3, 11, 0, 0, "lo:world", "hel", 3, "hel", 2, 4),
                    (4, 11, 0, 0, "o:world", "hell", 4, "hell", 3, 5),
                    (5, 11, 0, 0, ":world", "hello", 5, "hello", 4, 6),
                    (6, 11, 0, 0, "world", "hello:", 6, "hello:", 5, 7),
                    (7, 11, 0, 0, "orld", "hello:w", 7, "hello:w", 6, 8),
                    (8, 11, 0, 0, "rld", "hello:wo", 8, "hello:wo", 7, 9),
                    (9, 11, 0, 0, "ld", "hello:wor", 9, "hello:wor", 8, 10),
                    (10, 11, 0, 0, "d", "hello:worl", 10, "hello:worl", 9, 11),
                    (11, 11, 0, 0, "", "hello:world", 11, "hello:world", 10, 11),
                ],
            ),
            (
                "can't foo_bar 3.14",
                &[
                    (
                        0,
                        6,
                        0,
                        0,
                        "can't foo_bar 3.14",
                        "",
                        0,
                        "foo_bar 3.14",
                        0,
                        1,
                    ),
                    (
                        1,
                        6,
                        0,
                        0,
                        "an't foo_bar 3.14",
                        "c",
                        1,
                        "cfoo_bar 3.14",
                        0,
                        2,
                    ),
                    (
                        2,
                        6,
                        0,
                        0,
                        "n't foo_bar 3.14",
                        "ca",
                        2,
                        "cafoo_bar 3.14",
                        1,
                        3,
                    ),
                    (
                        3,
                        6,
                        0,
                        0,
                        "'t foo_bar 3.14",
                        "can",
                        3,
                        "canfoo_bar 3.14",
                        2,
                        4,
                    ),
                    (
                        4,
                        6,
                        0,
                        0,
                        "t foo_bar 3.14",
                        "can'",
                        4,
                        "can'foo_bar 3.14",
                        3,
                        5,
                    ),
                    (
                        5,
                        6,
                        0,
                        0,
                        " foo_bar 3.14",
                        "can't",
                        5,
                        "can'tfoo_bar 3.14",
                        4,
                        6,
                    ),
                    (6, 14, 0, 0, "foo_bar 3.14", "can't ", 6, "can't 3.14", 5, 7),
                    (
                        7,
                        14,
                        6,
                        6,
                        "can't oo_bar 3.14",
                        "f",
                        7,
                        "can't f3.14",
                        6,
                        8,
                    ),
                    (
                        8,
                        14,
                        6,
                        6,
                        "can't o_bar 3.14",
                        "fo",
                        8,
                        "can't fo3.14",
                        7,
                        9,
                    ),
                    (
                        9,
                        14,
                        6,
                        6,
                        "can't _bar 3.14",
                        "foo",
                        9,
                        "can't foo3.14",
                        8,
                        10,
                    ),
                    (
                        10,
                        14,
                        6,
                        6,
                        "can't bar 3.14",
                        "foo_",
                        10,
                        "can't foo_3.14",
                        9,
                        11,
                    ),
                    (
                        11,
                        14,
                        6,
                        6,
                        "can't ar 3.14",
                        "foo_b",
                        11,
                        "can't foo_b3.14",
                        10,
                        12,
                    ),
                    (
                        12,
                        14,
                        6,
                        6,
                        "can't r 3.14",
                        "foo_ba",
                        12,
                        "can't foo_ba3.14",
                        11,
                        13,
                    ),
                    (
                        13,
                        14,
                        6,
                        6,
                        "can't  3.14",
                        "foo_bar",
                        13,
                        "can't foo_bar3.14",
                        12,
                        14,
                    ),
                    (
                        14,
                        18,
                        6,
                        6,
                        "can't 3.14",
                        "foo_bar ",
                        14,
                        "can't foo_bar ",
                        13,
                        15,
                    ),
                    (
                        15,
                        18,
                        14,
                        14,
                        "can't foo_bar .14",
                        "3",
                        15,
                        "can't foo_bar 3",
                        14,
                        16,
                    ),
                    (
                        16,
                        18,
                        14,
                        14,
                        "can't foo_bar 14",
                        "3.",
                        16,
                        "can't foo_bar 3.",
                        15,
                        17,
                    ),
                    (
                        17,
                        18,
                        14,
                        14,
                        "can't foo_bar 4",
                        "3.1",
                        17,
                        "can't foo_bar 3.1",
                        16,
                        18,
                    ),
                    (
                        18,
                        18,
                        14,
                        14,
                        "can't foo_bar ",
                        "3.14",
                        18,
                        "can't foo_bar 3.14",
                        17,
                        18,
                    ),
                ],
            ),
            (
                "élan café",
                &[
                    (0, 5, 0, 0, "élan café", "", 0, "café", 0, 1),
                    (1, 5, 0, 0, "lan café", "é", 1, "écafé", 0, 2),
                    (2, 5, 0, 0, "an café", "él", 2, "élcafé", 1, 3),
                    (3, 5, 0, 0, "n café", "éla", 3, "élacafé", 2, 4),
                    (4, 5, 0, 0, " café", "élan", 4, "élancafé", 3, 5),
                    (5, 9, 0, 0, "café", "élan ", 5, "élan ", 4, 6),
                    (6, 9, 5, 5, "élan afé", "c", 6, "élan c", 5, 7),
                    (7, 9, 5, 5, "élan fé", "ca", 7, "élan ca", 6, 8),
                    (8, 9, 5, 5, "élan é", "caf", 8, "élan caf", 7, 9),
                    (9, 9, 5, 5, "élan ", "café", 9, "élan café", 8, 9),
                    (11, 9, 5, 5, "élan ", "café", 9, "élan café", 8, 9),
                ],
            ),
            (
                "A👩‍💻B",
                &[
                    (0, 6, 0, 0, "A👩‍💻B", "", 0, "B", 0, 1),
                    (1, 6, 0, 0, "👩‍💻B", "A", 1, "AB", 0, 6),
                    (6, 7, 0, 0, "B", "A👩‍💻", 6, "A👩‍💻", 1, 7),
                    (7, 7, 6, 6, "A👩‍💻", "B", 7, "A👩‍💻B", 6, 7),
                ],
            ),
            (
                "👨‍👩‍👧‍👦 hello",
                &[
                    (0, 12, 0, 0, "👨‍👩‍👧‍👦 hello", "", 0, "hello", 0, 11),
                    (11, 12, 0, 0, " hello", "👨‍👩‍👧‍👦", 11, "👨‍👩‍👧‍👦hello", 0, 12),
                    (12, 17, 0, 0, "hello", "👨‍👩‍👧‍👦 ", 12, "👨‍👩‍👧‍👦 ", 11, 13),
                    (13, 17, 12, 12, "👨‍👩‍👧‍👦 ello", "h", 13, "👨‍👩‍👧‍👦 h", 12, 14),
                    (14, 17, 12, 12, "👨‍👩‍👧‍👦 llo", "he", 14, "👨‍👩‍👧‍👦 he", 13, 15),
                    (15, 17, 12, 12, "👨‍👩‍👧‍👦 lo", "hel", 15, "👨‍👩‍👧‍👦 hel", 14, 16),
                    (16, 17, 12, 12, "👨‍👩‍👧‍👦 o", "hell", 16, "👨‍👩‍👧‍👦 hell", 15, 17),
                    (17, 17, 12, 12, "👨‍👩‍👧‍👦 ", "hello", 17, "👨‍👩‍👧‍👦 hello", 16, 17),
                ],
            ),
            (
                "👩‍💻你好，世界！",
                &[
                    (0, 5, 0, 0, "👩‍💻你好，世界！", "", 0, "你好，世界！", 0, 5),
                    (5, 8, 0, 0, "你好，世界！", "👩‍💻", 5, "👩‍💻世界！", 0, 6),
                    (6, 8, 5, 5, "👩‍💻好，世界！", "你", 6, "👩‍💻你世界！", 5, 7),
                    (7, 8, 5, 5, "👩‍💻，世界！", "你好", 7, "👩‍💻你好世界！", 6, 8),
                    (8, 11, 5, 5, "👩‍💻世界！", "你好，", 8, "👩‍💻你好，", 7, 9),
                    (9, 11, 8, 8, "👩‍💻你好，界！", "世", 9, "👩‍💻你好，世", 8, 10),
                    (
                        10,
                        11,
                        8,
                        8,
                        "👩‍💻你好，！",
                        "世界",
                        10,
                        "👩‍💻你好，世界",
                        9,
                        11,
                    ),
                    (
                        11,
                        11,
                        8,
                        8,
                        "👩‍💻你好，",
                        "世界！",
                        11,
                        "👩‍💻你好，世界！",
                        10,
                        11,
                    ),
                ],
            ),
            (
                "[Image #12] x",
                &[
                    (0, 1, 0, 0, "[Image #12] x", "", 0, " x", 0, 11),
                    (1, 8, 0, 0, "Image #12] x", "[", 1, "[ x", 0, 2),
                    (2, 8, 1, 0, "mage #12] x", "[I", 2, "[I x", 1, 3),
                    (3, 8, 1, 0, "age #12] x", "[Im", 3, "[Im x", 2, 4),
                    (4, 8, 1, 0, "ge #12] x", "[Ima", 4, "[Ima x", 3, 5),
                    (5, 8, 1, 0, "e #12] x", "[Imag", 5, "[Imag x", 4, 6),
                    (6, 8, 1, 0, " #12] x", "[Image", 6, "[Image x", 5, 7),
                    (7, 8, 1, 0, "#12] x", "[Image ", 7, "[Image  x", 6, 8),
                    (8, 12, 1, 0, "12] x", "[Image #", 8, "[Image #x", 7, 9),
                    (9, 12, 8, 0, "2] x", "[Image #1", 9, "[Image #1x", 8, 10),
                    (10, 12, 8, 0, "] x", "[Image #12", 10, "[Image #12x", 9, 11),
                    (11, 12, 8, 0, " x", "[Image #12]", 11, "[Image #12]x", 0, 12),
                    (
                        12,
                        13,
                        8,
                        0,
                        "x",
                        "[Image #12] ",
                        12,
                        "[Image #12] ",
                        11,
                        13,
                    ),
                    (
                        13,
                        13,
                        12,
                        12,
                        "[Image #12] ",
                        "x",
                        13,
                        "[Image #12] x",
                        12,
                        13,
                    ),
                ],
            ),
            (
                "foo\nbar baz",
                &[
                    (0, 4, 0, 0, "foo\nbar baz", "", 0, "bar baz", 0, 1),
                    (1, 4, 0, 0, "oo\nbar baz", "f", 1, "fbar baz", 0, 2),
                    (2, 4, 0, 0, "o\nbar baz", "fo", 2, "fobar baz", 1, 3),
                    (3, 4, 0, 0, "\nbar baz", "foo", 3, "foobar baz", 2, 4),
                    (4, 8, 0, 0, "bar baz", "foo\n", 4, "foo\nbaz", 3, 5),
                    (5, 8, 4, 4, "foo\nar baz", "b", 5, "foo\nbbaz", 4, 6),
                    (6, 8, 4, 4, "foo\nr baz", "ba", 6, "foo\nbabaz", 5, 7),
                    (7, 8, 4, 4, "foo\n baz", "bar", 7, "foo\nbarbaz", 6, 8),
                    (8, 11, 4, 4, "foo\nbaz", "bar ", 8, "foo\nbar ", 7, 9),
                    (9, 11, 8, 8, "foo\nbar az", "b", 9, "foo\nbar b", 8, 10),
                    (10, 11, 8, 8, "foo\nbar z", "ba", 10, "foo\nbar ba", 9, 11),
                    (11, 11, 8, 8, "foo\nbar ", "baz", 11, "foo\nbar baz", 10, 11),
                ],
            ),
            (
                "ภาษาไทยภาษาไทย",
                &[
                    (0, 4, 0, 0, "ภาษาไทยภาษาไทย", "", 0, "ไทยภาษาไทย", 0, 1),
                    (1, 4, 0, 0, "าษาไทยภาษาไทย", "ภ", 1, "ภไทยภาษาไทย", 0, 2),
                    (2, 4, 0, 0, "ษาไทยภาษาไทย", "ภา", 2, "ภาไทยภาษาไทย", 1, 3),
                    (3, 4, 0, 0, "าไทยภาษาไทย", "ภาษ", 3, "ภาษไทยภาษาไทย", 2, 4),
                    (4, 7, 0, 0, "ไทยภาษาไทย", "ภาษา", 4, "ภาษาภาษาไทย", 3, 5),
                    (5, 7, 4, 4, "ภาษาทยภาษาไทย", "ไ", 5, "ภาษาไภาษาไทย", 4, 6),
                    (6, 7, 4, 4, "ภาษายภาษาไทย", "ไท", 6, "ภาษาไทภาษาไทย", 5, 7),
                    (7, 11, 4, 4, "ภาษาภาษาไทย", "ไทย", 7, "ภาษาไทยไทย", 6, 8),
                    (8, 11, 7, 7, "ภาษาไทยาษาไทย", "ภ", 8, "ภาษาไทยภไทย", 7, 9),
                    (9, 11, 7, 7, "ภาษาไทยษาไทย", "ภา", 9, "ภาษาไทยภาไทย", 8, 10),
                    (
                        10,
                        11,
                        7,
                        7,
                        "ภาษาไทยาไทย",
                        "ภาษ",
                        10,
                        "ภาษาไทยภาษไทย",
                        9,
                        11,
                    ),
                    (
                        11,
                        14,
                        7,
                        7,
                        "ภาษาไทยไทย",
                        "ภาษา",
                        11,
                        "ภาษาไทยภาษา",
                        10,
                        12,
                    ),
                    (
                        12,
                        14,
                        11,
                        11,
                        "ภาษาไทยภาษาทย",
                        "ไ",
                        12,
                        "ภาษาไทยภาษาไ",
                        11,
                        13,
                    ),
                    (
                        13,
                        14,
                        11,
                        11,
                        "ภาษาไทยภาษาย",
                        "ไท",
                        13,
                        "ภาษาไทยภาษาไท",
                        12,
                        14,
                    ),
                    (
                        14,
                        14,
                        11,
                        11,
                        "ภาษาไทยภาษา",
                        "ไทย",
                        14,
                        "ภาษาไทยภาษาไทย",
                        13,
                        14,
                    ),
                ],
            ),
            // Accepted ICU78 grapheme steps (separate from terminal width):
            // right@1: Bun 3 -> ICU78 5; left@5: 3 -> 1;
            // right@11: 13 -> 15; left@15: 13 -> 11.
            // All word movement/deletion fields retain the Bun oracle.
            (
                "កម្ពុជាភាសាខ្មែរ",
                &[
                    (0, 7, 0, 0, "កម្ពុជាភាសាខ្មែរ", "", 0, "ភាសាខ្មែរ", 0, 1),
                    (1, 7, 0, 0, "ម្ពុជាភាសាខ្មែរ", "ក", 1, "កភាសាខ្មែរ", 0, 5),
                    (3, 7, 0, 0, "ពុជាភាសាខ្មែរ", "កម្", 3, "កម្ភាសាខ្មែរ", 1, 5),
                    (5, 7, 0, 0, "ជាភាសាខ្មែរ", "កម្ពុ", 5, "កម្ពុភាសាខ្មែរ", 1, 7),
                    (7, 16, 0, 0, "ភាសាខ្មែរ", "កម្ពុជា", 7, "កម្ពុជា", 5, 9),
                    (9, 16, 7, 7, "កម្ពុជាសាខ្មែរ", "ភា", 9, "កម្ពុជាភា", 7, 11),
                    (11, 16, 7, 7, "កម្ពុជាខ្មែរ", "ភាសា", 11, "កម្ពុជាភាសា", 9, 15),
                    (13, 16, 7, 7, "កម្ពុជាមែរ", "ភាសាខ្", 13, "កម្ពុជាភាសាខ្", 11, 15),
                    (15, 16, 7, 7, "កម្ពុជារ", "ភាសាខ្មែ", 15, "កម្ពុជាភាសាខ្មែ", 11, 16),
                    (16, 16, 7, 7, "កម្ពុជា", "ភាសាខ្មែរ", 16, "កម្ពុជាភាសាខ្មែរ", 15, 16),
                ],
            ),
            (
                "… — 👩‍💻",
                &[
                    (0, 9, 0, 0, "… — 👩‍💻", "", 0, "", 0, 1),
                    (1, 9, 0, 0, " — 👩‍💻", "…", 1, "…", 0, 2),
                    (2, 9, 0, 0, "— 👩‍💻", "… ", 2, "… ", 1, 3),
                    (3, 9, 0, 0, " 👩‍💻", "… —", 3, "… —", 2, 4),
                    (4, 9, 0, 0, "👩‍💻", "… — ", 4, "… — ", 3, 9),
                    (9, 9, 0, 0, "", "… — 👩‍💻", 9, "… — 👩‍💻", 4, 9),
                ],
            ),
            (
                "中文abc def",
                &[
                    (0, 2, 0, 0, "中文abc def", "", 0, "abc def", 0, 1),
                    (1, 2, 0, 0, "文abc def", "中", 1, "中abc def", 0, 2),
                    (2, 6, 0, 0, "abc def", "中文", 2, "中文def", 1, 3),
                    (3, 6, 2, 2, "中文bc def", "a", 3, "中文adef", 2, 4),
                    (4, 6, 2, 2, "中文c def", "ab", 4, "中文abdef", 3, 5),
                    (5, 6, 2, 2, "中文 def", "abc", 5, "中文abcdef", 4, 6),
                    (6, 9, 2, 2, "中文def", "abc ", 6, "中文abc ", 5, 7),
                    (7, 9, 6, 6, "中文abc ef", "d", 7, "中文abc d", 6, 8),
                    (8, 9, 6, 6, "中文abc f", "de", 8, "中文abc de", 7, 9),
                    (9, 9, 6, 6, "中文abc ", "def", 9, "中文abc def", 8, 9),
                ],
            ),
            (
                "กาแฟสวัสดี",
                &[
                    (0, 4, 0, 0, "กาแฟสวัสดี", "", 0, "สวัสดี", 0, 1),
                    (1, 4, 0, 0, "าแฟสวัสดี", "ก", 1, "กสวัสดี", 0, 2),
                    (2, 4, 0, 0, "แฟสวัสดี", "กา", 2, "กาสวัสดี", 1, 3),
                    (3, 4, 0, 0, "ฟสวัสดี", "กาแ", 3, "กาแสวัสดี", 2, 4),
                    (4, 10, 0, 0, "สวัสดี", "กาแฟ", 4, "กาแฟ", 3, 5),
                    (5, 10, 4, 4, "กาแฟวัสดี", "ส", 5, "กาแฟส", 4, 7),
                    (7, 10, 4, 4, "กาแฟสดี", "สวั", 7, "กาแฟสวั", 5, 8),
                    (8, 10, 4, 4, "กาแฟดี", "สวัส", 8, "กาแฟสวัส", 7, 10),
                    (10, 10, 4, 4, "กาแฟ", "สวัสดี", 10, "กาแฟสวัสดี", 8, 10),
                ],
            ),
        ];
        let mut differences = Vec::new();
        for (text, operations) in cases {
            for &(
                offset,
                next,
                previous,
                before_offset,
                before_text,
                killed,
                after_offset,
                after_text,
                left,
                right,
            ) in *operations
            {
                let cursor = Cursor::from_text_utf16(*text, 80, offset);
                let before = cursor.delete_word_before();
                let after = cursor.delete_word_after();
                let actual = (
                    cursor.next_word().offset_utf16(),
                    cursor.prev_word().offset_utf16(),
                    cursor.left().offset_utf16(),
                    cursor.right().offset_utf16(),
                    (
                        before.cursor.text(),
                        before.cursor.offset_utf16(),
                        before.killed.as_str(),
                    ),
                    (after.text(), after.offset_utf16()),
                );
                let expected = (
                    next,
                    previous,
                    left,
                    right,
                    (before_text, before_offset, killed),
                    (after_text, after_offset),
                );
                if actual != expected {
                    differences.push(format!(
                        "{text:?}@{offset} [next,prev,left,right,delete-before,delete-after]: actual={actual:?}, Bun oracle / accepted ICU78 expectation={expected:?}"
                    ));
                }
            }
        }
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }

    #[test]
    fn utf16_constructor_and_modify_text_preserve_source_nfc_number_semantics() {
        let cursor = Cursor::from_text_utf16("e\u{301}lan", 80, 2);
        assert_eq!(
            (cursor.text(), cursor.offset(), cursor.offset_utf16()),
            ("élan", 3, 2)
        );
        let cursor = Cursor::from_text_utf16("ᄀ x", 80, 1).insert("ᅡ");
        assert_eq!(
            (cursor.text(), cursor.offset(), cursor.offset_utf16()),
            ("가 x", 4, 2)
        );
        let cursor = Cursor::from_text_utf16("e x", 80, 1).insert("\u{301}");
        assert_eq!(
            (cursor.text(), cursor.offset(), cursor.offset_utf16()),
            ("é x", 3, 2)
        );
        let cursor = Cursor::from_text_utf16("👩‍💻x", 80, 2);
        assert_eq!(cursor.offset(), 4); // source constructor does not grapheme-snap
        let cursor = cursor.insert("!");
        assert_eq!((cursor.text(), cursor.offset_utf16()), ("👩!‍💻x", 3));
    }

    #[test]
    fn yank_ring_preserves_original_raw_utf16_lengths_across_nfc() {
        // Source useTextInput.ts:198-222 records raw text.length even though
        // Cursor.insert normalizes. The subsequent slice therefore also removes
        // the space after é: do not silently replace the source length with NFC.
        clear_kill_ring_for_tests();
        push_to_kill_ring("FIRST", KillDirection::Append);
        kill_ring::reset_kill_accumulation();
        push_to_kill_ring("e\u{301}", KillDirection::Append);
        let cursor = Cursor::from_text_utf16("🙂 x", 80, 2).yank();
        assert_eq!((cursor.text(), cursor.offset_utf16()), ("🙂é x", 3));
        let cursor = cursor.yank_pop();
        assert_eq!((cursor.text(), cursor.offset_utf16()), ("🙂FIRSTx", 7));
        clear_kill_ring_for_tests();
    }

    #[test]
    fn nfc_seam_matches_official_surrogate_internal_numeric_navigation() {
        // Cursor.ts:845-859,301-319; actual TS shared-intl-surrogate-oracle.json.
        // A scalar-only input reaches this position after NFC; do not snap it.
        let cursor = Cursor::from_text_utf16("e🙂", 80, 1).insert("\u{0301}");
        assert_eq!(
            (cursor.text(), cursor.offset_utf16(), cursor.offset()),
            ("é🙂", 2, 2)
        );
        assert_eq!(cursor.left().offset_utf16(), 1);
        assert_eq!(cursor.right().offset_utf16(), 3);
        assert_eq!(cursor.prev_word().offset_utf16(), 0);
        assert_eq!(cursor.next_word().offset_utf16(), 3);
        assert_eq!(cursor.insert("").offset_utf16(), 2);
        assert_eq!(cursor.insert("").text(), "é🙂");
        // Source del emits [00E9,D83D] and backspace [00E9,DE42]. The final
        // unpaired code unit is explicitly lossy at Rust String's boundary;
        // these assert the source UTF-16 edit projected through that boundary.
        let forward = cursor.del();
        assert_eq!(forward.text(), String::from_utf16_lossy(&[0x00e9, 0xd83d]));
        assert_eq!(forward.offset_utf16(), 2);
        let backward = cursor.backspace();
        assert_eq!(backward.text(), String::from_utf16_lossy(&[0x00e9, 0xde42]));
        assert_eq!(backward.offset_utf16(), 1);
    }

    #[test]
    fn surrogate_internal_offset_matches_official_start_end_guards() {
        // Actual Cursor.ts constructor, isAtStart/isAtEnd and del/backspace:
        // Cursor.fromText("🙂",80,1) is neither endpoint although native byte=0.
        let cursor = Cursor::from_text_utf16("🙂", 80, 1);
        assert_eq!(cursor.offset(), 0);
        assert_eq!(cursor.offset_utf16(), 1);
        assert!(!cursor.is_at_start());
        assert!(!cursor.is_at_end());
        assert_eq!(cursor.left().offset_utf16(), 0);
        assert_eq!(cursor.right().offset_utf16(), 2);
        // Source keeps D83D/DE42 respectively; String projects either to U+FFFD.
        let forward = cursor.del();
        assert_eq!(forward.text(), String::from_utf16_lossy(&[0xd83d]));
        assert_eq!(forward.offset_utf16(), 1);
        let backward = cursor.backspace();
        assert_eq!(backward.text(), String::from_utf16_lossy(&[0xde42]));
        assert_eq!(backward.offset_utf16(), 0);
    }

    #[test]
    fn del_and_backspace_match_official_image_chip_navigation() {
        // Cursor.ts:866-877 calls right/left, including their Image chip hop.
        let forward = Cursor::from_text_utf16("x[Image #12]y", 80, 1).del();
        let backward = Cursor::from_text_utf16("x[Image #12]y", 80, 12).backspace();
        assert_eq!((forward.text(), forward.offset_utf16()), ("xy", 1));
        assert_eq!((backward.text(), backward.offset_utf16()), ("xy", 1));
    }
}
