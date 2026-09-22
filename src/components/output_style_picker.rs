//! Maps to: CC `components/OutputStylePicker.tsx`:12-91.
//!
//! Official OutputStylePicker owns async custom-style loading and selection
//! callbacks. This Rust boundary keeps rendering pure/safe: callers provide the
//! already-loaded option list and own selection/persistence; helper functions
//! mirror the official read-only style loading path.

use crate::components::custom_select::{Select, SelectLayout, SelectOptionData};
use crate::components::design_system::dialog::Dialog;
use crate::constants::output_styles::{DEFAULT_OUTPUT_STYLE_NAME, OutputStyleConfig};
use iocraft::prelude::*;

pub(crate) const DEFAULT_OUTPUT_STYLE_LABEL: &str = "Default";
pub(crate) const DEFAULT_OUTPUT_STYLE_DESCRIPTION: &str =
    "Claude completes coding tasks efficiently and provides concise responses";

/// Maps to: CC `components/OutputStylePicker.tsx`:13-27 `mapConfigsToOptions(...)`.
pub(crate) fn map_configs_to_options(
    styles: &[(String, Option<OutputStyleConfig>)],
) -> Vec<SelectOptionData> {
    styles
        .iter()
        .map(|(style, config)| SelectOptionData {
            label: config
                .as_ref()
                .map(|config| config.name.clone())
                .unwrap_or_else(|| DEFAULT_OUTPUT_STYLE_LABEL.to_string()),
            value: style.to_string(),
            description: Some(
                config
                    .as_ref()
                    .map(|config| config.description.clone())
                    .unwrap_or_else(|| DEFAULT_OUTPUT_STYLE_DESCRIPTION.to_string()),
            ),
            dim_description: true,
            disabled: false,
            input: None,
        })
        .collect()
}

pub(crate) fn output_style_options_for_cwd(cwd: &std::path::Path) -> Vec<SelectOptionData> {
    // Maps to CC `OutputStylePicker` `useEffect` loading
    // `getAllOutputStyles(getCwd()).then(mapConfigsToOptions)`.
    map_configs_to_options(&crate::constants::output_styles::get_all_output_styles_ordered(cwd))
}

pub(crate) fn output_style_options() -> Vec<SelectOptionData> {
    let cwd = std::env::current_dir().unwrap_or_default();
    output_style_options_for_cwd(&cwd)
}

pub(crate) fn output_style_value_for_display(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case(DEFAULT_OUTPUT_STYLE_NAME) {
        DEFAULT_OUTPUT_STYLE_NAME.to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn output_style_display_for_option(option: &SelectOptionData) -> String {
    if option.value.eq_ignore_ascii_case(DEFAULT_OUTPUT_STYLE_NAME) {
        DEFAULT_OUTPUT_STYLE_LABEL.to_string()
    } else {
        option.label.clone()
    }
}

#[derive(Default, Props)]
pub(crate) struct OutputStylePickerProps {
    pub options: Vec<SelectOptionData>,
    pub focused_index: usize,
    pub selected_value: Option<String>,
    pub visible_from_index: usize,
    pub is_loading: bool,
    pub is_standalone_command: bool,
}

/// Maps to: CC `components/OutputStylePicker.tsx`:53-91 `OutputStylePicker(...)`.
#[component]
pub(crate) fn OutputStylePicker(props: &OutputStylePickerProps) -> impl Into<AnyElement<'static>> {
    let options = props.options.clone();
    let count = options.len();
    let visible_count = count.clamp(1, 10);
    let focused_index = props.focused_index.min(count.saturating_sub(1));
    let visible_from = props
        .visible_from_index
        .min(count.saturating_sub(visible_count));
    let is_loading = props.is_loading;
    let hide_input_guide = !props.is_standalone_command;
    let hide_border = !props.is_standalone_command;
    let selected_value = props.selected_value.clone();

    element! {
        Dialog(
            title: "Preferred output style".to_string(),
            on_cancel: |_| {},
            hide_input_guide: hide_input_guide,
            hide_border: hide_border,
        ) {
            View(flex_direction: FlexDirection::Column, row_gap: 1u32) {
                View(margin_top: 1u32) {
                    Text(content: "This changes how Claude Code communicates with you".to_string(), dim: true)
                }
                #(if is_loading {
                    element! { Text(content: "Loading output styles…".to_string(), dim: true) }.into_any()
                } else {
                    element! {
                        Select(
                            is_disabled: false,
                            hide_indexes: false,
                            visible_option_count: visible_count,
                            options: options,
                            focused_index: focused_index,
                            selected_value: selected_value,
                            visible_from_index: visible_from,
                            layout: SelectLayout::Compact,
                        )
                    }.into_any()
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::theme;

    fn render_picker(props: OutputStylePickerProps) -> String {
        let current_theme = *theme::current();
        element! {
            ContextProvider(value: Context::owned(current_theme)) {
                OutputStylePicker(
                    options: props.options,
                    focused_index: props.focused_index,
                    selected_value: props.selected_value,
                    visible_from_index: props.visible_from_index,
                    is_loading: props.is_loading,
                    is_standalone_command: props.is_standalone_command,
                )
            }
        }
        .render(Some(120))
        .to_string()
    }

    #[test]
    fn map_configs_to_options_uses_official_default_fallback_copy() {
        let options = map_configs_to_options(&[
            (DEFAULT_OUTPUT_STYLE_NAME.to_string(), None),
            (
                "Custom".to_string(),
                Some(OutputStyleConfig {
                    name: "Custom".to_string(),
                    description: "Custom description".to_string(),
                    prompt: "Prompt".to_string(),
                    source: "projectSettings".to_string(),
                    keep_coding_instructions: None,
                    force_for_plugin: None,
                }),
            ),
        ]);

        assert_eq!(options[0].label, DEFAULT_OUTPUT_STYLE_LABEL);
        assert_eq!(options[0].value, DEFAULT_OUTPUT_STYLE_NAME);
        assert_eq!(
            options[0].description.as_deref(),
            Some(DEFAULT_OUTPUT_STYLE_DESCRIPTION)
        );
        assert_eq!(options[1].label, "Custom");
        assert_eq!(
            options[1].description.as_deref(),
            Some("Custom description")
        );
    }

    #[test]
    fn output_style_picker_renders_official_dialog_copy_and_options() {
        let text = render_picker(OutputStylePickerProps {
            options: output_style_options(),
            selected_value: Some(DEFAULT_OUTPUT_STYLE_NAME.to_string()),
            ..OutputStylePickerProps::default()
        });

        assert!(text.contains("Preferred output style"), "canvas=\n{text}");
        assert!(
            text.contains("This changes how Claude Code communicates with you"),
            "canvas=\n{text}"
        );
        assert!(text.contains("Default"), "canvas=\n{text}");
        assert!(
            text.contains(DEFAULT_OUTPUT_STYLE_DESCRIPTION),
            "canvas=\n{text}"
        );
        assert!(text.contains("Explanatory"), "canvas=\n{text}");
        assert!(
            text.contains("Claude explains its implementation choices"),
            "canvas=\n{text}"
        );
        assert!(text.contains("Learning"), "canvas=\n{text}");
    }

    #[test]
    fn output_style_options_for_cwd_include_custom_markdown_styles() {
        let _lock = crate::utils::env_utils::TEST_ENV_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "cometix-output-style-picker-{}",
            uuid::Uuid::new_v4()
        ));
        let cwd = root.join("repo");
        let config_home = root.join("config");
        std::fs::create_dir_all(cwd.join(".git")).unwrap();
        std::fs::create_dir_all(cwd.join(".claude/output-styles")).unwrap();
        std::fs::create_dir_all(&config_home).unwrap();
        std::fs::write(
            cwd.join(".claude/output-styles/mentor.md"),
            "---\nname: Mentor\ndescription: Project mentor\n---\nPrompt",
        )
        .unwrap();
        let _config = crate::utils::env_utils::EnvVarGuard::set("CLAUDE_CONFIG_DIR", &config_home);
        let options = output_style_options_for_cwd(&cwd);
        let _ = std::fs::remove_dir_all(root);

        let mentor = options
            .iter()
            .find(|option| option.value == "Mentor")
            .unwrap();
        assert_eq!(mentor.label, "Mentor");
        assert_eq!(mentor.description.as_deref(), Some("Project mentor"));
    }

    #[test]
    fn output_style_picker_renders_loading_and_standalone_input_guide() {
        let text = render_picker(OutputStylePickerProps {
            is_loading: true,
            is_standalone_command: true,
            ..OutputStylePickerProps::default()
        });

        assert!(text.contains("Loading output styles…"), "canvas=\n{text}");
        assert!(
            text.contains("Enter to confirm · Esc to cancel"),
            "canvas=\n{text}"
        );
    }
}
