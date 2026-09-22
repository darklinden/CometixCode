//! Maps to: CC `components/tasks/ShellDetailDialog.tsx:1-247`.

use super::task_status_utils::TaskStatus;
use crate::components::design_system::dialog::Dialog;
use crate::keybindings::keybinding_context::KeybindingRuntime;
use crate::keybindings::types::ContextName;
use crate::keybindings::use_keybinding::use_keybinding;
use crate::utils::format::format_file_size;
use crate::utils::theme::Theme;
use crate::utils::truncate::truncate_to_width;
use futures::{FutureExt, StreamExt};
use iocraft::prelude::*;

pub const SHELL_DETAIL_TAIL_BYTES: u64 = 8192;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskOutputResult {
    pub content: String,
    pub bytes_total: u64,
}

/// Maps to CC `ShellDetailDialog.tsx#getTaskOutput`.
async fn get_task_output(task_id: String) -> TaskOutputResult {
    #[cfg(test)]
    {
        let reader = tests::OUTPUT_READERS.lock().unwrap().get(&task_id).cloned();
        if let Some(reader) = reader {
            let (sender, receiver) = futures::channel::oneshot::channel();
            let _ = reader.send(sender).await;
            return receiver.await.unwrap_or_default();
        }
    }
    crate::utils::fs_operations::tail_file(
        &crate::utils::task::disk_output::get_task_output_path(&task_id),
        SHELL_DETAIL_TAIL_BYTES as usize,
    )
    .await
    .map(|result| TaskOutputResult {
        content: result.content,
        bytes_total: result.bytes_total,
    })
    .unwrap_or_default()
}

pub fn shell_output_lines(output: &TaskOutputResult) -> (Vec<String>, bool) {
    if output.content.is_empty() {
        return (Vec::new(), false);
    }
    let mut starts = Vec::new();
    let mut pos = output.content.len();
    for _ in 0..10 {
        if pos == 0 {
            break;
        }
        let previous = output.content[..pos].rfind('\n');
        starts.push(previous.map(|index| index + 1).unwrap_or(0));
        pos = previous.unwrap_or(0);
    }
    starts.reverse();
    starts.dedup();
    let mut lines = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(output.content.len());
        let line = &output.content[*start..end];
        if !line.is_empty() {
            lines.push(line.to_string());
        }
    }
    // CC ShellOutputContent: string.length counts UTF-16 code units.
    let incomplete = output.bytes_total > output.content.encode_utf16().count() as u64;
    (lines, incomplete)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellDetailData {
    pub id: String,
    pub is_monitor: bool,
    pub status: TaskStatus,
    pub exit_code: Option<i32>,
    pub runtime: String,
    pub command: String,
    pub output: Option<TaskOutputResult>,
}
#[derive(Clone, Copy)]
enum Action {
    Done,
    Back,
    Stop,
}
#[derive(Default, Props)]
pub struct ShellDetailDialogProps<'a> {
    pub shell: Option<ShellDetailData>,
    pub on_done: HandlerMut<'a, String>,
    pub on_kill_shell: HandlerMut<'a, ()>,
    pub on_back: HandlerMut<'a, ()>,
}

#[component]
pub fn ShellDetailDialog<'a>(
    props: &mut ShellDetailDialogProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let Some(shell) = props.shell.clone() else {
        return element! { Fragment }.into_any();
    };
    let initial_output = shell.output.clone();
    let fixture_output = initial_output.is_some();
    let mut output = hooks.use_state(move || initial_output);
    let poll_id = shell.id.clone();
    let mut status = hooks.use_state(|| shell.status);
    let status_changes =
        hooks.use_const(|| std::sync::Arc::new(async_channel::unbounded::<TaskStatus>()));
    if status.get() != shell.status {
        status.set(shell.status);
        let _ = status_changes.0.try_send(shell.status);
    }
    let initially_running = shell.status == TaskStatus::Running;
    let status_rx = status_changes.1.clone();
    // Maps to CC :67-93: the interval starts reads independently of completion.
    // A generation carries outputPromise identity; older promises must not
    // overwrite the current one. Pending replacements retain displayed output.
    hooks.use_future(async move {
        if fixture_output {
            return;
        }
        let mut reads = futures::stream::FuturesUnordered::new();
        let mut generation = 0u64;
        let mut deferred_generation = 0u64;
        let initial_id = poll_id.clone();
        reads.push(async move { (0u64, get_task_output(initial_id).await) }.boxed());
        let mut timer = if initially_running {
            futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse()
        } else { futures::future::Fuse::terminated() };
        loop {
            // Native payloads carry source promise/interval/effect changes.
            enum Ready { Output(u64, TaskOutputResult), Tick, Status(TaskStatus), Closed }
            let ready = {
                let next_read = async {
                    if reads.is_empty() {
                        futures::future::pending().await
                    } else {
                        reads.next().await
                    }
                }
                .fuse();
                futures::pin_mut!(next_read);
                futures::select! {
                    result = next_read => match result { Some((id, value)) => Ready::Output(id, value), None => Ready::Closed },
                    _ = timer => Ready::Tick,
                    next_status = status_rx.recv().fuse() => match next_status { Ok(status) => Ready::Status(status), Err(_) => Ready::Closed },
                }
            };
            match ready {
                Ready::Output(completed, value) => {
                    if completed == generation || completed == deferred_generation {
                        // Initial deferred promise can settle while a newer
                        // background render still suspends. Once the new
                        // promise commits, the earlier one cannot roll it back.
                        deferred_generation = completed;
                        output.set(Some(value));
                    }
                },
                Ready::Tick => {
                    generation = generation.wrapping_add(1);
                    let request_generation = generation;
                    let id = poll_id.clone();
                    reads.push(async move { (request_generation, get_task_output(id).await) }.boxed());
                    timer = futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse();
                },
                Ready::Status(status) => {
                    timer = if status == TaskStatus::Running {
                        futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse()
                    } else { futures::future::Fuse::terminated() };
                },
                Ready::Closed => break,
            }
        }
    });
    let mut pending = hooks.use_state(|| None::<Action>);
    let action = { *pending.read() };
    if let Some(action) = action {
        pending.set(None);
        match action {
            Action::Done => (props.on_done)("Shell details dismissed".to_string()),
            Action::Back if !props.on_back.is_default() => (props.on_back)(()),
            Action::Stop
                if shell.status == TaskStatus::Running && !props.on_kill_shell.is_default() =>
            {
                (props.on_kill_shell)(())
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
            shell.status == TaskStatus::Running && !props.on_kill_shell.is_default(),
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
    let output_ready = output.read().is_some();
    let output_value = output.read().clone().unwrap_or_default();
    let (lines, incomplete) = shell_output_lines(&output_value);
    let rows = lines
        .iter()
        .map(|line| element! { Text(content: line.clone(), wrap: TextWrap::Truncate) })
        .collect::<Vec<_>>();
    let output_block: AnyElement<'static> = if !output_ready {
        element! { Text(content: "Loading output…", dim: true) }.into_any()
    } else if output_value.content.is_empty() {
        element! { Text(content: "No output available".to_string(), dim: true) }.into_any()
    } else {
        element! { View(border_style: BorderStyle::Round, padding_left: 1u32, padding_right: 1u32, flex_direction: FlexDirection::Column, height: 12u32) { #(rows) } }.into_any()
    };
    let status_color = match shell.status {
        TaskStatus::Running | TaskStatus::Pending => theme.background,
        TaskStatus::Completed => theme.success,
        TaskStatus::Failed | TaskStatus::Killed => theme.error,
    };
    let status = format!(
        "{}{}",
        match shell.status {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Killed => "killed",
        },
        shell
            .exit_code
            .map(|code| format!(" (exit code: {code})"))
            .unwrap_or_default()
    );
    let guide = format!(
        "{}Esc/Enter/Space to close{}",
        if props.on_back.is_default() {
            ""
        } else {
            "← to go back · "
        },
        if shell.status == TaskStatus::Running && !props.on_kill_shell.is_default() {
            " · x to stop"
        } else {
            ""
        }
    );
    let mut done = pending;
    element! { Dialog(title: if shell.is_monitor { "Monitor details".to_string() } else { "Shell details".to_string() }, color: Some(theme.background), input_guide: Some(guide), on_cancel: move |_| done.set(Some(Action::Done))) {
        View(flex_direction: FlexDirection::Column) {
            MixedText(contents: vec![MixedTextContent::new("Status: ").weight(Weight::Bold), MixedTextContent::new(status).color(status_color)])
            MixedText(contents: vec![MixedTextContent::new("Runtime: ").weight(Weight::Bold), MixedTextContent::new(shell.runtime)])
            MixedText(contents: vec![MixedTextContent::new(if shell.is_monitor { "Script: " } else { "Command: " }).weight(Weight::Bold), MixedTextContent::new(truncate_to_width(&shell.command, 280))])
            Text(content: "Output:".to_string(), weight: Weight::Bold)
            #(vec![output_block])
            #((!output_value.content.is_empty()).then(|| element! { Text(content: format!("Showing {} lines{}", lines.len(), if incomplete { format!(" of {}", format_file_size(output_value.bytes_total)) } else { String::new() }), dim: true, italic: true) }))
        }
    }}.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    type ControlledRead = futures::channel::oneshot::Sender<TaskOutputResult>;
    pub(super) static OUTPUT_READERS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, async_channel::Sender<ControlledRead>>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    async fn await_read<S>(
        frames: &mut S,
        requests: &async_channel::Receiver<ControlledRead>,
    ) -> ControlledRead
    where
        S: futures::Stream<Item = Canvas> + Unpin,
    {
        crate::utils::race(
            async {
                loop {
                    futures::select! {
                        request = requests.recv().fuse() => return request.unwrap(),
                        _ = frames.next().fuse() => {},
                    }
                }
            },
            async {
                futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
                panic!("source interval did not start a read");
            },
        )
        .await
    }

    async fn await_output<S>(frames: &mut S, needle: &str) -> String
    where
        S: futures::Stream<Item = Canvas> + Unpin,
    {
        crate::utils::race(
            async {
                loop {
                    let text = frames.next().await.unwrap().to_string();
                    if text.contains(needle) {
                        return text;
                    }
                }
            },
            async {
                futures_timer::Delay::new(std::time::Duration::from_secs(3)).await;
                panic!("deferred output never displayed {needle}");
            },
        )
        .await
    }

    #[test]
    fn shell_deferred_output_matches_official_slow_initial_and_no_stale_rollback() {
        // CC :71-77 useDeferredValue retains the previous promise while the
        // replacement suspends, including the initial unresolved promise.
        futures::executor::block_on(async {
            for latest_first in [false, true] {
                let id = format!("controlled-output-{}", uuid::Uuid::new_v4());
                let (tx, requests) = async_channel::unbounded();
                OUTPUT_READERS.lock().unwrap().insert(id.clone(), tx);
                let mut app = element! { ContextProvider(value: Context::owned(*crate::utils::theme::current())) {
                    ShellDetailDialog(shell: Some(ShellDetailData { id: id.clone(), status: TaskStatus::Running, ..Default::default() }))
                }};
                let mut frames = Box::pin(app.mock_terminal_render_loop(
                    MockTerminalConfig::with_events(futures::stream::pending()).with_size(100, 20),
                ));
                let first = await_read(&mut frames, &requests).await;
                let second = await_read(&mut frames, &requests).await;
                let value = |text: &str| TaskOutputResult {
                    content: text.into(),
                    bytes_total: text.len() as u64,
                };
                if latest_first {
                    second.send(value("latest-output")).unwrap();
                    let mut last = await_output(&mut frames, "latest-output").await;
                    first.send(value("stale-output")).unwrap();
                    let quiet =
                        futures_timer::Delay::new(std::time::Duration::from_millis(100)).fuse();
                    futures::pin_mut!(quiet);
                    loop {
                        futures::select! {
                            _ = quiet => break,
                            frame = frames.next().fuse() => if let Some(frame) = frame { last = frame.to_string(); },
                        }
                    }
                    assert!(last.contains("latest-output"), "{last}");
                    assert!(!last.contains("stale-output"), "{last}");
                } else {
                    first.send(value("initial-output")).unwrap();
                    let initial = await_output(&mut frames, "initial-output").await;
                    assert!(!initial.contains("Loading output…"));
                    second.send(value("latest-output")).unwrap();
                    await_output(&mut frames, "latest-output").await;
                }
                drop(frames);
                OUTPUT_READERS.lock().unwrap().remove(&id);
            }
        });
    }
    #[test]
    fn output_keeps_last_ten_nonempty_lines_and_reports_incomplete_bytes() {
        let content = (0..12)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, incomplete) = shell_output_lines(&TaskOutputResult {
            bytes_total: content.len() as u64 + 20,
            content,
        });
        assert_eq!(lines.len(), 10);
        assert_eq!(lines.first().unwrap(), "line 2");
        assert_eq!(lines.last().unwrap(), "line 11");
        assert!(incomplete);
    }
    #[test]
    fn shell_initial_output_matches_official_suspense_loading_and_resolved_empty() {
        let shell = ShellDetailData {
            id: "initial-output".into(),
            status: TaskStatus::Running,
            ..Default::default()
        };
        let pending =
            element! { ContextProvider(value: Context::owned(*crate::utils::theme::current())) {
                ShellDetailDialog(shell: Some(shell.clone()))
            }}
            .render(Some(100))
            .to_string();
        assert!(pending.contains("Loading output…"), "{pending}");
        assert!(!pending.contains("No output available"), "{pending}");
        let ready = element! { ContextProvider(value: Context::owned(*crate::utils::theme::current())) {
            ShellDetailDialog(shell: Some(ShellDetailData { output: Some(TaskOutputResult::default()), ..shell }))
        }}.render(Some(100)).to_string();
        assert!(ready.contains("No output available"), "{ready}");
        assert!(!ready.contains("Loading output…"), "{ready}");
    }
    #[test]
    fn asynchronous_tail_read_returns_latest_output_and_missing_file_fallback() {
        futures::executor::block_on(async {
            let id = format!("shell-tail-{}", uuid::Uuid::new_v4());
            assert_eq!(
                get_task_output(id.clone()).await,
                TaskOutputResult::default()
            );
            let path = crate::utils::task::disk_output::get_task_output_path(&id);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let first = "a".repeat(SHELL_DETAIL_TAIL_BYTES as usize + 10);
            std::fs::write(&path, &first).unwrap();
            let output = get_task_output(id.clone()).await;
            assert_eq!(output.bytes_total, first.len() as u64);
            assert_eq!(output.content.len(), SHELL_DETAIL_TAIL_BYTES as usize);
            std::fs::write(&path, "second output").unwrap();
            assert_eq!(get_task_output(id).await.content, "second output");
            std::fs::remove_file(path).unwrap();
        });
    }
    #[test]
    fn shell_output_matches_official_utf16_incomplete_check() {
        // CC components/tasks/ShellDetailDialog.tsx ShellOutputContent:
        // bytesTotal > content.length uses JavaScript UTF-16 string length.
        let output = TaskOutputResult {
            content: "中文".into(),
            bytes_total: 6,
        };
        assert!(shell_output_lines(&output).1);
        assert!(
            !shell_output_lines(&TaskOutputResult {
                content: "ok".into(),
                bytes_total: 2
            })
            .1
        );
    }
}
