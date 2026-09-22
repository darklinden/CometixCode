//! Maps to: CC `components/PromptInput/PromptInputHelpMenu.tsx`.

use iocraft::prelude::*;

pub const PROMPT_INPUT_HELP_MENU_HEIGHT: usize = 6;

#[derive(Default, Props)]
pub struct PromptInputHelpMenuProps {
    pub dim_color: bool,
    pub fixed_width: bool,
    pub gap: u32,
    pub padding_x: u32,
}

/// Maps to CC `components/PromptInput/PromptInputHelpMenu.tsx#PromptInputHelpMenu`.
#[component]
pub fn PromptInputHelpMenu(
    props: &PromptInputHelpMenuProps,
    _hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // CC's `fixedWidth` branch resolves to these same widths in this port,
    // so the prop is inert for now.
    let col1_width = 24u32;
    let col2_width = 35u32;

    element! {
        View(
            flex_direction: FlexDirection::Row,
            padding_left: props.padding_x.saturating_add(1),
            padding_right: props.padding_x.saturating_add(1),
            height: PROMPT_INPUT_HELP_MENU_HEIGHT as u32,
            overflow: Overflow::Hidden,
        ) {
            View(flex_direction: FlexDirection::Column, width: col1_width, margin_right: props.gap) {
                Text(content: "! for bash mode".to_string(), dim: props.dim_color)
                Text(content: "/ for commands".to_string(), dim: props.dim_color)
                Text(content: "@ for file paths".to_string(), dim: props.dim_color)
                Text(content: "& for background".to_string(), dim: props.dim_color)
                Text(content: "/btw for side question".to_string(), dim: props.dim_color)
            }
            View(flex_direction: FlexDirection::Column, width: col2_width, margin_right: props.gap) {
                Text(content: "double tap esc to clear input".to_string(), dim: props.dim_color)
                Text(content: "shift + tab to auto-accept edits".to_string(), dim: props.dim_color)
                Text(content: "ctrl + o for verbose output".to_string(), dim: props.dim_color)
                Text(content: "ctrl + t to toggle tasks".to_string(), dim: props.dim_color)
                Text(content: "shift + enter for newline".to_string(), dim: props.dim_color)
            }
            View(flex_direction: FlexDirection::Column) {
                Text(content: "ctrl + _ to undo".to_string(), dim: props.dim_color)
                Text(content: "ctrl + z to suspend".to_string(), dim: props.dim_color)
                Text(content: "ctrl + v to paste images".to_string(), dim: props.dim_color)
                Text(content: "alt + p to switch model".to_string(), dim: props.dim_color)
                Text(content: "ctrl + s to stash prompt".to_string(), dim: props.dim_color)
                Text(content: "ctrl + g to edit in $EDITOR".to_string(), dim: props.dim_color)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_help_menu_remains_shortcut_grid() {
        let canvas = element! {
            ContextProvider(value: Context::owned(*crate::utils::theme::current())) {
                View(width: 100u32) {
                    PromptInputHelpMenu(
                        dim_color: true,
                        fixed_width: true,
                        padding_x: 2u32,
                    )
                }
            }
        }
        .render(Some(100));
        let text = canvas.to_string();
        assert!(text.contains("! for bash mode"), "canvas=\n{text}");
        assert!(!text.contains("Cometix Code v"), "canvas=\n{text}");
    }
}
