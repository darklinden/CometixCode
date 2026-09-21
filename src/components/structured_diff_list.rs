//! Maps to: CC `components/StructuredDiffList.tsx`.
//!
//! Official `StructuredDiffList` maps each hunk to `StructuredDiff` and
//! intersperses dim `...` separators. The retained component preserves those
//! child boundaries and their per-segment styles.

use crate::components::messages::user_tool_result_message::utils::ToolRenderLine;
use crate::components::structured_diff::{self, StructuredDiff, color_diff::SyntaxHighlightTheme};
use crate::types::message::StructuredDiffHunk;
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct StructuredDiffListProps {
    pub hunks: Vec<StructuredDiffHunk>,
    pub dim: bool,
    pub width: usize,
    pub file_path: String,
    pub first_line: Option<String>,
    pub file_content: Option<String>,
    /// Mirrors `StructuredDiff.skipHighlighting`/settings suppression as an
    /// explicit pure prop; official `StructuredDiffList` itself has no logic.
    pub skip_highlighting: bool,
}

/// Maps to: CC `StructuredDiffList` render mapping.
pub fn structured_diff_list_lines(
    hunks: &[StructuredDiffHunk],
    dim: bool,
    width: usize,
    file_path: &str,
    first_line: Option<&str>,
    file_content: Option<&str>,
    skip_highlighting: bool,
    syntax_theme: SyntaxHighlightTheme,
) -> Vec<ToolRenderLine> {
    if skip_highlighting || file_path.is_empty() {
        return structured_diff::render_hunks(hunks, dim, width.max(1));
    }
    structured_diff::render_hunks_with_syntax(
        hunks,
        dim,
        width.max(1),
        structured_diff::SyntaxHighlightOptions {
            file_path: Some(file_path.to_string()),
            first_line: first_line.map(ToString::to_string),
            theme: syntax_theme,
            prefix_content: file_content.map(ToString::to_string),
        },
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn lines_to_text(lines: &[ToolRenderLine]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Maps to: CC `components/StructuredDiffList.tsx#StructuredDiffList`.
#[component]
pub fn StructuredDiffList(
    props: &StructuredDiffListProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let syntax_highlighting_disabled =
        crate::state::app_state::use_app_state(&mut hooks, |state| {
            state.settings.syntax_highlighting_disabled.unwrap_or(false)
        });
    let skip_highlighting = props.skip_highlighting || syntax_highlighting_disabled;
    let children = props
        .hunks
        .iter()
        .enumerate()
        .flat_map(|(index, hunk)| {
            let mut children = Vec::<AnyElement<'static>>::new();
            if index > 0 {
                children.push(
                    element! { Text(content: "...".to_string(), dim: true, wrap: TextWrap::NoWrap) }
                        .into_any(),
                );
            }
            children.push(
                element! {
                    StructuredDiff(
                        patch: hunk.clone(),
                        dim: props.dim,
                        width: props.width,
                        file_path: props.file_path.clone(),
                        first_line: props.first_line.clone(),
                        file_content: props.file_content.clone(),
                        skip_highlighting: skip_highlighting,
                    )
                }
                .into_any(),
            );
            children
        })
        .collect::<Vec<_>>();

    element! {
        View(flex_direction: FlexDirection::Column) { #(children) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::theme;

    fn hunk(new_start: usize, lines: &[&str]) -> StructuredDiffHunk {
        StructuredDiffHunk {
            old_start: new_start,
            old_lines: lines.iter().filter(|line| !line.starts_with('+')).count(),
            new_start,
            new_lines: lines.iter().filter(|line| !line.starts_with('-')).count(),
            lines: lines.iter().map(|line| line.to_string()).collect(),
        }
    }

    #[test]
    fn structured_diff_list_intersperse_separator_like_official_component() {
        let lines = structured_diff_list_lines(
            &[
                hunk(1, &["-old", "+new"]),
                hunk(10, &[" context", "-gone", "+back"]),
            ],
            false,
            80,
            "src/lib.rs",
            None,
            None,
            true,
            SyntaxHighlightTheme::Dark,
        );
        let text = lines_to_text(&lines);
        assert!(text.contains("..."), "text=\n{text}");
        assert!(text.contains("-old"), "text=\n{text}");
        assert!(text.contains("+back"), "text=\n{text}");
    }

    #[test]
    fn structured_diff_list_component_renders_diff_text() {
        let text = element! {
            ContextProvider(value: Context::owned(*theme::current())) {
                // The component reads `settings.syntax_highlighting_disabled`
                // from AppState. Default state is the right fixture here:
                // `skip_highlighting: true` already forces the branch this test
                // asserts on, so the AppState value cannot affect the outcome.
                crate::state::app_state::AppStateProvider(
                    children: crate::state::app_state::ProviderChildren::new(|| element! {
                        StructuredDiffList(
                            hunks: vec![hunk(1, &["-old", "+new"])],
                            dim: false,
                            width: 80usize,
                            file_path: "src/lib.rs".to_string(),
                            first_line: None,
                            file_content: None,
                            skip_highlighting: true,
                        )
                    }.into_any()),
                )
            }
        }
        .render(Some(120))
        .to_string();

        assert!(text.contains("-old"), "canvas=\n{text}");
        assert!(text.contains("+new"), "canvas=\n{text}");
    }
}
