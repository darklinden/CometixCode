//! Maps to: CC `components/diff/DiffDialog.tsx:1-289`.

use super::diff_detail_view::DiffDetailView;
use super::diff_file_list::DiffFileList;
use crate::components::design_system::dialog::Dialog;
use crate::context::overlay_context::OverlayRegistration;
use crate::hooks::use_diff_data::{DiffData, DiffFile, use_diff_data};
use crate::hooks::use_turn_diffs::{TurnDiff, use_turn_diffs};
use crate::keybindings::keybinding_context::KeybindingRuntime;
use crate::keybindings::shortcut_format::get_shortcut_display;
use crate::keybindings::types::ContextName;
use crate::keybindings::use_keybinding::use_keybinding;
use crate::types::message::Message;
use crate::utils::theme::Theme;
use crate::utils::worktree::CommandResultDisplay;
use iocraft::prelude::*;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffDialogDone {
    pub result: String,
    pub display: CommandResultDisplay,
}

#[derive(Default, Props)]
pub struct DiffDialogProps<'a> {
    pub messages: Arc<Vec<Message>>,
    pub on_done: HandlerMut<'a, DiffDialogDone>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ViewMode {
    #[default]
    List,
    Detail,
}

/// Maps to: CC `DiffDialog.tsx:29-57` `turnDiffToDiffData`.
pub fn turn_diff_to_diff_data(turn: &TurnDiff) -> DiffData {
    let files = turn
        .files
        .values()
        .map(|file| DiffFile {
            path: file.file_path.clone(),
            lines_added: file.lines_added,
            lines_removed: file.lines_removed,
            is_new_file: file.is_new_file,
            ..DiffFile::default()
        })
        .collect::<Vec<_>>();
    let hunks = turn
        .files
        .values()
        .map(|file| (file.file_path.clone(), file.hunks.clone()))
        .collect();
    DiffData {
        stats: Some(crate::utils::git_diff::GitDiffStats {
            files_count: turn.stats.files_changed,
            lines_added: turn.stats.lines_added,
            lines_removed: turn.stats.lines_removed,
        }),
        files,
        hunks,
        loading: false,
    }
}

/// Maps to: CC `DiffDialog.tsx:206-223` empty-state selection.
pub fn diff_empty_message(data: &DiffData, is_turn: bool) -> &'static str {
    if data.loading {
        "Loading diff…"
    } else if is_turn {
        "No file changes in this turn"
    } else if data
        .stats
        .as_ref()
        .is_some_and(|stats| stats.files_count > 0 && data.files.is_empty())
    {
        "Too many files to display details"
    } else {
        "Working tree is clean"
    }
}

fn plural_file(count: usize) -> &'static str {
    if count == 1 { "file" } else { "files" }
}

/// Maps to: CC `components/diff/DiffDialog.tsx:59-289` `DiffDialog`.
#[component]
pub fn DiffDialog<'a>(
    props: &mut DiffDialogProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let git_diff_data = use_diff_data(&mut hooks);
    let turn_diffs = use_turn_diffs(&mut hooks, &props.messages);

    let view_mode = hooks.use_state(ViewMode::default);
    let mut selected_index = hooks.use_state(|| 0usize);
    let mut source_index = hooks.use_state(|| 0usize);
    let mut previous_source_index = hooks.use_state(|| 0usize);
    let mut pending_done = hooks.use_state(|| Option::<DiffDialogDone>::None);

    let source_count = turn_diffs.len() + 1;
    if source_index.get() >= source_count {
        source_index.set(source_count.saturating_sub(1));
    }
    if previous_source_index.get() != source_index.get() {
        selected_index.set(0);
        previous_source_index.set(source_index.get());
    }

    // Maps to: CC `DiffDialog.tsx:103-105` `useRegisterOverlay('diff-dialog')`.
    let overlay_store = hooks
        .try_use_context::<crate::state::store::AppStore>()
        .map(|store| store.clone());
    let _overlay_registration = hooks.use_state(move || {
        overlay_store.map(|store| OverlayRegistration::register(store, "diff-dialog"))
    });

    let current_turn = source_index
        .get()
        .checked_sub(1)
        .and_then(|index| turn_diffs.get(index));
    let diff_data = current_turn
        .map(turn_diff_to_diff_data)
        .unwrap_or_else(|| git_diff_data.clone());
    let selected_file = diff_data.files.get(selected_index.get()).cloned();
    let selected_hunks = selected_file
        .as_ref()
        .and_then(|file| diff_data.hunks.get(&file.path))
        .cloned()
        .unwrap_or_default();

    let keybinding_runtime = hooks
        .try_use_context::<KeybindingRuntime>()
        .map(|runtime| runtime.clone());

    // Maps to: CC `DiffDialog.tsx:117-156` `useKeybindings`.
    use_keybinding(
        &mut hooks,
        keybinding_runtime.clone(),
        "diff:previousSource",
        ContextName::DiffDialog,
        || true,
        {
            let mut view_mode = view_mode;
            let mut source_index = source_index;
            move || {
                if view_mode.get() == ViewMode::Detail {
                    view_mode.set(ViewMode::List);
                } else if source_count > 1 {
                    source_index.set(source_index.get().saturating_sub(1));
                }
                true
            }
        },
    );
    use_keybinding(
        &mut hooks,
        keybinding_runtime.clone(),
        "diff:nextSource",
        ContextName::DiffDialog,
        || true,
        {
            let mut source_index = source_index;
            move || {
                if view_mode.get() == ViewMode::List && source_count > 1 {
                    source_index.set((source_index.get() + 1).min(source_count - 1));
                }
                true
            }
        },
    );
    use_keybinding(
        &mut hooks,
        keybinding_runtime.clone(),
        "diff:back",
        ContextName::DiffDialog,
        || true,
        {
            let mut view_mode = view_mode;
            move || {
                if view_mode.get() == ViewMode::Detail {
                    view_mode.set(ViewMode::List);
                }
                true
            }
        },
    );
    use_keybinding(
        &mut hooks,
        keybinding_runtime.clone(),
        "diff:viewDetails",
        ContextName::DiffDialog,
        || true,
        {
            let mut view_mode = view_mode;
            let has_selected_file = selected_file.is_some();
            move || {
                if view_mode.get() == ViewMode::List && has_selected_file {
                    view_mode.set(ViewMode::Detail);
                }
                true
            }
        },
    );
    use_keybinding(
        &mut hooks,
        keybinding_runtime.clone(),
        "diff:previousFile",
        ContextName::DiffDialog,
        || true,
        {
            let mut selected_index = selected_index;
            move || {
                if view_mode.get() == ViewMode::List {
                    selected_index.set(selected_index.get().saturating_sub(1));
                }
                true
            }
        },
    );
    use_keybinding(
        &mut hooks,
        keybinding_runtime,
        "diff:nextFile",
        ContextName::DiffDialog,
        || true,
        {
            let mut selected_index = selected_index;
            let last_index = diff_data.files.len().saturating_sub(1);
            move || {
                if view_mode.get() == ViewMode::List {
                    selected_index.set((selected_index.get() + 1).min(last_index));
                }
                true
            }
        },
    );

    let done = { pending_done.read().clone() };
    if let Some(done) = done {
        pending_done.set(None);
        (props.on_done)(done);
    }

    let header_title = current_turn
        .map(|turn| format!("Turn {}", turn.turn_index))
        .unwrap_or_else(|| "Uncommitted changes".to_string());
    let header_subtitle = current_turn
        .map(|turn| {
            if turn.user_prompt_preview.is_empty() {
                String::new()
            } else {
                format!("\"{}\"", turn.user_prompt_preview)
            }
        })
        .unwrap_or_else(|| "(git diff HEAD)".to_string());

    let source_selector =
        if source_count > 1 {
            let mut children = Vec::new();
            if source_index.get() > 0 {
                children.push(element! {
                Text(content: "◀ ".to_string(), color: theme.inactive, wrap: TextWrap::NoWrap)
            }.into_any());
            }
            for index in 0..source_count {
                let selected = index == source_index.get();
                let label = if index == 0 {
                    "Current".to_string()
                } else {
                    format!("T{}", turn_diffs[index - 1].turn_index)
                };
                children.push(
                    element! {
                        Text(
                            content: format!("{}{label}", if index > 0 { " · " } else { "" }),
                            color: (!selected).then_some(theme.inactive),
                            weight: if selected { Weight::Bold } else { Weight::Normal },
                            wrap: TextWrap::NoWrap,
                        )
                    }
                    .into_any(),
                );
            }
            if source_index.get() < source_count - 1 {
                children.push(element! {
                Text(content: " ▶".to_string(), color: theme.inactive, wrap: TextWrap::NoWrap)
            }.into_any());
            }
            Some(
                element! {
                    View(flex_direction: FlexDirection::Row) { #(children) }
                }
                .into_any(),
            )
        } else {
            None
        };

    let stats = diff_data.stats.as_ref().map(|stats| {
        element! {
            View(flex_direction: FlexDirection::Row) {
                Text(
                    content: format!("{} {} changed", stats.files_count, plural_file(stats.files_count)),
                    color: theme.inactive,
                    wrap: TextWrap::NoWrap,
                )
                #(if stats.lines_added > 0 {
                    Some(element! {
                        Text(content: format!(" +{}", stats.lines_added), color: theme.diff_added_word, wrap: TextWrap::NoWrap)
                    })
                } else { None })
                #(if stats.lines_removed > 0 {
                    Some(element! {
                        Text(content: format!(" -{}", stats.lines_removed), color: theme.diff_removed_word, wrap: TextWrap::NoWrap)
                    })
                } else { None })
            }
        }
        .into_any()
    });

    let dismiss_shortcut = get_shortcut_display("diff:dismiss", &ContextName::DiffDialog, "esc");
    let input_guide = if view_mode.get() == ViewMode::List {
        [
            (source_count > 1).then_some("←/→ source"),
            Some("↑/↓ select"),
            Some("Enter view"),
            Some(&format!("{dismiss_shortcut} close")),
        ]
        .into_iter()
        .flatten()
        .map(str::to_string)
        .collect::<Vec<_>>()
        .join(" · ")
    } else {
        format!("← back · {dismiss_shortcut} close")
    };

    let body = if diff_data.files.is_empty() {
        element! {
            View(margin_top: 1u32) {
                Text(
                    content: diff_empty_message(&diff_data, current_turn.is_some()).to_string(),
                    color: theme.inactive,
                    wrap: TextWrap::NoWrap,
                )
            }
        }
        .into_any()
    } else if view_mode.get() == ViewMode::List {
        element! {
            View(flex_direction: FlexDirection::Column, margin_top: 1u32) {
                DiffFileList(files: diff_data.files.clone(), selected_index: selected_index.get())
            }
        }
        .into_any()
    } else {
        let selected_file = selected_file.unwrap_or_default();
        element! {
            View(flex_direction: FlexDirection::Column, margin_top: 1u32) {
                DiffDetailView(
                    file_path: selected_file.path,
                    hunks: selected_hunks,
                    is_large_file: selected_file.is_large_file,
                    is_binary: selected_file.is_binary,
                    is_truncated: selected_file.is_truncated,
                    is_untracked: selected_file.is_untracked,
                )
            }
        }
        .into_any()
    };

    let mut view_mode_for_cancel = view_mode;
    let mut pending_done_for_cancel = pending_done;
    element! {
        Dialog(
            title: String::new(),
            title_children: vec![
                element! {
                    Text(content: header_title, color: theme.background, weight: Weight::Bold, wrap: TextWrap::NoWrap)
                }.into_any(),
                element! {
                    Text(
                        content: if header_subtitle.is_empty() { String::new() } else { format!(" {header_subtitle}") },
                        color: theme.inactive,
                        weight: Weight::Bold,
                        wrap: TextWrap::NoWrap,
                    )
                }.into_any(),
            ],
            color: theme.background,
            input_guide: input_guide,
            on_cancel: move |_| {
                if view_mode_for_cancel.get() == ViewMode::Detail {
                    view_mode_for_cancel.set(ViewMode::List);
                } else {
                    pending_done_for_cancel.set(Some(DiffDialogDone {
                        result: "Diff dialog dismissed".to_string(),
                        display: CommandResultDisplay::System,
                    }));
                }
            },
        ) {
            #(source_selector)
            #(stats)
            #(vec![body])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::use_diff_data::DiffDataOverride;
    use crate::utils::git_diff::GitDiffStats;
    use crate::utils::theme;
    use futures::{StreamExt, stream};
    use std::sync::Mutex;
    use std::time::Duration;

    fn data(files: Vec<DiffFile>) -> DiffData {
        DiffData {
            stats: Some(GitDiffStats {
                files_count: files.len(),
                lines_added: 3,
                lines_removed: 2,
            }),
            files,
            loading: false,
            ..DiffData::default()
        }
    }

    #[test]
    fn turn_diff_to_diff_data_matches_official_sorted_file_projection() {
        let turn = TurnDiff {
            stats: crate::hooks::use_turn_diffs::TurnDiffStats {
                files_changed: 1,
                lines_added: 2,
                lines_removed: 1,
            },
            files: std::collections::BTreeMap::from([(
                "src/lib.rs".to_string(),
                crate::hooks::use_turn_diffs::TurnFileDiff {
                    file_path: "src/lib.rs".to_string(),
                    lines_added: 2,
                    lines_removed: 1,
                    is_new_file: false,
                    hunks: vec![crate::types::message::StructuredDiffHunk::default()],
                },
            )]),
            ..TurnDiff::default()
        };
        let data = turn_diff_to_diff_data(&turn);
        assert_eq!(data.files[0].path, "src/lib.rs");
        assert_eq!(data.stats.unwrap().lines_added, 2);
        assert_eq!(data.hunks["src/lib.rs"].len(), 1);
    }

    #[test]
    fn diff_empty_message_matches_all_official_branches() {
        assert_eq!(
            diff_empty_message(
                &DiffData {
                    loading: true,
                    ..DiffData::default()
                },
                false
            ),
            "Loading diff…"
        );
        assert_eq!(
            diff_empty_message(&DiffData::default(), true),
            "No file changes in this turn"
        );
        assert_eq!(
            diff_empty_message(
                &DiffData {
                    stats: Some(GitDiffStats {
                        files_count: 501,
                        ..GitDiffStats::default()
                    }),
                    ..DiffData::default()
                },
                false,
            ),
            "Too many files to display details"
        );
        assert_eq!(
            diff_empty_message(&DiffData::default(), false),
            "Working tree is clean"
        );
    }

    #[test]
    fn diff_dialog_renders_official_current_header_stats_list_and_guide() {
        let current_theme = *theme::current();
        let text = element! {
            ContextProvider(value: Context::owned(current_theme)) {
                ContextProvider(value: Context::owned(DiffDataOverride(data(vec![
                    DiffFile {
                        path: "src/lib.rs".to_string(),
                        lines_added: 3,
                        lines_removed: 2,
                        ..DiffFile::default()
                    },
                ])))) {
                    DiffDialog(messages: Arc::new(Vec::new()))
                }
            }
        }
        .render(Some(100))
        .to_string();

        assert!(
            text.contains("Uncommitted changes (git diff HEAD)"),
            "canvas=\n{text}"
        );
        assert!(text.contains("1 file changed +3 -2"), "canvas=\n{text}");
        assert!(text.contains("src/lib.rs"), "canvas=\n{text}");
        assert!(
            text.contains("↑/↓ select · Enter view · Esc close"),
            "canvas=\n{text}"
        );
    }

    #[test]
    fn diff_dialog_navigation_and_cancel_match_official_detail_then_dismiss_flow() {
        let current_theme = *theme::current();
        let done = Arc::new(Mutex::new(Vec::<DiffDialogDone>::new()));
        let done_for_handler = Arc::clone(&done);
        let mut current = data(vec![DiffFile {
            path: "src/lib.rs".to_string(),
            lines_added: 1,
            lines_removed: 1,
            ..DiffFile::default()
        }]);
        current.hunks.insert(
            "src/lib.rs".to_string(),
            vec![crate::types::message::StructuredDiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec!["-old".to_string(), "+new".to_string()],
            }],
        );
        let events = vec![
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                30u64,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Left)),
                30u64,
            ),
            (
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
                30u64,
            ),
        ];

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(
                    crate::keybindings::keybinding_context::KeybindingRuntime::with_default_bindings()
                )) {
                    ContextProvider(value: Context::owned(current_theme)) {
                        ContextProvider(value: Context::owned(DiffDataOverride(current))) {
                            // The detail view renders StructuredDiffList, which
                            // reads `settings.syntax_highlighting_disabled` —
                            // strict since P7.
                            crate::state::app_state::AppStateProvider(
                                children: crate::state::app_state::ProviderChildren::new(move || element! {
                                    DiffDialog(
                                        messages: Arc::new(Vec::new()),
                                        on_done: {
                                            let done_for_handler = Arc::clone(&done_for_handler);
                                            move |result| done_for_handler.lock().unwrap().push(result)
                                        },
                                    )
                                }.into_any()),
                            )
                        }
                    }
                }
            };
            let event_stream = stream::unfold(events.into_iter(), |mut events| async move {
                let (event, delay_ms) = events.next()?;
                futures_timer::Delay::new(Duration::from_millis(delay_ms)).await;
                Some((event, events))
            });
            let mut render_loop = Box::pin(app.mock_terminal_render_loop(
                MockTerminalConfig::with_events(event_stream).with_size(100, 30),
            ));
            for _ in 0..12 {
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

        let done = done.lock().unwrap();
        assert_eq!(
            done.as_slice(),
            &[DiffDialogDone {
                result: "Diff dialog dismissed".to_string(),
                display: CommandResultDisplay::System,
            }]
        );
    }
}
