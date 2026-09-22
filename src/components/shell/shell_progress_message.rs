//! Maps to: CC `components/shell/ShellProgressMessage.tsx`.

use super::shell_time_display::{ShellTimeDisplay, shell_time_display_text};
use crate::components::message_response::MessageResponse;
use crate::components::offscreen_freeze::OffscreenFreeze;
use crate::utils::format::format_file_size;
use iocraft::prelude::*;
use regex::Regex;
use std::sync::LazyLock;

static ANSI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());

#[derive(Default, Props)]
pub struct ShellProgressMessageProps {
    pub output: String,
    pub full_output: String,
    pub elapsed_time_seconds: Option<u64>,
    pub total_lines: Option<usize>,
    pub total_bytes: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub verbose: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellProgressDisplay {
    pub display_lines: Option<String>,
    pub status_items: Vec<String>,
    pub running_only: bool,
}

fn strip_ansi(input: &str) -> String {
    ANSI_RE.replace_all(input, "").to_string()
}

/// Maps to: CC `components/shell/ShellProgressMessage.tsx` display shaping.
pub fn shell_progress_display(
    output: &str,
    full_output: &str,
    elapsed_time_seconds: Option<u64>,
    total_lines: Option<usize>,
    total_bytes: Option<u64>,
    timeout_ms: Option<u64>,
    verbose: bool,
) -> ShellProgressDisplay {
    let stripped_full_output = strip_ansi(full_output.trim());
    let stripped_output = strip_ansi(output.trim());
    let lines = stripped_output
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    let mut status_items = Vec::new();
    if lines.is_empty() {
        status_items.push("Running…".to_string());
        if let Some(time) = shell_time_display_text(elapsed_time_seconds, timeout_ms) {
            status_items.push(time);
        }
        return ShellProgressDisplay {
            display_lines: None,
            status_items,
            running_only: true,
        };
    }

    let display_lines = if verbose {
        stripped_full_output
    } else {
        lines
            .iter()
            .skip(lines.len().saturating_sub(5))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    };

    let extra_lines = total_lines.map_or(0, |count| count.saturating_sub(5));
    if !verbose {
        if total_bytes.is_some() && total_lines.is_some() {
            if let Some(total_lines) = total_lines {
                status_items.push(format!("~{total_lines} lines"));
            }
        } else if extra_lines > 0 {
            status_items.push(format!("+{extra_lines} lines"));
        }
    }
    if let Some(time) = shell_time_display_text(elapsed_time_seconds, timeout_ms) {
        status_items.push(time);
    }
    if let Some(total_bytes) = total_bytes {
        status_items.push(format_file_size(total_bytes));
    }

    ShellProgressDisplay {
        display_lines: Some(display_lines),
        status_items,
        running_only: false,
    }
}

#[component]
pub fn ShellProgressMessage(props: &ShellProgressMessageProps) -> impl Into<AnyElement<'static>> {
    let display = shell_progress_display(
        &props.output,
        &props.full_output,
        props.elapsed_time_seconds,
        props.total_lines,
        props.total_bytes,
        props.timeout_ms,
        props.verbose,
    );

    if display.running_only {
        return element! {
            MessageResponse {
                OffscreenFreeze {
                    View(flex_direction: FlexDirection::Row) {
                        Text(content: "Running… ".to_string(), dim: true, wrap: TextWrap::NoWrap)
                        ShellTimeDisplay(elapsed_time_seconds: props.elapsed_time_seconds, timeout_ms: props.timeout_ms)
                    }
                }
            }
        }
        .into_any();
    }

    let display_lines = display.display_lines.unwrap_or_default();
    let status_items = display.status_items;

    element! {
        MessageResponse {
            OffscreenFreeze {
                View(flex_direction: FlexDirection::Column) {
                    View(
                        flex_direction: FlexDirection::Column,
                        height: if props.verbose { display_lines.lines().count().max(1) as u32 } else { display_lines.lines().count().clamp(1, 5) as u32 },
                        overflow: Overflow::Hidden,
                    ) {
                        Text(content: display_lines, dim: true, wrap: TextWrap::Wrap)
                    }
                    View(flex_direction: FlexDirection::Row, column_gap: 1u32) {
                        #(status_items.into_iter().map(|item| element! {
                            Text(content: item, dim: true, wrap: TextWrap::NoWrap)
                        }).collect::<Vec<_>>())
                    }
                }
            }
        }
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::theme;

    #[test]
    fn shell_progress_display_matches_running_branch() {
        let display = shell_progress_display("", "", Some(1), None, None, None, false);
        assert!(display.running_only);
        assert_eq!(display.status_items, vec!["Running…", "(1s)"]);
    }

    #[test]
    fn shell_progress_display_matches_tail_line_and_status_branches() {
        let output = "1\n2\n3\n4\n5\n6";
        let display = shell_progress_display(output, output, Some(65), Some(8), None, None, false);
        assert_eq!(display.display_lines.as_deref(), Some("2\n3\n4\n5\n6"));
        assert_eq!(display.status_items, vec!["+3 lines", "(1m 5s)"]);

        let truncated =
            shell_progress_display(output, output, None, Some(2000), Some(1024), None, false);
        assert_eq!(truncated.status_items, vec!["~2000 lines", "1KB"]);
    }

    #[test]
    fn shell_progress_display_verbose_uses_full_output() {
        let display = shell_progress_display(
            "tail",
            "full\noutput",
            None,
            Some(10),
            Some(2048),
            None,
            true,
        );
        assert_eq!(display.display_lines.as_deref(), Some("full\noutput"));
        assert_eq!(display.status_items, vec!["2KB"]);
    }

    #[test]
    fn shell_progress_message_renders_message_response_chrome() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                ShellProgressMessage(
                    output: "one\ntwo".to_string(),
                    full_output: "one\ntwo".to_string(),
                    elapsed_time_seconds: Some(1u64),
                    verbose: false,
                )
            }
        }
        .render(Some(80))
        .to_string();

        assert!(text.contains("⎿"), "canvas=\n{text}");
        assert!(text.contains("one"), "canvas=\n{text}");
        assert!(text.contains("(1s)"), "canvas=\n{text}");
    }
}
