//! Maps to: CC `components/HistorySearchDialog.tsx`.
//!
//! Safety boundary: the official dialog asynchronously reads prompt history and
//! lazily resolves pasted content before `onSelect`. Cometix keeps the official
//! picker shell, query/editing behavior, exact-then-subsequence filtering,
//! age/preview helpers, and select/cancel callbacks from explicit snapshots.
//! History file I/O and paste-store resolution remain in the prompt-history
//! runtime slice.

use crate::components::design_system::fuzzy_picker::{
    FuzzyPicker, FuzzyPickerDirection, FuzzyPickerItem, FuzzyPickerPreviewPosition,
};
use crate::components::prompt_input::input_paste::PastedContent;
use crate::hooks::use_search_input::SearchInput;
use crate::utils::format::format_relative_time_ago_millis;
use crate::utils::prompt_history::HistoryEntry;
use crate::utils::truncate::truncate_to_width;
use chrono::Utc;
use iocraft::prelude::*;
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

const PREVIEW_ROWS: usize = 6;
const AGE_WIDTH: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimestampedHistoryEntry {
    pub display: String,
    pub timestamp_ms: i64,
    pub pasted_contents: BTreeMap<usize, PastedContent>,
}

impl TimestampedHistoryEntry {
    pub fn new(display: impl Into<String>, timestamp_ms: i64) -> Self {
        Self {
            display: display.into(),
            timestamp_ms,
            pasted_contents: BTreeMap::new(),
        }
    }

    pub fn with_pasted_contents(mut self, pasted_contents: BTreeMap<usize, PastedContent>) -> Self {
        self.pasted_contents = pasted_contents;
        self
    }
}

#[derive(Default, Props)]
pub struct HistorySearchDialogProps<'a> {
    pub initial_query: Option<String>,
    pub entries: Option<Vec<TimestampedHistoryEntry>>,
    pub now_ms: Option<i64>,
    pub on_select: HandlerMut<'a, HistoryEntry>,
    pub on_cancel: HandlerMut<'a, ()>,
}

/// Maps to: CC `HistorySearchDialog.tsx` `isSubsequence`.
pub fn history_search_is_subsequence(text: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut query_chars = query.chars();
    let mut wanted = query_chars.next();
    for ch in text.chars() {
        if Some(ch) == wanted {
            wanted = query_chars.next();
            if wanted.is_none() {
                return true;
            }
        }
    }
    false
}

/// Maps to: CC `HistorySearchDialog.tsx` `filtered` memo: exact includes first,
/// fuzzy subsequence matches second.
pub fn filter_history_search_entries(
    entries: &[TimestampedHistoryEntry],
    query: &str,
) -> Vec<TimestampedHistoryEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return entries.to_vec();
    }

    let mut exact = Vec::new();
    let mut fuzzy = Vec::new();
    for entry in entries {
        let lower = entry.display.to_lowercase();
        if lower.contains(&q) {
            exact.push(entry.clone());
        } else if history_search_is_subsequence(&lower, &q) {
            fuzzy.push(entry.clone());
        }
    }
    exact.extend(fuzzy);
    exact
}

pub fn history_search_first_line(display: &str) -> &str {
    display
        .split_once('\n')
        .map(|(first, _)| first)
        .unwrap_or(display)
}

pub fn history_search_age(timestamp_ms: i64, now_ms: i64) -> String {
    let age = format_relative_time_ago_millis(timestamp_ms, now_ms);
    let width = UnicodeWidthStr::width(age.as_str());
    format!("{}{}", age, " ".repeat(AGE_WIDTH.saturating_sub(width)))
}

pub fn history_search_empty_message(entries_loaded: bool, query: &str) -> &'static str {
    if !entries_loaded {
        "Loading…"
    } else if query.is_empty() {
        "No history yet"
    } else {
        "No matching prompts"
    }
}

pub fn history_search_preview_on_right(columns: u16) -> bool {
    columns >= 100
}

fn wrap_preview_lines(display: &str, preview_width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw in display.split('\n') {
        if raw.trim().is_empty() {
            continue;
        }
        if UnicodeWidthStr::width(raw) <= preview_width {
            out.push(raw.to_string());
            continue;
        }
        let mut current = String::new();
        let mut current_width = 0usize;
        for ch in raw.chars() {
            let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width + width > preview_width && !current.is_empty() {
                out.push(current);
                current = String::new();
                current_width = 0;
            }
            current.push(ch);
            current_width += width;
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

/// Maps to: CC `HistorySearchDialog.tsx` `renderPreview` overflow copy.
pub fn history_search_preview_text(display: &str, preview_width: usize) -> String {
    let wrapped = wrap_preview_lines(display, preview_width);
    let overflow = wrapped.len() > PREVIEW_ROWS;
    let shown_count = if overflow {
        PREVIEW_ROWS.saturating_sub(1)
    } else {
        PREVIEW_ROWS
    };
    let mut shown = wrapped.into_iter().take(shown_count).collect::<Vec<_>>();
    let more = wrap_preview_lines(display, preview_width)
        .len()
        .saturating_sub(shown.len());
    if more > 0 {
        shown.push(format!("… +{more} more lines"));
    }
    shown.join("\n")
}

fn history_search_items(
    entries: &[TimestampedHistoryEntry],
    now_ms: i64,
    columns: u16,
    preview_on_right: bool,
) -> Vec<FuzzyPickerItem> {
    let list_width = if preview_on_right {
        ((usize::from(columns)).saturating_sub(6)) / 2
    } else {
        usize::from(columns).saturating_sub(6)
    };
    let row_width = list_width.saturating_sub(AGE_WIDTH + 1).max(20);
    let preview_width = if preview_on_right {
        usize::from(columns)
            .saturating_sub(list_width)
            .saturating_sub(12)
            .max(20)
    } else {
        usize::from(columns).saturating_sub(10).max(20)
    };

    entries
        .iter()
        .map(|entry| FuzzyPickerItem {
            key: entry.timestamp_ms.to_string(),
            label: format!(
                "{} {}",
                history_search_age(entry.timestamp_ms, now_ms),
                truncate_to_width(history_search_first_line(&entry.display), row_width)
            ),
            description: None,
            preview: Some(history_search_preview_text(&entry.display, preview_width)),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HistorySearchAction {
    Cancel,
    Select(HistoryEntry),
}

/// Maps to: CC `components/HistorySearchDialog.tsx` `HistorySearchDialog`.
#[component]
pub fn HistorySearchDialog<'a>(
    props: &mut HistorySearchDialogProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let initial_query = props.initial_query.clone().unwrap_or_default();
    let initial_query_for_query = initial_query.clone();
    let initial_query_len = initial_query.encode_utf16().count();
    let mut input = SearchInput {
        columns: hooks.use_terminal_size().0 as usize,
        query: hooks.use_state(move || initial_query_for_query.clone()),
        cursor: hooks.use_state(move || initial_query_len),
    };
    let focused_index = hooks.use_state(|| 0usize);
    let mut pending_action = hooks.use_state(|| Option::<HistorySearchAction>::None);

    let pending = {
        let action = pending_action.read();
        action.clone()
    };
    if let Some(action) = pending {
        pending_action.set(None);
        match action {
            HistorySearchAction::Cancel => (props.on_cancel)(()),
            HistorySearchAction::Select(entry) => (props.on_select)(entry),
        }
    }

    let (columns, _rows) = hooks.use_terminal_size();
    let query = input.text();
    let entries_loaded = props.entries.is_some();
    let entries = props.entries.clone().unwrap_or_default();
    let filtered = filter_history_search_entries(&entries, &query);
    let now_ms = props
        .now_ms
        .unwrap_or_else(|| Utc::now().timestamp_millis());
    let preview_on_right = history_search_preview_on_right(columns);
    let items = history_search_items(&filtered, now_ms, columns, preview_on_right);
    let item_count = items.len();

    hooks.use_terminal_events({
        let mut focused_index = focused_index;
        let mut pending_action = pending_action;
        let filtered = filtered.clone();
        move |event| {
            let TerminalEvent::Key(KeyEvent {
                code,
                kind,
                modifiers,
                ..
            }) = event
            else {
                return;
            };
            if kind == KeyEventKind::Release {
                return;
            }

            let ctrl = modifiers.contains(KeyModifiers::CONTROL);
            match code {
                KeyCode::Esc => pending_action.set(Some(HistorySearchAction::Cancel)),
                KeyCode::Char('c' | 'C' | 'd' | 'D') if ctrl => {
                    pending_action.set(Some(HistorySearchAction::Cancel));
                }
                KeyCode::Up | KeyCode::Char('p') if matches!(code, KeyCode::Up) || ctrl => {
                    focused_index
                        .set((focused_index.get() + 1).min(filtered.len().saturating_sub(1)));
                }
                KeyCode::Down | KeyCode::Char('n') if matches!(code, KeyCode::Down) || ctrl => {
                    focused_index.set(focused_index.get().saturating_sub(1));
                }
                KeyCode::Enter => {
                    if let Some(entry) = filtered.get(focused_index.get()) {
                        pending_action.set(Some(HistorySearchAction::Select(HistoryEntry {
                            display: entry.display.clone(),
                            timestamp_ms: entry.timestamp_ms,
                            pasted_contents: entry.pasted_contents.clone(),
                        })));
                    }
                }
                _ => {
                    if input.handle_edit_key(&code, &modifiers) {
                        focused_index.set(0);
                    }
                }
            }
        }
    });

    element! {
        FuzzyPicker(
            title: "Search prompts".to_string(),
            placeholder: Some("Filter history…".to_string()),
            query: query.clone(),
            cursor_offset: Some(input.offset()),
            items: items,
            focused_index: focused_index.get().min(item_count.saturating_sub(1)),
            direction: FuzzyPickerDirection::Up,
            preview_position: if preview_on_right { FuzzyPickerPreviewPosition::Right } else { FuzzyPickerPreviewPosition::Bottom },
            empty_message: Some(history_search_empty_message(entries_loaded, &query).to_string()),
            select_action: Some("use".to_string()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::theme;
    use futures::{StreamExt, stream};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }

    #[test]
    fn history_search_helpers_match_official_filtering_and_copy() {
        assert!(history_search_is_subsequence("abcdef", "ace"));
        assert!(!history_search_is_subsequence("abcdef", "aec"));
        assert_eq!(history_search_first_line("first\nsecond"), "first");
        assert_eq!(history_search_empty_message(false, ""), "Loading…");
        assert_eq!(history_search_empty_message(true, ""), "No history yet");
        assert_eq!(
            history_search_empty_message(true, "needle"),
            "No matching prompts"
        );
        assert!(history_search_preview_on_right(100));
        assert_eq!(
            history_search_age(1_700_000_000_000 - 60_000, 1_700_000_000_000),
            "1m ago  "
        );

        let entries = vec![
            TimestampedHistoryEntry::new("deploy prod", 1),
            TimestampedHistoryEntry::new("describe repository", 2),
            TimestampedHistoryEntry::new("abc", 3),
        ];
        assert_eq!(
            filter_history_search_entries(&entries, "dep"),
            vec![
                TimestampedHistoryEntry::new("deploy prod", 1),
                TimestampedHistoryEntry::new("describe repository", 2),
            ]
        );
    }

    #[test]
    fn history_search_preview_text_matches_overflow_copy() {
        let text = history_search_preview_text("one\ntwo\nthree\nfour\nfive\nsix\nseven", 80);
        assert!(text.contains("one"), "preview=\n{text}");
        assert!(text.contains("… +2 more lines"), "preview=\n{text}");
    }

    #[test]
    fn history_search_dialog_renders_loaded_items_preview_and_hints() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                HistorySearchDialog(
                    initial_query: Some("deploy".to_string()),
                    entries: Some(vec![TimestampedHistoryEntry::new("deploy prod\nwith flags", 1_700_000_000_000 - 60_000)]),
                    now_ms: Some(1_700_000_000_000),
                )
            }
        }
        .render(Some(120))
        .to_string();

        assert!(text.contains("Search prompts"), "canvas=\n{text}");
        assert!(text.contains("1m ago"), "canvas=\n{text}");
        assert!(text.contains("deploy prod"), "canvas=\n{text}");
        assert!(text.contains("with flags"), "canvas=\n{text}");
        assert!(text.contains("Enter to use"), "canvas=\n{text}");
    }

    #[test]
    fn history_search_dialog_selects_focused_entry_callback_only() {
        let selected = Arc::new(Mutex::new(Vec::<String>::new()));
        let selected_for_handler = Arc::clone(&selected);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    HistorySearchDialog(
                        initial_query: Some("".to_string()),
                        entries: Some(vec![
                            TimestampedHistoryEntry::new("first prompt", 1),
                            TimestampedHistoryEntry::new("second prompt", 2),
                        ]),
                        now_ms: Some(10_000),
                        on_select: move |entry: HistoryEntry| {
                            selected_for_handler.lock().expect("selected mutex").push(entry.display);
                        },
                    )
                }
            };
            let mut render_loop = Box::pin(
                app.mock_terminal_render_loop(
                    MockTerminalConfig::with_events(stream::iter(vec![
                        key(KeyCode::Up),
                        key(KeyCode::Enter),
                    ]))
                    .with_size(120, 24),
                ),
            );
            for _ in 0..8 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                if next.is_none() {
                    break;
                }
            }
        });

        assert_eq!(
            selected.lock().expect("selected mutex").as_slice(),
            &["second prompt".to_string()]
        );
    }

    #[test]
    fn history_search_dialog_escape_cancels() {
        let cancelled = Arc::new(Mutex::new(0usize));
        let cancelled_for_handler = Arc::clone(&cancelled);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    HistorySearchDialog(
                        entries: Some(vec![TimestampedHistoryEntry::new("first prompt", 1)]),
                        on_cancel: move |_| *cancelled_for_handler.lock().expect("cancel mutex") += 1,
                    )
                }
            };
            let mut render_loop = Box::pin(
                app.mock_terminal_render_loop(
                    MockTerminalConfig::with_events(stream::iter(vec![key(KeyCode::Esc)]))
                        .with_size(120, 24),
                ),
            );
            for _ in 0..8 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                if next.is_none() {
                    break;
                }
            }
        });

        assert_eq!(*cancelled.lock().expect("cancel mutex"), 1);
    }
}
