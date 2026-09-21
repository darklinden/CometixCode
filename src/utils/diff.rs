//! Diff helpers shared by Write/Edit tool results.
//!
//! Maps to: CC `utils/diff.ts` (`getPatchFromContents`,
//! `getPatchForDisplay`, and `countLinesChanged`).

use crate::components::highlighted_code::fallback::convert_leading_tabs_to_spaces;
use crate::types::message::StructuredDiffHunk;
use similar::{ChangeTag, TextDiff};

/// Maps to: CC `utils/diff.ts#CONTEXT_LINES`.
pub const CONTEXT_LINES: usize = 3;
/// Maps to: CC `utils/diff.ts#DIFF_TIMEOUT_MS`.
pub const DIFF_TIMEOUT_MS: u64 = 5_000;
const NO_NEWLINE_MARKER: &str = "\\ No newline at end of file";
const AMPERSAND_TOKEN: &str = "<<:AMPERSAND_TOKEN:>>";
const DOLLAR_TOKEN: &str = "<<:DOLLAR_TOKEN:>>";

fn escape_for_diff(value: &str) -> String {
    value
        .replace('&', AMPERSAND_TOKEN)
        .replace('$', DOLLAR_TOKEN)
}

fn unescape_from_diff(value: &str) -> String {
    value
        .replace(AMPERSAND_TOKEN, "&")
        .replace(DOLLAR_TOKEN, "$")
}

/// Maps to: CC `utils/diff.ts:49-79` `countLinesChanged`.
///
/// Counts lines added and removed in a patch and updates the session total.
/// For new files (empty patch), pass the content: every line counts as an
/// addition, split on `\r?\n` like the source (a truthy check, so an empty
/// string falls through to the zero/zero branch).
pub fn count_lines_changed(patch: &[StructuredDiffHunk], new_file_content: Option<&str>) {
    let (num_additions, num_removals) = match new_file_content {
        Some(content) if patch.is_empty() && !content.is_empty() => {
            (content.split('\n').count(), 0)
        }
        _ => (
            patch
                .iter()
                .map(|hunk| count_added_lines(&hunk.lines))
                .sum(),
            patch
                .iter()
                .map(|hunk| count_removed_lines(&hunk.lines))
                .sum(),
        ),
    };

    crate::cost_tracker::add_to_total_lines_changed(num_additions as u64, num_removals as u64);
    // CC: getLocCounter()?.add(numAdditions/numRemovals, {type}) — joins with
    // OTel metrics.
    // CC: logEvent('tengu_file_changed', {lines_added, lines_removed}) —
    // joins with analytics.
}

/// Maps to: CC `utils/diff.ts:81-114` `getPatchFromContents`.
///
/// Escapes both sides (the `&`/`$` token quirk shared with
/// `get_patch_for_display`), diffs, then unescapes each hunk line.
/// `single_hunk` widens the context window so the whole change lands in one
/// hunk (CC uses 100_000). Narrowed against the source: CC's `filePath` only
/// feeds the unified-diff header, which `StructuredDiffHunk` does not carry,
/// and no caller sets `ignoreWhitespace` (Edit and useDiffInIDE both leave
/// the default), so neither parameter is carried.
pub fn get_patch_from_contents(
    old_content: &str,
    new_content: &str,
    single_hunk: bool,
) -> Vec<StructuredDiffHunk> {
    let context = if single_hunk { 100_000 } else { CONTEXT_LINES };
    let mut hunks = structured_diff_hunks(
        &escape_for_diff(old_content),
        &escape_for_diff(new_content),
        context,
    );
    for hunk in &mut hunks {
        hunk.lines = hunk
            .lines
            .iter()
            .map(|line| unescape_from_diff(line))
            .collect();
    }
    hunks
}

/// One edit for display patching. Maps to the `edits` entries consumed by
/// CC `getPatchForDisplay` (same fields as `FileEdit`).
#[derive(Clone, Copy, Debug)]
pub struct DisplayEdit<'a> {
    pub old_string: &'a str,
    pub new_string: &'a str,
    pub replace_all: bool,
}

/// Maps to: CC `utils/diff.ts#getPatchForDisplay`.
///
/// Applies `edits` onto `file_contents` and returns structured hunks. Leading
/// tabs are converted to spaces for display, matching upstream.
pub fn get_patch_for_display(
    file_contents: &str,
    edits: &[DisplayEdit<'_>],
) -> Vec<StructuredDiffHunk> {
    let prepared = escape_for_diff(&convert_leading_tabs_to_spaces(file_contents));
    let mut updated = prepared.clone();
    for edit in edits {
        let old = escape_for_diff(&convert_leading_tabs_to_spaces(edit.old_string));
        let new = escape_for_diff(&convert_leading_tabs_to_spaces(edit.new_string));
        updated = if edit.replace_all {
            updated.replace(&old, &new)
        } else {
            updated.replacen(&old, &new, 1)
        };
    }
    let mut hunks = structured_diff_hunks(&prepared, &updated, CONTEXT_LINES);
    for hunk in &mut hunks {
        hunk.lines = hunk
            .lines
            .iter()
            .map(|line| unescape_from_diff(line))
            .collect();
    }
    hunks
}

/// Maps to: CC `utils/diff.ts#adjustHunkLineNumbers`.
pub fn adjust_hunk_line_numbers(
    hunks: &[StructuredDiffHunk],
    offset: isize,
) -> Vec<StructuredDiffHunk> {
    if offset == 0 {
        return hunks.to_vec();
    }
    hunks
        .iter()
        .map(|hunk| StructuredDiffHunk {
            old_start: (hunk.old_start as isize + offset).max(1) as usize,
            old_lines: hunk.old_lines,
            new_start: (hunk.new_start as isize + offset).max(1) as usize,
            new_lines: hunk.new_lines,
            lines: hunk.lines.clone(),
        })
        .collect()
}

/// Unified-diff lines produced by an LCS line diff.
pub(crate) fn simple_diff_lines(old: &str, new: &str) -> Vec<String> {
    if old == new {
        return Vec::new();
    }
    TextDiff::from_lines(old, new)
        .iter_all_changes()
        .map(|change| {
            let prefix = match change.tag() {
                ChangeTag::Delete => '-',
                ChangeTag::Insert => '+',
                ChangeTag::Equal => ' ',
            };
            format!("{prefix}{}", change.value().trim_end_matches('\n'))
        })
        .collect()
}

/// Wrap diff lines into a whole-input hunk for legacy callers.
#[allow(dead_code)]
pub(crate) fn simple_diff_hunk(old: &str, new: &str, lines: &[String]) -> StructuredDiffHunk {
    StructuredDiffHunk {
        old_start: 1,
        old_lines: old.lines().count(),
        new_start: 1,
        new_lines: new.lines().count(),
        lines: lines.to_vec(),
    }
}

#[derive(Clone, Debug)]
struct DiffComponent {
    count: usize,
    added: bool,
    removed: bool,
    previous: Option<std::sync::Arc<DiffComponent>>,
}

#[derive(Clone, Debug)]
struct DiffPath {
    old_position: isize,
    last_component: Option<std::sync::Arc<DiffComponent>>,
}

#[derive(Clone, Debug)]
struct LineDiffPart {
    added: bool,
    removed: bool,
    lines: Vec<String>,
}

fn line_tokens(value: &str) -> Vec<&str> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let mut start = 0;
    for (index, byte) in value.bytes().enumerate() {
        if byte == b'\n' {
            tokens.push(&value[start..=index]);
            start = index + 1;
        }
    }
    if start < value.len() {
        tokens.push(&value[start..]);
    }
    tokens
}

fn add_component(path: &mut DiffPath, added: bool, removed: bool, count: usize) {
    if count == 0 {
        return;
    }
    path.last_component = match path.last_component.as_ref() {
        Some(last) if last.added == added && last.removed == removed => {
            Some(std::sync::Arc::new(DiffComponent {
                count: last.count + count,
                added,
                removed,
                previous: last.previous.clone(),
            }))
        }
        previous => Some(std::sync::Arc::new(DiffComponent {
            count,
            added,
            removed,
            previous: previous.cloned(),
        })),
    };
}

fn extract_common(
    path: &mut DiffPath,
    new_tokens: &[&str],
    old_tokens: &[&str],
    diagonal: isize,
) -> isize {
    let mut old_position = path.old_position;
    let mut new_position = old_position - diagonal;
    let mut common_count = 0usize;
    while new_position + 1 < new_tokens.len() as isize
        && old_position + 1 < old_tokens.len() as isize
        && old_tokens[(old_position + 1) as usize] == new_tokens[(new_position + 1) as usize]
    {
        old_position += 1;
        new_position += 1;
        common_count += 1;
    }
    add_component(path, false, false, common_count);
    path.old_position = old_position;
    new_position
}

fn build_line_diff_parts(
    last_component: Option<std::sync::Arc<DiffComponent>>,
    new_tokens: &[&str],
    old_tokens: &[&str],
) -> Vec<LineDiffPart> {
    let mut components = Vec::new();
    let mut current = last_component;
    while let Some(component) = current {
        current = component.previous.clone();
        components.push(component);
    }
    components.reverse();

    let mut new_position = 0usize;
    let mut old_position = 0usize;
    components
        .into_iter()
        .map(|component| {
            let lines = if component.removed {
                let lines = old_tokens[old_position..old_position + component.count]
                    .iter()
                    .map(|line| (*line).to_string())
                    .collect();
                old_position += component.count;
                lines
            } else {
                let lines = new_tokens[new_position..new_position + component.count]
                    .iter()
                    .map(|line| (*line).to_string())
                    .collect();
                new_position += component.count;
                if !component.added {
                    old_position += component.count;
                }
                lines
            };
            LineDiffPart {
                added: component.added,
                removed: component.removed,
                lines,
            }
        })
        .collect()
}

/// Exact Rust projection of `diff`'s line-level Myers implementation and its
/// remove-vs-add tie breaking (`diff/libesm/diff/base.js`).
fn npm_line_diff(old: &str, new: &str) -> Option<Vec<LineDiffPart>> {
    npm_line_diff_with_timeout(old, new, std::time::Duration::from_millis(DIFF_TIMEOUT_MS))
}

fn npm_line_diff_with_timeout(
    old: &str,
    new: &str,
    timeout: std::time::Duration,
) -> Option<Vec<LineDiffPart>> {
    let old_tokens = line_tokens(old);
    let new_tokens = line_tokens(new);
    let old_len = old_tokens.len() as isize;
    let new_len = new_tokens.len() as isize;
    let mut initial = DiffPath {
        old_position: -1,
        last_component: None,
    };
    let new_position = extract_common(&mut initial, &new_tokens, &old_tokens, 0);
    if initial.old_position + 1 >= old_len && new_position + 1 >= new_len {
        return Some(build_line_diff_parts(
            initial.last_component,
            &new_tokens,
            &old_tokens,
        ));
    }

    let mut best_paths = std::collections::HashMap::<isize, DiffPath>::new();
    best_paths.insert(0, initial);
    let max_edit_length = old_tokens.len() + new_tokens.len();
    let started = std::time::Instant::now();
    let mut minimum_diagonal = isize::MIN;
    let mut maximum_diagonal = isize::MAX;

    for edit_length in 1..=max_edit_length {
        if started.elapsed() > timeout {
            return None;
        }
        let edit_length = edit_length as isize;
        let mut diagonal = minimum_diagonal.max(-edit_length);
        while diagonal <= maximum_diagonal.min(edit_length) {
            let remove_path = best_paths.remove(&(diagonal - 1));
            let add_path = best_paths.get(&(diagonal + 1)).cloned();
            let can_add = add_path.as_ref().is_some_and(|path| {
                let position = path.old_position - diagonal;
                position >= 0 && position < new_len
            });
            let can_remove = remove_path
                .as_ref()
                .is_some_and(|path| path.old_position + 1 < old_len);
            if !can_add && !can_remove {
                best_paths.remove(&diagonal);
                diagonal += 2;
                continue;
            }

            let choose_add = !can_remove
                || (can_add
                    && remove_path.as_ref().is_some_and(|remove| {
                        add_path
                            .as_ref()
                            .is_some_and(|add| remove.old_position < add.old_position)
                    }));
            let mut path = if choose_add {
                let mut path = add_path.expect("can_add path");
                add_component(&mut path, true, false, 1);
                path
            } else {
                let mut path = remove_path.expect("can_remove path");
                path.old_position += 1;
                add_component(&mut path, false, true, 1);
                path
            };
            let new_position = extract_common(&mut path, &new_tokens, &old_tokens, diagonal);
            if path.old_position + 1 >= old_len && new_position + 1 >= new_len {
                return Some(build_line_diff_parts(
                    path.last_component,
                    &new_tokens,
                    &old_tokens,
                ));
            }
            if path.old_position + 1 >= old_len {
                maximum_diagonal = maximum_diagonal.min(diagonal - 1);
            }
            if new_position + 1 >= new_len {
                minimum_diagonal = minimum_diagonal.max(diagonal + 1);
            }
            best_paths.insert(diagonal, path);
            diagonal += 2;
        }
    }
    None
}

/// Maps byte-for-byte to `diff`'s `structuredPatch(...).hunks`, including
/// context merging and `\\ No newline at end of file` placement.
pub(crate) fn structured_diff_hunks(
    old: &str,
    new: &str,
    context_radius: usize,
) -> Vec<StructuredDiffHunk> {
    if old == new {
        return Vec::new();
    }
    let Some(mut parts) = npm_line_diff(old, new) else {
        return Vec::new();
    };
    parts.push(LineDiffPart {
        added: false,
        removed: false,
        lines: Vec::new(),
    });

    let mut hunks = Vec::new();
    let mut old_range_start = 0usize;
    let mut new_range_start = 0usize;
    let mut current_range = Vec::<String>::new();
    let mut old_line = 1usize;
    let mut new_line = 1usize;
    for index in 0..parts.len() {
        let current = &parts[index];
        if current.added || current.removed {
            if old_range_start == 0 {
                old_range_start = old_line;
                new_range_start = new_line;
                if let Some(previous) = index.checked_sub(1).and_then(|i| parts.get(i)) {
                    let context = previous
                        .lines
                        .iter()
                        .rev()
                        .take(context_radius)
                        .cloned()
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>();
                    old_range_start = old_range_start.saturating_sub(context.len());
                    new_range_start = new_range_start.saturating_sub(context.len());
                    current_range.extend(context.into_iter().map(|line| format!(" {line}")));
                }
            }
            let prefix = if current.added { '+' } else { '-' };
            current_range.extend(current.lines.iter().map(|line| format!("{prefix}{line}")));
            if current.added {
                new_line += current.lines.len();
            } else {
                old_line += current.lines.len();
            }
            continue;
        }

        if old_range_start != 0 {
            if current.lines.len() <= context_radius * 2 && index < parts.len().saturating_sub(2) {
                current_range.extend(current.lines.iter().map(|line| format!(" {line}")));
            } else {
                let context_size = current.lines.len().min(context_radius);
                current_range.extend(
                    current
                        .lines
                        .iter()
                        .take(context_size)
                        .map(|line| format!(" {line}")),
                );
                hunks.push(StructuredDiffHunk {
                    old_start: old_range_start,
                    old_lines: old_line - old_range_start + context_size,
                    new_start: new_range_start,
                    new_lines: new_line - new_range_start + context_size,
                    lines: std::mem::take(&mut current_range),
                });
                old_range_start = 0;
                new_range_start = 0;
            }
        }
        old_line += current.lines.len();
        new_line += current.lines.len();
    }

    for hunk in &mut hunks {
        let mut index = 0;
        while index < hunk.lines.len() {
            if hunk.lines[index].ends_with('\n') {
                hunk.lines[index].pop();
            } else {
                hunk.lines.insert(index + 1, NO_NEWLINE_MARKER.to_string());
                index += 1;
            }
            index += 1;
        }
    }
    hunks
}

/// Maps to CC `countLinesChanged` — added half.
pub(crate) fn count_added_lines(lines: &[String]) -> usize {
    lines.iter().filter(|line| line.starts_with('+')).count()
}

/// Maps to CC `countLinesChanged` — removed half.
pub(crate) fn count_removed_lines(lines: &[String]) -> usize {
    lines.iter().filter(|line| line.starts_with('-')).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(
        old_start: usize,
        old_lines: usize,
        new_start: usize,
        new_lines: usize,
        lines: &[&str],
    ) -> StructuredDiffHunk {
        StructuredDiffHunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines: lines.iter().map(|line| (*line).to_string()).collect(),
        }
    }

    #[test]
    fn line_diff_keeps_equal_context_instead_of_full_remove_add() {
        let lines = simple_diff_lines("a\nold\nz\n", "a\nnew\nz\n");
        assert_eq!(lines, vec![" a", "-old", "+new", " z"]);
    }

    #[test]
    fn display_patch_roundtrips_official_ampersand_and_dollar_tokens() {
        let hunks = get_patch_for_display(
            "old &= $value\n",
            &[DisplayEdit {
                old_string: "old",
                new_string: "new",
                replace_all: false,
            }],
        );
        assert_eq!(hunks[0].lines, vec!["-old &= $value", "+new &= $value"]);
    }

    #[test]
    fn structured_diff_preserves_official_no_newline_markers() {
        let hunks = structured_diff_hunks("old", "new", 3);
        assert_eq!(
            hunks[0].lines,
            vec![
                "-old",
                "\\ No newline at end of file",
                "+new",
                "\\ No newline at end of file"
            ]
        );
        let hunks = structured_diff_hunks("x", "x\n", 3);
        assert_eq!(
            hunks[0].lines,
            vec!["-x", "\\ No newline at end of file", "+x"]
        );
    }

    #[test]
    fn structured_diff_matches_official_myers_tie_breaking() {
        let hunks = structured_diff_hunks("a\na\na\n", "a\nb\na\n", 3);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].lines, vec![" a", "+b", " a", "-a"]);
    }

    #[test]
    fn structured_diff_splits_distant_changes_with_context() {
        let old = (1..=20).map(|i| format!("old-{i}\n")).collect::<String>();
        let mut new_lines = (1..=20).map(|i| format!("old-{i}\n")).collect::<Vec<_>>();
        new_lines[1] = "new-2\n".to_string();
        new_lines[18] = "new-19\n".to_string();
        let new = new_lines.concat();
        let hunks = structured_diff_hunks(&old, &new, 3);
        assert_eq!(hunks.len(), 2, "hunks={hunks:?}");
        assert!(hunks[0].lines.iter().any(|line| line == "-old-2"));
        assert!(hunks[1].lines.iter().any(|line| line == "+new-19"));
    }

    #[test]
    fn structured_diff_matches_npm_crlf_and_eof_oracles() {
        assert_eq!(
            structured_diff_hunks("a\r\nb\r\nc\r\n", "a\r\nB\r\nc\r\n", 3),
            vec![hunk(1, 3, 1, 3, &[" a\r", "-b\r", "+B\r", " c\r"])]
        );
        assert_eq!(
            structured_diff_hunks("a\nb", "a\nb\n", 3),
            vec![hunk(1, 2, 1, 2, &[" a", "-b", NO_NEWLINE_MARKER, "+b"])]
        );
        assert_eq!(
            structured_diff_hunks("a\nb\n", "a\nb", 3),
            vec![hunk(1, 2, 1, 2, &[" a", "-b", "+b", NO_NEWLINE_MARKER])]
        );
        assert_eq!(
            structured_diff_hunks("a\nb\nc", "a\nB\nc", 1),
            vec![hunk(
                1,
                3,
                1,
                3,
                &[" a", "-b", "+B", " c", NO_NEWLINE_MARKER]
            )]
        );
    }

    #[test]
    fn structured_diff_matches_npm_context_merge_boundary_oracle() {
        let old = (1..=15).map(|i| format!("L{i}\n")).collect::<String>();

        let mut merged_lines = (1..=15).map(|i| format!("L{i}\n")).collect::<Vec<_>>();
        merged_lines[1] = "X2\n".to_string();
        merged_lines[8] = "X9\n".to_string();
        assert_eq!(
            structured_diff_hunks(&old, &merged_lines.concat(), 3),
            vec![hunk(
                1,
                12,
                1,
                12,
                &[
                    " L1", "-L2", "+X2", " L3", " L4", " L5", " L6", " L7", " L8", "-L9", "+X9",
                    " L10", " L11", " L12",
                ]
            )]
        );

        let mut split_lines = (1..=15).map(|i| format!("L{i}\n")).collect::<Vec<_>>();
        split_lines[1] = "X2\n".to_string();
        split_lines[9] = "X10\n".to_string();
        assert_eq!(
            structured_diff_hunks(&old, &split_lines.concat(), 3),
            vec![
                hunk(1, 5, 1, 5, &[" L1", "-L2", "+X2", " L3", " L4", " L5"]),
                hunk(
                    7,
                    7,
                    7,
                    7,
                    &[" L7", " L8", " L9", "-L10", "+X10", " L11", " L12", " L13"]
                ),
            ]
        );
    }

    #[test]
    fn structured_diff_matches_npm_empty_file_oracles() {
        assert_eq!(
            structured_diff_hunks("", "x\n", 3),
            vec![hunk(1, 0, 1, 1, &["+x"])]
        );
        assert_eq!(
            structured_diff_hunks("x\n", "", 3),
            vec![hunk(1, 1, 1, 0, &["-x"])]
        );
    }

    #[test]
    fn display_patch_matches_official_literal_token_collision_behavior() {
        let hunks = get_patch_for_display(
            "literal <<:AMPERSAND_TOKEN:>> and <<:DOLLAR_TOKEN:>>\nold & $\n",
            &[DisplayEdit {
                old_string: "old",
                new_string: "new",
                replace_all: false,
            }],
        );
        assert_eq!(
            hunks,
            vec![hunk(
                1,
                2,
                1,
                2,
                &[" literal & and $", "-old & $", "+new & $"]
            )]
        );
    }

    /// Maps to: CC `utils/diff.ts:81-114` — the edit patch path shares the
    /// display path's literal-token collision quirk, and `singleHunk` widens
    /// the context so distant changes land in one hunk.
    #[test]
    fn edit_patch_from_contents_matches_official_escape_and_single_hunk_behavior() {
        let hunks = get_patch_from_contents(
            "literal <<:AMPERSAND_TOKEN:>> and <<:DOLLAR_TOKEN:>>\nold & $\n",
            "literal <<:AMPERSAND_TOKEN:>> and <<:DOLLAR_TOKEN:>>\nnew & $\n",
            false,
        );
        assert_eq!(
            hunks,
            vec![hunk(
                1,
                2,
                1,
                2,
                &[" literal & and $", "-old & $", "+new & $"]
            )]
        );

        let old = (0..20).map(|i| format!("line-{i}\n")).collect::<String>();
        let new = old
            .replace("line-0\n", "changed-0\n")
            .replace("line-19\n", "changed-19\n");
        assert_eq!(get_patch_from_contents(&old, &new, false).len(), 2);
        assert_eq!(get_patch_from_contents(&old, &new, true).len(), 1);
    }

    /// Maps to: CC `utils/diff.ts:49-79` — hunk prefixes count, the new-file
    /// branch counts every content line as an addition, and the truthy
    /// `newFileContent` check means an empty string adds nothing.
    #[test]
    fn count_lines_changed_counts_hunks_and_new_file_content_like_official() {
        let added_before = crate::cost_tracker::get_total_lines_added();
        let removed_before = crate::cost_tracker::get_total_lines_removed();

        count_lines_changed(
            &[hunk(1, 2, 1, 2, &[" ctx", "-old", "+new", "+extra"])],
            None,
        );
        assert_eq!(
            crate::cost_tracker::get_total_lines_added() - added_before,
            2
        );
        assert_eq!(
            crate::cost_tracker::get_total_lines_removed() - removed_before,
            1
        );

        count_lines_changed(&[], Some("a\nb\nc"));
        assert_eq!(
            crate::cost_tracker::get_total_lines_added() - added_before,
            5
        );

        count_lines_changed(&[], Some(""));
        assert_eq!(
            crate::cost_tracker::get_total_lines_added() - added_before,
            5
        );
    }

    #[test]
    fn npm_line_diff_large_input_honors_timeout_seam() {
        let old = (0..4_000).map(|i| format!("old-{i}\n")).collect::<String>();
        let new = (0..4_000).map(|i| format!("new-{i}\n")).collect::<String>();
        let started = std::time::Instant::now();
        assert!(npm_line_diff_with_timeout(&old, &new, std::time::Duration::ZERO).is_none());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(DIFF_TIMEOUT_MS, 5_000);
    }
}
