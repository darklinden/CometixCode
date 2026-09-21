//! Maps to: CC `components/tasks/InProcessTeammateDetailDialog.tsx:1-193`.

use super::task_status_utils::TaskStatus;
use crate::components::design_system::dialog::Dialog;
use crate::keybindings::keybinding_context::KeybindingRuntime;
use crate::keybindings::types::ContextName;
use crate::keybindings::use_keybinding::use_keybinding;
use crate::utils::theme::{Theme, ThemeColorKey};
use crate::utils::truncate::truncate_to_width;
use iocraft::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeammateDetailData {
    pub agent_name: String,
    pub color: ThemeColorKey,
    pub activity: String,
    pub prompt: String,
    pub status: TaskStatus,
    pub elapsed: String,
    pub token_count: Option<u64>,
    pub tool_use_count: Option<usize>,
    pub recent_activities: Vec<String>,
    pub error: Option<String>,
}
#[derive(Clone, Copy)]
enum Action {
    Done,
    Back,
    Stop,
    Foreground,
}
#[derive(Default, Props)]
pub struct InProcessTeammateDetailDialogProps<'a> {
    pub teammate: Option<TeammateDetailData>,
    pub on_done: HandlerMut<'a, ()>,
    pub on_kill: HandlerMut<'a, ()>,
    pub on_back: HandlerMut<'a, ()>,
    pub on_foreground: HandlerMut<'a, ()>,
}

#[component]
pub fn InProcessTeammateDetailDialog<'a>(
    props: &mut InProcessTeammateDetailDialogProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let Some(teammate) = props.teammate.clone() else {
        return element! { Fragment }.into_any();
    };
    let mut pending = hooks.use_state(|| None::<Action>);
    let action = { *pending.read() };
    if let Some(action) = action {
        pending.set(None);
        match action {
            Action::Done => (props.on_done)(()),
            Action::Back if !props.on_back.is_default() => (props.on_back)(()),
            Action::Stop
                if teammate.status == TaskStatus::Running && !props.on_kill.is_default() =>
            {
                (props.on_kill)(())
            }
            Action::Foreground
                if teammate.status == TaskStatus::Running && !props.on_foreground.is_default() =>
            {
                (props.on_foreground)(())
            }
            _ => {}
        }
    }
    let runtime = hooks
        .try_use_context::<KeybindingRuntime>()
        .map(|runtime| runtime.clone());
    for (name, context, action, active) in [
        ("confirm:yes", ContextName::Confirmation, Action::Done, true),
        ("task:close", ContextName::Task, Action::Done, true),
        (
            "task:back",
            ContextName::Task,
            Action::Back,
            !props.on_back.is_default(),
        ),
        (
            "task:stop",
            ContextName::Task,
            Action::Stop,
            teammate.status == TaskStatus::Running && !props.on_kill.is_default(),
        ),
        (
            "task:foreground",
            ContextName::Task,
            Action::Foreground,
            teammate.status == TaskStatus::Running && !props.on_foreground.is_default(),
        ),
    ] {
        let mut pending = pending;
        use_keybinding(
            &mut hooks,
            runtime.clone(),
            name,
            context,
            move || active,
            move || {
                pending.set(Some(action));
                true
            },
        );
    }
    let theme = hooks.use_context::<Theme>();
    let _terminal = teammate.status != TaskStatus::Running;
    let terminal_label = match teammate.status {
        TaskStatus::Completed => Some(("Completed", theme.success)),
        TaskStatus::Failed => Some(("Failed", theme.error)),
        TaskStatus::Killed => Some(("Stopped", theme.warning)),
        _ => None,
    };
    let subtitle = format!(
        "{}{}{}{}",
        terminal_label
            .map(|(label, _)| format!("{label} · "))
            .unwrap_or_default(),
        teammate.elapsed,
        teammate
            .token_count
            .filter(|count| *count > 0)
            .map(|count| format!(" · {} tokens", crate::utils::format::format_number(count)))
            .unwrap_or_default(),
        teammate
            .tool_use_count
            .filter(|count| *count > 0)
            .map(|count| format!(" · {count} {}", if count == 1 { "tool" } else { "tools" }))
            .unwrap_or_default()
    );
    let activities = teammate.recent_activities.iter().enumerate().map(|(index, activity)| { let last = index + 1 == teammate.recent_activities.len(); element! { Text(content: format!("{} {activity}", if last { "›" } else { " " }), dim: !last, wrap: TextWrap::Truncate) } }).collect::<Vec<_>>();
    let guide = format!(
        "{}Esc/Enter/Space to close{}{}",
        if props.on_back.is_default() {
            ""
        } else {
            "← to go back · "
        },
        if teammate.status == TaskStatus::Running && !props.on_kill.is_default() {
            " · x to stop"
        } else {
            ""
        },
        if teammate.status == TaskStatus::Running && !props.on_foreground.is_default() {
            " · f to foreground"
        } else {
            ""
        }
    );
    let mut done = pending;
    element! { Dialog(
        title_children: vec![element! { MixedText(contents: vec![MixedTextContent::new(format!("@{}", teammate.agent_name)).color(theme.color(teammate.color)), MixedTextContent::new(format!(" ({})", teammate.activity)).weight(Weight::Light)]) }.into_any()],
        subtitle: Some(subtitle), color: Some(theme.background), input_guide: Some(guide), on_cancel: move |_| done.set(Some(Action::Done)),
    ) {
        View(flex_direction: FlexDirection::Column) {
            #((teammate.status == TaskStatus::Running && !activities.is_empty()).then(|| element! { View(flex_direction: FlexDirection::Column) { Text(content: "Progress".to_string(), weight: Weight::Bold, dim: true) #(activities) } }))
            View(flex_direction: FlexDirection::Column, margin_top: 1u32) { Text(content: "Prompt".to_string(), weight: Weight::Bold, dim: true) Text(content: truncate_to_width(&teammate.prompt, 300)) }
            #((teammate.status == TaskStatus::Failed).then(|| teammate.error.clone()).flatten().map(|error| element! { View(flex_direction: FlexDirection::Column, margin_top: 1u32) { Text(content: "Error".to_string(), weight: Weight::Bold, color: theme.error) Text(content: error, color: theme.error) } }))
        }
    }}.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_activity_progress_prompt_and_error_render() {
        let data = TeammateDetailData {
            agent_name: "reviewer".to_string(),
            color: ThemeColorKey::AgentBlue,
            activity: "working".to_string(),
            prompt: "review this".to_string(),
            status: TaskStatus::Failed,
            elapsed: "3s".to_string(),
            token_count: Some(1200),
            tool_use_count: Some(2),
            recent_activities: vec![],
            error: Some("boom".to_string()),
        };
        let text = element! { ContextProvider(value: Context::owned(*crate::utils::theme::current())) { InProcessTeammateDetailDialog(teammate: Some(data)) } }.render(Some(100)).to_string();
        assert!(text.contains("@reviewer (working)"));
        assert!(text.contains("Failed · 3s · 1.2k tokens · 2 tools"));
        assert!(text.contains("Prompt"));
        assert!(text.contains("boom"));
    }
}
