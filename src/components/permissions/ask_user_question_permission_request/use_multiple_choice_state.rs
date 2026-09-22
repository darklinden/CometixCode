//! Maps to: CC
//! `components/permissions/AskUserQuestionPermissionRequest/use-multiple-choice-state.ts`.
//!
//! The official file owns the reducer shape used by the AskUserQuestion
//! permission UI. Rust keeps the same state vocabulary and pure transitions so
//! the retained iocraft component can render from deterministic state snapshots.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type AnswerValue = String;

/// Maps to: CC `QuestionOption` from
/// `tools/AskUserQuestionTool/AskUserQuestionTool.tsx`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
}

/// Maps to: CC `Question` from
/// `tools/AskUserQuestionTool/AskUserQuestionTool.tsx`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

/// Maps to: CC `QuestionState`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionState {
    pub selected_value: Vec<String>,
    pub text_input_value: String,
}

/// Maps to: CC reducer `State`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultipleChoiceState {
    pub current_question_index: usize,
    pub answers: BTreeMap<String, AnswerValue>,
    pub question_states: BTreeMap<String, QuestionState>,
    pub is_in_text_input: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultipleChoiceAction {
    NextQuestion {
        question_count: usize,
        hide_submit_tab: bool,
    },
    PrevQuestion,
    UpdateQuestionState {
        question_text: String,
        updates: QuestionStateUpdate,
        is_multi_select: bool,
    },
    SetAnswer {
        question_text: String,
        answer: String,
        should_advance: bool,
        question_count: usize,
        hide_submit_tab: bool,
    },
    SetTextInputMode {
        is_in_input: bool,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionStateUpdate {
    pub selected_value: Option<Vec<String>>,
    pub text_input_value: Option<String>,
}

/// Maps to: CC reducer in `use-multiple-choice-state.ts`.
pub fn reduce_multiple_choice_state(
    state: &MultipleChoiceState,
    action: MultipleChoiceAction,
) -> MultipleChoiceState {
    match action {
        MultipleChoiceAction::NextQuestion {
            question_count,
            hide_submit_tab,
        } => {
            let max_index = max_question_index(question_count, hide_submit_tab);
            MultipleChoiceState {
                current_question_index: (state.current_question_index + 1).min(max_index),
                is_in_text_input: false,
                ..state.clone()
            }
        }
        MultipleChoiceAction::PrevQuestion => MultipleChoiceState {
            current_question_index: state.current_question_index.saturating_sub(1),
            is_in_text_input: false,
            ..state.clone()
        },
        MultipleChoiceAction::UpdateQuestionState {
            question_text,
            updates,
            // CC's `isMultiSelect` branch yields the same empty selection
            // list on both sides here, so the flag is not read.
            is_multi_select: _,
        } => {
            let existing = state.question_states.get(&question_text).cloned();
            let new_state = QuestionState {
                selected_value: updates
                    .selected_value
                    .or_else(|| existing.as_ref().map(|state| state.selected_value.clone()))
                    .unwrap_or_default(),
                text_input_value: updates
                    .text_input_value
                    .or_else(|| {
                        existing
                            .as_ref()
                            .map(|state| state.text_input_value.clone())
                    })
                    .unwrap_or_default(),
            };
            let mut question_states = state.question_states.clone();
            question_states.insert(question_text, new_state);
            MultipleChoiceState {
                question_states,
                ..state.clone()
            }
        }
        MultipleChoiceAction::SetAnswer {
            question_text,
            answer,
            should_advance,
            question_count,
            hide_submit_tab,
        } => {
            let mut answers = state.answers.clone();
            answers.insert(question_text, answer);
            let current_question_index = if should_advance {
                (state.current_question_index + 1)
                    .min(max_question_index(question_count, hide_submit_tab))
            } else {
                state.current_question_index
            };
            MultipleChoiceState {
                answers,
                current_question_index,
                is_in_text_input: false,
                ..state.clone()
            }
        }
        MultipleChoiceAction::SetTextInputMode { is_in_input } => MultipleChoiceState {
            is_in_text_input: is_in_input,
            ..state.clone()
        },
    }
}

fn max_question_index(question_count: usize, hide_submit_tab: bool) -> usize {
    if hide_submit_tab {
        question_count.saturating_sub(1)
    } else {
        question_count
    }
}

/// Maps to: CC `AskUserQuestionTool.inputSchema.safeParse(...).data.questions`.
pub fn questions_from_input(input: &Value) -> Vec<Question> {
    input
        .get("questions")
        .and_then(Value::as_array)
        .map(|questions| questions.iter().filter_map(question_from_value).collect())
        .unwrap_or_default()
}

fn question_from_value(value: &Value) -> Option<Question> {
    let object = value.as_object()?;
    let question = object.get("question")?.as_str()?.to_string();
    let header = object
        .get("header")
        .and_then(Value::as_str)
        .unwrap_or("Question")
        .to_string();
    let options = object
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(question_option_from_value)
                .collect()
        })
        .unwrap_or_default();
    let multi_select = object
        .get("multiSelect")
        .or_else(|| object.get("multi_select"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(Question {
        question,
        header,
        options,
        multi_select,
    })
}

fn question_option_from_value(value: &Value) -> Option<QuestionOption> {
    let object = value.as_object()?;
    Some(QuestionOption {
        label: object.get("label")?.as_str()?.to_string(),
        description: object
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        preview: object
            .get("preview")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Maps to: CC `QuestionView` preview branch guard.
pub fn question_has_preview(question: &Question) -> bool {
    !question.multi_select
        && question
            .options
            .iter()
            .any(|option| option.preview.is_some())
}

pub fn hide_submit_tab(questions: &[Question]) -> bool {
    questions.len() == 1
        && !questions
            .first()
            .is_some_and(|question| question.multi_select)
}

pub fn all_questions_answered(
    questions: &[Question],
    answers: &BTreeMap<String, AnswerValue>,
) -> bool {
    questions
        .iter()
        .all(|question| !question.question.is_empty() && answers.contains_key(&question.question))
}

/// Maps to: CC `handleQuestionAnswer(...)` answer-string normalization.
pub fn answer_for_selection(
    label: &str,
    selected_values: &[String],
    text_input: Option<&str>,
    is_multi_select: bool,
) -> String {
    if is_multi_select {
        let mut values = selected_values
            .iter()
            .filter(|value| value.as_str() != "__other__")
            .cloned()
            .collect::<Vec<_>>();
        if selected_values.iter().any(|value| value == "__other__") {
            if let Some(text_input) = text_input.filter(|text| !text.trim().is_empty()) {
                values.push(text_input.to_string());
            }
        }
        return values.join(", ");
    }
    if let Some(text_input) = text_input.filter(|text| !text.trim().is_empty()) {
        return text_input.to_string();
    }
    if label == "__other__" {
        "Other".to_string()
    } else {
        label.to_string()
    }
}

pub fn toggle_multi_select_value(mut values: Vec<String>, value: &str) -> Vec<String> {
    if let Some(index) = values.iter().position(|candidate| candidate == value) {
        values.remove(index);
    } else {
        values.push(value.to_string());
    }
    values
}

/// Maps to: CC `submitAnswers(...)` `updatedInput` construction, including
/// selected-option previews and user notes in `annotations`.
pub fn build_updated_input_with_answers(
    original_input: &Value,
    questions: &[Question],
    answers: &BTreeMap<String, AnswerValue>,
    question_states: &BTreeMap<String, QuestionState>,
) -> Value {
    let mut object = original_input.as_object().cloned().unwrap_or_default();
    object.insert(
        "answers".to_string(),
        Value::Object(
            answers
                .iter()
                .map(|(question, answer)| (question.clone(), Value::String(answer.clone())))
                .collect::<Map<_, _>>(),
        ),
    );

    let annotations = build_annotations(questions, answers, question_states);
    if !annotations.is_empty() {
        object.insert("annotations".to_string(), Value::Object(annotations));
    }

    Value::Object(object)
}

fn build_annotations(
    questions: &[Question],
    answers: &BTreeMap<String, AnswerValue>,
    question_states: &BTreeMap<String, QuestionState>,
) -> Map<String, Value> {
    let mut annotations = Map::new();
    for question in questions {
        let Some(answer) = answers.get(&question.question) else {
            continue;
        };
        let notes = question_states
            .get(&question.question)
            .map(|state| state.text_input_value.trim())
            .filter(|notes| !notes.is_empty());
        let preview = question
            .options
            .iter()
            .find(|option| option.label == *answer)
            .and_then(|option| option.preview.as_ref());
        if preview.is_none() && notes.is_none() {
            continue;
        }
        let mut annotation = Map::new();
        if let Some(preview) = preview {
            annotation.insert("preview".to_string(), Value::String(preview.clone()));
        }
        if let Some(notes) = notes {
            annotation.insert("notes".to_string(), Value::String(notes.to_string()));
        }
        annotations.insert(question.question.clone(), Value::Object(annotation));
    }
    annotations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reducer_updates_answer_and_advances_like_official_state() {
        let state = MultipleChoiceState::default();
        let state = reduce_multiple_choice_state(
            &state,
            MultipleChoiceAction::SetAnswer {
                question_text: "Proceed?".to_string(),
                answer: "Yes".to_string(),
                should_advance: true,
                question_count: 2,
                hide_submit_tab: false,
            },
        );
        assert_eq!(state.answers.get("Proceed?"), Some(&"Yes".to_string()));
        assert_eq!(state.current_question_index, 1);
    }

    #[test]
    fn updated_input_includes_answers_preview_annotations_and_notes() {
        let input = serde_json::json!({
            "questions": [{
                "question": "Pick UI?",
                "header": "UI",
                "options": [
                    {"label": "A", "description": "First", "preview": "```ts\nA\n```"},
                    {"label": "B", "description": "Second"}
                ]
            }]
        });
        let questions = questions_from_input(&input);
        let answers = BTreeMap::from([("Pick UI?".to_string(), "A".to_string())]);
        let states = BTreeMap::from([(
            "Pick UI?".to_string(),
            QuestionState {
                selected_value: vec!["A".to_string()],
                text_input_value: "looks clearer".to_string(),
            },
        )]);

        let updated = build_updated_input_with_answers(&input, &questions, &answers, &states);

        assert_eq!(updated["answers"]["Pick UI?"], "A");
        assert_eq!(
            updated["annotations"]["Pick UI?"]["preview"],
            "```ts\nA\n```"
        );
        assert_eq!(updated["annotations"]["Pick UI?"]["notes"], "looks clearer");
    }

    #[test]
    fn answer_for_selection_replaces_multi_select_other_with_custom_text() {
        let answer = answer_for_selection(
            "",
            &["Cache".to_string(), "__other__".to_string()],
            Some("custom notes"),
            true,
        );
        assert_eq!(answer, "Cache, custom notes");

        let empty_other = answer_for_selection("", &["__other__".to_string()], Some(""), true);
        assert_eq!(empty_other, "");
    }

    #[test]
    fn parses_multi_select_camel_case_shape() {
        let questions = questions_from_input(&serde_json::json!({
            "questions": [{
                "question": "Features?",
                "header": "Features",
                "multiSelect": true,
                "options": [
                    {"label": "A", "description": "First"},
                    {"label": "B", "description": "Second"}
                ]
            }]
        }));

        assert_eq!(questions.len(), 1);
        assert!(questions[0].multi_select);
    }
}
