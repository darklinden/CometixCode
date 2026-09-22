//! Maps to: CC `utils/suggestions/directoryCompletion.ts`.
//! Directory and path completion producers used by `/add-dir` and `@` tokens.
use crate::components::prompt_input::prompt_input_footer_suggestions::SuggestionItem;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Maps to CC `DirectoryEntry` (the only variant is `type: 'directory'`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: PathBuf,
}

/// Maps to CC `PathEntry` used by `getPathCompletions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: PathEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathEntryKind {
    Directory,
    File,
}
/// Maps to CC `CompletionOptions`/`PathCompletionOptions`.
#[derive(Clone, Debug, Default)]
pub struct CompletionOptions {
    pub base_path: Option<PathBuf>,
    pub max_results: Option<usize>,
    /// Source `PathCompletionOptions.includeFiles`; `None` keeps its `true`
    /// default while remaining compatible with directory-only callers.
    pub include_files: Option<bool>,
    /// Source `PathCompletionOptions.includeHidden`; `None` keeps `false`.
    pub include_hidden: Option<bool>,
}
/// Maps to CC `ParsedPath`.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedPath {
    pub directory: PathBuf,
    pub prefix: String,
}
const CACHE_SIZE: usize = 500;
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
// Native carrier for CC's LRUCache: front is LRU, back is MRU. TTL starts
// on insertion; reads promote without extending the expiry (lru-cache default).
static DIRECTORY_CACHE: LazyLock<Mutex<VecDeque<(PathBuf, Instant, Vec<DirectoryEntry>)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static PATH_CACHE: LazyLock<Mutex<VecDeque<(String, Instant, Vec<PathEntry>)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// Maps to CC `parsePartialPath`.
pub fn parse_partial_path(
    partial_path: &str,
    base_path: Option<&Path>,
) -> anyhow::Result<ParsedPath> {
    let base = base_path
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| crate::bootstrap::state::get_original_cwd())
        });
    if partial_path.is_empty() {
        return Ok(ParsedPath {
            directory: base,
            prefix: String::new(),
        });
    }
    let resolved =
        crate::utils::path::expand_path(partial_path, base_path).map_err(anyhow::Error::msg)?;
    if partial_path.ends_with('/') || partial_path.ends_with(std::path::MAIN_SEPARATOR) {
        return Ok(ParsedPath {
            directory: resolved,
            prefix: String::new(),
        });
    }
    Ok(ParsedPath {
        directory: match resolved.parent() {
            Some(parent) if parent.as_os_str().is_empty() => PathBuf::from("."),
            Some(parent) => parent.to_path_buf(),
            None if resolved.has_root() => resolved.clone(),
            None => PathBuf::from("."),
        },
        // Node basename retains the literal dot/dot-dot token. Path::file_name
        // normalizes those components away, so split the original token.
        prefix: partial_path
            .rsplit(std::path::MAIN_SEPARATOR)
            .next()
            .unwrap_or("")
            .to_string(),
    })
}

/// Maps to CC `scanDirectory`: hidden and symlink directories excluded, 100 cap.
pub async fn scan_directory(dir_path: &Path) -> Vec<DirectoryEntry> {
    {
        let mut cache = DIRECTORY_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(index) = cache.iter().position(|entry| entry.0 == dir_path) {
            let entry = cache.remove(index).unwrap();
            if entry.1.elapsed() < CACHE_TTL {
                let result = entry.2.clone();
                cache.push_back(entry);
                return result;
            }
        }
    }
    let scanned: std::io::Result<Vec<DirectoryEntry>> = async {
        let entries = crate::utils::fs_operations::readdir(dir_path).await?;
        let mut directories = Vec::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_dir() && !name.starts_with('.') {
                directories.push(DirectoryEntry {
                    path: dir_path.join(&name),
                    name,
                });
            }
        }
        directories.truncate(100);
        Ok(directories)
    }
    .await;
    match scanned {
        Ok(directories) => {
            let mut cache = DIRECTORY_CACHE.lock().unwrap_or_else(|p| p.into_inner());
            cache.retain(|entry| entry.0 != dir_path);
            cache.push_back((dir_path.to_path_buf(), Instant::now(), directories.clone()));
            if cache.len() > CACHE_SIZE {
                cache.pop_front();
            }
            directories
        }
        Err(error) => {
            crate::utils::log::log_error(crate::utils::log::LogError::new(error.to_string()));
            Vec::new()
        }
    }
}
/// Maps to CC `getDirectoryCompletions`.
pub async fn get_directory_completions(
    partial_path: &str,
    options: CompletionOptions,
) -> anyhow::Result<Vec<SuggestionItem>> {
    let parsed = parse_partial_path(partial_path, options.base_path.as_deref())?;
    let prefix = parsed.prefix.to_lowercase();
    Ok(scan_directory(&parsed.directory)
        .await
        .into_iter()
        .filter(|entry| entry.name.to_lowercase().starts_with(&prefix))
        .take(options.max_results.unwrap_or(10))
        .map(|entry| SuggestionItem {
            id: entry.path.to_string_lossy().into_owned(),
            display_text: format!("{}/", entry.name),
            tag: None,
            command_text: String::new(),
            description: "directory".to_string(),
            metadata: Some(serde_json::json!({"type":"directory"})),
            color: None,
        })
        .collect())
}

/// Maps to CC `isPathLikeToken` used to prioritize directory traversal over
/// the fuzzy file index for `@~/`, `@./`, absolute and parent paths.
pub fn is_path_like_token(token: &str) -> bool {
    token.starts_with("~/")
        || token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || matches!(token, "~" | "." | "..")
}

/// Maps to CC `scanDirectoryForPaths`: files and directories, hidden entries
/// filtered by default, directories first, then the source's 100-entry cap.
pub async fn scan_directory_for_paths(dir_path: &Path, include_hidden: bool) -> Vec<PathEntry> {
    let cache_key = format!("{}:{include_hidden}", dir_path.display());
    {
        let mut cache = PATH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(index) = cache.iter().position(|entry| entry.0 == cache_key) {
            let entry = cache.remove(index).unwrap();
            if entry.1.elapsed() < CACHE_TTL {
                let result = entry.2.clone();
                cache.push_back(entry);
                return result;
            }
        }
    }

    let Ok(mut entries) = crate::utils::fs_operations::readdir(dir_path).await else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    while let Some(entry) = entries.pop() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let kind = if file_type.is_dir() {
            PathEntryKind::Directory
        } else {
            // Node's Dirent path scanner treats every non-directory entry as
            // a file, including symlinks and other filesystem leaf types.
            PathEntryKind::File
        };
        paths.push(PathEntry {
            name,
            path: dir_path.join(entry.file_name()),
            kind,
        });
    }
    paths.sort_by(|left, right| {
        let left_dir = matches!(left.kind, PathEntryKind::Directory);
        let right_dir = matches!(right.kind, PathEntryKind::Directory);
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.cmp(&right.name))
    });
    paths.truncate(100);
    let mut cache = PATH_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    cache.retain(|entry| entry.0 != cache_key);
    cache.push_back((cache_key, Instant::now(), paths.clone()));
    if cache.len() > CACHE_SIZE {
        cache.pop_front();
    }
    paths
}

/// Maps to CC `getPathCompletions` for path-like `@` tokens.
pub async fn get_path_completions(
    partial_path: &str,
    options: CompletionOptions,
) -> anyhow::Result<Vec<SuggestionItem>> {
    let base_path = options.base_path.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| crate::bootstrap::state::get_original_cwd())
    });
    let max_results = options.max_results.unwrap_or(10);
    let parsed = parse_partial_path(partial_path, Some(&base_path))?;
    let prefix = parsed.prefix.to_lowercase();
    let entries =
        scan_directory_for_paths(&parsed.directory, options.include_hidden.unwrap_or(false)).await;
    let include_files = options.include_files.unwrap_or(true);

    let separator = partial_path
        .char_indices()
        .filter(|(_, ch)| *ch == '/' || *ch == '\\')
        .map(|(index, ch)| index + ch.len_utf8())
        .max();
    let mut dir_portion = separator
        .map(|index| partial_path[..index].to_string())
        .unwrap_or_default();
    if dir_portion.starts_with("./") || dir_portion.starts_with(".\\") {
        dir_portion.drain(..2);
    }

    Ok(entries
        .into_iter()
        .filter(|entry| {
            (include_files || matches!(entry.kind, PathEntryKind::Directory))
                && entry.name.to_lowercase().starts_with(&prefix)
        })
        .take(max_results)
        .map(|entry| {
            let full_path = format!("{}{}", dir_portion, entry.name,);
            let is_directory = matches!(entry.kind, PathEntryKind::Directory);
            SuggestionItem {
                id: full_path.clone(),
                display_text: if is_directory {
                    format!("{full_path}/")
                } else {
                    full_path
                },
                tag: None,
                command_text: String::new(),
                description: String::new(),
                metadata: Some(serde_json::json!({
                    "type": if is_directory { "directory" } else { "file" }
                })),
                color: None,
            }
        })
        .collect())
}
/// Maps to CC `clearDirectoryCache`.
pub fn clear_directory_cache() {
    DIRECTORY_CACHE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
}

/// Maps to CC `clearPathCache`.
///
/// The source helper intentionally invalidates both the directory and path
/// LRU caches.  `getDirectoryCompletions` and `getPathCompletions` share this
/// public reset entry point even though they keep separate cache keys.
pub fn clear_path_cache() {
    DIRECTORY_CACHE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
    PATH_CACHE.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scan_directory_matches_official_node_order_before_hundred_entry_cap() {
        // directoryCompletion.ts:103-114 consumes NodeFsOperations.readdir's
        // order, then filters and slices; sorting only the displayed ten is too late.
        let root = std::env::temp_dir().join(format!("directory-cap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for index in (0..105).rev() {
            std::fs::create_dir(root.join(format!("target-{index:03}"))).unwrap();
        }
        let entries = scan_directory(&root).await;
        assert_eq!(entries.len(), 100);
        assert_eq!(entries[0].name, "target-000");
        assert_eq!(entries[99].name, "target-099");
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn parse_partial_path_matches_official_literal_prefix_and_trailing_separator() {
        // directoryCompletion.ts:56-82: basename(partialPath), not resolved.
        let base = Path::new("/workspace");
        assert_eq!(
            parse_partial_path(".", Some(base)).unwrap(),
            ParsedPath {
                directory: "/".into(),
                prefix: ".".into()
            }
        );
        assert_eq!(
            parse_partial_path("./", Some(base)).unwrap(),
            ParsedPath {
                directory: base.into(),
                prefix: "".into()
            }
        );
        assert_eq!(parse_partial_path("", Some(base)).unwrap().directory, base);
        assert_eq!(
            parse_partial_path(" ", Some(Path::new("")))
                .unwrap()
                .directory,
            Path::new(".")
        );
        assert_eq!(
            parse_partial_path("../Fo", Some(base)).unwrap().prefix,
            "Fo"
        );
        assert_eq!(
            parse_partial_path("/.", Some(base)).unwrap().directory,
            Path::new("/")
        );
        assert_eq!(
            parse_partial_path("..", Some(Path::new("/")))
                .unwrap()
                .directory,
            Path::new("/")
        );
    }
    #[tokio::test]
    async fn directory_completion_matches_official_filter_cache_and_metadata() {
        // directoryCompletion.ts:89-156: directories only, case-insensitive
        // prefix, cached directory scan, maxResults, directory metadata.
        let temp = std::env::temp_dir().join(format!(
            "cometix-directory-completion-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::create_dir(temp.as_path().join("Folder")).unwrap();
        std::fs::create_dir(temp.as_path().join(".hidden")).unwrap();
        std::fs::write(temp.as_path().join("file"), "").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            temp.as_path().join("Folder"),
            temp.as_path().join("fake-dir"),
        )
        .unwrap();
        let options = CompletionOptions {
            base_path: Some(temp.as_path().into()),
            max_results: None,
            include_files: None,
            include_hidden: None,
        };
        let items = get_directory_completions("f", options.clone())
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].display_text, "Folder/");
        assert_eq!(
            items[0].metadata,
            Some(serde_json::json!({"type":"directory"}))
        );
        std::fs::create_dir(temp.as_path().join("Fresh")).unwrap();
        assert_eq!(
            get_directory_completions("f", options.clone())
                .await
                .unwrap()
                .len(),
            1
        );
        // The source `clearPathCache` resets both LRU stores, including the
        // directory scan used by this producer.
        clear_path_cache();
        assert_eq!(
            get_directory_completions("f", options).await.unwrap().len(),
            2
        );
        assert!(
            scan_directory(&temp.as_path().join("missing"))
                .await
                .is_empty()
        );
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[tokio::test]
    async fn path_completion_matches_official_directory_first_and_include_files() {
        let temp =
            std::env::temp_dir().join(format!("cometix-path-completion-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::create_dir(temp.join("Folder")).unwrap();
        std::fs::write(temp.join("file.txt"), "").unwrap();
        std::fs::write(temp.join(".hidden"), "").unwrap();

        let items = get_path_completions(
            "",
            CompletionOptions {
                base_path: Some(temp.clone()),
                max_results: None,
                include_files: Some(true),
                include_hidden: Some(false),
            },
        )
        .await
        .unwrap();
        assert_eq!(items[0].display_text, "Folder/");
        assert!(items.iter().any(|item| item.display_text == "file.txt"));
        assert!(!items.iter().any(|item| item.display_text == ".hidden"));

        let directories_only = get_path_completions(
            "",
            CompletionOptions {
                base_path: Some(temp.clone()),
                max_results: None,
                include_files: Some(false),
                include_hidden: Some(false),
            },
        )
        .await
        .unwrap();
        assert_eq!(directories_only.len(), 1);
        assert_eq!(
            directories_only[0].metadata,
            Some(serde_json::json!({"type":"directory"}))
        );
        std::fs::remove_dir_all(temp).unwrap();
    }
}
