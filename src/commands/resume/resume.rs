//! Maps to: CC `commands/resume/resume.tsx`.
//! `ResumeCommand` view. Typed session resolution/loading lives in the parent
//! `commands::resume` module; selector UI is `components/log_selector.rs`,
//! matching the official `ResumeCommand → LogSelector` split.

use crate::components::log_selector::LogSelector;
use crate::components::spinner::SpinnerGlyph;
use crate::utils::cross_project_resume::{CrossProjectResumeResult, check_cross_project_resume};
use crate::utils::get_worktree_paths::get_worktree_paths;
use crate::utils::session_storage::{
    SessionSelection, SessionSummary, load_all_projects_message_logs, load_same_repo_message_logs,
};
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct ResumeCommandProps<'a> {
    /// L1 source process.stdout carrier retained outside this command panel.
    pub command_stdout: Option<StdoutHandle>,
    pub current_session_id: Option<String>,
    pub on_close: HandlerMut<'a, ()>,
    pub on_select: HandlerMut<'a, SessionSelection>,
    pub on_result: Handler<String>,
}

const NO_CONVERSATIONS_NOTICE: &str = "No conversations found to resume";

/// Maps to: CC `commands/resume/resume.tsx::filterResumableSessions`.
/// Team sessions remain resumable; only sidechains and the current session are
/// excluded here.
pub(super) fn filter_resumable_sessions(
    logs: Vec<SessionSummary>,
    current_session_id: Option<&str>,
) -> Vec<SessionSummary> {
    logs.into_iter()
        .filter(|log| !log.is_sidechain)
        .filter(|log| current_session_id != Some(log.session_id.as_str()))
        .collect()
}

fn cross_project_message(command: &str) -> String {
    [
        "",
        "This conversation is from a different directory.",
        "",
        "To resume, run:",
        &format!("  {command}"),
        "",
        "(Command copied to clipboard)",
        "",
    ]
    .join("\n")
}

fn picker_empty_notice(log_count: usize) -> Option<&'static str> {
    (log_count == 0).then_some(NO_CONVERSATIONS_NOTICE)
}

fn resume_progress_message(is_resuming: bool) -> &'static str {
    if is_resuming {
        "Resuming conversation…"
    } else {
        "Loading conversations…"
    }
}

/// Maps to: CC `commands/resume/resume.tsx::ResumeCommand`.
/// Prepares logs, renders `LogSelector`, and hands the selected log back to REPL.
#[component]
pub fn ResumeCommand<'a>(
    props: &mut ResumeCommandProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let (local_stdout, _) = hooks.use_output();
    let stdout = props.command_stdout.clone().unwrap_or(local_stdout);
    let initial_project_path = crate::bootstrap::state::get_original_cwd()
        .display()
        .to_string();
    let project_path = hooks.use_state({
        let initial_project_path = initial_project_path.clone();
        move || initial_project_path
    });
    let sessions = hooks.use_state(Vec::new);
    let worktree_paths = hooks.use_state(Vec::new);
    let mut show_all_projects = hooks.use_state(|| false);
    let mut is_loading = hooks.use_state(|| true);
    let mut is_resuming = hooks.use_state(|| false);
    let mut initial_load_sent = hooks.use_state(|| false);
    let mut pending_close = hooks.use_state(|| false);
    let mut pending_select = hooks.use_state(|| Option::<SessionSelection>::None);
    let mut pending_result = hooks.use_state(|| Option::<String>::None);
    // Maps to: CC resume.tsx:154-174 handleSelect's awaited cross-project branch.
    // The owned callback survives child unmount like the source async function.
    let on_result = props.on_result.clone();
    let handle_select_cross_project = Handler::from(move |command: String| {
        let stdout = stdout.clone();
        let on_result = on_result.clone();
        let copy = stdout.prepare_clipboard(&command);
        crate::utils::process_runtime::runtime_handle_for_detached_work()
            .expect("clipboard requires the initialized process runtime")
            .spawn(async move {
                let raw = copy.await;
                if !raw.is_empty() {
                    if let Err(error) = stdout.write_control_sequence_and_wait(raw).await {
                        crate::utils::debug::log_for_debugging(&error.to_string());
                        return;
                    }
                }
                on_result(cross_project_message(&command));
            });
    });
    let load_channel = hooks.use_const(|| std::sync::Arc::new(async_channel::unbounded::<bool>()));
    let load_tx_initial = load_channel.0.clone();
    let load_tx_toggle = load_channel.0.clone();
    let load_tx_reload = load_channel.0.clone();
    let load_rx = load_channel.1.clone();
    let current_session_id = props.current_session_id.clone();

    hooks.use_future({
        let project_path = project_path;
        let mut sessions = sessions;
        let mut worktree_paths = worktree_paths;
        let mut is_loading = is_loading;
        let mut pending_result = pending_result;
        async move {
            while let Ok(show_all_projects) = load_rx.recv().await {
                is_loading.set(true);
                let project_path = project_path.read().clone();
                let paths = get_worktree_paths(&project_path);
                let all_logs = if show_all_projects {
                    load_all_projects_message_logs()
                } else {
                    load_same_repo_message_logs(&paths)
                };
                let resumable = filter_resumable_sessions(all_logs, current_session_id.as_deref());
                if let Some(result) = picker_empty_notice(resumable.len()) {
                    pending_result.set(Some(result.to_string()));
                }
                sessions.set(resumable);
                worktree_paths.set(paths);
                is_loading.set(false);
            }
        }
    });
    // Keep every hook above retained state effects and
    // loading/resuming/selector branches; iocraft uses React's stable order.
    let (_, rows) = hooks.use_terminal_size();

    if !initial_load_sent.get() {
        initial_load_sent.set(true);
        let _ = load_tx_initial.try_send(show_all_projects.get());
    }
    // Maps to CC commands/resume/resume.tsx + main-screen local command UI behavior.
    // Official LogSelector renders as a main-screen overlay and fills the
    // visible terminal region while preserving native scrollback.
    // Our LogSelector subtracts one from max_height for its root height, so pass
    // rows + 1 to make the retained inline canvas exactly viewport-tall. Drawing
    // a viewport-tall frame pushes any pre-existing shell/prompt rows into normal
    // terminal scrollback, which is what makes mouse-wheel history scrolling work.
    let max_height = rows.saturating_add(1).max(1) as usize;

    if pending_close.get() {
        pending_close.set(false);
        (props.on_close)(());
    }
    let result = { pending_result.read().clone() };
    if let Some(message) = result {
        pending_result.set(None);
        (props.on_result)(message);
    }
    let selected = { pending_select.read().clone() };
    if let Some(selection) = selected {
        pending_select.set(None);
        match check_cross_project_resume(
            &selection,
            show_all_projects.get(),
            &worktree_paths.read(),
        ) {
            CrossProjectResumeResult::SameProject
            | CrossProjectResumeResult::SameRepoWorktree { .. } => {
                is_resuming.set(true);
                (props.on_select)(selection);
            }
            CrossProjectResumeResult::DifferentProject { command, .. } => {
                handle_select_cross_project(command);
            }
        }
    }

    if is_loading.get() || is_resuming.get() {
        let message = resume_progress_message(is_resuming.get()).to_string();
        return element! {
            View(flex_direction: FlexDirection::Row) {
                SpinnerGlyph(frame: 0usize)
                Text(content: format!(" {message}"), wrap: TextWrap::NoWrap)
            }
        }
        .into_any();
    }

    element! {
        LogSelector(
            logs: sessions.read().clone(),
            max_height: max_height,
            show_all_projects: show_all_projects.get(),
            on_cancel: move |_| {
                pending_close.set(true);
            },
            on_select: move |selection: SessionSelection| {
                pending_select.set(Some(selection));
            },
            on_logs_changed: move |_| {
                is_loading.set(true);
                let _ = load_tx_reload.try_send(show_all_projects.get());
            },
            on_toggle_all_projects: move |_| {
                let next = !show_all_projects.get();
                show_all_projects.set(next);
                is_loading.set(true);
                let _ = load_tx_toggle.try_send(next);
            },
        )
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_empty_notice_matches_official_resume_command_copy() {
        assert_eq!(picker_empty_notice(0), Some(NO_CONVERSATIONS_NOTICE));
        assert_eq!(NO_CONVERSATIONS_NOTICE, "No conversations found to resume");
        assert_eq!(picker_empty_notice(1), None);
    }

    #[test]
    fn cross_project_message_matches_official_clipboard_copy() {
        let message = cross_project_message("cd /tmp/project && claude --resume abc");

        assert_eq!(
            message,
            "\nThis conversation is from a different directory.\n\nTo resume, run:\n  cd /tmp/project && claude --resume abc\n\n(Command copied to clipboard)\n"
        );
    }

    #[test]
    fn resume_progress_messages_match_official_resume_command_copy() {
        assert_eq!(resume_progress_message(false), "Loading conversations…");
        assert_eq!(resume_progress_message(true), "Resuming conversation…");
    }
}
