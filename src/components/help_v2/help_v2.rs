//! Maps to: CC `components/HelpV2/HelpV2.tsx`.

use std::collections::HashSet;
use std::sync::Arc;

use crate::commands::{Command, CommandSource};
use crate::components::design_system::pane::Pane;
use crate::components::design_system::tabs::{TabItem, TabsHeader};
use crate::components::help_v2::commands::Commands;
use crate::components::help_v2::general::General;
use crate::constants::product;
use crate::utils::theme::Theme;
use iocraft::prelude::*;

const HELP_DISPLAY_NAME: &str = "Cometix Code";
const DOCS_URL: &str = "https://code.claude.com/docs/en/overview";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelpTab {
    General,
    Commands,
    CustomCommands,
}

impl HelpTab {
    fn all() -> &'static [HelpTab] {
        &[Self::General, Self::Commands, Self::CustomCommands]
    }

    fn index(self) -> usize {
        match self {
            Self::General => 0,
            Self::Commands => 1,
            Self::CustomCommands => 2,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::General => Self::Commands,
            Self::Commands => Self::CustomCommands,
            Self::CustomCommands => Self::General,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::General => Self::CustomCommands,
            Self::Commands => Self::General,
            Self::CustomCommands => Self::Commands,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Commands => "Commands",
            Self::CustomCommands => "Custom commands",
        }
    }
}

#[derive(Default, Props)]
pub struct HelpV2Props<'a> {
    pub on_close: HandlerMut<'a, ()>,
    pub commands: Option<Arc<Vec<Command>>>,
}

/// Maps to CC `components/HelpV2/HelpV2.tsx#HelpV2`.
#[component]
pub fn HelpV2<'a>(props: &mut HelpV2Props<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let (columns, rows) = hooks.use_terminal_size();
    let max_description_chars = usize::from(columns).saturating_sub(10).max(1);
    let visible_count = usize::from(rows / 2).saturating_sub(10).max(1);
    let commands = hooks.use_const({
        let provided = props.commands.clone();
        move || {
            provided.unwrap_or_else(|| {
                #[cfg(test)]
                {
                    Arc::new(crate::commands::declared_commands_for_tests())
                }
                #[cfg(not(test))]
                {
                    Arc::new(crate::commands::get_commands(
                        &crate::bootstrap::state::get_original_cwd(),
                    ))
                }
            })
        }
    });
    let builtin_commands = Arc::new(
        commands
            .iter()
            .filter(|command| command.source == CommandSource::Builtin)
            .filter(|command| !crate::commands::is_command_hidden(command))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let custom_commands = Arc::new(
        commands
            .iter()
            .filter(|command| command.source != CommandSource::Builtin)
            .filter(|command| !crate::commands::is_command_hidden(command))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let command_count = builtin_commands
        .iter()
        .map(|command| command.name.as_ref())
        .collect::<HashSet<_>>()
        .len();
    let custom_command_count = custom_commands
        .iter()
        .map(|command| command.name.as_ref())
        .collect::<HashSet<_>>()
        .len();

    let selected_tab = hooks.use_state(|| HelpTab::General);
    let mut header_focused = hooks.use_state(|| true);
    let mut command_focused_index = hooks.use_state(|| 0usize);
    let mut should_close = hooks.use_state(|| false);

    crate::components::design_system::tabs::use_tabs_keybindings(
        &mut hooks,
        header_focused.get(),
        {
            let mut selected_tab = selected_tab;
            let mut header_focused = header_focused;
            move || {
                selected_tab.set(selected_tab.get().next());
                header_focused.set(true);
            }
        },
        {
            let mut selected_tab = selected_tab;
            let mut header_focused = header_focused;
            move || {
                selected_tab.set(selected_tab.get().previous());
                header_focused.set(true);
            }
        },
    );

    hooks.use_terminal_events(move |event| {
        let TerminalEvent::Key(KeyEvent { code, kind, .. }) = event else {
            return;
        };
        if kind == KeyEventKind::Release {
            return;
        }
        let selected = selected_tab.get();
        let active_count = match selected {
            HelpTab::Commands => command_count,
            HelpTab::CustomCommands => custom_command_count,
            HelpTab::General => 0,
        };
        let is_list_tab = matches!(selected, HelpTab::Commands | HelpTab::CustomCommands);
        let list_active = is_list_tab && active_count > 0 && !header_focused.get();
        match code {
            KeyCode::Esc => should_close.set(true),
            KeyCode::Down if is_list_tab && active_count > 0 && header_focused.get() => {
                header_focused.set(false);
                command_focused_index.set(
                    command_focused_index
                        .get()
                        .min(active_count.saturating_sub(1)),
                );
            }
            KeyCode::Down if list_active => {
                command_focused_index.set((command_focused_index.get() + 1) % active_count);
            }
            KeyCode::Up if list_active => {
                if command_focused_index.get() == 0 {
                    header_focused.set(true);
                } else {
                    command_focused_index.set(command_focused_index.get() - 1);
                }
            }
            KeyCode::PageDown if list_active => {
                command_focused_index.set(
                    command_focused_index
                        .get()
                        .saturating_add(visible_count)
                        .min(active_count - 1),
                );
            }
            KeyCode::PageUp if list_active => {
                command_focused_index
                    .set(command_focused_index.get().saturating_sub(visible_count));
            }
            _ => {}
        }
    });

    if should_close.get() {
        should_close.set(false);
        (props.on_close)(());
    }

    let selected = selected_tab.get();
    let active_count = match selected {
        HelpTab::Commands => command_count,
        HelpTab::CustomCommands => custom_command_count,
        HelpTab::General => 0,
    };
    let focused_index = command_focused_index
        .get()
        .min(active_count.saturating_sub(1));
    let visible_count_for_tab = visible_count.max(1).min(active_count.max(1));
    let visible_from = focused_index
        .saturating_add(1)
        .saturating_sub(visible_count_for_tab)
        .min(active_count.saturating_sub(visible_count_for_tab));
    let tabs = HelpTab::all()
        .iter()
        .map(|tab| TabItem::new(tab.title(), tab.title()))
        .collect::<Vec<_>>();

    element! {
        Pane(color: theme.professional_blue) {
            TabsHeader(
                title: Some(format!("{HELP_DISPLAY_NAME} v{}", product::VERSION)),
                color: Some(theme.professional_blue),
                tabs: tabs,
                selected_index: selected.index(),
                header_focused: header_focused.get(),
            )
            #(match selected {
                HelpTab::General => element! { General }.into_any(),
                HelpTab::Commands => element! {
                    Commands(
                        commands: builtin_commands.clone(),
                        max_description_chars: max_description_chars,
                        visible_count: visible_count,
                        focused_index: focused_index,
                        visible_from_index: visible_from,
                        header_focused: header_focused.get(),
                        title: "Browse default commands:".to_string(),
                    )
                }.into_any(),
                HelpTab::CustomCommands => element! {
                    Commands(
                        commands: custom_commands.clone(),
                        max_description_chars: max_description_chars,
                        visible_count: visible_count,
                        focused_index: focused_index,
                        visible_from_index: visible_from,
                        header_focused: header_focused.get(),
                        title: "Browse custom commands:".to_string(),
                        empty_message: Some("No custom commands found".to_string()),
                    )
                }.into_any(),
            })
            View(margin_top: 1u32) {
                Text(content: format!("For more help: {DOCS_URL}"))
            }
            View(margin_top: 1u32) {
                Text(content: "Esc to cancel".to_string(), dim: true, italic: true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, stream};
    use std::time::Duration;

    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }

    fn render_help_canvas_after(events: Vec<TerminalEvent>) -> Canvas {
        let theme = *crate::utils::theme::current();
        let keybindings =
            crate::keybindings::keybinding_context::KeybindingRuntime::with_default_bindings();
        futures::executor::block_on(async move {
            let mut app = element! {
                ContextProvider(value: Context::owned(keybindings)) {
                    ContextProvider(value: Context::owned(theme)) {
                        View(width: 100u32) { HelpV2(on_close: move |_| {}) }
                    }
                }
            };
            let mut render_loop = Box::pin(app.mock_terminal_render_loop(
                MockTerminalConfig::with_events(stream::iter(events)).with_size(100, 32),
            ));
            let mut last = None;
            for _ in 0..8 {
                let next = crate::utils::race(render_loop.next(), async {
                    futures_timer::Delay::new(Duration::from_millis(100)).await;
                    None
                })
                .await;
                let Some(canvas) = next else { break };
                last = Some(canvas);
            }
            last.expect("mock render should produce a canvas")
        })
    }

    #[test]
    fn help_v2_uses_official_tab_shape() {
        let text = render_help_canvas_after(Vec::new()).to_string();
        assert!(text.contains("Cometix Code v"), "canvas=\n{text}");
        assert!(
            text.contains("General   Commands   Custom commands"),
            "canvas=\n{text}"
        );
        assert!(text.contains("Shortcuts"), "canvas=\n{text}");
        assert!(text.contains("For more help:"), "canvas=\n{text}");
    }

    #[test]
    fn help_v2_commands_tab_lists_default_commands() {
        let text = render_help_canvas_after(vec![key(KeyCode::Tab)]).to_string();
        assert!(text.contains("Browse default commands:"), "canvas=\n{text}");
        assert!(text.contains("/add-dir"), "canvas=\n{text}");
        assert!(
            text.contains("Add a new working directory"),
            "canvas=\n{text}"
        );
    }

    #[test]
    fn help_v2_command_list_accepts_focus_from_header() {
        let text =
            render_help_canvas_after(vec![key(KeyCode::Tab), key(KeyCode::Down)]).to_string();
        assert!(text.contains("❯ /add-dir"), "canvas=\n{text}");
    }
}
