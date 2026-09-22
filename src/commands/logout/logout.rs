//! Maps to CC `commands/logout/logout.tsx`.

use crate::commands::Command;
use crate::components::design_system::dialog::Dialog;
use crate::tool::ToolUseContext;
use crate::utils::process_user_input::ProcessUserInputBaseResult;
use crate::utils::process_user_input::process_slash_command::{
    LocalCommandUi, SlashCommandAction, SlashCommandInvocation,
};
use crate::utils::theme::Theme;
use iocraft::prelude::*;

/// Source-shaped local JSX dispatch. The actual cache/credential operation is
/// completed by the REPL-owned panel callback so its transcript and authVersion
/// update stay in one UI turn.
pub fn dispatch(
    command: &Command,
    args: &str,
    _uuid: Option<String>,
    _context: &ToolUseContext,
) -> ProcessUserInputBaseResult {
    ProcessUserInputBaseResult {
        messages: Vec::new(),
        should_query: false,
        allowed_tools: None,
        local_action: Some(SlashCommandAction::OpenLocalCommandUi {
            command: LocalCommandUi::Logout {
                args: args.to_string(),
            },
            invocation: SlashCommandInvocation::new(command.name.as_ref(), args),
        }),
        query_source: crate::constants::query_source::QuerySource::Prompt,
    }
}

/// Maps to CC `commands/logout/logout.tsx#performLogout`.
///
/// Source cache invalidations that have a concrete Rust owner are kept here;
/// secure-storage deletion is the final side-effect outlet and remains closed
/// unless the existing product gate is explicitly enabled.
pub fn perform_logout(clear_onboarding: bool) -> anyhow::Result<()> {
    let storage = crate::utils::secure_storage::get_secure_storage();
    if !crate::constants::oauth::OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED {
        return Err(crate::constants::oauth::OAuthCredentialSideEffectsUnavailable.into());
    }
    storage.delete()?;
    clear_auth_related_caches();
    crate::utils::config::save_global_config(|config| {
        config.oauth_account = None;
        if clear_onboarding {
            config.has_completed_onboarding = Some(false);
        }
    })?;
    Ok(())
}

/// Maps to CC `commands/logout/logout.tsx#clearAuthRelatedCaches` for the
/// cache families that have a source-owned Rust implementation. Unsupported
/// Grove, remote-managed, policy-limit and trusted-device caches are left at
/// their existing owners until those services expose the corresponding clear
/// operation.
pub fn clear_auth_related_caches() {
    crate::utils::auth::clear_api_key_helper_cache();
    crate::utils::betas::clear_betas_caches();
    crate::utils::tool_schema_cache::clear_tool_schema_cache();
}

#[derive(Default, Props)]
pub struct LogoutCommandProps<'a> {
    pub on_done: HandlerMut<'a, String>,
}

/// Maps to CC `commands/logout/logout.tsx#call`'s completion text. The source
/// schedules process shutdown after completion; REPL owns that lifecycle and
/// keeps the current process alive while the OAuth side-effect gate is closed.
#[component]
pub fn LogoutCommand<'a>(
    props: &mut LogoutCommandProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let theme = hooks.use_context::<Theme>();
    let mut started = hooks.use_state(|| false);
    let mut result = hooks.use_state(|| Option::<String>::None);
    if !started.get() {
        started.set(true);
        let output = match perform_logout(true) {
            Ok(()) => "Successfully logged out from your Anthropic account.".to_string(),
            Err(error) => format!("Logout unavailable: {error}"),
        };
        result.set(Some(output));
    }
    let output = result.read().clone();
    if let Some(output) = output {
        result.set(None);
        (props.on_done)(output);
    }
    element! {
        Dialog(
            title: "Logout".to_string(),
            color: Some(theme.permission),
            on_cancel: move |_| result.set(Some("Logout cancelled".to_string())),
        ) {
            Text(content: "Signing out from your Anthropic account…".to_string(), wrap: TextWrap::Wrap)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn perform_logout_stays_closed_before_any_credential_mutation() {
        const _: () = assert!(!crate::constants::oauth::OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED);
        let error = super::perform_logout(true).expect_err("logout gate must be closed");
        assert!(
            error
                .downcast_ref::<crate::constants::oauth::OAuthCredentialSideEffectsUnavailable>()
                .is_some()
        );
    }
}
