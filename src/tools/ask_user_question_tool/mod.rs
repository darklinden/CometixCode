//! AskUserQuestion tool metadata and UI.
//!
//! Maps to:
//! - CC `tools/AskUserQuestionTool/AskUserQuestionTool.tsx`
//! - CC `tools/AskUserQuestionTool/prompt.ts`
//! - CC `tools/AskUserQuestionTool/UI.tsx`
//!
//! Execution lives in this module, dispatched from `services/tools/tool_execution.rs`.

pub mod prompt;
pub mod ui;

/// Maps to: CC `AskUserQuestionTool.tsx:23-42` `questionOptionSchema`.
fn question_option_schema() -> crate::utils::zod::Schema {
    use crate::utils::zod;
    zod::object(vec![
        (
            "label",
            zod::string().describe(
                "The display text for this option that the user will see and select. Should be concise (1-5 words) and clearly describe the choice.",
            ),
        ),
        (
            "description",
            zod::string().describe(
                "Explanation of what this option means or what will happen if chosen. Useful for providing context about trade-offs or implications.",
            ),
        ),
        (
            "preview",
            zod::string().optional().describe(
                "Optional preview content rendered when this option is focused. Use for mockups, code snippets, or visual comparisons that help users compare options. See the tool description for the expected content format.",
            ),
        ),
    ])
}

/// Maps to: CC `AskUserQuestionTool.tsx:44-70` `questionSchema`.
fn question_schema() -> crate::utils::zod::Schema {
    use crate::utils::zod;
    zod::object(vec![
        (
            "question",
            zod::string().describe(
                "The complete question to ask the user. Should be clear, specific, and end with a question mark. Example: \"Which library should we use for date formatting?\" If multiSelect is true, phrase it accordingly, e.g. \"Which features do you want to enable?\"",
            ),
        ),
        (
            "header",
            zod::string().describe(format!(
                "Very short label displayed as a chip/tag (max {} chars). Examples: \"Auth method\", \"Library\", \"Approach\".",
                prompt::ASK_USER_QUESTION_TOOL_CHIP_WIDTH
            )),
        ),
        (
            "options",
            zod::array(question_option_schema()).min(2).max(4).describe(
                "The available choices for this question. Must have 2-4 options. Each option should be a distinct, mutually exclusive choice (unless multiSelect is enabled). There should be no 'Other' option, that will be provided automatically.",
            ),
        ),
        (
            "multiSelect",
            zod::boolean().default(serde_json::json!(false)).describe(
                "Set to true to allow the user to select multiple options instead of just one. Use when choices are not mutually exclusive.",
            ),
        ),
    ])
}

/// Maps to: CC `AskUserQuestionTool.tsx:72-92` `annotationsSchema`.
fn annotations_schema() -> crate::utils::zod::Schema {
    use crate::utils::zod;
    let annotation_schema = zod::object(vec![
        (
            "preview",
            zod::string().optional().describe(
                "The preview content of the selected option, if the question used previews.",
            ),
        ),
        (
            "notes",
            zod::string()
                .optional()
                .describe("Free-text notes the user added to their selection."),
        ),
    ]);
    zod::record(annotation_schema).optional().describe(
        "Optional per-question annotations from the user (e.g., notes on preview selections). Keyed by question text.",
    )
}

/// Maps to: CC `AskUserQuestionTool.tsx:114-133` `commonFields` — spread into
/// the input schema after `questions`.
fn common_fields() -> Vec<crate::utils::zod::ObjectField> {
    use crate::utils::zod;
    vec![
        (
            "answers",
            zod::record(zod::string())
                .optional()
                .describe("User answers collected by the permission component"),
        ),
        ("annotations", annotations_schema()),
        (
            "metadata",
            zod::object(vec![(
                "source",
                zod::string().optional().describe(
                    "Optional identifier for the source of this question (e.g., \"remember\" for /remember command). Used for analytics tracking.",
                ),
            )])
            .optional()
            .describe("Optional metadata for tracking and analytics purposes. Not displayed to user."),
        ),
    ]
}

/// Maps to: CC `AskUserQuestionTool.tsx:135-148` `inputSchema`.
///
/// The trailing `.refine(UNIQUENESS_REFINE.check, …)` is invisible to the
/// projection but rejects duplicate question texts / option labels — duplicates
/// would silently overwrite entries in the `answers` map, which is keyed by
/// question text.
pub fn input_schema() -> &'static crate::utils::zod::Schema {
    static SCHEMA: std::sync::OnceLock<crate::utils::zod::Schema> = std::sync::OnceLock::new();
    SCHEMA.get_or_init(|| {
        use crate::utils::zod;
        let mut fields: Vec<zod::ObjectField> = vec![(
            "questions",
            zod::array(question_schema())
                .min(1)
                .max(4)
                .describe("Questions to ask the user (1-4 questions)"),
        )];
        fields.extend(common_fields());
        zod::strict_object(fields).refine(
            |data| {
                let questions = data
                    .get("questions")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                questions_are_unique(questions)
            },
            UNIQUENESS_REFINE_MESSAGE,
        )
    })
}

pub fn ask_user_question_tool_schema() -> crate::types::tools::Tool {
    crate::types::tools::Tool {
        name: prompt::ASK_USER_QUESTION_TOOL_NAME.to_string(),
        description: prompt::tool_prompt(),
        input_schema: crate::utils::zod_to_json_schema::zod_to_json_schema(input_schema()),
        ..Default::default()
    }
}

/// Maps to: CC `AskUserQuestionTool.tsx:110-111` `UNIQUENESS_REFINE.message`.
const UNIQUENESS_REFINE_MESSAGE: &str =
    "Question texts must be unique, option labels must be unique within each question";

/// Maps to: CC `AskUserQuestionTool.tsx:95-109` `UNIQUENESS_REFINE.check`.
/// Duplicate question text silently overwrites entries in the `answers` map,
/// which is keyed by question text.
fn questions_are_unique(questions: &[serde_json::Value]) -> bool {
    let mut seen_questions = std::collections::HashSet::new();
    for question in questions {
        let text = question
            .get("question")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !seen_questions.insert(text) {
            return false;
        }
        let mut seen_labels = std::collections::HashSet::new();
        for option in question
            .get("options")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let label = option
                .get("label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if !seen_labels.insert(label) {
                return false;
            }
        }
    }
    true
}

/// Maps to: CC `AskUserQuestionTool.tsx:327-342` `validateHtmlPreview`.
/// Deliberately not a parser — HTML5 parsers are error-recovering by spec and
/// accept anything; this checks model intent and the specific prohibitions.
fn validate_html_preview(preview: Option<&str>) -> Option<&'static str> {
    let preview = preview?;
    static FULL_DOCUMENT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)<\s*(html|body|!doctype)\b").expect("static regex")
    });
    static EXECUTABLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)<\s*(script|style)\b").expect("static regex")
    });
    static ANY_TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)<[a-z][^>]*>").expect("static regex"));

    if FULL_DOCUMENT.is_match(preview) {
        return Some(
            "preview must be an HTML fragment, not a full document (no <html>, <body>, or <!DOCTYPE>)",
        );
    }
    if EXECUTABLE.is_match(preview) {
        return Some(
            "preview must not contain <script> or <style> tags. Use inline styles via the style attribute if needed.",
        );
    }
    if !ANY_TAG.is_match(preview) {
        return Some(
            "preview must contain HTML (previewFormat is set to \"html\"). Wrap content in a tag like <div> or <pre>.",
        );
    }
    None
}

/// CC `tools/AskUserQuestionTool/AskUserQuestionTool.tsx` outputSchema
/// (:151-163): `{ questions, answers, annotations? }`.
///
/// CC also exports `_sdkInputSchema`/`_sdkOutputSchema` aliases (:168-169)
/// for out-of-repo SDK typegen; they are identical to the internal schemas
/// and have no in-repo reader, so Rust (no SDK pipeline) declares nothing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Output {
    /// CC `questions` — the questions that were asked.
    pub(crate) questions: Vec<serde_json::Value>,
    /// CC `answers` — question text to answer string. Insertion-ordered: CC
    /// iterates the object as the permission component built it, so the model
    /// copy follows the order the questions were asked in.
    pub(crate) answers: indexmap::IndexMap<String, String>,
    /// CC optional `annotations` keyed by question text.
    pub(crate) annotations: Option<serde_json::Value>,
}

/// Echo the user's interactive answers as the official result payload.
/// Maps to: CC `tools/AskUserQuestionTool/AskUserQuestionTool.tsx` `call`
/// (:296) — interaction happens in the permission UI; `call` returns answers.
pub(crate) fn ask_user_question_output(args: &serde_json::Value) -> Output {
    let questions = args
        .get("questions")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let answers = args
        .get("answers")
        .and_then(|value| value.as_object())
        .map(|map| {
            map.iter()
                .map(|(question, answer)| {
                    let answer_text = answer
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| answer.to_string());
                    (question.clone(), answer_text)
                })
                .collect::<indexmap::IndexMap<_, _>>()
        })
        .unwrap_or_default();
    let annotations = args.get("annotations").cloned();

    Output {
        questions,
        answers,
        annotations,
    }
}

fn ask_user_question_model_content(output: &Output) -> String {
    let answers_text = output
        .answers
        .iter()
        .map(|(question_text, answer)| {
            let annotation = output
                .annotations
                .as_ref()
                .and_then(|value| value.get(question_text));
            let mut parts = vec![format!("\"{question_text}\"=\"{answer}\"")];
            // CC `if (annotation?.preview)` / `if (annotation?.notes)`
            // (:306/:309) — JS truthy: the empty string is skipped.
            if let Some(preview) = annotation
                .and_then(|value| value.get("preview"))
                .and_then(|value| value.as_str())
                .filter(|preview| !preview.is_empty())
            {
                parts.push(format!("selected preview:\n{preview}"));
            }
            if let Some(notes) = annotation
                .and_then(|value| value.get("notes"))
                .and_then(|value| value.as_str())
                .filter(|notes| !notes.is_empty())
            {
                parts.push(format!("user notes: {notes}"));
            }
            parts.join(" ")
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "User has answered your questions: {answers_text}. You can now continue with the user's answers in mind."
    )
}

/// Behavioral half of CC `AskUserQuestionTool` — dispatched via `crate::tool::ToolCall`.
pub(crate) struct AskUserQuestionTool;

impl crate::tool::ToolCall for AskUserQuestionTool {
    fn name(&self) -> &'static str {
        "AskUserQuestion"
    }

    /// Maps to: CC `AskUserQuestionTool.tsx:207-215` `async prompt()` —
    /// computed: base prompt, plus the preview-format guidance when
    /// `getQuestionPreviewFormat()` is set (SDK opt-in). Same source the wire
    /// schema renders eagerly (`prompt::tool_prompt` owns the branch).
    fn prompt(
        &self,
        _tool: &crate::types::tools::Tool,
        _options: &crate::tool::ToolPromptOptions<'_>,
    ) -> String {
        prompt::tool_prompt()
    }

    /// Maps to: CC `AskUserQuestionTool.isEnabled()` channels guard
    /// (`AskUserQuestionTool.tsx:225-238`): `(feature('KAIROS') ||
    /// feature('KAIROS_CHANNELS')) && getAllowedChannels().length > 0` → false.
    /// These are build features, not the `tengu_harbor` GrowthBook gate.
    /// Interactive questions have no channel relay and would otherwise wait
    /// forever for a terminal user.
    fn is_enabled(&self) -> bool {
        let kairos_built = crate::utils::feature_flags::feature_enabled(
            crate::utils::feature_flags::FeatureFlag::Kairos,
        ) || crate::utils::feature_flags::feature_enabled(
            crate::utils::feature_flags::FeatureFlag::KairosChannels,
        );
        !kairos_built || crate::bootstrap::state::get_allowed_channels().is_empty()
    }

    /// Maps to: CC `AskUserQuestionTool.isConcurrencySafe()`
    /// (AskUserQuestionTool.tsx:239) — true.
    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }

    /// Maps to: CC `AskUserQuestionTool.isReadOnly()` (:242-244).
    fn is_read_only(&self, _args: &serde_json::Value) -> bool {
        true
    }

    /// Maps to: CC `AskUserQuestionTool.toAutoClassifierInput()`.
    fn to_auto_classifier_input(&self, args: &serde_json::Value) -> String {
        args.get("questions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|question| question.get("question").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Maps to: CC `AskUserQuestionTool.requiresUserInteraction()` — bypass
    /// mode must still prompt (permissions.ts 1e).
    fn requires_user_interaction(&self) -> bool {
        true
    }

    /// Maps to: CC `AskUserQuestionTool.searchHint` (:201).
    fn search_hint(&self) -> Option<&'static str> {
        Some("prompt the user with a multiple-choice question")
    }

    /// Maps to: CC `AskUserQuestionTool.tsx:204-206` `description()`.
    fn description(&self, _args: &serde_json::Value) -> String {
        prompt::DESCRIPTION.to_string()
    }

    /// Maps to: CC `AskUserQuestionTool.maxResultSizeChars` (:202).
    fn max_result_size_chars(&self) -> usize {
        100_000
    }

    /// Maps to: CC `AskUserQuestionTool.shouldDefer` (:203).
    fn should_defer(&self) -> bool {
        true
    }

    /// Maps to: CC `AskUserQuestionTool.userFacingName()` (:222-224) — the
    /// empty string; the transcript renders the questions themselves.
    fn user_facing_name(&self, _args: Option<&serde_json::Value>) -> String {
        String::new()
    }

    /// Maps to: CC `AskUserQuestionTool.validateInput()` (:251-268) plus the
    /// `UNIQUENESS_REFINE` (:94-112) that zod applies before `validateInput`
    /// runs. Rust has no schema-level refinement seam, so the uniqueness check
    /// leads here to keep the same rejection order.
    fn validate_input(
        &self,
        args: &serde_json::Value,
        _context: &crate::tool::ToolUseContext,
    ) -> crate::tool::ValidationResult {
        let questions = args
            .get("questions")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();

        if !questions_are_unique(questions) {
            return crate::tool::ValidationResult::error(UNIQUENESS_REFINE_MESSAGE.to_string(), 1);
        }

        if crate::bootstrap::state::get_question_preview_format()
            != Some(crate::bootstrap::state::QuestionPreviewFormat::Html)
        {
            return crate::tool::ValidationResult::Ok;
        }

        for question in questions {
            let question_text = question
                .get("question")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            for option in question
                .get("options")
                .and_then(serde_json::Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let preview = option.get("preview").and_then(serde_json::Value::as_str);
                if let Some(error) = validate_html_preview(preview) {
                    let label = option
                        .get("label")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    return crate::tool::ValidationResult::error(
                        format!("Option \"{label}\" in question \"{question_text}\": {error}"),
                        1,
                    );
                }
            }
        }

        crate::tool::ValidationResult::Ok
    }

    /// Maps to: CC `AskUserQuestionTool.checkPermissions()` (:269-275) —
    /// `{ behavior: 'ask', message: 'Answer questions?', updatedInput: input }`.
    ///
    /// The echo used to be dropped with the note "PermissionResult::Ask has no
    /// such field"; #142 step 3 gave the Ask arm CC's full
    /// `PermissionAskDecision` shape (`types/permissions.ts:204`), so the field
    /// exists and carries the input as upstream does.
    fn check_permissions(
        &self,
        args: &serde_json::Value,
        _context: &crate::tool::ToolUseContext,
    ) -> crate::utils::permissions::permission_result::PermissionResult {
        crate::utils::permissions::permission_result::PermissionResult::Ask {
            message: "Answer questions?".to_string(),
            updated_input: Some(args.clone()),
            decision_reason: None,
            suggestions: Vec::new(),
            blocked_path: None,
            metadata: None,
            is_bash_security_check_for_misparsing: false,
            pending_classifier_check: None,
            content_blocks: Vec::new(),
        }
    }

    fn call<'a>(
        &'a self,
        args: &'a serde_json::Value,
        request: &'a crate::types::permissions::PermissionRequest,
        _context: &'a crate::tool::ToolUseContext,
        _can_use_tool: Option<crate::tool::CanUseToolFn<'a>>,
        _parent_message: Option<&'a crate::types::message::AssistantMessage>,
        _on_progress: Option<crate::tool::ToolCallProgressFn<'a>>,
    ) -> futures::future::BoxFuture<'a, crate::tool::ToolResult> {
        Box::pin(async move {
            let _ = request;
            crate::tool::ToolResult {
                data: crate::tool::ToolOutput::AskUserQuestion(ask_user_question_output(args)),
                new_messages: Vec::new(),
            }
        })
    }

    /// Maps to: CC `tools/AskUserQuestionTool/AskUserQuestionTool.tsx`
    /// `mapToolResultToToolResultBlockParam` (:301-321).
    fn map_tool_result_to_tool_result_block_param(
        &self,
        data: &crate::tool::ToolOutput,
        _tool_use_id: &str,
    ) -> (String, crate::types::message::ToolResultStatus) {
        match data {
            crate::tool::ToolOutput::AskUserQuestion(output) => (
                ask_user_question_model_content(output),
                crate::types::message::ToolResultStatus::Success,
            ),
            crate::tool::ToolOutput::Composed {
                content, status, ..
            } => (content.clone(), *status),
            _ => (
                "<tool_use_error>AskUserQuestion returned an unexpected output variant</tool_use_error>"
                    .to_string(),
                crate::types::message::ToolResultStatus::Error,
            ),
        }
    }

    /// Maps to: CC recording AskUserQuestionTool's `Output` — the `call()`
    /// data echo (`AskUserQuestionTool.tsx:296-300`) — as the message's
    /// `toolUseResult`.
    fn tool_use_result(&self, data: &crate::tool::ToolOutput) -> Option<serde_json::Value> {
        match data {
            crate::tool::ToolOutput::AskUserQuestion(output) => Some(ui::output_to_value(output)),
            crate::tool::ToolOutput::Composed {
                content,
                status: crate::types::message::ToolResultStatus::Error,
                ..
            } => {
                let message = crate::utils::messages::extract_tag(content, "tool_use_error")
                    .unwrap_or_else(|| content.clone());
                Some(serde_json::Value::String(message))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_user_question_tool_schema_matches_official_input_shape() {
        let schema = ask_user_question_tool_schema();
        assert_eq!(schema.name, "AskUserQuestion");
        assert!(
            schema
                .description
                .contains("Users will always be able to select")
        );
        assert_eq!(
            schema.input_schema["required"],
            serde_json::json!(["questions"])
        );
        assert_eq!(
            schema.input_schema["properties"]["questions"]["minItems"],
            1
        );
        assert_eq!(
            schema.input_schema["properties"]["questions"]["maxItems"],
            4
        );
        let question = &schema.input_schema["properties"]["questions"]["items"];
        assert_eq!(
            question["required"],
            serde_json::json!(["question", "header", "options", "multiSelect"])
        );
        assert!(
            question["properties"]["options"]["description"]
                .as_str()
                .unwrap()
                .ends_with(
                    "There should be no 'Other' option, that will be provided automatically."
                )
        );
        let annotations = &schema.input_schema["properties"]["annotations"];
        assert_eq!(
            annotations["additionalProperties"]["properties"]["notes"]["type"],
            "string"
        );
        assert_eq!(
            annotations["additionalProperties"]["properties"]["preview"]["type"],
            "string"
        );
        assert_eq!(
            schema.input_schema["properties"]["metadata"]["properties"]["source"]["type"],
            "string"
        );
    }

    #[test]
    fn ask_user_question_uniqueness_refine_matches_official_message() {
        use crate::tool::ToolCall;

        let duplicate_questions = serde_json::json!({
            "questions": [
                {"question": "Same?", "header": "A", "options": [
                    {"label": "Yes", "description": "y"},
                    {"label": "No", "description": "n"}
                ]},
                {"question": "Same?", "header": "B", "options": [
                    {"label": "Yes", "description": "y"},
                    {"label": "No", "description": "n"}
                ]}
            ]
        });
        let duplicate_labels = serde_json::json!({
            "questions": [
                {"question": "Pick?", "header": "A", "options": [
                    {"label": "Yes", "description": "y"},
                    {"label": "Yes", "description": "also y"}
                ]}
            ]
        });

        let tool = AskUserQuestionTool;
        let context = crate::tool::ToolUseContext::default();
        for args in [duplicate_questions, duplicate_labels] {
            match tool.validate_input(&args, &context) {
                crate::tool::ValidationResult::Error {
                    message,
                    error_code,
                } => {
                    assert_eq!(message, UNIQUENESS_REFINE_MESSAGE);
                    assert_eq!(error_code, 1);
                }
                other => panic!("expected the uniqueness refinement to reject: {other:?}"),
            }
        }
    }

    #[test]
    fn ask_user_question_html_preview_validation_matches_official_messages() {
        assert_eq!(validate_html_preview(None), None);
        assert_eq!(validate_html_preview(Some("<div>ok</div>")), None);
        assert_eq!(
            validate_html_preview(Some("<!DOCTYPE html><div>x</div>")),
            Some(
                "preview must be an HTML fragment, not a full document (no <html>, <body>, or <!DOCTYPE>)"
            )
        );
        assert_eq!(
            validate_html_preview(Some("<div><script>x()</script></div>")),
            Some(
                "preview must not contain <script> or <style> tags. Use inline styles via the style attribute if needed."
            )
        );
        assert_eq!(
            validate_html_preview(Some("plain text")),
            Some(
                "preview must contain HTML (previewFormat is set to \"html\"). Wrap content in a tag like <div> or <pre>."
            )
        );
    }

    #[test]
    fn ask_user_question_prompt_appends_preview_guidance_for_the_active_format() {
        use crate::bootstrap::state::QuestionPreviewFormat;

        let base = prompt::ASK_USER_QUESTION_TOOL_PROMPT;
        for (format, expected_suffix) in [
            (None, ""),
            (
                Some(QuestionPreviewFormat::Markdown),
                prompt::PREVIEW_FEATURE_PROMPT_MARKDOWN,
            ),
            (
                Some(QuestionPreviewFormat::Html),
                prompt::PREVIEW_FEATURE_PROMPT_HTML,
            ),
        ] {
            crate::bootstrap::state::set_question_preview_format(format);
            assert_eq!(prompt::tool_prompt(), format!("{base}{expected_suffix}"));
        }
        crate::bootstrap::state::set_question_preview_format(Some(QuestionPreviewFormat::Markdown));
    }

    #[test]
    fn ask_user_question_metadata_matches_official_channels_and_classifier_contract() {
        use crate::tool::ToolCall;

        let tool = AskUserQuestionTool;
        assert_eq!(
            tool.to_auto_classifier_input(&serde_json::json!({
                "questions": [
                    {"question": "First?"},
                    {"question": "Second?"}
                ]
            })),
            "First? | Second?"
        );
        assert!(tool.requires_user_interaction());
        assert!(tool.is_read_only(&serde_json::json!({})));
        assert!(tool.should_defer());
        assert_eq!(tool.max_result_size_chars(), 100_000);
        assert_eq!(
            tool.search_hint(),
            Some("prompt the user with a multiple-choice question")
        );
        assert_eq!(tool.user_facing_name(None), "");
        assert!(matches!(
            tool.check_permissions(
                &serde_json::json!({}),
                &crate::tool::ToolUseContext::default()
            ),
            crate::utils::permissions::permission_result::PermissionResult::Ask { message, .. }
                if message == "Answer questions?"
        ));
    }

    #[tokio::test]
    async fn ask_user_question_tool_call_returns_official_output_schema_and_model_copy() {
        use crate::tool::ToolCall;

        let args = serde_json::json!({
            "questions": [{
                "question": "Proceed?",
                "header": "Proceed",
                "options": [
                    {"label": "Yes", "description": "Continue"},
                    {"label": "No", "description": "Stop"}
                ]
            }],
            "answers": {"Proceed?": "Yes"},
            "annotations": {"Proceed?": {"preview": "<div>ok</div>", "notes": "Looks good"}}
        });
        let request = crate::utils::permissions::permissions::mock_permission_request_with_input(
            "perm-ask".to_string(),
            "toolu_ask".to_string(),
            "AskUserQuestion".to_string(),
            "Answer questions?".to_string(),
            args.clone(),
            crate::types::permissions::PermissionMode::Default,
        );
        let tool = AskUserQuestionTool;
        let result = tool
            .call(
                &args,
                &request,
                &crate::tool::ToolUseContext::default(),
                None,
                None,
                None,
            )
            .await;

        let crate::tool::ToolOutput::AskUserQuestion(output) = result.data else {
            panic!("AskUserQuestion should return its official ToolOutput variant");
        };
        assert_eq!(output.questions.len(), 1);
        assert_eq!(output.answers.get("Proceed?"), Some(&"Yes".to_string()));
        assert!(output.annotations.is_some());

        let data = crate::tool::ToolOutput::AskUserQuestion(output);
        let (content, status) = tool.map_tool_result_to_tool_result_block_param(&data, "toolu_ask");
        assert_eq!(status, crate::types::message::ToolResultStatus::Success);
        assert!(content.contains("User has answered your questions"));
        assert!(content.contains("\"Proceed?\"=\"Yes\""));
        assert!(content.contains("selected preview:\n<div>ok</div>"));
        assert!(content.contains("user notes: Looks good"));

        // No display shape — the trait projects the raw Output object.
        let raw = tool
            .tool_use_result(&data)
            .expect("raw output should ride the row");
        assert_eq!(
            raw.get("answers"),
            Some(&serde_json::json!({"Proceed?": "Yes"}))
        );
        assert!(raw.get("annotations").is_some());
        assert_eq!(
            raw.get("questions")
                .and_then(|q| q.as_array())
                .map(Vec::len),
            Some(1)
        );
    }
}
