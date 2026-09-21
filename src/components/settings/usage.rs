//! Maps to: CC components/Settings/Usage.tsx
//! Usage tab — API token usage, rate limits, cost display.
//! Phase 1: Placeholder. Phase 2: integrate with API usage tracking.

use iocraft::prelude::*;

#[component]
pub fn Usage(hooks: Hooks) -> impl Into<AnyElement<'static>> {
    element! {
        View(padding_left: 1u32) {
            Text(content: "Usage data not yet available.", color: hooks.use_context::<crate::utils::theme::Theme>().subtle)
        }
    }
}
