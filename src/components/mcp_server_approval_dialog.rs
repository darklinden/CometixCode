//! Maps to: CC `components/MCPServerApprovalDialog.tsx`.
//!
//! Safety boundary: the official dialog logs analytics and mutates local
//! settings (`enabledMcpjsonServers`, `disabledMcpjsonServers`,
//! `enableAllProjectMcpServers`). Cometix preserves the official copy,
//! options, cancel-as-reject behavior, and exposes the selected choice via a
//! callback. Settings writes and analytics are deferred to the MCP settings
//! runtime slice.

use crate::components::custom_select::{Select, SelectLayout, SelectOptionData};
use crate::components::design_system::dialog::Dialog;
use crate::components::mcp_server_dialog_copy::MCPServerDialogCopy;
use crate::utils::theme::Theme;
use iocraft::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MCPServerApprovalChoice {
    YesAll,
    Yes,
    No,
}

impl MCPServerApprovalChoice {
    pub fn value(&self) -> &'static str {
        match self {
            Self::YesAll => "yes_all",
            Self::Yes => "yes",
            Self::No => "no",
        }
    }
}

pub fn mcp_server_approval_choice_from_value(value: &str) -> MCPServerApprovalChoice {
    match value {
        "yes_all" => MCPServerApprovalChoice::YesAll,
        "yes" => MCPServerApprovalChoice::Yes,
        _ => MCPServerApprovalChoice::No,
    }
}

/// Maps to: CC `MCPServerApprovalDialog.tsx` `Select` options.
pub fn mcp_server_approval_options() -> Vec<SelectOptionData> {
    vec![
        SelectOptionData {
            label: "Use this and all future MCP servers in this project".to_string(),
            value: MCPServerApprovalChoice::YesAll.value().to_string(),
            ..SelectOptionData::default()
        },
        SelectOptionData {
            label: "Use this MCP server".to_string(),
            value: MCPServerApprovalChoice::Yes.value().to_string(),
            ..SelectOptionData::default()
        },
        SelectOptionData {
            label: "Continue without using this MCP server".to_string(),
            value: MCPServerApprovalChoice::No.value().to_string(),
            ..SelectOptionData::default()
        },
    ]
}

#[derive(Default, Props)]
pub struct MCPServerApprovalDialogProps<'a> {
    pub server_name: String,
    pub on_choice: HandlerMut<'a, MCPServerApprovalChoice>,
    pub on_done: HandlerMut<'a, ()>,
}

/// Maps to: CC `components/MCPServerApprovalDialog.tsx`.
#[component]
pub fn MCPServerApprovalDialog<'a>(
    props: &mut MCPServerApprovalDialogProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let focused_index = hooks.use_state(|| 0usize);
    let mut pending_choice = hooks.use_state(|| Option::<MCPServerApprovalChoice>::None);
    let options = mcp_server_approval_options();
    let option_count = options.len().max(1);

    hooks.use_terminal_events({
        let mut focused_index = focused_index;
        let mut pending_choice = pending_choice;
        let options = options.clone();
        move |event| {
            let TerminalEvent::Key(KeyEvent { code, kind, .. }) = event else {
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
                        pending_choice
                            .set(Some(mcp_server_approval_choice_from_value(&option.value)));
                    }
                }
                _ => {}
            }
        }
    });

    let choice = {
        let pending = pending_choice.read();
        pending.clone()
    };
    if let Some(choice) = choice {
        pending_choice.set(None);
        (props.on_choice)(choice);
        (props.on_done)(());
    }

    element! {
        Dialog(
            title: format!("New MCP server found in .mcp.json: {}", props.server_name),
            color: Some(theme.warning),
            on_cancel: move |_| pending_choice.set(Some(MCPServerApprovalChoice::No)),
        ) {
            MCPServerDialogCopy()
            Select(
                options: options,
                focused_index: focused_index.get().min(option_count - 1),
                visible_option_count: option_count,
                layout: SelectLayout::CompactVertical,
                hide_indexes: true,
            )
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
    fn mcp_server_approval_options_match_official_values() {
        let options = mcp_server_approval_options();
        assert_eq!(options[0].value, "yes_all");
        assert_eq!(options[1].value, "yes");
        assert_eq!(options[2].value, "no");
        assert_eq!(
            mcp_server_approval_choice_from_value("yes_all"),
            MCPServerApprovalChoice::YesAll
        );
        assert_eq!(
            mcp_server_approval_choice_from_value("missing"),
            MCPServerApprovalChoice::No
        );
    }

    #[test]
    fn mcp_server_approval_dialog_renders_official_copy_and_options() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                MCPServerApprovalDialog(server_name: "filesystem".to_string())
            }
        }
        .render(Some(120))
        .to_string();

        assert!(
            text.contains("New MCP server found in .mcp.json: filesystem"),
            "canvas=\n{text}"
        );
        assert!(
            text.contains("MCP servers may execute code"),
            "canvas=\n{text}"
        );
        assert!(
            text.contains("Use this and all future MCP servers in this project"),
            "canvas=\n{text}"
        );
        assert!(
            text.contains("Continue without using this MCP server"),
            "canvas=\n{text}"
        );
    }

    #[test]
    fn mcp_server_approval_enter_emits_choice_and_done_callback_only() {
        let choices = Arc::new(Mutex::new(Vec::<MCPServerApprovalChoice>::new()));
        let done = Arc::new(Mutex::new(0usize));
        let choices_for_handler = Arc::clone(&choices);
        let done_for_handler = Arc::clone(&done);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(*theme::current())) {
                    MCPServerApprovalDialog(
                        server_name: "filesystem".to_string(),
                        on_choice: move |choice| choices_for_handler.lock().expect("choices mutex").push(choice),
                        on_done: move |_| *done_for_handler.lock().expect("done mutex") += 1,
                    )
                }
            };
            let mut render_loop = Box::pin(
                app.mock_terminal_render_loop(
                    MockTerminalConfig::with_events(stream::iter(vec![
                        key(KeyCode::Down),
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
            choices.lock().expect("choices mutex").as_slice(),
            &[MCPServerApprovalChoice::Yes]
        );
        assert_eq!(*done.lock().expect("done mutex"), 1);
    }

    #[test]
    fn mcp_server_approval_escape_rejects_like_official_cancel() {
        let choices = Arc::new(Mutex::new(Vec::<MCPServerApprovalChoice>::new()));
        let choices_for_handler = Arc::clone(&choices);

        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(
                    crate::keybindings::keybinding_context::KeybindingRuntime::with_default_bindings()
                )) {
                    ContextProvider(value: Context::owned(*theme::current())) {
                        MCPServerApprovalDialog(
                            server_name: "filesystem".to_string(),
                            on_choice: move |choice| choices_for_handler.lock().expect("choices mutex").push(choice),
                        )
                    }
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

        assert_eq!(
            choices.lock().expect("choices mutex").as_slice(),
            &[MCPServerApprovalChoice::No]
        );
    }
}
