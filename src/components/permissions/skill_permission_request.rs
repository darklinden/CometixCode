//! Maps to: CC
//! `components/permissions/SkillPermissionRequest/SkillPermissionRequest.tsx`.
//!
//! UI-only permission dialog for Skill tool invocations. It mirrors the
//! official option model (`yes`, `yes-exact`, `yes-prefix`, `no`) and returns
//! permission-rule updates through the existing Rust prompt response seam;
//! persistence remains outside this component.

use super::permission_dialog::PermissionDialog;
use super::permission_rule_explanation::{PermissionRuleExplanation, PermissionRuleToolType};
use super::worker_badge::WorkerBadgeProps;
use crate::components::custom_select::{Select, SelectLayout, SelectOptionData};
use crate::tools::skill_tool::constants::SKILL_TOOL_NAME;
use crate::types::permissions::{
    PermissionBehavior, PermissionMode, PermissionPromptChoice, PermissionPromptResponse,
    PermissionRequest as PermissionRequestData, PermissionRuleValue, PermissionUpdate,
    PermissionUpdateDestination,
};
use crate::utils::theme::Theme;
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillPermissionOptionValue {
    Yes,
    YesExact,
    YesPrefix,
    No,
}

impl SkillPermissionOptionValue {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::YesExact => "yes-exact",
            Self::YesPrefix => "yes-prefix",
            Self::No => "no",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillPermissionOption {
    pub label: String,
    pub value: SkillPermissionOptionValue,
}

impl SkillPermissionOption {
    fn select(label: impl Into<String>, value: SkillPermissionOptionValue) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }

    pub fn to_select_option(&self) -> SelectOptionData {
        SelectOptionData {
            label: self.label.clone(),
            description: None,
            dim_description: true,
            value: self.value.as_str().to_string(),
            disabled: false,
            input: None,
        }
    }
}

#[derive(Default, Props)]
pub struct SkillPermissionRequestProps<'a> {
    pub request: Option<PermissionRequestData>,
    pub worker_badge: Option<WorkerBadgeProps>,
    /// Maps to CC `shouldShowAlwaysAllowOptions()`.
    pub show_always_allow_options: bool,
    pub on_select: HandlerMut<'a, PermissionPromptResponse>,
    pub on_cancel: HandlerMut<'a, ()>,
}

fn default_request() -> PermissionRequestData {
    PermissionRequestData {
        permission_result: None,
        id: String::new(),
        tool_use_id: String::new(),
        tool_name: SKILL_TOOL_NAME.to_string(),
        mcp_info: None,
        decision_reason: None,
        description: String::new(),
        message: String::new(),
        input_summary: String::new(),
        input: serde_json::Value::Null,
        call_input: None,
        rule: PermissionRuleValue::new(SKILL_TOOL_NAME, None),
        suggestions: Vec::new(),
        blocked_path: None,
        metadata: None,
        is_compound_command: false,
        mode: PermissionMode::Default,
    }
}

/// Maps to: CC `parseInput(...)` inside `SkillPermissionRequest`.
pub fn skill_name_from_permission_input(input: &serde_json::Value) -> String {
    input
        .get("skill")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string()
}

fn original_cwd_for_label() -> String {
    crate::bootstrap::state::get_original_cwd()
        .display()
        .to_string()
}

fn skill_prefix(skill: &str) -> &str {
    match skill.find(' ') {
        Some(space_index) if space_index > 0 => &skill[..space_index],
        _ => skill,
    }
}

/// Maps to: CC `options = useMemo(...)` in `SkillPermissionRequest`.
pub fn skill_permission_options(
    skill: &str,
    original_cwd: &str,
    show_always_allow_options: bool,
) -> Vec<SkillPermissionOption> {
    let mut options = vec![SkillPermissionOption::select(
        "Yes",
        SkillPermissionOptionValue::Yes,
    )];

    if show_always_allow_options {
        options.push(SkillPermissionOption::select(
            format!("Yes, and don't ask again for {skill} in {original_cwd}"),
            SkillPermissionOptionValue::YesExact,
        ));

        if let Some(space_index) = skill.find(' ').filter(|space_index| *space_index > 0) {
            let command_prefix = &skill[..space_index];
            options.push(SkillPermissionOption::select(
                format!(
                    "Yes, and don't ask again for {command_prefix}:* commands in {original_cwd}"
                ),
                SkillPermissionOptionValue::YesPrefix,
            ));
        }
    }

    options.push(SkillPermissionOption::select(
        "No",
        SkillPermissionOptionValue::No,
    ));
    options
}

/// Maps to: CC `handleSelect(...)` update payloads in
/// `SkillPermissionRequest`.
pub fn skill_permission_option_to_prompt_response(
    value: SkillPermissionOptionValue,
    skill: &str,
) -> PermissionPromptResponse {
    match value {
        SkillPermissionOptionValue::Yes => {
            PermissionPromptResponse::new(PermissionPromptChoice::AllowOnce)
        }
        SkillPermissionOptionValue::YesExact => {
            PermissionPromptResponse::new(PermissionPromptChoice::AllowOnce)
                .with_permission_updates(vec![PermissionUpdate::AddRules {
                    destination: PermissionUpdateDestination::LocalSettings,
                    behavior: PermissionBehavior::Allow,
                    rules: vec![PermissionRuleValue::new(
                        SKILL_TOOL_NAME,
                        Some(skill.to_string()),
                    )],
                }])
        }
        SkillPermissionOptionValue::YesPrefix => {
            PermissionPromptResponse::new(PermissionPromptChoice::AllowOnce)
                .with_permission_updates(vec![PermissionUpdate::AddRules {
                    destination: PermissionUpdateDestination::LocalSettings,
                    behavior: PermissionBehavior::Allow,
                    rules: vec![PermissionRuleValue::new(
                        SKILL_TOOL_NAME,
                        Some(format!("{}:*", skill_prefix(skill))),
                    )],
                }])
        }
        SkillPermissionOptionValue::No => {
            PermissionPromptResponse::new(PermissionPromptChoice::Deny)
        }
    }
}

/// Maps to: CC `SkillPermissionRequest` render path.
#[component]
pub fn SkillPermissionRequest<'a>(
    props: &mut SkillPermissionRequestProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let request = props.request.clone().unwrap_or_else(default_request);
    let skill = skill_name_from_permission_input(&request.input);
    let original_cwd = original_cwd_for_label();
    let options = skill_permission_options(&skill, &original_cwd, props.show_always_allow_options);
    let option_count = options.len().max(1);
    let focused_index = hooks.use_state(|| 0usize);
    let mut pending_select = hooks.use_state(|| Option::<SkillPermissionOptionValue>::None);
    let mut pending_cancel = hooks.use_state(|| false);

    hooks.use_terminal_events({
        let mut focused_index = focused_index;
        let mut pending_select = pending_select;
        let mut pending_cancel = pending_cancel;
        let options = options.clone();
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
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    focused_index.set(focused_index.get().saturating_sub(1));
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    focused_index
                        .set((focused_index.get() + 1).min(options.len().saturating_sub(1)));
                }
                KeyCode::Enter => {
                    if let Some(option) = options.get(focused_index.get()) {
                        pending_select.set(Some(option.value));
                    }
                }
                KeyCode::Esc => {
                    // CC `PermissionPrompt.onCancel` rejects the tool use.
                    pending_select.set(Some(SkillPermissionOptionValue::No));
                }
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    pending_cancel.set(true);
                }
                _ => {}
            }
        }
    });

    let selected = { pending_select.read().clone() };
    if let Some(value) = selected {
        pending_select.set(None);
        (props.on_select)(skill_permission_option_to_prompt_response(value, &skill));
    }
    if pending_cancel.get() {
        pending_cancel.set(false);
        (props.on_cancel)(());
    }

    let focused = focused_index.get().min(option_count.saturating_sub(1));
    let select_options = options
        .iter()
        .map(SkillPermissionOption::to_select_option)
        .collect::<Vec<_>>();
    let visible_from = focused.saturating_sub(6);
    // Maps to: CC `SkillPermissionRequest.tsx:46-52`
    // ```
    // const commandObj =
    //   toolUseConfirm.permissionResult.behavior === 'ask' &&
    //   toolUseConfirm.permissionResult.metadata &&
    //   'command' in toolUseConfirm.permissionResult.metadata
    //     ? toolUseConfirm.permissionResult.metadata.command
    //     : undefined
    // ```
    // and `:236` `<Text dimColor>{commandObj?.description}</Text>`.
    //
    // This is the SKILL's own description, produced by `SkillTool.ts:576`. It is
    // NOT `toolUseConfirm.description` (`SkillTool.ts:342` →
    // `Execute skill: ${skill}`, the tool describing itself) and not the
    // decision's `message`; the port rendered `description` until #133 gave the
    // request its `metadata` carrier.
    //
    // The `behavior === 'ask'` guard is structural in this port: `metadata` only
    // ever reaches `PermissionRequest` from `PermissionResult::Ask`
    // (`permissions.rs#process_tool_permission_result`).
    let command_description = match request.metadata.as_ref() {
        Some(crate::utils::permissions::permission_result::PermissionMetadata::Command {
            command,
        }) => command.description.clone().unwrap_or_default(),
        None => String::new(),
    };

    element! {
        PermissionDialog(
            title: format!("Use skill \"{skill}\"?"),
            worker_badge: props.worker_badge.clone(),
        ) {
            Text(
                content: "Claude may use instructions, code, or files from this Skill.".to_string(),
                wrap: TextWrap::Wrap,
            )
            // CC `:235-237` renders this Box unconditionally; `commandObj?.description`
            // simply resolves to `undefined` and the dim Text is empty, so the
            // `paddingY={1}` block still occupies its rows.
            View(flex_direction: FlexDirection::Column, padding_left: 2u32, padding_right: 2u32, padding_top: 1u32, padding_bottom: 1u32) {
                Text(content: command_description.clone(), dim: true, wrap: TextWrap::Wrap)
            }
            View(flex_direction: FlexDirection::Column) {
                PermissionRuleExplanation(
                    decision_reason: request.decision_reason.clone(),
                    tool_type: PermissionRuleToolType::Tool,
                    permission_mode: request.mode,
                )
                Select(
                    options: select_options,
                    focused_index: focused,
                    selected_value: None,
                    visible_option_count: option_count,
                    visible_from_index: visible_from,
                    layout: SelectLayout::Expanded,
                    hide_indexes: true,
                    is_disabled: false,
                )
                Text(content: "Esc to cancel".to_string(), color: theme.inactive)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::theme;

    #[test]
    fn skill_permission_options_match_official_exact_and_prefix_rules() {
        let options = skill_permission_options("deploy staging", "/repo", true);
        assert_eq!(
            options
                .iter()
                .map(|option| option.value)
                .collect::<Vec<_>>(),
            vec![
                SkillPermissionOptionValue::Yes,
                SkillPermissionOptionValue::YesExact,
                SkillPermissionOptionValue::YesPrefix,
                SkillPermissionOptionValue::No,
            ]
        );
        assert_eq!(
            options[1].label,
            "Yes, and don't ask again for deploy staging in /repo"
        );
        assert_eq!(
            options[2].label,
            "Yes, and don't ask again for deploy:* commands in /repo"
        );
    }

    #[test]
    fn skill_permission_options_hide_always_allow_when_managed_only() {
        let options = skill_permission_options("deploy staging", "/repo", false);
        assert_eq!(
            options
                .iter()
                .map(|option| option.value)
                .collect::<Vec<_>>(),
            vec![
                SkillPermissionOptionValue::Yes,
                SkillPermissionOptionValue::No
            ]
        );
    }

    #[test]
    fn skill_permission_response_emits_official_local_settings_rule_updates() {
        let exact = skill_permission_option_to_prompt_response(
            SkillPermissionOptionValue::YesExact,
            "deploy staging",
        );
        assert_eq!(exact.choice, PermissionPromptChoice::AllowOnce);
        assert_eq!(
            exact.permission_updates,
            vec![PermissionUpdate::AddRules {
                destination: PermissionUpdateDestination::LocalSettings,
                behavior: PermissionBehavior::Allow,
                rules: vec![PermissionRuleValue::new(
                    SKILL_TOOL_NAME,
                    Some("deploy staging".to_string())
                )],
            }]
        );

        let prefix = skill_permission_option_to_prompt_response(
            SkillPermissionOptionValue::YesPrefix,
            "deploy staging",
        );
        assert_eq!(
            prefix.permission_updates,
            vec![PermissionUpdate::AddRules {
                destination: PermissionUpdateDestination::LocalSettings,
                behavior: PermissionBehavior::Allow,
                rules: vec![PermissionRuleValue::new(
                    SKILL_TOOL_NAME,
                    Some("deploy:*".to_string())
                )],
            }]
        );
    }

    fn skill_request(
        metadata: Option<crate::utils::permissions::permission_result::PermissionMetadata>,
        description: &str,
    ) -> PermissionRequestData {
        PermissionRequestData {
            permission_result: None,
            id: "req".to_string(),
            tool_use_id: "toolu".to_string(),
            tool_name: SKILL_TOOL_NAME.to_string(),
            mcp_info: None,
            decision_reason: None,
            description: description.to_string(),
            message: String::new(),
            input_summary: "deploy staging".to_string(),
            input: serde_json::json!({ "skill": "deploy staging" }),
            call_input: None,
            rule: PermissionRuleValue::new(SKILL_TOOL_NAME, Some("deploy staging".to_string())),
            suggestions: Vec::new(),
            blocked_path: None,
            metadata,
            is_compound_command: false,
            mode: PermissionMode::Default,
        }
    }

    fn command_metadata(
        description: &str,
    ) -> crate::utils::permissions::permission_result::PermissionMetadata {
        crate::utils::permissions::permission_result::PermissionMetadata::Command {
            command: crate::utils::permissions::permission_result::PermissionCommandMetadata {
                name: "deploy".to_string(),
                description: Some(description.to_string()),
                extra: std::collections::BTreeMap::new(),
            },
        }
    }

    #[test]
    fn skill_permission_request_renders_official_title_and_warning_copy() {
        // Re-derived 2026-08-26 (#133): the dim line under the warning is
        // `commandObj?.description` (`SkillPermissionRequest.tsx:236`), sourced
        // from `permissionResult.metadata.command` (`:47-52`) — not
        // `toolUseConfirm.description`, which for this tool is
        // `Execute skill: ${skill}` (`SkillTool.ts:342`). The old fixture put
        // the skill's description on `description` and asserted it rendered,
        // which is the port's shape, not CC's.
        let request = skill_request(Some(command_metadata("Deploys the app")), "");

        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                SkillPermissionRequest(
                    request: Some(request),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(120))
        .to_string();

        assert!(
            text.contains("Use skill \"deploy staging\"?"),
            "canvas=\n{text}"
        );
        assert!(
            text.contains("Claude may use instructions, code, or files from this Skill."),
            "canvas=\n{text}"
        );
        assert!(text.contains("Deploys the app"), "canvas=\n{text}");
        assert!(text.contains("deploy:*"), "canvas=\n{text}");
    }

    /// Maps to: CC `SkillPermissionRequest.tsx:46-52` and `:236`
    /// `<Text dimColor>{commandObj?.description}</Text>`, whose source is
    /// `toolUseConfirm.permissionResult.metadata.command` — produced by
    /// `SkillTool.ts:576`.
    ///
    /// `toolUseConfirm.description` is a different string entirely
    /// (`SkillTool.ts:342` `Execute skill: ${skill}`, computed at
    /// `useCanUseTool.tsx:138-143`); CC never renders it in this dialog.
    #[test]
    fn skill_permission_request_matches_official_metadata_command_description() {
        let render = |request: PermissionRequestData| {
            element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    SkillPermissionRequest(
                        request: Some(request),
                        show_always_allow_options: true,
                    )
                }
            }
            .render(Some(120))
            .to_string()
        };

        // The tool's own `description()` is present but must NOT be rendered;
        // the skill's description from `metadata.command` must be.
        let with_metadata = render(skill_request(
            Some(command_metadata("Deploys the app to staging")),
            "Execute skill: deploy staging",
        ));
        assert!(
            with_metadata.contains("Deploys the app to staging"),
            "canvas=\n{with_metadata}"
        );
        assert!(
            !with_metadata.contains("Execute skill: deploy staging"),
            "the tool's own description is not this dialog's dim line; canvas=\n{with_metadata}"
        );

        // No metadata: CC's `commandObj?.description` is `undefined`, so the
        // dim line is empty — and `description` still must not stand in.
        let without_metadata = render(skill_request(None, "Execute skill: deploy staging"));
        assert!(
            !without_metadata.contains("Execute skill: deploy staging"),
            "canvas=\n{without_metadata}"
        );

        // The tool's own `description(input)` stays reachable and unchanged
        // (`SkillTool.ts:342`).
        assert_eq!(
            crate::tool::ToolCall::description(
                &crate::tools::skill_tool::SkillTool,
                &serde_json::json!({ "skill": "deploy staging" }),
            ),
            "Execute skill: deploy staging",
        );
    }
}
