//! Maps to: CC `components/permissions/FallbackPermissionRequest.tsx`.
//!
//! Official fallback permission dialog for tools without a bespoke permission
//! request component. Analytics, feedback text editing, and persistence remain
//! outside this UI-only seam; selection is projected to the existing Rust
//! `PermissionPromptChoice` path.

use super::permission_dialog::PermissionDialog;
use super::permission_rule_explanation::{PermissionRuleExplanation, PermissionRuleToolType};
use super::worker_badge::WorkerBadgeProps;
use crate::components::custom_select::{Select, SelectLayout, SelectOptionData};
use crate::types::permissions::{
    PermissionMode, PermissionPromptChoice, PermissionRequest as PermissionRequestData,
    PermissionRuleValue,
};
use crate::utils::theme::Theme;
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackPermissionOptionValue {
    Yes,
    YesDontAskAgain,
    No,
}

impl FallbackPermissionOptionValue {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::YesDontAskAgain => "yes-dont-ask-again",
            Self::No => "no",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackPermissionOption {
    pub label: String,
    pub value: FallbackPermissionOptionValue,
}

impl FallbackPermissionOption {
    fn select(label: impl Into<String>, value: FallbackPermissionOptionValue) -> Self {
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
pub struct FallbackPermissionRequestProps<'a> {
    pub request: Option<PermissionRequestData>,
    pub worker_badge: Option<WorkerBadgeProps>,
    pub show_always_allow_options: bool,
    pub on_select: HandlerMut<'a, FallbackPermissionOptionValue>,
    pub on_cancel: HandlerMut<'a, ()>,
}

fn default_request() -> PermissionRequestData {
    PermissionRequestData {
        permission_result: None,
        id: String::new(),
        tool_use_id: String::new(),
        tool_name: "Tool".to_string(),
        mcp_info: None,
        description: String::new(),
        message: String::new(),
        input_summary: String::new(),
        input: serde_json::Value::Null,
        call_input: None,
        rule: PermissionRuleValue::new("Tool", None),
        decision_reason: None,
        suggestions: Vec::new(),
        blocked_path: None,
        metadata: None,
        is_compound_command: false,
        mode: PermissionMode::Default,
    }
}

/// Maps to: CC `FallbackOptionValue` selection branches.
pub fn fallback_permission_option_to_prompt_choice(
    value: FallbackPermissionOptionValue,
) -> PermissionPromptChoice {
    match value {
        FallbackPermissionOptionValue::Yes => PermissionPromptChoice::AllowOnce,
        FallbackPermissionOptionValue::YesDontAskAgain => PermissionPromptChoice::AlwaysAllow,
        FallbackPermissionOptionValue::No => PermissionPromptChoice::Deny,
    }
}

/// Maps to: CC fallback option construction, including MCP suffix stripping.
pub fn fallback_permission_options(
    user_facing_name: &str,
    original_cwd: &str,
    show_always_allow_options: bool,
) -> Vec<FallbackPermissionOption> {
    let mut options = vec![FallbackPermissionOption::select(
        "Yes",
        FallbackPermissionOptionValue::Yes,
    )];
    if show_always_allow_options {
        options.push(FallbackPermissionOption::select(
            format!("Yes, and don't ask again for {user_facing_name} commands in {original_cwd}"),
            FallbackPermissionOptionValue::YesDontAskAgain,
        ));
    }
    options.push(FallbackPermissionOption::select(
        "No",
        FallbackPermissionOptionValue::No,
    ));
    options
}

fn original_cwd_for_label() -> String {
    crate::bootstrap::state::get_original_cwd()
        .display()
        .to_string()
}

/// Maps to: CC `FallbackPermissionRequest.tsx:30-35`.
///
/// CC reads `toolUseConfirm.tool.userFacingName(toolUseConfirm.input)` — the
/// tool's OWN method, evaluated at render time against the live input — then
/// strips one trailing `" (MCP)"`, returning `(stripped, was_mcp)`.
///
/// The Rust analogue of `toolUseConfirm.tool` is the runtime registry
/// (`find_tool_call`, the same lookup `fill_tool_description` uses one step
/// earlier in this path). An unresolved name falls back to the tool name, which
/// is exactly `buildTool`'s effective default: `TOOL_DEFAULTS.userFacingName`
/// is `() => ''`, but `buildTool` overwrites it with `() => def.name` before
/// spreading `def` (`Tool.ts:786-791`).
///
/// Dynamic `mcp__*` tools are never in the static registry — CC creates them per
/// connection in `fetchToolsForClient(...)` — so their `userFacingName` comes
/// from that owner's Rust counterpart,
/// `services::mcp::client::mcp_tool_user_facing_name`, keyed off the
/// `mcp_info` this port already carries on the request (`Tool.mcpInfo`,
/// `client.ts:1774`). PARTIAL: CC prefers `tool.annotations?.title` over
/// `tool.name` for the display half (`client.ts:1973`); the title lives in the
/// live `McpState`, which `PermissionRequestProps.toolUseContext` carries in CC
/// but this port's dialog props do not, so the raw tool name is used.
///
/// This used to be derived from `request.title`, a field whose only producer
/// fabricated `"Claude wants to use {tool}"` — a string that does not exist
/// anywhere in CC. It fed BOTH the body line and the always-allow option label.
pub fn fallback_user_facing_names(request: &PermissionRequestData) -> (String, bool) {
    let original = crate::services::tools::tool_execution::find_tool_call(&request.tool_name)
        .map(|tool| tool.user_facing_name(Some(&request.input)))
        .filter(|name| !name.is_empty())
        .or_else(|| {
            request.mcp_info.as_ref().map(|info| {
                crate::services::mcp::client::mcp_tool_user_facing_name(
                    &info.server_name,
                    &crate::services::mcp::client::mcp_tool_display_name(None, &info.tool_name),
                )
            })
        })
        .unwrap_or_else(|| request.tool_name.clone());
    match original.strip_suffix(" (MCP)") {
        Some(stripped) => (stripped.to_string(), true),
        None => (original, false),
    }
}

/// Maps to: CC `FallbackPermissionRequest.tsx:166-178` — the single `<Text>`
/// holding `` `${userFacingName}(` ``, the tool's own
/// `renderToolUseMessage(input, { theme, verbose: true })`, `')'`, and a dim
/// `' (MCP)'` when the ORIGINAL name ended with it.
///
/// A renderer returning `null` produces no children, i.e. `Name()` — CC's
/// row-hiding meaning for `null` belongs to `AssistantToolUseMessage.tsx:148-150`
/// and has no counterpart inside a dialog that is already on screen.
pub fn fallback_tool_preview(request: &PermissionRequestData) -> String {
    let (user_facing_name, is_mcp) = fallback_user_facing_names(request);
    let rendered =
        crate::components::messages::assistant_tool_use_message::render_tool_use_message(
            &request.tool_name,
            &request.input,
            crate::components::messages::user_tool_result_message::utils::ToolRenderOptions {
                verbose: true,
                ..Default::default()
            },
        )
        .unwrap_or_default();
    let suffix = if is_mcp { " (MCP)" } else { "" };
    format!("{user_facing_name}({rendered}){suffix}")
}

pub fn truncate_to_lines(input: &str, max_lines: usize) -> String {
    if max_lines == 0 {
        return String::new();
    }
    let mut lines = input.lines();
    let mut out = Vec::new();
    for _ in 0..max_lines {
        let Some(line) = lines.next() else {
            return out.join("\n");
        };
        out.push(line.to_string());
    }
    if lines.next().is_some() {
        if let Some(last) = out.last_mut() {
            last.push('…');
        }
    }
    out.join("\n")
}

/// Maps to: CC `FallbackPermissionRequest` render path.
#[component]
pub fn FallbackPermissionRequest<'a>(
    props: &mut FallbackPermissionRequestProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let request = props.request.clone().unwrap_or_else(default_request);
    let (user_facing_name, _) = fallback_user_facing_names(&request);
    let original_cwd = original_cwd_for_label();
    let options = fallback_permission_options(
        &user_facing_name,
        &original_cwd,
        props.show_always_allow_options,
    );
    let option_count = options.len().max(1);
    let focused_index = hooks.use_state(|| 0usize);
    let mut pending_select = hooks.use_state(|| Option::<FallbackPermissionOptionValue>::None);
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
                    // CC PermissionPrompt.onCancel in Fallback rejects the tool use.
                    pending_select.set(Some(FallbackPermissionOptionValue::No));
                }
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    pending_cancel.set(true);
                }
                _ => {}
            }
        }
    });

    let selected = { *pending_select.read() };
    if let Some(value) = selected {
        pending_select.set(None);
        (props.on_select)(value);
    }
    if pending_cancel.get() {
        pending_cancel.set(false);
        (props.on_cancel)(());
    }

    let select_options = options
        .iter()
        .map(FallbackPermissionOption::to_select_option)
        .collect::<Vec<_>>();
    let focused = focused_index.get().min(option_count - 1);
    let preview = fallback_tool_preview(&request);
    let description = truncate_to_lines(&request.description, 3);

    element! {
        PermissionDialog(
            title: "Tool use".to_string(),
            worker_badge: props.worker_badge.clone(),
        ) {
            View(flex_direction: FlexDirection::Column, padding_left: 2u32, padding_right: 2u32, padding_top: 1u32, padding_bottom: 1u32) {
                Text(content: preview, wrap: TextWrap::Wrap)
                #(if description.trim().is_empty() {
                    None
                } else {
                    Some(element! { Text(content: description, color: theme.inactive, wrap: TextWrap::Wrap) })
                })
            }
            View(flex_direction: FlexDirection::Column) {
                PermissionRuleExplanation(
                    decision_reason: request.decision_reason.clone(),
                    tool_type: PermissionRuleToolType::Tool,
                    permission_mode: request.mode,
                )
                Text(content: "Do you want to proceed?".to_string(), wrap: TextWrap::NoWrap)
                Select(
                    options: select_options,
                    focused_index: focused,
                    visible_option_count: option_count,
                    layout: SelectLayout::Compact,
                    hide_indexes: true,
                )
                View(margin_top: 1u32) {
                    Text(content: "Esc to cancel".to_string(), color: theme.inactive, wrap: TextWrap::NoWrap)
                }
            }
        }
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

    fn fallback_request() -> PermissionRequestData {
        PermissionRequestData {
            permission_result: None,
            id: "req".to_string(),
            tool_use_id: "toolu".to_string(),
            tool_name: "CustomTool".to_string(),
            mcp_info: None,
            decision_reason: None,
            description: "line1\nline2\nline3\nline4".to_string(),
            message: String::new(),
            input_summary: "arg: value".to_string(),
            input: serde_json::json!({"description":"arg: value"}),
            call_input: None,
            rule: PermissionRuleValue::new("CustomTool", Some("arg: value".to_string())),
            suggestions: Vec::new(),
            blocked_path: None,
            metadata: None,
            is_compound_command: false,
            mode: PermissionMode::Default,
        }
    }

    /// Builds the Agent request through the production producers rather than by
    /// hand: `mock_permission_request_with_input` is what
    /// `hasPermissionsToUseToolInner` uses, and `fill_tool_description` is the
    /// Rust owner of CC `useCanUseTool.tsx:138-143`
    /// (`await tool.description(input, …)`).
    fn agent_request(input: serde_json::Value) -> PermissionRequestData {
        let mut request =
            crate::utils::permissions::permissions::mock_permission_request_with_input(
                "perm-agent",
                "toolu_agent",
                "Agent",
                serde_json::to_string(&input).unwrap_or_default(),
                input,
                PermissionMode::Default,
            );
        crate::hooks::tool_permission::handlers::interactive_handler::fill_tool_description(
            &mut request,
        );
        request
    }

    fn explore_input() -> serde_json::Value {
        serde_json::json!({
            "description": "Explore workdir files",
            "prompt": "List the files under the working directory and summarise them.",
            "subagent_type": "Explore",
            "model": "haiku"
        })
    }

    #[test]
    fn fallback_permission_options_include_official_always_allow_label() {
        let options = fallback_permission_options("CustomTool", "/repo", true);
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].label, "Yes");
        assert_eq!(
            options[1].label,
            "Yes, and don't ask again for CustomTool commands in /repo"
        );
        assert_eq!(options[2].label, "No");
    }

    #[test]
    fn fallback_permission_request_renders_preview_rule_and_truncated_description() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(fallback_request()),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(120))
        .to_string();

        assert!(text.contains("Tool use"), "canvas=\n{text}");
        assert!(text.contains("CustomTool(arg: value)"), "canvas=\n{text}");
        assert!(text.contains("line3…"), "canvas=\n{text}");
        assert!(text.contains("Do you want to proceed?"), "canvas=\n{text}");
    }

    /// Maps to: CC `FallbackPermissionRequest.tsx:164-179`.
    /// - `:164` title is the literal `"Tool use"`.
    /// - `:166-172` body is `userFacingName(renderToolUseMessage(input, {verbose:true}))`,
    ///   i.e. `AgentTool/UI.tsx:1000-1007` + `UI.tsx:472-483` → `Explore(Explore workdir files)`.
    /// - `:179` dim description is `AgentTool.tsx:376-378` → `Launch a new agent`.
    #[test]
    fn fallback_permission_request_matches_official_agent_tool_use_line() {
        let request = agent_request(explore_input());
        assert_eq!(request.description, "Launch a new agent");

        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(request),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(140))
        .to_string();

        assert!(text.contains("Tool use"), "canvas=\n{text}");
        assert!(
            text.contains("Explore(Explore workdir files)"),
            "canvas=\n{text}"
        );
        assert!(text.contains("Launch a new agent"), "canvas=\n{text}");
        // The pre-fix canvas rendered `Claude wants to use Agent({"description":…,
        // "prompt":…,"subagent_type":…})` — a fabricated title plus the raw
        // serialized input.
        assert!(!text.contains("Claude wants to use"), "canvas=\n{text}");
        assert!(!text.contains("subagent_type"), "canvas=\n{text}");
        assert!(!text.contains("prompt"), "canvas=\n{text}");
    }

    /// Maps to: CC `FallbackPermissionRequest.tsx:136-141` — the always-allow
    /// label reuses the SAME `userFacingName`, not a separate projection.
    #[test]
    fn fallback_permission_options_matches_official_agent_always_allow_label() {
        let request = agent_request(explore_input());
        let (user_facing_name, is_mcp) = fallback_user_facing_names(&request);
        assert_eq!(user_facing_name, "Explore");
        assert!(!is_mcp);

        let cwd = original_cwd_for_label();
        let options = fallback_permission_options(&user_facing_name, &cwd, true);
        assert_eq!(
            options[1].label,
            format!("Yes, and don't ask again for Explore commands in {cwd}")
        );

        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(agent_request(explore_input())),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(200))
        .to_string();
        assert!(
            text.contains("Yes, and don't ask again for Explore commands in"),
            "canvas=\n{text}"
        );
        assert!(
            !text.contains("don't ask again for Agent commands"),
            "canvas=\n{text}"
        );
    }

    /// Maps to: CC `AgentTool/UI.tsx:995-1010` — absent, `general-purpose`, and
    /// `worker` all resolve to `Agent`; the empty string is JS-falsy and does
    /// the same.
    #[test]
    fn fallback_user_facing_names_matches_official_agent_special_cases() {
        for input in [
            serde_json::json!({"description":"d","prompt":"p"}),
            serde_json::json!({"description":"d","prompt":"p","subagent_type":"general-purpose"}),
            serde_json::json!({"description":"d","prompt":"p","subagent_type":"worker"}),
            serde_json::json!({"description":"d","prompt":"p","subagent_type":""}),
        ] {
            let request = agent_request(input.clone());
            assert_eq!(
                fallback_user_facing_names(&request).0,
                "Agent",
                "input={input}"
            );
            assert_eq!(fallback_tool_preview(&request), "Agent(d)", "input={input}");
        }
    }

    /// Maps to: CC `AgentTool/UI.tsx:478-480` — `if (!description || !prompt)
    /// return null`. A `null` renderer contributes no children inside the
    /// dialog's `<Text>`, so the line is `Explore()`.
    #[test]
    fn fallback_tool_preview_matches_official_null_render_tool_use_message() {
        let no_prompt = agent_request(serde_json::json!({
            "description": "Explore workdir files",
            "subagent_type": "Explore"
        }));
        assert_eq!(fallback_tool_preview(&no_prompt), "Explore()");

        let no_description = agent_request(serde_json::json!({
            "prompt": "do the thing",
            "subagent_type": "Explore"
        }));
        assert_eq!(fallback_tool_preview(&no_description), "Explore()");

        let empty_description = agent_request(serde_json::json!({
            "description": "",
            "prompt": "do the thing",
            "subagent_type": "Explore"
        }));
        assert_eq!(fallback_tool_preview(&empty_description), "Explore()");
    }

    /// Maps to: CC `PermissionRuleExplanation.tsx:91-97` reached from
    /// `FallbackPermissionRequest.tsx:183-186`. An `ask` produced by the step-3
    /// `passthrough → ask` conversion carries no `decisionReason`, so CC renders
    /// nothing; only a real matched rule produces the sentence.
    #[test]
    fn fallback_permission_request_matches_official_rule_explanation_only_for_real_reason() {
        let passthrough_ask = agent_request(explore_input());
        assert!(passthrough_ask.decision_reason.is_none());
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(passthrough_ask),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(140))
        .to_string();
        assert!(!text.contains("Permission rule"), "canvas=\n{text}");
        assert!(
            !text.contains("/permissions to update rules"),
            "canvas=\n{text}"
        );

        let mut matched = agent_request(explore_input());
        matched.decision_reason = Some(
            crate::utils::permissions::permission_result::PermissionDecisionReason::Rule {
                rule: crate::types::permissions::PermissionRule {
                    source: crate::types::permissions::PermissionRuleSource::LocalSettings,
                    rule_behavior: crate::types::permissions::PermissionBehavior::Ask,
                    rule_value: PermissionRuleValue::new("Agent", Some("Explore".to_string())),
                },
            },
        );
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(matched),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(140))
        .to_string();
        assert!(
            text.contains("Permission rule Agent(Explore) requires confirmation for this tool."),
            "canvas=\n{text}"
        );
        assert!(
            text.contains("/permissions to update rules"),
            "canvas=\n{text}"
        );
    }

    /// Maps to: CC `services/mcp/client.ts:1971-1975` — a dynamic MCP tool's
    /// `userFacingName()` is `` `${client.name} - ${displayName} (MCP)` `` —
    /// read by `FallbackPermissionRequest.tsx:30-35`, which strips the trailing
    /// `' (MCP)'` for the label and re-appends it dim at `:173-177`.
    ///
    /// `mcp__*` tools are not in the static registry (CC creates them per
    /// connection), so before this the dialog rendered the bare
    /// `mcp__server__tool` and the strip branch was unreachable.
    #[test]
    fn fallback_user_facing_names_matches_official_mcp_tool_name() {
        let mut request = fallback_request();
        request.tool_name = "mcp__github__create_issue".to_string();
        request.mcp_info = Some(crate::types::tools::McpToolInfo {
            server_name: "github".to_string(),
            tool_name: "create_issue".to_string(),
        });

        let (user_facing_name, is_mcp) = fallback_user_facing_names(&request);
        assert_eq!(user_facing_name, "github - create_issue");
        assert!(is_mcp);
        let preview = fallback_tool_preview(&request);
        assert!(
            preview.starts_with("github - create_issue("),
            "preview={preview}"
        );
        assert!(preview.ends_with(") (MCP)"), "preview={preview}");

        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(request),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(160))
        .to_string();
        assert!(text.contains("github - create_issue"), "canvas=\n{text}");
        assert!(text.contains("(MCP)"), "canvas=\n{text}");
        assert!(
            !text.contains("mcp__github__create_issue"),
            "canvas=\n{text}"
        );
    }

    /// Maps to: CC keeping two strings with two owners —
    /// `ToolUseConfirm.description` from `await tool.description(input, …)`
    /// (`useCanUseTool.tsx:138-143`, rendered dim at
    /// `FallbackPermissionRequest.tsx:179`) and `PermissionDecision.message`
    /// (`types/permissions.ts:233`, consumed by the MODEL at
    /// `services/tools/toolExecution.ts:1023`).
    ///
    /// A deny carrying its own explanation must not overwrite what the tool
    /// said about itself; before #132 both landed on `description`.
    #[test]
    fn fallback_permission_request_matches_official_description_survives_deny_message() {
        let mut request = agent_request(explore_input());
        assert_eq!(request.description, "Launch a new agent");
        request.message =
            crate::utils::messages::build_yolo_rejection_message("spawning agents is restricted");

        assert_eq!(request.description, "Launch a new agent");
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(request),
                    show_always_allow_options: true,
                )
            }
        }
        .render(Some(140))
        .to_string();
        assert!(text.contains("Launch a new agent"), "canvas=\n{text}");
        assert!(
            !text.contains("Permission for this action has been denied"),
            "canvas=\n{text}"
        );
    }

    #[tokio::test]
    async fn fallback_permission_request_escape_rejects_like_official_cancel() {
        let selected = Arc::new(Mutex::new(Vec::new()));
        let selected_clone = selected.clone();
        let mut app = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                FallbackPermissionRequest(
                    request: Some(fallback_request()),
                    show_always_allow_options: true,
                    on_select: move |value| selected_clone.lock().unwrap().push(value),
                )
            }
        };
        let mut render_loop = Box::pin(
            app.mock_terminal_render_loop(
                MockTerminalConfig::with_events(stream::iter(vec![key(KeyCode::Esc)]))
                    .with_size(120, 30),
            ),
        );
        for _ in 0..10 {
            let next = crate::utils::race(render_loop.next(), async {
                futures_timer::Delay::new(Duration::from_millis(100)).await;
                None
            })
            .await;
            if next.is_none() {
                break;
            }
        }
        assert_eq!(
            selected.lock().unwrap().as_slice(),
            &[FallbackPermissionOptionValue::No]
        );
    }
}
