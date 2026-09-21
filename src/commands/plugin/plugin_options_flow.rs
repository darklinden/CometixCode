//! Maps to: CC commands/plugin/PluginOptionsFlow.tsx.
use super::plugin_options_dialog::PluginOptionsDialog;
use crate::types::plugin::LoadedPlugin;
use crate::utils::plugins::{
    mcp_plugin_integration, mcpb_handler, plugin_options_storage as options,
};
use iocraft::prelude::*;
/// Maps to: CC PluginOptionsFlow.tsx#findPluginOptionsTarget.
pub async fn find_plugin_options_target(plugin_id: &str) -> anyhow::Result<Option<LoadedPlugin>> {
    let loaded = crate::utils::plugins::plugin_loader::load_all_plugins().await?;
    Ok(loaded
        .enabled
        .into_iter()
        .chain(loaded.disabled)
        .find(|p| p.source == plugin_id || p.repository == plugin_id))
}
/// Maps to: CC PluginOptionsFlow.tsx#ConfigStep. Struct payload carries source
/// load/save closure captures; functions still execute at their source sites.
#[derive(Clone)]
struct ConfigStep {
    key: String,
    title: String,
    subtitle: String,
    schema: options::PluginOptionSchema,
    full_schema: options::PluginOptionSchema,
    server: Option<String>,
    // Source load/save closures capture pluginId when the mount-only steps form.
    plugin_id: String,
}
#[derive(Default, Props)]
pub struct PluginOptionsFlowProps {
    pub plugin: LoadedPlugin,
    pub plugin_id: String,
    pub on_done: Handler<(String, Option<String>)>,
}
/// Maps to: CC PluginOptionsFlow.tsx#PluginOptionsFlow.
#[component]
pub fn PluginOptionsFlow(
    props: &mut PluginOptionsFlowProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let steps = hooks.use_state(|| {
        let plugin = &props.plugin;
        let mut result = Vec::new();
        let unconfigured = options::get_unconfigured_options(plugin);
        if unconfigured.as_object().is_some_and(|m| !m.is_empty()) {
            result.push(ConfigStep {
                key: "top-level".into(),
                title: format!("Configure {}", plugin.name),
                subtitle: "Plugin options".into(),
                schema: unconfigured,
                full_schema: plugin.manifest.user_config.clone().unwrap_or_default(),
                server: None,
                plugin_id: props.plugin_id.clone(),
            });
        }
        for channel in mcp_plugin_integration::get_unconfigured_channels(plugin) {
            result.push(ConfigStep {
                key: format!("channel:{}", channel.server),
                title: format!("Configure {}", channel.display_name),
                subtitle: format!("Plugin: {}", plugin.name),
                schema: channel.config_schema.clone(),
                full_schema: channel.config_schema,
                server: Some(channel.server),
                plugin_id: props.plugin_id.clone(),
            });
        }
        result
    });
    let index = hooks.use_state(|| 0usize);
    let latest_done =
        hooks.use_const(|| std::sync::Arc::new(std::sync::Mutex::new(props.on_done.clone())));
    *latest_done.lock().unwrap_or_else(|e| e.into_inner()) = props.on_done.clone();
    let len = steps.read().len();
    hooks.use_effect(
        move || {
            if len == 0 {
                (latest_done
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone())(("skipped".into(), None));
            }
        },
        len,
    );
    let Some(current) = steps.read().get(index.get()).cloned() else {
        return element! {View}.into_any();
    };
    let values = match &current.server {
        Some(server) => mcpb_handler::load_mcp_server_user_config(&current.plugin_id, server),
        None => Some(options::load_plugin_options(&current.plugin_id)),
    };
    let plugin_id = current.plugin_id.clone();
    let next_index = index.get() + 1;
    let save_step = current.clone();
    let on_done = props.on_done.clone();
    let on_cancel = props.on_done.clone();
    element!{PluginOptionsDialog(key:current.key,title:current.title,subtitle:current.subtitle,config_schema:current.schema,initial_values:values,
        on_save:move|values|{let mut index=index;
            let result=match &save_step.server{Some(server)=>mcpb_handler::save_mcp_server_user_config(&plugin_id,server,&values,&save_step.full_schema),None=>options::save_plugin_options(&plugin_id,&values,&save_step.full_schema)};
            if let Err(error)=result {on_done(("error".into(),Some(error.to_string())));return;}
            let next=next_index;if next<len{index.set(next);}else{on_done(("configured".into(),None));}
        },on_cancel:move |_|on_cancel(("skipped".into(),None)))}.into_any()
}
