//! Maps to: CC
//! `components/permissions/AskUserQuestionPermissionRequest/AskUserQuestionPermissionRequest.tsx`.
//!
//! This module ports the official AskUserQuestion permission UI boundary using
//! the same subcomponent files: `QuestionView`, `PreviewQuestionView`,
//! `PreviewBox`, `QuestionNavigationBar`, `SubmitQuestionsView`, and the
//! multiple-choice state reducer. The custom `Other` text input, external
//! editor handoff, per-question clipboard images, image persistence, and
//! model-visible permission content blocks are owned by this retained shell;
//! analytics remain outside the component.

pub mod preview_box;
pub mod preview_question_view;
pub mod question_navigation_bar;
pub mod question_view;
pub mod submit_questions_view;
pub mod use_multiple_choice_state;

use question_view::QuestionView;
use submit_questions_view::SubmitQuestionsView;
use use_multiple_choice_state::{
    MultipleChoiceAction, MultipleChoiceState, Question, QuestionStateUpdate, answer_for_selection,
    build_updated_input_with_answers, hide_submit_tab, question_has_preview, questions_from_input,
    reduce_multiple_choice_state, toggle_multi_select_value,
};

use crate::components::permissions::worker_badge::WorkerBadgeProps;
use crate::components::prompt_input::input_paste::PastedContent;
use crate::types::permissions::{
    PermissionContentBlock, PermissionMode, PermissionPromptChoice, PermissionPromptResponse,
    PermissionRequest as PermissionRequestData, PermissionRuleValue,
};
use crate::utils::prompt_editor::{EditorResult, ExternalEditorRuntime};
use crate::utils::theme::Theme;
use iocraft::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

const MIN_CONTENT_HEIGHT: usize = 12;
const MIN_CONTENT_WIDTH: usize = 40;
const CONTENT_CHROME_OVERHEAD: usize = 15;

#[derive(Default, Props)]
pub struct AskUserQuestionPermissionRequestProps {
    pub request: Option<PermissionRequestData>,
    pub worker_badge: Option<WorkerBadgeProps>,
    pub on_select: Handler<PermissionPromptResponse>,
    pub on_cancel: Handler<()>,
    /// Deterministic adapter seam for permission image-paste tests.
    pub clipboard_image_override: Option<crate::utils::image_paste::ClipboardImage>,
}

fn default_request() -> PermissionRequestData {
    PermissionRequestData {
        permission_result: None,
        id: String::new(),
        tool_use_id: String::new(),
        tool_name: "AskUserQuestion".to_string(),
        mcp_info: None,
        decision_reason: None,
        description: String::new(),
        message: String::new(),
        input_summary: String::new(),
        input: serde_json::Value::Null,
        call_input: None,
        rule: PermissionRuleValue::new("AskUserQuestion", None),
        suggestions: Vec::new(),
        blocked_path: None,
        metadata: None,
        is_compound_command: false,
        mode: PermissionMode::Default,
    }
}

/// Maps to: CC `globalContentHeight/globalContentWidth` calculation.
pub fn ask_user_question_content_dimensions(
    questions: &[Question],
    terminal_rows: usize,
) -> (usize, usize) {
    let mut max_height = 0usize;
    let mut max_width = MIN_CONTENT_WIDTH;
    let footer_help_lines = 7usize;
    let max_allowed_height =
        MIN_CONTENT_HEIGHT.max(terminal_rows.saturating_sub(CONTENT_CHROME_OVERHEAD));
    let preview_overhead = 11usize;

    for question in questions {
        if question_has_preview(question) {
            let max_preview_content_lines =
                1usize.max(max_allowed_height.saturating_sub(preview_overhead));
            let mut max_preview_box_height = 0usize;
            for option in &question.options {
                if let Some(preview) = &option.preview {
                    let preview_lines = preview.lines().collect::<Vec<_>>();
                    let is_truncated = preview_lines.len() > max_preview_content_lines;
                    let displayed_lines = if is_truncated {
                        max_preview_content_lines
                    } else {
                        preview_lines.len().max(1)
                    };
                    max_preview_box_height = max_preview_box_height
                        .max(displayed_lines + if is_truncated { 1 } else { 0 } + 2);
                    for line in preview_lines {
                        max_width = max_width.max(unicode_width::UnicodeWidthStr::width(line));
                    }
                }
            }
            let right_panel_height = max_preview_box_height + 2;
            let left_panel_height = question.options.len() + 2;
            max_height =
                max_height.max(right_panel_height.max(left_panel_height) + footer_help_lines);
        } else {
            max_height = max_height.max(question.options.len() + 3 + footer_help_lines);
        }
    }

    (
        max_height.max(MIN_CONTENT_HEIGHT).min(max_allowed_height),
        max_width.max(MIN_CONTENT_WIDTH),
    )
}

fn state_with_action(
    state: &State<MultipleChoiceState>,
    action: MultipleChoiceAction,
) -> MultipleChoiceState {
    reduce_multiple_choice_state(&state.read(), action)
}

fn reset_question_focus(
    focused_index: &mut State<usize>,
    footer_focused: &mut State<bool>,
    footer_index: &mut State<usize>,
    submit_focus: &mut State<usize>,
    notes_focused: &mut State<bool>,
) {
    focused_index.set(0);
    footer_focused.set(false);
    footer_index.set(0);
    submit_focus.set(0);
    notes_focused.set(false);
}

fn question_text_input(state: &MultipleChoiceState, question_text: &str) -> Option<String> {
    state
        .question_states
        .get(question_text)
        .map(|state| state.text_input_value.clone())
}

fn is_other_option_focus(question: &Question, preview_mode: bool, focused_index: usize) -> bool {
    !preview_mode && focused_index >= question.options.len()
}

fn update_question_notes(
    question: &Question,
    text: String,
    state: &mut State<MultipleChoiceState>,
) -> MultipleChoiceState {
    let next_state = state_with_action(
        state,
        MultipleChoiceAction::UpdateQuestionState {
            question_text: question.question.clone(),
            updates: QuestionStateUpdate {
                selected_value: None,
                text_input_value: Some(text),
            },
            is_multi_select: question.multi_select,
        },
    );
    state.set(next_state.clone());
    next_state
}

fn update_other_text_for_question(
    question: &Question,
    text: String,
    questions_len: usize,
    hide_submit_tab_flag: bool,
    state: &mut State<MultipleChoiceState>,
) -> MultipleChoiceState {
    let question_text = question.question.clone();
    let mut next_state = state_with_action(
        state,
        MultipleChoiceAction::UpdateQuestionState {
            question_text: question_text.clone(),
            updates: QuestionStateUpdate {
                selected_value: None,
                text_input_value: Some(text.clone()),
            },
            is_multi_select: question.multi_select,
        },
    );
    state.set(next_state.clone());

    let selected = next_state
        .question_states
        .get(&question_text)
        .map(|state| state.selected_value.clone())
        .unwrap_or_default();
    let should_refresh_answer = if question.multi_select {
        selected.iter().any(|value| value == "__other__")
    } else {
        selected.first().is_some_and(|value| value == "__other__")
    };

    if should_refresh_answer {
        let answer = if question.multi_select {
            answer_for_selection("", &selected, Some(&text), true)
        } else {
            answer_for_selection("__other__", &[], Some(&text), false)
        };
        next_state = state_with_action(
            state,
            MultipleChoiceAction::SetAnswer {
                question_text,
                answer,
                should_advance: false,
                question_count: questions_len,
                hide_submit_tab: hide_submit_tab_flag,
            },
        );
        state.set(next_state.clone());
    }

    next_state
}

fn answer_current_question(
    question: &Question,
    label: &str,
    text_input: Option<&str>,
    has_images: bool,
    should_advance: bool,
    questions_len: usize,
    hide_submit_tab_flag: bool,
    state: &mut State<MultipleChoiceState>,
) -> MultipleChoiceState {
    let question_text = question.question.clone();
    let mut next_state = state_with_action(
        state,
        MultipleChoiceAction::UpdateQuestionState {
            question_text: question_text.clone(),
            updates: QuestionStateUpdate {
                selected_value: Some(vec![label.to_string()]),
                text_input_value: None,
            },
            is_multi_select: false,
        },
    );
    state.set(next_state.clone());
    let mut answer = answer_for_selection(label, &[], text_input, false);
    // Maps to: CC `components/permissions/AskUserQuestionPermissionRequest/AskUserQuestionPermissionRequest.tsx:430-479`.
    if label == "__other__" && has_images {
        answer = if text_input.is_some_and(|text| !text.trim().is_empty()) {
            format!("{answer} (Image attached)")
        } else {
            "(Image attached)".to_string()
        };
    }
    next_state = state_with_action(
        state,
        MultipleChoiceAction::SetAnswer {
            question_text,
            answer,
            should_advance,
            question_count: questions_len,
            hide_submit_tab: hide_submit_tab_flag,
        },
    );
    state.set(next_state.clone());
    next_state
}

/// Maps to CC `handleRespondToClaude` / `handleFinishPlanInterview` question
/// summary, without analytics or image persistence.
fn question_feedback_summary(questions: &[Question], state: &MultipleChoiceState) -> String {
    questions
        .iter()
        .map(|question| match state.answers.get(&question.question) {
            Some(answer) => format!("- \"{}\"\n  Answer: {answer}", question.question),
            None => format!("- \"{}\"\n  (No answer provided)", question.question),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn respond_to_claude_feedback(questions: &[Question], state: &MultipleChoiceState) -> String {
    format!(
        "The user wants to clarify these questions.\n    This means they may have additional information, context or questions for you.\n    Take their response into account and then reformulate the questions if appropriate.\n    Start by asking them what they would like to clarify.\n\n    Questions asked:\n{}",
        question_feedback_summary(questions, state)
    )
}

fn finish_plan_interview_feedback(questions: &[Question], state: &MultipleChoiceState) -> String {
    format!(
        "The user has indicated they have provided enough answers for the plan interview.\nStop asking clarifying questions and proceed to finish the plan with the information you have.\n\nQuestions asked and answers provided:\n{}",
        question_feedback_summary(questions, state)
    )
}

/// Maps to: CC `components/permissions/AskUserQuestionPermissionRequest/AskUserQuestionPermissionRequest.tsx:586-604`.
fn permission_image_blocks(
    pasted_by_question: &BTreeMap<String, BTreeMap<usize, PastedContent>>,
) -> Vec<PermissionContentBlock> {
    let mut images = pasted_by_question
        .values()
        .flat_map(BTreeMap::values)
        .filter_map(|content| match content {
            PastedContent::Image {
                id,
                media_type,
                data: Some(data),
                ..
            } => Some((
                *id,
                PermissionContentBlock::image_base64(
                    media_type
                        .clone()
                        .unwrap_or_else(|| "image/png".to_string()),
                    data.clone(),
                ),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    images.sort_by_key(|(id, _)| *id);
    images.into_iter().map(|(_, block)| block).collect()
}

fn question_has_images(
    pasted_by_question: &BTreeMap<String, BTreeMap<usize, PastedContent>>,
    question: &str,
) -> bool {
    pasted_by_question.get(question).is_some_and(|contents| {
        contents
            .values()
            .any(|content| matches!(content, PastedContent::Image { .. }))
    })
}

/// Maps to: CC `components/permissions/AskUserQuestionPermissionRequest/AskUserQuestionPermissionRequest.tsx:187-210`.
fn cache_and_store_permission_image(id: usize, image: &crate::utils::image_paste::ClipboardImage) {
    let stored = crate::utils::image_store::PastedImageContent {
        id: id as u64,
        media_type: Some(image.media_type.clone()),
        data: Some(image.base64.clone()),
    };
    crate::utils::image_store::cache_image_path(&stored);
    std::thread::spawn(move || {
        let _ = crate::utils::image_store::store_image(&stored);
    });
}

fn submit_response_for_state(
    request_input: &serde_json::Value,
    questions: &[Question],
    state: &MultipleChoiceState,
    content_blocks: Vec<PermissionContentBlock>,
) -> PermissionPromptResponse {
    PermissionPromptResponse::allow_once_with_input(build_updated_input_with_answers(
        request_input,
        questions,
        &state.answers,
        &state.question_states,
    ))
    // Maps to: CC `AskUserQuestionPermissionRequest.tsx:412-416` explicit empty permission updates.
    .with_permission_updates(Vec::new())
    .with_content_blocks(content_blocks)
}

/// Maps to: CC `AskUserQuestionPermissionRequest`.
#[component]
pub fn AskUserQuestionPermissionRequest(
    props: &AskUserQuestionPermissionRequestProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let request = props.request.clone().unwrap_or_else(default_request);
    let questions = questions_from_input(&request.input);
    let hide_submit_tab_flag = hide_submit_tab(&questions);
    let mut state = hooks.use_state(MultipleChoiceState::default);
    // Maps to: CC `components/permissions/AskUserQuestionPermissionRequest/AskUserQuestionPermissionRequest.tsx:182-222`.
    let pasted_contents_by_question =
        hooks.use_state(BTreeMap::<String, BTreeMap<usize, PastedContent>>::new);
    let next_paste_id = hooks.use_state(|| 0usize);
    let focused_index = hooks.use_state(|| 0usize);
    let multi_submit_focused = hooks.use_state(|| false);
    let footer_focused = hooks.use_state(|| false);
    let footer_index = hooks.use_state(|| 0usize);
    let submit_focus = hooks.use_state(|| 0usize);
    let notes_focused = hooks.use_state(|| false);
    let mut pending_response = hooks.use_state(|| Option::<PermissionPromptResponse>::None);
    let mut pending_cancel = hooks.use_state(|| false);
    let mut editor_error = hooks.use_state(|| Option::<String>::None);
    let mut editor_result = hooks.use_state(|| Option::<(Question, EditorResult)>::None);
    let editor_runtime = hooks
        .try_use_context::<ExternalEditorRuntime>()
        .map(|runtime| *runtime);
    let external_editor_available = editor_runtime.is_some()
        && crate::utils::prompt_editor::external_editor_command().is_some();
    let editor_channel =
        hooks.use_const(|| Arc::new(async_channel::unbounded::<(Question, String)>()));
    let editor_receiver = editor_channel.1.clone();
    hooks.use_future(async move {
        while let Ok((question, current)) = editor_receiver.recv().await {
            let result = match editor_runtime {
                Some(runtime) => runtime.edit_prompt(&current).await,
                None => EditorResult {
                    content: None,
                    error: Some("External editor is unavailable".to_string()),
                },
            };
            editor_result.set(Some((question, result)));
        }
    });
    let editor_question_snapshot = {
        let snapshot = state.read();
        questions
            .get(
                snapshot
                    .current_question_index
                    .min(questions.len().saturating_sub(1)),
            )
            .cloned()
    };
    let editor_sender_for_action = editor_channel.0.clone();
    let editor_action_active = notes_focused.get()
        && external_editor_available
        && editor_question_snapshot
            .as_ref()
            .is_some_and(question_has_preview);
    let editor_action_question = editor_question_snapshot.clone();
    let editor_action_value = editor_action_question
        .as_ref()
        .and_then(|question| question_text_input(&state.read(), &question.question));
    let keybinding_runtime = hooks
        .try_use_context::<crate::keybindings::keybinding_context::KeybindingRuntime>()
        .map(|runtime| runtime.clone());
    crate::keybindings::use_keybinding::use_keybinding(
        &mut hooks,
        keybinding_runtime,
        "chat:externalEditor",
        crate::keybindings::types::ContextName::Chat,
        move || editor_action_active,
        move || {
            let Some(question) = editor_action_question.clone() else {
                return false;
            };
            let _ = editor_sender_for_action
                .try_send((question, editor_action_value.clone().unwrap_or_default()));
            true
        },
    );

    let completed_editor = editor_result.read().clone();
    if let Some((question, result)) = completed_editor {
        editor_result.set(None);
        if let Some(error) = result.error {
            editor_error.set(Some(error));
        } else if let Some(content) = result.content {
            update_other_text_for_question(
                &question,
                content,
                questions.len(),
                hide_submit_tab_flag,
                &mut state,
            );
            editor_error.set(None);
        }
    }
    let (_, terminal_rows) = hooks.use_terminal_size();
    let (global_content_height, global_content_width) =
        ask_user_question_content_dimensions(&questions, terminal_rows as usize);
    let pasted_contents_snapshot = pasted_contents_by_question.read().clone();
    let all_content_blocks = permission_image_blocks(&pasted_contents_snapshot);

    hooks.use_terminal_events({
        let questions = questions.clone();
        let request_input = request.input.clone();
        let request_mode = request.mode;
        let mut state = state;
        let mut focused_index = focused_index;
        let mut multi_submit_focused = multi_submit_focused;
        let mut footer_focused = footer_focused;
        let mut footer_index = footer_index;
        let mut submit_focus = submit_focus;
        let mut notes_focused = notes_focused;
        let mut pending_response = pending_response;
        let mut pending_cancel = pending_cancel;
        let pasted_contents = pasted_contents_snapshot.clone();
        let all_content_blocks = all_content_blocks.clone();
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
            if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
                pending_cancel.set(true);
                return;
            }
            if questions.is_empty() {
                if matches!(code, KeyCode::Esc) {
                    pending_response.set(Some(PermissionPromptResponse::new(
                        PermissionPromptChoice::Deny,
                    )));
                }
                return;
            }

            let hide_submit_tab_flag = hide_submit_tab(&questions);
            let max_index = if hide_submit_tab_flag {
                questions.len().saturating_sub(1)
            } else {
                questions.len()
            };
            let snapshot = state.read().clone();
            let current_index = snapshot.current_question_index.min(max_index);

            if current_index == questions.len() && !hide_submit_tab_flag {
                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        submit_focus.set(submit_focus.get().saturating_sub(1));
                    }
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                        submit_focus.set((submit_focus.get() + 1).min(1));
                    }
                    KeyCode::Enter => {
                        if submit_focus.get() == 0 {
                            pending_response.set(Some(submit_response_for_state(
                                &request_input,
                                &questions,
                                &snapshot,
                                all_content_blocks.clone(),
                            )));
                        } else {
                            pending_response.set(Some(PermissionPromptResponse::new(
                                PermissionPromptChoice::Deny,
                            )));
                        }
                    }
                    KeyCode::Esc => pending_response.set(Some(PermissionPromptResponse::new(
                        PermissionPromptChoice::Deny,
                    ))),
                    KeyCode::Left | KeyCode::BackTab => {
                        let next = state_with_action(&state, MultipleChoiceAction::PrevQuestion);
                        state.set(next);
                        reset_question_focus(
                            &mut focused_index,
                            &mut footer_focused,
                            &mut footer_index,
                            &mut submit_focus,
                            &mut notes_focused,
                        );
                    }
                    _ => {}
                }
                return;
            }

            let Some(question) = questions.get(current_index).cloned() else {
                return;
            };
            let preview_mode = question_has_preview(&question);
            let option_count = question.options.len() + if preview_mode { 0 } else { 1 };
            let option_count = option_count.max(1);
            let is_plan_mode = request_mode == PermissionMode::Plan;
            let focus = focused_index.get().min(option_count.saturating_sub(1));
            let other_focused = !footer_focused.get()
                && !multi_submit_focused.get()
                && is_other_option_focus(&question, preview_mode, focus);

            if notes_focused.get() {
                match code {
                    KeyCode::Esc | KeyCode::Enter => {
                        notes_focused.set(false);
                        let mut next = state_with_action(
                            &state,
                            MultipleChoiceAction::SetTextInputMode { is_in_input: false },
                        );
                        state.set(next.clone());
                        if let Some(selected_label) = snapshot
                            .question_states
                            .get(&question.question)
                            .and_then(|state| state.selected_value.first())
                            .cloned()
                        {
                            next = answer_current_question(
                                &question,
                                &selected_label,
                                None,
                                false,
                                true,
                                questions.len(),
                                hide_submit_tab_flag,
                                &mut state,
                            );
                            if hide_submit_tab_flag {
                                pending_response.set(Some(submit_response_for_state(
                                    &request_input,
                                    &questions,
                                    &next,
                                    all_content_blocks.clone(),
                                )));
                            } else {
                                reset_question_focus(
                                    &mut focused_index,
                                    &mut footer_focused,
                                    &mut footer_index,
                                    &mut submit_focus,
                                    &mut notes_focused,
                                );
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        let mut text =
                            question_text_input(&snapshot, &question.question).unwrap_or_default();
                        text.pop();
                        update_question_notes(&question, text, &mut state);
                    }
                    KeyCode::Char(c)
                        if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        let mut text =
                            question_text_input(&snapshot, &question.question).unwrap_or_default();
                        text.push(c);
                        update_question_notes(&question, text, &mut state);
                    }
                    _ => {}
                }
                return;
            }

            match code {
                KeyCode::Esc => pending_response.set(Some(PermissionPromptResponse::new(
                    PermissionPromptChoice::Deny,
                ))),
                KeyCode::Left => {
                    let next = state_with_action(&state, MultipleChoiceAction::PrevQuestion);
                    state.set(next);
                    multi_submit_focused.set(false);
                    reset_question_focus(
                        &mut focused_index,
                        &mut footer_focused,
                        &mut footer_index,
                        &mut submit_focus,
                        &mut notes_focused,
                    );
                }
                KeyCode::Right => {
                    if current_index < max_index {
                        let next = state_with_action(
                            &state,
                            MultipleChoiceAction::NextQuestion {
                                question_count: questions.len(),
                                hide_submit_tab: hide_submit_tab_flag,
                            },
                        );
                        state.set(next);
                        multi_submit_focused.set(false);
                        reset_question_focus(
                            &mut focused_index,
                            &mut footer_focused,
                            &mut footer_index,
                            &mut submit_focus,
                            &mut notes_focused,
                        );
                    }
                }
                KeyCode::BackTab => {
                    if multi_submit_focused.get() {
                        multi_submit_focused.set(false);
                        focused_index.set(option_count - 1);
                    } else if focus == 0 {
                        focused_index.set(option_count - 1);
                    } else {
                        focused_index.set(focus - 1);
                    }
                }
                KeyCode::Tab => {
                    if question.multi_select && focus + 1 == option_count {
                        multi_submit_focused.set(true);
                    } else if !multi_submit_focused.get() {
                        focused_index.set((focus + 1) % option_count);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if footer_focused.get() {
                        if footer_index.get() == 0 {
                            footer_focused.set(false);
                        } else {
                            footer_index.set(footer_index.get().saturating_sub(1));
                        }
                    } else if multi_submit_focused.get() {
                        multi_submit_focused.set(false);
                        focused_index.set(option_count - 1);
                    } else {
                        focused_index.set(if focus == 0 {
                            option_count - 1
                        } else {
                            focus - 1
                        });
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if footer_focused.get() {
                        if is_plan_mode && footer_index.get() == 0 {
                            footer_index.set(1);
                        }
                    } else if multi_submit_focused.get() {
                        footer_focused.set(true);
                        footer_index.set(0);
                    } else if focus + 1 < option_count {
                        focused_index.set(focus + 1);
                    } else if question.multi_select {
                        multi_submit_focused.set(true);
                    } else {
                        footer_focused.set(true);
                        footer_index.set(0);
                    }
                }
                KeyCode::Char('n')
                    if preview_mode
                        && !footer_focused.get()
                        && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    notes_focused.set(true);
                    let next = state_with_action(
                        &state,
                        MultipleChoiceAction::SetTextInputMode { is_in_input: true },
                    );
                    state.set(next);
                }
                KeyCode::Backspace if other_focused => {
                    let mut text =
                        question_text_input(&snapshot, &question.question).unwrap_or_default();
                    text.pop();
                    update_other_text_for_question(
                        &question,
                        text,
                        questions.len(),
                        hide_submit_tab_flag,
                        &mut state,
                    );
                }
                KeyCode::Char(c)
                    if other_focused
                        && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let mut text =
                        question_text_input(&snapshot, &question.question).unwrap_or_default();
                    text.push(c);
                    update_other_text_for_question(
                        &question,
                        text,
                        questions.len(),
                        hide_submit_tab_flag,
                        &mut state,
                    );
                }
                KeyCode::Char(' ')
                    if question.multi_select
                        && !footer_focused.get()
                        && !multi_submit_focused.get() =>
                {
                    let value = question
                        .options
                        .get(focus)
                        .map(|option| option.label.clone())
                        .unwrap_or_else(|| "__other__".to_string());
                    let question_text = question.question.clone();
                    let existing = snapshot
                        .question_states
                        .get(&question_text)
                        .cloned()
                        .unwrap_or_default();
                    let selected = toggle_multi_select_value(existing.selected_value, &value);
                    let next_state = state_with_action(
                        &state,
                        MultipleChoiceAction::UpdateQuestionState {
                            question_text: question_text.clone(),
                            updates: QuestionStateUpdate {
                                selected_value: Some(selected.clone()),
                                text_input_value: None,
                            },
                            is_multi_select: true,
                        },
                    );
                    state.set(next_state);
                    let answer = answer_for_selection(
                        "",
                        &selected,
                        Some(existing.text_input_value.as_str()),
                        true,
                    );
                    let next_state = state_with_action(
                        &state,
                        MultipleChoiceAction::SetAnswer {
                            question_text,
                            answer,
                            should_advance: false,
                            question_count: questions.len(),
                            hide_submit_tab: hide_submit_tab_flag,
                        },
                    );
                    state.set(next_state);
                }
                KeyCode::Enter => {
                    if footer_focused.get() {
                        let feedback = if footer_index.get() == 0 {
                            respond_to_claude_feedback(&questions, &snapshot)
                        } else {
                            finish_plan_interview_feedback(&questions, &snapshot)
                        };
                        pending_response.set(Some(
                            PermissionPromptResponse::new(PermissionPromptChoice::Deny)
                                .with_feedback(feedback)
                                .with_content_blocks(all_content_blocks.clone()),
                        ));
                        return;
                    }
                    if question.multi_select {
                        if multi_submit_focused.get() {
                            let advanced = state_with_action(
                                &state,
                                MultipleChoiceAction::NextQuestion {
                                    question_count: questions.len(),
                                    hide_submit_tab: hide_submit_tab_flag,
                                },
                            );
                            state.set(advanced);
                            multi_submit_focused.set(false);
                            reset_question_focus(
                                &mut focused_index,
                                &mut footer_focused,
                                &mut footer_index,
                                &mut submit_focus,
                                &mut notes_focused,
                            );
                        } else {
                            // With a submit button, Enter toggles the focused
                            // value; only Enter on the submit row advances.
                            let value = question
                                .options
                                .get(focus)
                                .map(|option| option.label.clone())
                                .unwrap_or_else(|| "__other__".to_string());
                            let existing = snapshot
                                .question_states
                                .get(&question.question)
                                .cloned()
                                .unwrap_or_default();
                            let selected =
                                toggle_multi_select_value(existing.selected_value, &value);
                            let next_state = state_with_action(
                                &state,
                                MultipleChoiceAction::UpdateQuestionState {
                                    question_text: question.question.clone(),
                                    updates: QuestionStateUpdate {
                                        selected_value: Some(selected.clone()),
                                        text_input_value: None,
                                    },
                                    is_multi_select: true,
                                },
                            );
                            state.set(next_state);
                            let answer = answer_for_selection(
                                "",
                                &selected,
                                Some(existing.text_input_value.as_str()),
                                true,
                            );
                            let next_state = state_with_action(
                                &state,
                                MultipleChoiceAction::SetAnswer {
                                    question_text: question.question.clone(),
                                    answer,
                                    should_advance: false,
                                    question_count: questions.len(),
                                    hide_submit_tab: hide_submit_tab_flag,
                                },
                            );
                            state.set(next_state);
                        }
                    } else {
                        let label = question
                            .options
                            .get(focus)
                            .map(|option| option.label.as_str())
                            .unwrap_or("__other__");
                        let text_input = if label == "__other__" {
                            question_text_input(&snapshot, &question.question)
                        } else {
                            None
                        };
                        let has_question_images =
                            question_has_images(&pasted_contents, &question.question);
                        if label == "__other__"
                            && !has_question_images
                            && text_input
                                .as_deref()
                                .is_none_or(|text| text.trim().is_empty())
                        {
                            pending_response.set(Some(PermissionPromptResponse::new(
                                PermissionPromptChoice::Deny,
                            )));
                            return;
                        }
                        let next_state = answer_current_question(
                            &question,
                            label,
                            text_input.as_deref(),
                            has_question_images,
                            !hide_submit_tab_flag,
                            questions.len(),
                            hide_submit_tab_flag,
                            &mut state,
                        );
                        if hide_submit_tab_flag {
                            pending_response.set(Some(submit_response_for_state(
                                &request_input,
                                &questions,
                                &next_state,
                                all_content_blocks.clone(),
                            )));
                        } else {
                            reset_question_focus(
                                &mut focused_index,
                                &mut footer_focused,
                                &mut footer_index,
                                &mut submit_focus,
                                &mut notes_focused,
                            );
                        }
                    }
                }
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if let Some(digit) = c.to_digit(10) {
                        let index = digit as usize;
                        if index > 0 && index <= option_count {
                            multi_submit_focused.set(false);
                            focused_index.set(index - 1);
                        }
                    }
                }
                _ => {}
            }
        }
    });

    let selected_response = pending_response.read().clone();
    if let Some(response) = selected_response {
        pending_response.set(None);
        (props.on_select)(response);
    }
    if pending_cancel.get() {
        pending_cancel.set(false);
        (props.on_cancel)(());
    }

    let snapshot = state.read().clone();
    let current_index = snapshot
        .current_question_index
        .min(if hide_submit_tab_flag {
            questions.len().saturating_sub(1)
        } else {
            questions.len()
        });
    let plan_file_path = (request.mode == PermissionMode::Plan).then(|| "current plan".to_string());

    if questions.is_empty() {
        return element! {
            View(flex_direction: FlexDirection::Column, margin_top: 1u32) {
                // Cometix-only defensive branch: CC has no `questions.length === 0`
                // arm in `AskUserQuestionPermissionRequest.tsx`, so this heading
                // has no CC owner. It used to read the fabricated
                // `PermissionRequest.title`; the literal keeps the same text
                // without reviving that field.
                Text(content: "Answer questions?".to_string(), color: theme.permission, weight: Weight::Bold, wrap: TextWrap::Wrap)
                Text(content: "Claude asked a question, but the question payload was empty or invalid.".to_string(), color: theme.warning, wrap: TextWrap::Wrap)
                Text(content: "Esc to reject".to_string(), color: theme.inactive, wrap: TextWrap::NoWrap)
            }
        }
        .into_any();
    }

    if current_index == questions.len() && !hide_submit_tab_flag {
        return element! {
            SubmitQuestionsView(
                questions: questions.clone(),
                current_question_index: current_index,
                answers: snapshot.answers.clone(),
                decision_reason: request.decision_reason.clone(),
                permission_mode: request.mode,
                min_content_height: Some(global_content_height as u32),
                focused_index: submit_focus.get(),
            )
        }
        .into_any();
    }

    let current_question = questions
        .get(current_index)
        .cloned()
        .unwrap_or_else(|| questions[0].clone());
    let current_question_text = current_question.question.clone();
    let current_pasted_contents = pasted_contents_by_question
        .read()
        .get(&current_question_text)
        .cloned()
        .unwrap_or_default();
    let image_question = current_question_text.clone();
    let on_image_paste = Handler::from(move |image: crate::utils::image_paste::ClipboardImage| {
        let mut next_paste_id = next_paste_id;
        let mut pasted_contents_by_question = pasted_contents_by_question;
        let id = next_paste_id.get();
        next_paste_id.set(id + 1);
        cache_and_store_permission_image(id, &image);
        let mut all = pasted_contents_by_question.read().clone();
        all.entry(image_question.clone()).or_default().insert(
            id,
            PastedContent::Image {
                id,
                media_type: Some(image.media_type),
                data: Some(image.base64),
                filename: Some("Pasted image".to_string()),
                dimensions: image.dimensions,
                source_path: None,
            },
        );
        pasted_contents_by_question.set(all);
    });
    let remove_question = current_question_text.clone();
    let on_remove_image = Handler::from(move |id: usize| {
        let mut pasted_contents_by_question = pasted_contents_by_question;
        let mut all = pasted_contents_by_question.read().clone();
        if let Some(contents) = all.get_mut(&remove_question) {
            contents.remove(&id);
        }
        pasted_contents_by_question.set(all);
    });
    let editor_question = current_question.clone();
    let editor_sender = editor_channel.0.clone();
    let on_open_editor = Handler::from(move |current: String| {
        let _ = editor_sender.try_send((editor_question.clone(), current));
    });

    element! {
        View(flex_direction: FlexDirection::Column) {
            QuestionView(
                question: current_question,
                questions: questions.clone(),
                current_question_index: current_index,
                answers: snapshot.answers.clone(),
                question_states: snapshot.question_states.clone(),
                hide_submit_tab: hide_submit_tab_flag,
                plan_file_path: plan_file_path,
                min_content_height: Some(global_content_height as u32),
                min_content_width: Some(global_content_width),
                focused_index: focused_index.get(),
                multi_submit_focused: multi_submit_focused.get(),
                footer_focused: footer_focused.get(),
                footer_index: footer_index.get(),
                notes_focused: notes_focused.get(),
                external_editor_available: external_editor_available,
                editor_error: editor_error.read().clone(),
                pasted_contents: current_pasted_contents,
                on_remove_image: on_remove_image,
                on_image_paste: on_image_paste,
                on_open_editor: on_open_editor,
                clipboard_image_override: props.clipboard_image_override.clone(),
            )
        }
    }
    .into_any()
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

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> TerminalEvent {
        let mut event = KeyEvent::new(KeyEventKind::Press, code);
        event.modifiers = modifiers;
        TerminalEvent::Key(event)
    }

    fn ask_request(input: serde_json::Value) -> PermissionRequestData {
        PermissionRequestData {
            permission_result: None,
            id: "req".to_string(),
            tool_use_id: "toolu_ask".to_string(),
            tool_name: "AskUserQuestion".to_string(),
            mcp_info: None,
            decision_reason: None,
            description: String::new(),
            message: String::new(),
            input_summary: String::new(),
            input,
            call_input: None,
            rule: PermissionRuleValue::new("AskUserQuestion", None),
            suggestions: Vec::new(),
            blocked_path: None,
            metadata: None,
            is_compound_command: false,
            mode: PermissionMode::Default,
        }
    }

    fn single_question_input() -> serde_json::Value {
        serde_json::json!({
            "questions": [{
                "question": "Proceed?",
                "header": "Proceed",
                "options": [
                    {"label": "Yes", "description": "Continue"},
                    {"label": "No", "description": "Stop"}
                ]
            }]
        })
    }

    fn multi_question_input() -> serde_json::Value {
        serde_json::json!({
            "questions": [
                {
                    "question": "Library?",
                    "header": "Library",
                    "options": [
                        {"label": "Serde", "description": "Use serde"},
                        {"label": "Manual", "description": "Write parser"}
                    ]
                },
                {
                    "question": "Features?",
                    "header": "Features",
                    "multiSelect": true,
                    "options": [
                        {"label": "Cache", "description": "Enable cache"},
                        {"label": "Logs", "description": "Enable logs"}
                    ]
                }
            ]
        })
    }

    fn preview_question_input() -> serde_json::Value {
        serde_json::json!({
            "questions": [{
                "question": "Which layout?",
                "header": "Layout",
                "options": [
                    {"label": "List", "description": "Rows", "preview": "line 1\nline 2"},
                    {"label": "Grid", "description": "Cards", "preview": "card"}
                ]
            }]
        })
    }

    #[test]
    fn content_dimensions_follow_official_minimums() {
        let questions = questions_from_input(&preview_question_input());
        let (height, width) = ask_user_question_content_dimensions(&questions, 40);
        assert!(height >= MIN_CONTENT_HEIGHT);
        assert!(width >= MIN_CONTENT_WIDTH);
    }

    #[test]
    fn ask_user_question_permission_request_renders_single_question() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                AskUserQuestionPermissionRequest(request: Some(ask_request(single_question_input())))
            }
        }
        .render(Some(100))
        .to_string();

        assert!(text.contains("Proceed?"), "canvas=\n{text}");
        assert!(text.contains("Yes"), "canvas=\n{text}");
        assert!(text.contains("Other"), "canvas=\n{text}");
        assert!(text.contains("Chat about this"), "canvas=\n{text}");
    }

    #[test]
    fn ask_user_question_permission_request_renders_preview_question() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                AskUserQuestionPermissionRequest(request: Some(ask_request(preview_question_input())))
            }
        }
        .render(Some(110))
        .to_string();

        assert!(text.contains("Which layout?"), "canvas=\n{text}");
        assert!(text.contains("List"), "canvas=\n{text}");
        assert!(text.contains("┌"), "canvas=\n{text}");
        assert!(
            !text.contains("Other"),
            "preview questions should not add Other; canvas=\n{text}"
        );
    }

    #[test]
    fn preview_question_notes_submit_with_annotations() {
        let responses = Arc::new(Mutex::new(Vec::<PermissionPromptResponse>::new()));
        let responses_for_handler = Arc::clone(&responses);
        let responses_for_loop = Arc::clone(&responses);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    AskUserQuestionPermissionRequest(
                        request: Some(ask_request(preview_question_input())),
                        on_select: move |response| responses_for_handler.lock().expect("responses mutex").push(response),
                    )
                }
            };
            let events = vec![
                key(KeyCode::Char('n')),
                key(KeyCode::Char('o')),
                key(KeyCode::Char('k')),
                key(KeyCode::Esc),
                key(KeyCode::Enter),
            ];
            let mut render_loop = Box::pin(app.mock_terminal_render_loop(
                MockTerminalConfig::with_events(stream::iter(events)).with_size(110, 35),
            ));
            for _ in 0..16 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                if next.is_none() {
                    break;
                }
                if !responses_for_loop
                    .lock()
                    .expect("responses mutex")
                    .is_empty()
                {
                    break;
                }
            }
        });

        let responses = responses.lock().expect("responses mutex");
        assert_eq!(responses.len(), 1);
        let input = responses[0].updated_input.as_ref().expect("updated input");
        assert_eq!(input["answers"]["Which layout?"], "List");
        assert_eq!(input["annotations"]["Which layout?"]["notes"], "ok");
        assert_eq!(
            input["annotations"]["Which layout?"]["preview"],
            "line 1\nline 2"
        );
    }

    #[test]
    fn single_select_enter_submits_updated_input() {
        let responses = Arc::new(Mutex::new(Vec::<PermissionPromptResponse>::new()));
        let responses_for_handler = Arc::clone(&responses);
        let responses_for_loop = Arc::clone(&responses);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    AskUserQuestionPermissionRequest(
                        request: Some(ask_request(single_question_input())),
                        on_select: move |response| responses_for_handler.lock().expect("responses mutex").push(response),
                    )
                }
            };
            let mut render_loop = Box::pin(
                app.mock_terminal_render_loop(
                    MockTerminalConfig::with_events(stream::iter(vec![key(KeyCode::Enter)]))
                        .with_size(100, 30),
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
                if !responses_for_loop
                    .lock()
                    .expect("responses mutex")
                    .is_empty()
                {
                    break;
                }
            }
        });

        let responses = responses.lock().expect("responses mutex");
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].choice, PermissionPromptChoice::AllowOnce);
        assert_eq!(
            responses[0].updated_input.as_ref().unwrap()["answers"]["Proceed?"],
            "Yes"
        );
    }

    #[test]
    fn single_select_other_text_submits_updated_input() {
        let responses = Arc::new(Mutex::new(Vec::<PermissionPromptResponse>::new()));
        let responses_for_handler = Arc::clone(&responses);
        let responses_for_loop = Arc::clone(&responses);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    AskUserQuestionPermissionRequest(
                        request: Some(ask_request(single_question_input())),
                        on_select: move |response| responses_for_handler.lock().expect("responses mutex").push(response),
                    )
                }
            };
            let events = vec![
                key(KeyCode::Down),
                key(KeyCode::Down),
                key(KeyCode::Char('c')),
                key(KeyCode::Char('u')),
                key(KeyCode::Char('s')),
                key(KeyCode::Char('t')),
                key(KeyCode::Char('o')),
                key(KeyCode::Char('m')),
                key(KeyCode::Enter),
            ];
            let mut render_loop = Box::pin(app.mock_terminal_render_loop(
                MockTerminalConfig::with_events(stream::iter(events)).with_size(100, 30),
            ));
            for _ in 0..20 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                if next.is_none() {
                    break;
                }
                if !responses_for_loop
                    .lock()
                    .expect("responses mutex")
                    .is_empty()
                {
                    break;
                }
            }
        });

        let responses = responses.lock().expect("responses mutex");
        assert_eq!(responses.len(), 1);
        let input = responses[0].updated_input.as_ref().expect("updated input");
        assert_eq!(input["answers"]["Proceed?"], "custom");
        assert_eq!(input["annotations"]["Proceed?"]["notes"], "custom");
    }

    #[test]
    fn other_image_paste_honors_null_unbind_and_live_remap_before_model_transport() {
        let responses = Arc::new(Mutex::new(Vec::<PermissionPromptResponse>::new()));
        let responses_for_handler = Arc::clone(&responses);
        let responses_for_loop = Arc::clone(&responses);

        futures::executor::block_on(async move {
            let child = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    AskUserQuestionPermissionRequest(
                        request: Some(ask_request(single_question_input())),
                        clipboard_image_override: Some(crate::utils::image_paste::ClipboardImage {
                            base64: "AAAA".to_string(),
                            media_type: "image/png".to_string(),
                            dimensions: None,
                        }),
                        on_select: move |response| responses_for_handler.lock().expect("responses mutex").push(response),
                    )
                }
            }
            .into_any();
            let mut bindings = crate::keybindings::default_bindings::default_bindings();
            bindings.push(crate::keybindings::types::ParsedBinding {
                chord: crate::keybindings::parser::parse_chord("ctrl+v"),
                action: None,
                context: crate::keybindings::types::ContextName::Chat,
            });
            bindings.push(crate::keybindings::types::ParsedBinding {
                chord: crate::keybindings::parser::parse_chord("f4"),
                action: Some("chat:imagePaste".to_string()),
                context: crate::keybindings::types::ContextName::Chat,
            });
            let runtime = crate::keybindings::keybinding_context::KeybindingRuntime::new(bindings);
            let mut app = element! {
                ContextProvider(value: Context::owned(runtime)) {
                    #(vec![child])
                }
            };
            let events = vec![
                key(KeyCode::Down),
                key(KeyCode::Down),
                modified_key(KeyCode::Char('v'), KeyModifiers::CONTROL),
                key(KeyCode::F(4)),
                key(KeyCode::Enter),
            ];
            let paced = stream::unfold(events.into_iter(), |mut events| async move {
                let event = std::iter::Iterator::next(&mut events)?;
                futures_timer::Delay::new(Duration::from_millis(50)).await;
                Some((event, events))
            });
            let mut render_loop = Box::pin(app.mock_terminal_render_loop(
                MockTerminalConfig::with_events(paced).with_size(100, 30),
            ));
            for _ in 0..24 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(150)).await;
                    None
                })
                .await;
                if next.is_none()
                    || !responses_for_loop
                        .lock()
                        .expect("responses mutex")
                        .is_empty()
                {
                    break;
                }
            }
        });

        let responses = responses.lock().expect("responses mutex");
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].choice, PermissionPromptChoice::AllowOnce);
        assert!(responses[0].permission_updates_explicit);
        assert_eq!(
            responses[0].updated_input.as_ref().unwrap()["answers"]["Proceed?"],
            "(Image attached)"
        );
        assert!(matches!(
            responses[0].content_blocks.as_slice(),
            [PermissionContentBlock::Image { source }]
                if source.media_type == "image/png" && source.data == "AAAA"
        ));
    }

    #[test]
    fn multi_question_review_submits_all_answers() {
        let responses = Arc::new(Mutex::new(Vec::<PermissionPromptResponse>::new()));
        let responses_for_handler = Arc::clone(&responses);
        let responses_for_loop = Arc::clone(&responses);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    AskUserQuestionPermissionRequest(
                        request: Some(ask_request(multi_question_input())),
                        on_select: move |response| responses_for_handler.lock().expect("responses mutex").push(response),
                    )
                }
            };
            let events = vec![
                key(KeyCode::Enter),     // Library = Serde, advance
                key(KeyCode::Char(' ')), // Toggle Cache
                key(KeyCode::Down),      // Compress
                key(KeyCode::Down),      // Other
                key(KeyCode::Down),      // Submit row
                key(KeyCode::Enter),     // Advance to review
                key(KeyCode::Enter),     // Submit answers
            ];
            let mut render_loop = Box::pin(app.mock_terminal_render_loop(
                MockTerminalConfig::with_events(stream::iter(events)).with_size(110, 35),
            ));
            for _ in 0..16 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                if next.is_none() {
                    break;
                }
                if !responses_for_loop
                    .lock()
                    .expect("responses mutex")
                    .is_empty()
                {
                    break;
                }
            }
        });

        let responses = responses.lock().expect("responses mutex");
        assert_eq!(responses.len(), 1);
        let input = responses[0].updated_input.as_ref().expect("updated input");
        assert_eq!(input["answers"]["Library?"], "Serde");
        assert_eq!(input["answers"]["Features?"], "Cache");
    }

    #[test]
    fn footer_feedback_preserves_official_question_summary() {
        let questions = vec![Question {
            question: "Proceed?".to_string(),
            ..Default::default()
        }];
        let state = MultipleChoiceState {
            answers: std::collections::BTreeMap::from([(
                "Proceed?".to_string(),
                "Yes".to_string(),
            )]),
            ..Default::default()
        };
        let clarify = respond_to_claude_feedback(&questions, &state);
        assert!(clarify.starts_with("The user wants to clarify these questions."));
        assert!(clarify.contains("- \"Proceed?\"\n  Answer: Yes"));
        let finish = finish_plan_interview_feedback(&questions, &state);
        assert!(finish.starts_with("The user has indicated they have provided enough answers"));
        assert!(finish.contains("Questions asked and answers provided:"));
    }
}
