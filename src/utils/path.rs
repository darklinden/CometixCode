//! Native path expansion helpers.
//!
//! Maps to CC `utils/path.ts`. Permission-specific canonical/symlink checks
//! remain in `utils/permissions/path_validation.rs`.

use std::path::{Component, Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let normalized = normalized.display().to_string().nfc().collect::<String>();
    PathBuf::from(normalized)
}

#[cfg(windows)]
fn native_path(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b'/' {
        return format!(
            "{}:\\{}",
            (bytes[1] as char).to_ascii_uppercase(),
            &path[3..]
        );
    }
    path.to_string()
}

#[cfg(not(windows))]
fn native_path(path: &str) -> String {
    path.to_string()
}

/// Maps to: Node `path.relative(from, to)` (posix semantics), the primitive
/// CC call sites import directly from `node:path`. Node resolves both sides
/// before comparing; this narrowed projection resolves a relative `to`
/// against `from` — the callers pass the session cwd as `from`, which is
/// Node's resolve base in the source — and expects components to already be
/// normalized (no `.`/`..` segments). The walk strips the common component
/// prefix and turns each remaining `from` component into `..`. Equal paths
/// yield `""`, matching Node. The std library has no equivalent
/// (`Path::strip_prefix` cannot produce `..`); the community equivalent is
/// `pathdiff::diff_paths`, not pulled in while this narrowed contract
/// covers every caller.
/// Distinct from `file::get_display_path`, which maps outside-cwd paths to
/// `~/...` or the absolute form — some CC sites (FileWriteTool UI.tsx:59)
/// deliberately use the bare `relative` instead.
pub fn node_path_relative(from: &Path, to: &Path) -> String {
    let resolved_to;
    let to = if to.is_absolute() {
        to
    } else {
        resolved_to = from.join(to);
        &resolved_to
    };
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let common = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_components.len() {
        parts.push("..".to_string());
    }
    for component in &to_components[common..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// Maps to CC `utils/path.ts:32-83` `expandPath(...)`.
pub fn expand_path(path: &str, base_dir: Option<&Path>) -> Result<PathBuf, String> {
    let base = base_dir
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    if path.contains('\0') || base.as_os_str().to_string_lossy().contains('\0') {
        return Err("Path contains null bytes".to_string());
    }

    // CC utils/path.ts:54-55 uses ECMAScript trim: FEFF is whitespace,
    // whereas U+0085 is a literal path character (unlike Rust str::trim).
    let trimmed = path.trim_matches(|character| {
        matches!(character,
            '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
            '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
            '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
    });
    if trimmed.is_empty() {
        return Ok(normalize_path(&base));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if trimmed == "~" {
        return Ok(normalize_path(home.as_deref().unwrap_or(Path::new("~"))));
    }
    if let Some(suffix) = trimmed.strip_prefix("~/") {
        if let Some(home) = home {
            return Ok(normalize_path(&home.join(suffix)));
        }
    }

    let processed = PathBuf::from(native_path(trimmed));
    if processed.is_absolute() {
        Ok(normalize_path(&processed))
    } else {
        Ok(normalize_path(&base.join(processed)))
    }
}

/// Maps to CC `getDirectoryForPath(path)`.
pub fn get_directory_for_path(path: &str) -> String {
    let absolute = expand_path(path, None).unwrap_or_else(|_| PathBuf::from(path));
    let display = absolute.display().to_string();
    if !(display.starts_with("\\\\") || display.starts_with("//")) {
        if std::fs::metadata(&absolute).is_ok_and(|metadata| metadata.is_dir()) {
            return display;
        }
    }
    absolute
        .parent()
        .map(|parent| parent.display().to_string())
        .filter(|parent| !parent.is_empty())
        .unwrap_or(display)
}

#[cfg(test)]
// Further ported items follow the test module in this file.
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn expand_path_resolves_relative_segments_without_filesystem_canonicalization() {
        assert_eq!(
            expand_path("./src/../Cargo.toml", Some(Path::new("/repo"))).unwrap(),
            PathBuf::from("/repo/Cargo.toml")
        );
    }

    #[test]
    fn directory_for_path_keeps_existing_directories_and_uses_file_parent() {
        let directory = std::env::temp_dir().join(format!(
            "cometix-path-directory-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(
            get_directory_for_path(&directory.display().to_string()),
            directory.display().to_string()
        );
        assert_eq!(
            get_directory_for_path(&directory.join("missing.txt").display().to_string()),
            directory.display().to_string()
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    /// CC utils/path.ts:54-81 uses ECMAScript trim, then NFC normalization.
    #[test]
    fn expand_path_matches_official_ecmascript_trim_and_nfc() {
        assert_eq!(
            expand_path("\u{feff}cafe\u{301}\u{feff}", Some(Path::new("/repo"))).unwrap(),
            PathBuf::from("/repo/caf\u{e9}")
        );
        assert_eq!(
            expand_path("\u{85}", Some(Path::new("/repo"))).unwrap(),
            PathBuf::from("/repo/\u{85}")
        );
        assert_eq!(
            expand_path(" \t\u{feff}", Some(Path::new("/repo"))).unwrap(),
            PathBuf::from("/repo")
        );
    }

    #[test]
    fn expand_path_rejects_null_bytes() {
        assert_eq!(
            expand_path("bad\0path", Some(Path::new("/repo"))).unwrap_err(),
            "Path contains null bytes"
        );
    }

    /// Maps to: Node `path.relative` oracles — inside cwd, outside cwd
    /// (`..` climb), equal paths (`""`), and a relative `to` resolved
    /// against `from` (Node resolves both sides; wire data can carry the
    /// model's raw relative file_path).
    #[test]
    fn node_path_relative_matches_node_oracles() {
        let from = Path::new("/repo/project");
        assert_eq!(
            node_path_relative(from, Path::new("/repo/project/src/a.rs")),
            "src/a.rs"
        );
        assert_eq!(
            node_path_relative(from, Path::new("/repo/elsewhere/out.txt")),
            "../elsewhere/out.txt"
        );
        assert_eq!(node_path_relative(from, Path::new("/repo/project")), "");
        assert_eq!(node_path_relative(from, Path::new("a.txt")), "a.txt");
    }
}

/// Maps to: CC `utils/path.ts:133-135#containsPathTraversal`.
pub fn contains_path_traversal(path: &str) -> bool {
    path.split(['/', '\\']).any(|segment| segment == "..")
}
