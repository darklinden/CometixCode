//! Maps to CC `settings.ts:201-234,412-524`: preserve parsed JSON independently
//! of the typed consumer projection, especially when writing a cached value.
use super::{SettingSource, settings_cache, validation};
use crate::utils::fs_operations::{get_fs_implementation, safe_resolve_path};
use std::path::Path;

pub(super) fn parse_settings_file_uncached(path: &Path) -> settings_cache::ParsedSettings {
    let resolved = safe_resolve_path(get_fs_implementation().as_ref(), path).resolved_path;
    let content = match crate::utils::file_read::read_file_sync(&resolved) {
        Ok(content) => content,
        Err(_) => return settings_cache::ParsedSettings::default(),
    };
    if content.trim().is_empty() {
        return settings_cache::ParsedSettings {
            settings: Some(super::SettingsJson::default()),
            value: Some(serde_json::json!({})),
            errors: Vec::new(),
        };
    }
    // safeParseJSON(content, false) yields null on malformed JSON; validation
    // still runs so the caller receives the schema's invalid-type diagnostic.
    let mut raw = serde_json::from_str::<serde_json::Value>(
        content.strip_prefix('\u{feff}').unwrap_or(&content),
    )
    .unwrap_or(serde_json::Value::Null);
    let file = path.to_string_lossy();
    let mut errors = validation::filter_invalid_permission_rules(&mut raw, Some(&file));
    match crate::utils::zod::safe_parse(super::types::settings_schema(), &raw) {
        Ok(value) => match serde_json::from_value(value.clone()) {
            Ok(settings) => settings_cache::ParsedSettings {
                settings: Some(settings),
                value: Some(value),
                errors,
            },
            Err(error) => {
                tracing::warn!("Failed to parse {}: {}", path.display(), error);
                settings_cache::ParsedSettings::default()
            }
        },
        Err(error) => {
            errors.extend(validation::format_zod_error(&error, &file));
            settings_cache::ParsedSettings {
                settings: None,
                value: None,
                errors,
            }
        }
    }
}

pub(super) fn update_settings_for_source(
    source: SettingSource,
    updates: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    if !matches!(
        source,
        SettingSource::User | SettingSource::Project | SettingSource::Local
    ) {
        anyhow::bail!("settings source is not editable");
    }
    let path = super::get_settings_file_path_for_source(source)
        .ok_or_else(|| anyhow::anyhow!("settings path is unavailable"))?;
    let parent = crate::utils::fs_operations::native::dirname(&path);
    get_fs_implementation().mkdir_sync(&parent, None)?;
    // The source bypasses per-source cache, but parseSettingsFile still uses
    // its independent file cache. Keep the full validated JSON there.
    super::parse_settings_file(&path);
    let mut value = settings_cache::get_cached_parsed_file(&path).and_then(|parsed| parsed.value);
    if value.is_none() {
        match crate::utils::file_read::read_file_sync(&path) {
            Ok(content) => {
                let raw = serde_json::from_str::<serde_json::Value>(
                    content.strip_prefix('\u{feff}').unwrap_or(&content),
                )
                .ok()
                .filter(|raw| !raw.is_null())
                .ok_or_else(|| {
                    anyhow::anyhow!("Invalid JSON syntax in settings file at {}", path.display())
                })?;
                if raw.is_object() || raw.is_array() {
                    value = Some(raw);
                    crate::utils::debug::log_for_debugging(&format!(
                        "Using raw settings from {} due to validation failure",
                        path.display()
                    ));
                }
            }
            Err(error) if crate::utils::errors::io_errno_code(&error) == Some("ENOENT") => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut value = value.unwrap_or_else(|| serde_json::json!({}));
    // Lodash merges named properties into arrays, but JSON.stringify ignores
    // those properties; preserve the array's serialized indices in this case.
    if let Some(object) = value.as_object_mut() {
        super::apply_settings_update(object, updates);
    }
    super::internal_writes::mark_internal_write(&path);
    crate::utils::file::write_file_sync_and_flush_deprecated(
        &path,
        format!("{}\n", serde_json::to_string_pretty(&value)?).as_bytes(),
    )?;
    settings_cache::reset_settings_cache();
    Ok(())
}
