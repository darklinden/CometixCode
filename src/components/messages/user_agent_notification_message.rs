//! Maps to: CC `components/messages/UserAgentNotificationMessage.tsx:1-42`.
//!
//! Renders a queued `<task-notification>` user message as
//! `⏺ Agent "…" completed`, with the circle colored by the notification's
//! `<status>` tag and the summary left in the default text tone — CC nests
//! `<Text color={color}>{BLACK_CIRCLE}</Text>` inside an uncolored `<Text>`.

use crate::utils::theme::Theme;
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct UserAgentNotificationMessageProps {
    /// The full task-notification text. The component extracts its own
    /// `summary`/`status` tags, matching CC receiving `param` and calling
    /// `extractTag` itself (`UserAgentNotificationMessage.tsx:29-32`).
    pub text: String,
    pub add_margin: bool,
}

/// Maps to: CC `UserAgentNotificationMessage.tsx:13-24` `getStatusColor` —
/// completed → success, failed → error, killed → warning, default → text.
fn status_color(theme: &Theme, status: Option<&str>) -> Color {
    match status {
        Some("completed") => theme.success,
        Some("failed") => theme.error,
        Some("killed") => theme.warning,
        _ => theme.text,
    }
}

#[component]
pub fn UserAgentNotificationMessage(
    props: &UserAgentNotificationMessageProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // CC `:30` — `if (!summary) return null`.
    let Some(summary) = crate::utils::messages::extract_tag(&props.text, "summary") else {
        return element! { View }.into_any();
    };
    let status = crate::utils::messages::extract_tag(&props.text, "status");
    let theme = hooks.use_context::<Theme>();
    let mut circle = MixedTextContent::new(crate::constants::figures::BLACK_CIRCLE);
    circle.color = Some(status_color(&theme, status.as_deref()));
    element! {
        View(margin_top: if props.add_margin { 1u32 } else { 0u32 }) {
            MixedText(contents: vec![circle, MixedTextContent::new(format!(" {summary}"))])
        }
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str) -> String {
        let theme = *crate::utils::theme::current();
        element! {
            ContextProvider(value: Context::owned(theme)) {
                UserAgentNotificationMessage(text: text.to_string())
            }
        }
        .render(Some(80))
        .to_string()
    }

    #[test]
    fn renders_circle_and_summary_from_tags() {
        let text = "<task-notification>\n<task-id>agent-1</task-id>\n<status>completed</status>\n<summary>Agent \"inspect\" completed</summary>\n</task-notification>";
        let rendered = render(text);
        assert!(rendered.contains(&format!(
            "{} Agent \"inspect\" completed",
            crate::constants::figures::BLACK_CIRCLE
        )));
    }

    #[test]
    fn missing_summary_renders_nothing_like_official_null() {
        let rendered = render("<task-notification><status>completed</status></task-notification>");
        assert_eq!(rendered.trim(), "");
    }

    #[test]
    fn status_maps_to_official_color_arms() {
        let theme = *crate::utils::theme::current();
        assert_eq!(status_color(&theme, Some("completed")), theme.success);
        assert_eq!(status_color(&theme, Some("failed")), theme.error);
        assert_eq!(status_color(&theme, Some("killed")), theme.warning);
        assert_eq!(status_color(&theme, None), theme.text);
        assert_eq!(status_color(&theme, Some("unknown")), theme.text);
    }
}
