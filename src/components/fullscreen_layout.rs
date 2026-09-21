//! Maps to: CC `components/FullscreenLayout.tsx`.
//!
//! This module owns the official fullscreen transcript shell boundary:
//! transcript-derived unseen-divider helpers, the sticky prompt/new-message
//! chrome, and the retained ScrollBox layout used when fullscreen mode is
//! enabled. REPL wiring is intentionally incremental; protocol/runtime logic
//! stays out of this UI-only component.

use crate::components::scroll_keybinding_handler::ScrollKeybindingHandler;
use crate::constants::figures::figures;
use crate::types::message::{RenderableMessage, RenderableMessageKind};
use crate::utils::theme::{self, Theme, ThemeName};
use iocraft::prelude::*;

/// Rows of transcript context kept visible above the modal pane's divider.
///
/// Maps to: CC `components/FullscreenLayout.tsx#MODAL_TRANSCRIPT_PEEK`.
pub const MODAL_TRANSCRIPT_PEEK: usize = 2;

/// Maps to: CC `components/FullscreenLayout.tsx#UnseenDivider`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnseenDivider {
    pub first_unseen_id: String,
    pub count: usize,
}

/// Snapshot of the sticky prompt header chrome.
///
/// Maps to: CC `components/FullscreenLayout.tsx#StickyPromptHeader` props via
/// `VirtualMessageList.tsx#StickyPrompt`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StickyPromptSnapshot {
    pub text: String,
}

/// Props for the retained fullscreen REPL layout shell.
///
/// Maps to: CC `components/FullscreenLayout.tsx#Props`.
#[derive(Default, Props)]
pub struct FullscreenLayoutProps {
    /// Content that scrolls (messages, tool output).
    pub scrollable: Vec<AnyElement<'static>>,
    /// Content pinned to the bottom (spinner, prompt, permissions).
    pub bottom: Vec<AnyElement<'static>>,
    /// Content rendered inside the ScrollBox after messages.
    pub overlay: Vec<AnyElement<'static>>,
    /// Absolute-positioned bottom-right content floating over scrollback.
    pub bottom_float: Vec<AnyElement<'static>>,
    /// Slash-command dialog content rendered in a bottom-anchored modal pane.
    pub modal: Vec<AnyElement<'static>>,
    /// Imperative scroll handle equivalent to CC `scrollRef`.
    pub scroll_handle: Option<Ref<ScrollBoxHandle>>,
    /// Reserved for the official `ModalContext.scrollRef` handoff.
    pub modal_scroll_handle: Option<Ref<ScrollBoxHandle>>,
    /// Divider y-position (`dividerYRef.current`) used by the new-message pill.
    pub divider_y: Option<i32>,
    pub hide_pill: bool,
    pub hide_sticky: bool,
    pub sticky_prompt: Option<StickyPromptSnapshot>,
    pub new_message_count: usize,
    /// Enables the official action-driven transcript scroll handler.
    pub scroll_keybindings_active: Option<bool>,
    /// Enables less/tmux-style raw pager keys in transcript modal mode.
    pub scroll_keybindings_modal: Option<bool>,
    pub on_scroll: Handler<bool>,
    pub on_pill_click: HandlerMut<'static, ()>,
    pub on_sticky_prompt_click: HandlerMut<'static, ()>,
    /// Test seam / staged wiring override. `None` reads the official env gate.
    pub fullscreen: Option<bool>,
}

/// Counts assistant turns in `messages[divider_index..]` using the same user
/// visible turn model as the official fullscreen new-message pill.
///
/// Maps to: CC `components/FullscreenLayout.tsx#countUnseenAssistantTurns`.
pub fn count_unseen_assistant_turns(messages: &[RenderableMessage], divider_index: usize) -> usize {
    let mut count = 0usize;
    let mut prev_was_assistant = false;

    for message in messages.iter().skip(divider_index) {
        if is_progress_message(message) {
            continue;
        }
        if matches!(message.kind, RenderableMessageKind::Assistant { .. })
            && !assistant_has_visible_text(message)
        {
            continue;
        }

        let is_assistant = matches!(message.kind, RenderableMessageKind::Assistant { .. });
        if is_assistant && !prev_was_assistant {
            count += 1;
        }
        prev_was_assistant = is_assistant;
    }

    count
}

/// Builds the unseen-divider anchor and floored count for the fullscreen pill.
///
/// Maps to: CC `components/FullscreenLayout.tsx#computeUnseenDivider`.
pub fn compute_unseen_divider(
    messages: &[RenderableMessage],
    divider_index: Option<usize>,
) -> Option<UnseenDivider> {
    let divider_index = divider_index?;
    let mut anchor_index = divider_index;
    while anchor_index < messages.len() && is_fullscreen_hidden_anchor(&messages[anchor_index]) {
        anchor_index += 1;
    }
    let first_unseen_id = messages.get(anchor_index)?.uuid.clone();
    let count = count_unseen_assistant_turns(messages, divider_index).max(1);

    Some(UnseenDivider {
        first_unseen_id,
        count,
    })
}

/// Boolean snapshot used by the fullscreen pill subscription.
///
/// Maps to: CC `FullscreenLayout.tsx` `useSyncExternalStore` snapshot:
/// `scrollTop + pendingDelta + viewportHeight < dividerYRef.current`.
pub fn should_show_new_messages_pill(
    scroll_top: i32,
    pending_delta: i32,
    viewport_height: i32,
    divider_y: Option<i32>,
) -> bool {
    let Some(divider_y) = divider_y else {
        return false;
    };
    scroll_top
        .saturating_add(pending_delta)
        .saturating_add(viewport_height)
        < divider_y
}

/// Official fullscreen layout shell.
///
/// Maps to: CC `components/FullscreenLayout.tsx#FullscreenLayout`.
#[component]
pub fn FullscreenLayout(
    props: &mut FullscreenLayoutProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let (columns, rows) = hooks.use_terminal_size();
    let fullscreen = props
        .fullscreen
        .unwrap_or_else(crate::utils::fullscreen::is_fullscreen_env_enabled);
    let has_overlay = !props.overlay.is_empty();
    let has_modal = !props.modal.is_empty();
    let has_bottom_float = !props.bottom_float.is_empty();
    let local_scroll_handle = hooks.use_ref_default::<ScrollBoxHandle>();
    let scroll_handle = props.scroll_handle.unwrap_or(local_scroll_handle);
    let sticky_prompt = if props.hide_sticky || has_overlay {
        None
    } else {
        props.sticky_prompt.clone()
    };
    let pad_collapsed = sticky_prompt.is_some() && !has_overlay;
    let pill_visible = if props.hide_pill || has_overlay {
        false
    } else {
        let handle = scroll_handle.read();
        should_show_new_messages_pill(
            handle.get_scroll_top(),
            handle.get_pending_delta(),
            i32::from(handle.get_viewport_height()),
            props.divider_y,
        )
    };
    let bottom_max_height = (rows / 2).max(1);
    let modal_max_height = rows.saturating_sub(MODAL_TRANSCRIPT_PEEK as u16).max(1);
    let modal_context = crate::context::modal_context::ModalContextSnapshot {
        rows: rows
            .saturating_sub(MODAL_TRANSCRIPT_PEEK as u16)
            .saturating_sub(1),
        columns: columns.saturating_sub(4),
    };
    let permission_color = themed_color(&hooks, |theme| theme.permission);
    let modal_scroll_context =
        crate::context::modal_context::ModalScrollRefContext(props.modal_scroll_handle);
    let on_sticky_prompt_click = props.on_sticky_prompt_click.take();
    let on_pill_click = props.on_pill_click.take();

    if !fullscreen {
        return element! {
            View(flex_direction: FlexDirection::Column, width: 100pct) {
                #(props.scrollable.drain(..))
                #(props.bottom.drain(..))
                #(props.overlay.drain(..))
                #(props.modal.drain(..))
            }
        }
        .into_any();
    }

    element! {
        View(
            flex_grow: 1.0f32,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::Hidden,
            width: 100pct,
            height: 100pct,
        ) {
            View(flex_grow: 1.0f32, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden, width: 100pct) {
                #(sticky_prompt.map(|sticky| element! {
                    StickyPromptHeader(
                        text: sticky.text,
                        on_click: on_sticky_prompt_click,
                    )
                }.into_any()))

                ScrollKeybindingHandler(
                    is_active: props.scroll_keybindings_active.unwrap_or(true),
                    is_modal: props.scroll_keybindings_modal.unwrap_or(false),
                    scroll_handle: Some(scroll_handle),
                    on_scroll: props.on_scroll.clone(),
                )

                ScrollBox(
                    handle: Some(scroll_handle),
                    sticky_scroll: true,
                    wheel_acceleration: Some(true),
                    scroll_drain_mode: Some(ScrollDrainMode::for_current_terminal()),
                    keyboard_scroll: Some(true),
                ) {
                    View(
                        flex_direction: FlexDirection::Column,
                        padding_top: if pad_collapsed { 0u32 } else { 1u32 },
                        width: 100pct,
                    ) {
                        #(props.scrollable.drain(..))
                        #(props.overlay.drain(..))
                    }
                }

                #(if pill_visible {
                    Some(element! {
                        NewMessagesPill(
                            count: props.new_message_count,
                            on_click: on_pill_click,
                        )
                    }.into_any())
                } else { None })

                #(if has_bottom_float {
                    Some(element! {
                        View(
                            position: Position::Absolute,
                            bottom: 0,
                            right: 0,
                            opaque: true,
                        ) {
                            #(props.bottom_float.drain(..))
                        }
                    }.into_any())
                } else { None })
            }

            View(
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0f32,
                width: 100pct,
                max_height: bottom_max_height,
                overflow: Overflow::Hidden,
            ) {
                #(props.bottom.drain(..))
            }

            #(if has_modal {
                Some(element! {
                    View(
                        position: Position::Absolute,
                        bottom: 0,
                        left: 0,
                        right: 0,
                        max_height: modal_max_height,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::Hidden,
                        opaque: true,
                    ) {
                        View(flex_shrink: 0.0f32, width: 100pct) {
                            Text(color: permission_color, content: "▔".repeat(columns as usize), wrap: TextWrap::NoWrap)
                        }
                        ContextProvider(value: Context::owned(modal_context)) {
                            ContextProvider(value: Context::owned(modal_scroll_context)) {
                            View(
                                flex_direction: FlexDirection::Column,
                                padding_left: 2u32,
                                padding_right: 2u32,
                                flex_shrink: 0.0f32,
                                overflow: Overflow::Hidden,
                            ) {
                                #(props.modal.drain(..))
                            }
                            }
                        }
                    }
                }.into_any())
            } else { None })
        }
    }
    .into_any()
}

/// Slack-style pill floated over the ScrollBox tail.
///
/// Maps to: CC `components/FullscreenLayout.tsx#NewMessagesPill`.
#[derive(Default, Props)]
pub struct NewMessagesPillProps {
    pub count: usize,
    pub on_click: HandlerMut<'static, ()>,
}

#[component]
pub fn NewMessagesPill(
    props: &mut NewMessagesPillProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = current_theme(&hooks);
    let mut on_click = props.on_click.take();
    let label = if props.count > 0 {
        format!(
            "{} new {}",
            props.count,
            if props.count == 1 {
                "message"
            } else {
                "messages"
            }
        )
    } else {
        "Jump to bottom".to_string()
    };
    let content = format!(" {label} {} ", figures().arrow_down);

    element! {
        View(
            position: Position::Absolute,
            bottom: 0,
            left: 0,
            right: 0,
            justify_content: JustifyContent::CENTER,
        ) {
            View(on_click: move |_| (on_click)(())) {
                Text(
                    background_color: theme.user_message_bg,
                    dim: true,
                    content: content,
                    wrap: TextWrap::NoWrap,
                )
            }
        }
    }
}

/// Context breadcrumb pinned above the viewport while scrolled into history.
///
/// Maps to: CC `components/FullscreenLayout.tsx#StickyPromptHeader`.
#[derive(Default, Props)]
pub struct StickyPromptHeaderProps {
    pub text: String,
    pub on_click: HandlerMut<'static, ()>,
}

#[component]
pub fn StickyPromptHeader(
    props: &mut StickyPromptHeaderProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = current_theme(&hooks);
    let mut on_click = props.on_click.take();
    let content = format!("{} {}", figures().pointer, props.text);
    element! {
        View(
            flex_shrink: 0.0f32,
            width: 100pct,
            height: 1u32,
            padding_right: 1u32,
            background_color: theme.user_message_bg,
            on_click: move |_| (on_click)(()),
        ) {
            Text(
                color: theme.subtle,
                wrap: TextWrap::TruncateEnd,
                content: content,
            )
        }
    }
}

fn current_theme(hooks: &Hooks<'_, '_>) -> Theme {
    hooks
        .try_use_context::<Theme>()
        .map(|theme| *theme)
        .unwrap_or(*theme::get_theme(ThemeName::Dark))
}

fn themed_color(hooks: &Hooks<'_, '_>, pick: impl FnOnce(Theme) -> Color) -> Color {
    pick(current_theme(hooks))
}

fn assistant_has_visible_text(message: &RenderableMessage) -> bool {
    match &message.kind {
        RenderableMessageKind::Assistant { message } => {
            let is_api_error = message.content.iter().any(|block| {
                matches!(
                    block,
                    crate::types::message::AssistantContent::MessageIdentity(identity)
                        if identity.is_api_error_message
                )
            });
            !is_api_error
                && matches!(
                    message.first_content_block(),
                    Some(crate::types::message::AssistantContent::Text(text))
                        if !text.trim().is_empty()
                )
        }
        _ => false,
    }
}

fn is_progress_message(_message: &RenderableMessage) -> bool {
    // `types/message.ts::ProgressMessage` has not yet been added to the Rust
    // RenderableMessage union. Do not substitute Cometix-only fixture rows.
    false
}

fn is_fullscreen_hidden_anchor(_message: &RenderableMessage) -> bool {
    // No currently modeled RenderableMessage variant is null-rendering here.
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn assistant(id: &str, text: &str) -> RenderableMessage {
        RenderableMessage::assistant_block(
            id,
            crate::types::message::AssistantContent::Text(text.to_string()),
        )
    }

    fn tool_use(id: &str) -> RenderableMessage {
        RenderableMessage::assistant_block(
            id,
            crate::types::message::AssistantContent::ToolUse(crate::types::message::ToolUseBlock {
                id: crate::types::ids::ToolUseId(id.to_string()),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "README.md"}),
            }),
        )
    }

    fn user(id: &str, text: &str) -> RenderableMessage {
        RenderableMessage::user(id, text)
    }

    #[test]
    fn count_unseen_assistant_turns_skips_progress_and_tool_use_only_entries() {
        let messages = vec![
            user("u1", "question"),
            tool_use("t1"),
            assistant("a1", "answer"),
            assistant("a2", "same turn continuation"),
            user("u2", "next"),
            tool_use("t2"),
            assistant("a3", "second answer"),
        ];

        assert_eq!(count_unseen_assistant_turns(&messages, 1), 2);
        assert_eq!(count_unseen_assistant_turns(&messages, 2), 2);
        assert_eq!(count_unseen_assistant_turns(&messages, 5), 1);
    }

    #[test]
    fn compute_unseen_divider_skips_hidden_anchor_and_floors_count() {
        let messages = vec![
            user("u1", "question"),
            tool_use("t1"),
            user("u2", "tool result"),
        ];

        assert_eq!(
            compute_unseen_divider(&messages, Some(1)),
            Some(UnseenDivider {
                first_unseen_id: "t1".to_string(),
                count: 1,
            })
        );
        assert_eq!(compute_unseen_divider(&messages, None), None);
    }

    #[test]
    fn new_message_pill_visibility_matches_official_boolean_snapshot() {
        assert!(should_show_new_messages_pill(10, 0, 5, Some(16)));
        assert!(!should_show_new_messages_pill(10, 1, 5, Some(16)));
        assert!(!should_show_new_messages_pill(10, 0, 5, Some(15)));
        assert!(!should_show_new_messages_pill(10, 0, 5, None));
    }

    #[component]
    fn FullscreenLayoutMainScreenHarness(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let handle = hooks.use_ref_default::<ScrollBoxHandle>();
        element! {
            ContextProvider(value: Context::owned(*theme::get_theme(ThemeName::Dark))) {
                FullscreenLayout(
                    fullscreen: Some(false),
                    scroll_handle: Some(handle),
                    hide_pill: false,
                    sticky_prompt: Some(StickyPromptSnapshot { text: "previous question".to_string() }),
                    new_message_count: 3usize,
                    scrollable: vec![element!(Text(content: "transcript row")).into_any()],
                    bottom: vec![element!(Text(content: "prompt row")).into_any()],
                    overlay: vec![element!(Text(content: "overlay row")).into_any()],
                    modal: vec![element!(Text(content: "modal row")).into_any()],
                )
            }
        }
    }

    #[component]
    fn FullscreenLayoutHarness(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let handle = hooks.use_ref_default::<ScrollBoxHandle>();
        element! {
            ContextProvider(value: Context::owned(*theme::get_theme(ThemeName::Dark))) {
                FullscreenLayout(
                    fullscreen: Some(true),
                    scroll_handle: Some(handle),
                    hide_pill: true,
                    sticky_prompt: Some(StickyPromptSnapshot { text: "previous question".to_string() }),
                    scrollable: vec![element!(Text(content: "transcript row")).into_any()],
                    bottom: vec![element!(Text(content: "prompt row")).into_any()],
                )
            }
        }
    }

    #[component]
    fn FullscreenLayoutModalHarness(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let handle = hooks.use_ref_default::<ScrollBoxHandle>();
        element! {
            ContextProvider(value: Context::owned(*theme::get_theme(ThemeName::Dark))) {
                FullscreenLayout(
                    fullscreen: Some(true),
                    scroll_handle: Some(handle),
                    hide_pill: true,
                    scrollable: vec![element!(Text(content: "transcript row")).into_any()],
                    bottom: vec![element!(Text(content: "prompt row")).into_any()],
                    modal: vec![element!(Text(content: "modal row")).into_any()],
                )
            }
        }
    }

    #[test]
    fn fullscreen_layout_main_screen_branch_keeps_document_flow_without_fullscreen_chrome() {
        let output = futures::executor::block_on(
            element!(FullscreenLayoutMainScreenHarness)
                .mock_terminal_render_loop(MockTerminalConfig::default().with_size(40, 8))
                .take(1)
                .collect::<Vec<_>>(),
        )
        .last()
        .unwrap()
        .to_string();

        let transcript = output.find("transcript row").expect(&output);
        let prompt = output.find("prompt row").expect(&output);
        let overlay = output.find("overlay row").expect(&output);
        let modal = output.find("modal row").expect(&output);

        assert!(
            transcript < prompt && prompt < overlay && overlay < modal,
            "{output}"
        );
        assert!(!output.contains("previous question"), "{output}");
        assert!(!output.contains("new messages"), "{output}");
        assert!(!output.contains('▔'), "{output}");
    }

    #[test]
    fn fullscreen_layout_shell_renders_scroll_bottom_and_sticky_slots() {
        let output = futures::executor::block_on(
            element!(FullscreenLayoutHarness)
                .mock_terminal_render_loop(MockTerminalConfig::default().with_size(40, 8))
                .take(1)
                .collect::<Vec<_>>(),
        )
        .last()
        .unwrap()
        .to_string();

        assert!(output.contains("previous question"), "{output}");
        assert!(output.contains("transcript row"), "{output}");
        assert!(output.contains("prompt row"), "{output}");
    }

    #[test]
    fn fullscreen_layout_modal_slot_bottom_anchors_over_bottom_slot_like_official() {
        let output = futures::executor::block_on(
            element!(FullscreenLayoutModalHarness)
                .mock_terminal_render_loop(MockTerminalConfig::default().with_size(40, 8))
                .take(1)
                .collect::<Vec<_>>(),
        )
        .last()
        .unwrap()
        .to_string();

        assert!(output.contains("modal row"), "{output}");
        assert!(output.contains('▔'), "{output}");
    }
}
