//! Maps to: CC `components/sandbox/SandboxSettings.tsx`.
//!
//! The official component owns the `/sandbox` local-jsx panel, tabs, mode
//! selection, override selection, and completion strings. Cometix dispatches
//! settings writes through `utils/sandbox/sandbox_adapter.rs`; no sandbox
//! runtime initialization or command wrapping happens in this UI component.

use super::sandbox_config_tab::SandboxConfigTab;
use super::sandbox_dependencies_tab::SandboxDependenciesTab;
use super::sandbox_overrides_tab::SandboxOverridesTab;
use crate::components::custom_select::{Select, SelectLayout, SelectOptionData};
use crate::components::design_system::pane::Pane;
use crate::components::design_system::tabs::{TabItem, TabsHeader};
use crate::utils::sandbox::sandbox_adapter::{
    SandboxDependencyCheck, SandboxSettingsUpdate, are_sandbox_settings_locked_by_policy,
    are_unsandboxed_commands_allowed, check_dependencies_readonly, get_sandbox_enabled_setting,
    is_auto_allow_bash_if_sandboxed_enabled, set_sandbox_settings,
};
use crate::utils::settings::get_initial_settings;
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxMode {
    AutoAllow,
    Regular,
    Disabled,
}

impl SandboxMode {
    fn value(self) -> &'static str {
        match self {
            Self::AutoAllow => "auto-allow",
            Self::Regular => "regular",
            Self::Disabled => "disabled",
        }
    }
}

pub fn current_sandbox_mode(enabled: bool, auto_allow: bool) -> SandboxMode {
    if !enabled {
        SandboxMode::Disabled
    } else if auto_allow {
        SandboxMode::AutoAllow
    } else {
        SandboxMode::Regular
    }
}

pub fn sandbox_mode_options(current_mode: SandboxMode) -> Vec<SelectOptionData> {
    [
        (SandboxMode::AutoAllow, "Sandbox BashTool, with auto-allow"),
        (
            SandboxMode::Regular,
            "Sandbox BashTool, with regular permissions",
        ),
        (SandboxMode::Disabled, "No Sandbox"),
    ]
    .into_iter()
    .map(|(mode, label)| SelectOptionData {
        label: if mode == current_mode {
            format!("{label} (current)")
        } else {
            label.to_string()
        },
        description: None,
        dim_description: true,
        value: mode.value().to_string(),
        disabled: false,
        input: None,
    })
    .collect()
}

pub fn sandbox_mode_completion_and_update(
    mode: SandboxMode,
) -> (&'static str, SandboxSettingsUpdate) {
    match mode {
        SandboxMode::AutoAllow => (
            "✓ Sandbox enabled with auto-allow for bash commands",
            SandboxSettingsUpdate {
                enabled: Some(true),
                auto_allow_bash_if_sandboxed: Some(true),
                allow_unsandboxed_commands: None,
            },
        ),
        SandboxMode::Regular => (
            "✓ Sandbox enabled with regular bash permissions",
            SandboxSettingsUpdate {
                enabled: Some(true),
                auto_allow_bash_if_sandboxed: Some(false),
                allow_unsandboxed_commands: None,
            },
        ),
        SandboxMode::Disabled => (
            "○ Sandbox disabled",
            SandboxSettingsUpdate {
                enabled: Some(false),
                auto_allow_bash_if_sandboxed: Some(false),
                allow_unsandboxed_commands: None,
            },
        ),
    }
}

fn tabs_for_dep_check(dep_check: &SandboxDependencyCheck) -> Vec<TabItem> {
    if !dep_check.errors.is_empty() {
        vec![TabItem::new("dependencies", "Dependencies")]
    } else {
        let mut tabs = vec![TabItem::new("mode", "Mode")];
        if !dep_check.warnings.is_empty() {
            tabs.push(TabItem::new("dependencies", "Dependencies"));
        }
        tabs.push(TabItem::new("overrides", "Overrides"));
        tabs.push(TabItem::new("config", "Config"));
        tabs
    }
}

#[derive(Default, Props)]
pub struct SandboxSettingsProps<'a> {
    pub dep_check: Option<SandboxDependencyCheck>,
    pub on_complete: HandlerMut<'a, Option<String>>,
}

#[component]
pub fn SandboxSettings<'a>(
    props: &mut SandboxSettingsProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<crate::utils::theme::Theme>();
    let settings = get_initial_settings();
    let dep_check = props
        .dep_check
        .clone()
        .unwrap_or_else(check_dependencies_readonly);
    let tabs = tabs_for_dep_check(&dep_check);
    let tab_index = hooks.use_state(|| 0usize);
    let mode_focus = hooks.use_state(|| 0usize);
    let override_focus = hooks.use_state(|| 0usize);
    let mut pending_result = hooks.use_state(|| Option::<Option<String>>::None);

    let current_enabled = get_sandbox_enabled_setting(&settings);
    let current_auto_allow = is_auto_allow_bash_if_sandboxed_enabled(&settings);
    let current_mode = current_sandbox_mode(current_enabled, current_auto_allow);
    let allow_unsandboxed = are_unsandboxed_commands_allowed(&settings);
    let is_locked = are_sandbox_settings_locked_by_policy();
    let selected_index = tab_index.get().min(tabs.len().saturating_sub(1));
    let selected_id = tabs
        .get(selected_index)
        .map(|tab| tab.id.clone())
        .unwrap_or_else(|| "mode".to_string());
    crate::components::design_system::tabs::use_tabs_keybindings(
        &mut hooks,
        !tabs.is_empty(),
        {
            let mut tab_index = tab_index;
            let tab_count = tabs.len();
            move || tab_index.set((tab_index.get() + 1) % tab_count)
        },
        {
            let mut tab_index = tab_index;
            let tab_count = tabs.len();
            move || {
                let index = tab_index.get();
                tab_index.set(if index == 0 { tab_count - 1 } else { index - 1 });
            }
        },
    );

    let show_socket_warning = !dep_check.warnings.is_empty()
        && !settings
            .sandbox
            .as_ref()
            .and_then(|sandbox| sandbox.pointer("/network/allowAllUnixSockets"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

    hooks.use_terminal_events({
        let selected_id = selected_id.clone();
        let mut mode_focus = mode_focus;
        let mut override_focus = override_focus;
        let mut pending_result = pending_result;
        let current_enabled = current_enabled;
        move |event| {
            let TerminalEvent::Key(KeyEvent { code, kind, .. }) = event else {
                return;
            };
            if kind == KeyEventKind::Release {
                return;
            }
            match code {
                KeyCode::Esc => pending_result.set(Some(None)),
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected_id == "mode" {
                        mode_focus.set(mode_focus.get().saturating_sub(1));
                    } else if selected_id == "overrides" && current_enabled {
                        override_focus.set(override_focus.get().saturating_sub(1));
                    }
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    if selected_id == "mode" {
                        mode_focus.set((mode_focus.get() + 1).min(2));
                    } else if selected_id == "overrides" && current_enabled {
                        override_focus.set((override_focus.get() + 1).min(1));
                    }
                }
                KeyCode::Enter => {
                    if selected_id == "mode" {
                        let mode = match mode_focus.get().min(2) {
                            0 => SandboxMode::AutoAllow,
                            1 => SandboxMode::Regular,
                            _ => SandboxMode::Disabled,
                        };
                        let (message, update) = sandbox_mode_completion_and_update(mode);
                        let result = set_sandbox_settings(update)
                            .map(|_| message.to_string())
                            .unwrap_or_else(|error| format!("Error writing settings: {error}"));
                        pending_result.set(Some(Some(result)));
                    } else if selected_id == "overrides" && current_enabled {
                        let allow = override_focus.get().min(1) == 0;
                        let result = set_sandbox_settings(SandboxSettingsUpdate {
                            enabled: None,
                            auto_allow_bash_if_sandboxed: None,
                            allow_unsandboxed_commands: Some(allow),
                        })
                        .map(|_| {
                            if allow {
                                "✓ Unsandboxed fallback allowed - commands can run outside sandbox when necessary".to_string()
                            } else {
                                "✓ Strict sandbox mode - all commands must run in sandbox or be excluded via the `excludedCommands` option".to_string()
                            }
                        })
                        .unwrap_or_else(|error| format!("Error writing settings: {error}"));
                        pending_result.set(Some(Some(result)));
                    }
                }
                _ => {}
            }
        }
    });

    let result = { pending_result.read().clone() };
    if let Some(result) = result {
        pending_result.set(None);
        (props.on_complete)(result);
    }

    let body = match selected_id.as_str() {
        "dependencies" => element! {
            SandboxDependenciesTab(dep_check: dep_check.clone())
        }
        .into_any(),
        "overrides" => element! {
            SandboxOverridesTab(
                is_enabled: current_enabled,
                is_locked: is_locked,
                current_allow_unsandboxed: allow_unsandboxed,
                focused_index: override_focus.get().min(1),
            )
        }
        .into_any(),
        "config" => element! {
            SandboxConfigTab(settings: settings.clone(), dep_check: dep_check.clone())
        }
        .into_any(),
        _ => element! {
            View(flex_direction: FlexDirection::Column, padding_top: 1u32, padding_bottom: 1u32) {
                #(if show_socket_warning { Some(element! { Text(content: "Cannot block unix domain sockets (see Dependencies tab)".to_string(), color: theme.warning, wrap: TextWrap::Wrap) }) } else { None })
                View(margin_bottom: 1u32) {
                    Text(content: "Configure Mode:".to_string(), weight: Weight::Bold, wrap: TextWrap::NoWrap)
                }
                Select(
                    options: sandbox_mode_options(current_mode),
                    focused_index: mode_focus.get().min(2),
                    visible_option_count: 3usize,
                    layout: SelectLayout::Compact,
                    hide_indexes: true,
                )
                View(flex_direction: FlexDirection::Column, margin_top: 1u32) {
                    Text(content: "Auto-allow mode: Commands will try to run in the sandbox automatically, and attempts to run outside of the sandbox fallback to regular permissions. Explicit ask/deny rules are always respected.".to_string(), color: theme.inactive, wrap: TextWrap::Wrap)
                    Text(content: "Learn more: https://code.claude.com/docs/en/sandboxing".to_string(), color: theme.inactive, wrap: TextWrap::Wrap)
                }
            }
        }
        .into_any(),
    };

    element! {
        Pane(color: Some(theme.permission)) {
            TabsHeader(
                title: Some("Sandbox:".to_string()),
                color: Some(theme.permission),
                tabs: tabs,
                selected_index: selected_index,
                header_focused: false,
            )
            #(body)
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

    #[test]
    fn sandbox_mode_options_and_completion_match_official_copy() {
        let options = sandbox_mode_options(SandboxMode::AutoAllow);
        assert_eq!(
            options[0].label,
            "Sandbox BashTool, with auto-allow (current)"
        );
        assert_eq!(
            options[1].label,
            "Sandbox BashTool, with regular permissions"
        );
        assert_eq!(options[2].label, "No Sandbox");
        assert_eq!(
            sandbox_mode_completion_and_update(SandboxMode::Regular).0,
            "✓ Sandbox enabled with regular bash permissions"
        );
    }

    #[test]
    fn sandbox_settings_renders_mode_tabs_and_copy() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                SandboxSettings(dep_check: Some(SandboxDependencyCheck::default()))
            }
        }
        .render(Some(120))
        .to_string();
        assert!(text.contains("Sandbox:"), "canvas=\n{text}");
        assert!(text.contains("Mode"), "canvas=\n{text}");
        assert!(text.contains("Overrides"), "canvas=\n{text}");
        assert!(text.contains("Config"), "canvas=\n{text}");
        assert!(text.contains("Configure Mode:"), "canvas=\n{text}");
        assert!(
            text.contains("Sandbox BashTool, with auto-allow"),
            "canvas=\n{text}"
        );
        assert!(
            text.contains("code.claude.com/docs/en/sandboxing"),
            "canvas=\n{text}"
        );
    }

    #[tokio::test]
    async fn sandbox_settings_escape_completes_with_skip_none() {
        let results = Arc::new(Mutex::new(Vec::new()));
        let results_clone = Arc::clone(&results);
        let mut app = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                SandboxSettings(
                    dep_check: Some(SandboxDependencyCheck::default()),
                    on_complete: move |result| {
                        results_clone.lock().unwrap().push(result);
                    },
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
        assert_eq!(results.lock().unwrap().clone(), vec![None]);
    }
}
