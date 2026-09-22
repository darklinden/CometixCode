//! Filesystem operation boundary.
//!
//! Maps to: CC `utils/fsOperations.ts` `ReadFileRangeResult`,
//! `readFileRange(...)`, and `tailFile(...)`.
//!
//! The full injectable `FsOperations` surface remains partial. These bounded
//! readers are source-shaped because `TaskOutput.ts` consumes them directly.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Maps to: CC `utils/fsOperations.ts#NodeFsOperations.readdir:398-400`.
/// Native carrier for Node's withFileTypes result. Node's libuv scandir returns
/// filename order on macOS; Tokio exposes filesystem iteration order instead.
/// Preserve that boundary before callers filter or truncate entries.
pub async fn readdir(path: &Path) -> std::io::Result<Vec<tokio::fs::DirEntry>> {
    let mut reader = tokio::fs::read_dir(path).await?;
    let mut entries = Vec::new();
    while let Some(entry) = reader.next_entry().await? {
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

/// Maps to: CC `utils/fsOperations.ts#NodeFsOperations.rm:410-412`.
/// Native carrier for the source recursive/force options. Neither force nor
/// recursion permits swallowing errors other than a missing path. Ordinary
/// files and symlinks are unlinked; directory contents use native removal.
/// Bun-specific nonrecursive-directory diagnostics and trailing-separator
/// symlink behavior are not claimed equivalent to the native filesystem API.
pub async fn rm(path: &Path, recursive: bool, force: bool) -> std::io::Result<()> {
    let result = async {
        let metadata = tokio::fs::symlink_metadata(path).await?;
        if metadata.is_dir() {
            if recursive {
                tokio::fs::remove_dir_all(path).await
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::IsADirectory,
                    "recursive removal is required for a directory",
                ))
            }
        } else {
            tokio::fs::remove_file(path).await
        }
    }
    .await;
    match result {
        Err(error) if force && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Maps to: CC `utils/fsOperations.ts#NodeFsOperations.mkdir:414-425`.
/// The source options object has only `mode`; recursive is always true. The
/// EEXIST catch is unconditional, despite its Bun/Windows explanatory comment.
pub async fn mkdir(path: &Path, mode: Option<u32>) -> std::io::Result<()> {
    let mut builder = tokio::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    if let Some(mode) = mode {
        builder.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    match builder.create(path).await {
        Err(error) if crate::utils::errors::io_errno_code(&error) == Some("EEXIST") => Ok(()),
        result => result,
    }
}

/// Maps to CC `safeResolvePath(...)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeResolvedPath {
    pub resolved_path: PathBuf,
    pub is_symlink: bool,
    pub is_canonical: bool,
}

/// Maps to CC `utils/fsOperations.ts#isDuplicatePath`.
/// The check and insertion remain synchronous, including failed realpath cases.
pub fn is_duplicate_path(
    file_path: &Path,
    loaded_paths: &mut std::collections::HashSet<std::ffi::OsString>,
) -> bool {
    !loaded_paths.insert(safe_resolve_path(file_path).resolved_path.into_os_string())
}

fn is_special_file_type(file_type: &std::fs::FileType) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        file_type.is_fifo()
            || file_type.is_socket()
            || file_type.is_char_device()
            || file_type.is_block_device()
    }
    #[cfg(not(unix))]
    {
        let _ = file_type;
        false
    }
}

/// Maps to CC `utils/fsOperations.ts#safeResolvePath`.
///
/// UNC paths are returned without touching the filesystem. Missing, broken,
/// inaccessible, and special paths retain their logical path so callers can
/// apply the same creation/error policy as CC.
pub fn safe_resolve_path(file_path: &Path) -> SafeResolvedPath {
    let raw = file_path.to_string_lossy();
    if raw.starts_with("//") || raw.starts_with("\\\\") {
        return SafeResolvedPath {
            resolved_path: file_path.to_path_buf(),
            is_symlink: false,
            is_canonical: false,
        };
    }

    let Ok(metadata) = std::fs::symlink_metadata(file_path) else {
        return SafeResolvedPath {
            resolved_path: file_path.to_path_buf(),
            is_symlink: false,
            is_canonical: false,
        };
    };
    if is_special_file_type(&metadata.file_type()) {
        return SafeResolvedPath {
            resolved_path: file_path.to_path_buf(),
            is_symlink: false,
            is_canonical: false,
        };
    }
    let Ok(resolved_path) = file_path.canonicalize() else {
        return SafeResolvedPath {
            resolved_path: file_path.to_path_buf(),
            is_symlink: false,
            is_canonical: false,
        };
    };
    // CC fsOperations.ts:138 receives FsOperations; its default realpathSync
    // (:523-525) returns NFC before safeResolvePath compares literal spelling.
    let resolved_path = PathBuf::from(resolved_path.to_string_lossy().nfc().collect::<String>());
    SafeResolvedPath {
        // CC fsOperations.ts:166 compares string spelling, not normalized
        // path components: '/cwd/' and '/cwd/./' differ from '/cwd'.
        is_symlink: resolved_path.as_os_str() != file_path.as_os_str(),
        resolved_path,
        is_canonical: true,
    }
}

/// Maps to CC `resolveDeepestExistingAncestorSync(...)`.
pub fn resolve_deepest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    let mut tail = Vec::<std::ffi::OsString>::new();
    loop {
        let parent = current.parent()?.to_path_buf();
        if current == parent {
            return None;
        }
        match std::fs::symlink_metadata(&current) {
            Err(_) => {
                tail.push(current.file_name()?.to_os_string());
                current = parent;
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let resolved = current.canonicalize().ok().or_else(|| {
                    let target = std::fs::read_link(&current).ok()?;
                    Some(if target.is_absolute() {
                        target
                    } else {
                        parent.join(target)
                    })
                })?;
                return Some(
                    tail.iter()
                        .rev()
                        .fold(resolved, |path, part| path.join(part)),
                );
            }
            Ok(_) => {
                let resolved = current.canonicalize().ok()?;
                if resolved != current {
                    return Some(
                        tail.iter()
                            .rev()
                            .fold(resolved, |path, part| path.join(part)),
                    );
                }
                return None;
            }
        }
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

/// Maps to CC `utils/fsOperations.ts#getPathsForPermissionCheck`.
/// Preserves insertion order: logical path, each immediate link target, then
/// the final canonical/deepest-existing destination.
pub fn get_paths_for_permission_check(input_path: &Path) -> Vec<PathBuf> {
    let mut path = input_path.to_path_buf();
    let raw = input_path.to_string_lossy();
    if raw == "~" || raw.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
        {
            path = if raw == "~" {
                home
            } else {
                home.join(&raw[2..])
            };
        }
    }

    let mut paths = vec![path.clone()];
    let raw = path.to_string_lossy();
    if raw.starts_with("//") || raw.starts_with("\\\\") {
        return paths;
    }

    let mut current = path.clone();
    let mut visited = Vec::<PathBuf>::new();
    for _ in 0..40 {
        if visited.iter().any(|seen| seen == &current) {
            break;
        }
        visited.push(current.clone());
        if std::fs::metadata(&current).is_err() {
            if current == path {
                if let Some(resolved) = resolve_deepest_existing_ancestor(&path) {
                    push_unique_path(&mut paths, resolved);
                }
            }
            break;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            break;
        };
        if is_special_file_type(&metadata.file_type()) || !metadata.file_type().is_symlink() {
            break;
        }
        let Ok(target) = std::fs::read_link(&current) else {
            break;
        };
        let target = if target.is_absolute() {
            target
        } else {
            current
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(target)
        };
        push_unique_path(&mut paths, target.clone());
        current = target;
    }

    let resolved = safe_resolve_path(&path);
    if resolved.is_symlink && resolved.resolved_path != path {
        push_unique_path(&mut paths, resolved.resolved_path);
    }
    paths
}

/// Maps to CC `utils/fsOperations.ts:578-599#readFileBytes`.
/// With `max_bytes`, CC reads at most that many bytes from the start rather
/// than reading the whole file and throwing for a larger file.
pub fn read_file_bytes(path: &Path, max_bytes: Option<f64>) -> std::io::Result<Vec<u8>> {
    let at_path = |error: std::io::Error, operation| {
        let kind = error.kind();
        std::io::Error::new(
            kind,
            crate::utils::errors::format_native_file_error(&error, operation, Some(path)),
        )
    };
    let mut file = std::fs::File::open(path).map_err(|error| at_path(error, "open"))?;
    let Some(max_bytes) = max_bytes else {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| at_path(error, "read"))?;
        return Ok(bytes);
    };
    if max_bytes.is_nan() || max_bytes < 0.0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "The value of maxBytes is out of range",
        ));
    }

    let file_size = file
        .metadata()
        .map_err(|error| at_path(error, "fstat"))?
        .len() as f64;
    let read_size = file_size.min(max_bytes).floor();
    let read_size = if read_size >= usize::MAX as f64 {
        usize::MAX
    } else {
        read_size as usize
    };
    let mut bytes = vec![0_u8; read_size];
    let mut offset = 0usize;
    while offset < read_size {
        let count = file
            .read(&mut bytes[offset..])
            .map_err(|error| at_path(error, "read"))?;
        if count == 0 {
            break;
        }
        offset += count;
    }
    bytes.truncate(offset);
    Ok(bytes)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadFileRangeResult {
    pub content: String,
    pub bytes_read: usize,
    pub bytes_total: u64,
}

/// Rust security hardening for the source `open(path, 'r')` call sites. Bash
/// output paths are controlled by an untrusted child after spawn, so opening
/// with no-follow semantics prevents a rename-and-symlink swap from redirecting
/// host-side reads or truncation.
pub(crate) fn open_regular_file_no_follow(
    path: &Path,
    writable: bool,
) -> std::io::Result<std::fs::File> {
    let before = std::fs::symlink_metadata(path)?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refusing unsafe file path",
        ));
    }

    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(writable);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // FILE_FLAG_OPEN_REPARSE_POINT prevents a late symlink/junction swap
        // from resolving to its target between metadata and open.
        options.custom_flags(0x0020_0000);
    }

    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "refusing non-regular file",
        ));
    }
    Ok(file)
}

#[cfg(test)]
thread_local! {
    static FORCE_REPLACE_FILE_ATOMIC_FAILURE: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn force_next_replace_file_atomic_failure_for_test() {
    FORCE_REPLACE_FILE_ATOMIC_FAILURE.set(true);
}

fn take_forced_replace_failure() -> bool {
    #[cfg(test)]
    {
        FORCE_REPLACE_FILE_ATOMIC_FAILURE.replace(false)
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// Maps to CC `FsOperations.renameSync(...)`, with Windows replace-existing
/// parity required by atomic settings and retained Bash-output commits.
#[cfg(not(windows))]
pub(crate) fn replace_file_atomic(source: &Path, target: &Path) -> std::io::Result<()> {
    if take_forced_replace_failure() {
        return Err(std::io::Error::other("forced atomic replace failure"));
    }
    std::fs::rename(source, target)
}

/// Windows `std::fs::rename` does not replace an existing target. MoveFileExW
/// supplies the same atomic replace boundary as POSIX rename.
#[cfg(windows)]
pub(crate) fn replace_file_atomic(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    if take_forced_replace_failure() {
        return Err(std::io::Error::other("forced atomic replace failure"));
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Maps to CC `utils/fsOperations.ts#readFileRange`.
pub fn read_file_range(
    path: &Path,
    offset: u64,
    max_bytes: usize,
) -> std::io::Result<Option<ReadFileRangeResult>> {
    let mut file = open_regular_file_no_follow(path, false)?;
    let bytes_total = file.metadata()?.len();
    if bytes_total <= offset {
        return Ok(None);
    }

    file.seek(SeekFrom::Start(offset))?;
    let bytes_to_read = ((bytes_total - offset) as usize).min(max_bytes);
    let mut bytes = vec![0; bytes_to_read];
    let mut bytes_read = 0usize;
    while bytes_read < bytes_to_read {
        let count = file.read(&mut bytes[bytes_read..])?;
        if count == 0 {
            break;
        }
        bytes_read += count;
    }
    bytes.truncate(bytes_read);
    crate::utils::task::disk_output::trim_partial_utf8_boundaries(&mut bytes, offset > 0);
    Ok(Some(ReadFileRangeResult {
        content: String::from_utf8_lossy(&bytes).into_owned(),
        bytes_read,
        bytes_total,
    }))
}

/// Maps to CC `utils/fsOperations.ts#tailFile`.
pub fn tail_file(path: &Path, max_bytes: usize) -> std::io::Result<ReadFileRangeResult> {
    let mut file = open_regular_file_no_follow(path, false)?;
    let bytes_total = file.metadata()?.len();
    if bytes_total == 0 {
        return Ok(ReadFileRangeResult::default());
    }

    let offset = bytes_total.saturating_sub(max_bytes as u64);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::with_capacity((bytes_total - offset) as usize);
    file.read_to_end(&mut bytes)?;
    let bytes_read = bytes.len();
    crate::utils::task::disk_output::trim_partial_utf8_boundaries(&mut bytes, offset > 0);
    Ok(ReadFileRangeResult {
        content: String::from_utf8_lossy(&bytes).into_owned(),
        bytes_read,
        bytes_total,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn duplicate_path_reuses_safe_resolution_and_remembers_missing_paths() {
        let root = std::env::temp_dir().join(format!("plugin-dedup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("file");
        std::fs::write(&file, "").unwrap();
        let mut seen = std::collections::HashSet::new();
        assert!(!super::is_duplicate_path(&file, &mut seen));
        assert!(super::is_duplicate_path(&file, &mut seen));
        #[cfg(unix)]
        {
            let alias = root.join("alias");
            std::os::unix::fs::symlink(&file, &alias).unwrap();
            assert!(super::is_duplicate_path(&alias, &mut seen));
        }
        let missing = root.join("missing");
        assert!(!super::is_duplicate_path(&missing, &mut seen));
        assert!(super::is_duplicate_path(&missing, &mut seen));
        // Source Set<string> preserves spelling when safeResolvePath falls back.
        let trailing = std::path::PathBuf::from(format!("{}/", missing.display()));
        let dot = std::path::PathBuf::from(format!("{}/.", missing.display()));
        assert!(!super::is_duplicate_path(&trailing, &mut seen));
        assert!(!super::is_duplicate_path(&dot, &mut seen));
        assert!(super::is_duplicate_path(&trailing, &mut seen));
        std::fs::remove_dir_all(root).unwrap();
    }

    use super::*;

    #[tokio::test]
    async fn mkdir_matches_official_recursive_mode_and_eexist_catch() {
        let root = std::env::temp_dir().join(format!("mkdir-parity-{}", uuid::Uuid::new_v4()));
        let nested = root.join("nested/leaf");
        mkdir(&nested, Some(0o700)).await.unwrap();
        assert!(nested.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        mkdir(&nested, None).await.unwrap();
        let file = root.join("ordinary-file");
        std::fs::write(&file, "unchanged").unwrap();
        // CC swallows EEXIST even when the target is a plain file on macOS.
        mkdir(&file, None).await.unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "unchanged");
        // A non-directory ancestor yields ENOTDIR, which is not swallowed.
        let error = mkdir(&file.join("child"), None).await.unwrap_err();
        assert_eq!(crate::utils::errors::io_errno_code(&error), Some("ENOTDIR"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn readdir_matches_official_node_filename_order_and_entry_types() {
        // fsOperations.ts:398-400 delegates to Node withFileTypes. The local
        // Node oracle includes mixed case and non-ASCII names, not locale sort.
        let root = std::env::temp_dir().join(format!("readdir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for name in ["z-dir", "é-dir", "a-lower", "A-upper"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        std::fs::write(root.join("file"), "").unwrap();
        let entries = readdir(&root).await.unwrap();
        let names: Vec<_> = entries.iter().map(|entry| entry.file_name()).collect();
        assert_eq!(names, ["A-upper", "a-lower", "file", "z-dir", "é-dir"]);
        assert!(entries[0].file_type().await.unwrap().is_dir());
        assert!(entries[2].file_type().await.unwrap().is_file());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            readdir(&root).await.unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    #[cfg(unix)]
    fn safe_resolve_path_matches_official_literal_path_spelling() {
        // CC fsOperations.ts:166 compares JS strings, including trailing / and /.
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        for suffix in ["/", "/."] {
            let spelling = format!("{}{suffix}", cwd.display());
            let result = safe_resolve_path(Path::new(&spelling));
            assert!(result.is_symlink);
            assert_eq!(result.resolved_path, cwd);
        }
    }

    #[test]
    fn safe_resolve_path_matches_official_nfc_realpath() {
        // CC FsOperations.realpathSync:523-525 normalizes before comparison.
        let root = std::env::temp_dir().join(format!("realpath-nfc-{}", uuid::Uuid::new_v4()));
        let decomposed = root.join("cafe\u{301}");
        std::fs::create_dir_all(&decomposed).unwrap();
        let raw = decomposed.canonicalize().unwrap();
        let expected = PathBuf::from(raw.to_string_lossy().nfc().collect::<String>());
        let result = safe_resolve_path(&decomposed);
        assert_eq!(result.resolved_path.as_os_str(), expected.as_os_str());
        assert!(result.is_symlink);
        assert!(result.is_canonical);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_file_bytes_max_bytes_matches_official_prefix_read() {
        let path = std::env::temp_dir().join(format!(
            "cometix-read-file-bytes-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, b"abcdef").unwrap();
        assert_eq!(read_file_bytes(&path, None).unwrap(), b"abcdef");
        assert_eq!(read_file_bytes(&path, Some(3.9)).unwrap(), b"abc");
        assert_eq!(read_file_bytes(&path, Some(20.0)).unwrap(), b"abcdef");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bounded_ranges_preserve_utf8_boundaries() {
        let path = std::env::temp_dir().join(format!(
            "cometix-fs-range-unicode-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, "😀abc😀").unwrap();
        let prefix = read_file_range(&path, 0, 2).unwrap().unwrap();
        assert!(prefix.content.is_empty());
        assert_eq!(prefix.bytes_read, 2);
        assert_eq!(prefix.bytes_total, 11);
        let tail = tail_file(&path, 9).unwrap();
        assert_eq!(tail.content, "abc😀");
        assert!(!tail.content.contains('\u{fffd}'));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn atomic_replace_overwrites_existing_target() {
        let root = std::env::temp_dir().join(format!(
            "cometix-fs-atomic-replace-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        std::fs::write(&source, "new").unwrap();
        std::fs::write(&target, "old").unwrap();
        replace_file_atomic(&source, &target).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn permission_paths_include_parent_symlink_destination_for_new_files() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "cometix-fs-permission-paths-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let logical = root.join("logical");
        let target = root.join("target");
        std::fs::create_dir_all(&target).unwrap();
        symlink(&target, &logical).unwrap();
        let input = logical.join("new.txt");
        let paths = get_paths_for_permission_check(&input);
        assert_eq!(paths.first(), Some(&input));
        let resolved_target = target.canonicalize().unwrap().join("new.txt");
        assert!(paths.iter().any(|path| path == &resolved_target));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn bounded_readers_reject_symlink_paths() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "cometix-fs-range-symlink-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let victim = root.join("victim");
        let link = root.join("output");
        std::fs::write(&victim, "must-not-be-read").unwrap();
        symlink(&victim, &link).unwrap();
        assert_eq!(
            read_file_range(&link, 0, 1024).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            tail_file(&link, 1024).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(std::fs::read_to_string(victim).unwrap(), "must-not-be-read");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn rm_matches_official_files_trees_force_and_symlinks() {
        // CC fsOperations.ts:410-412 delegates to fs/promises.rm. Fresh Bun
        // oracle: plugin-marketplace-git-0914/fs-rm-oracle.json. No trailing
        // separators/nonrecursive directories are asserted by this native slice.
        let root = std::env::temp_dir().join(format!("cometix-rm-{}", uuid::Uuid::new_v4()));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        std::fs::create_dir_all(&root).unwrap();
        let _cleanup = Cleanup(root.clone());
        let file = root.join("file");
        std::fs::write(&file, "value").unwrap();
        rm(&file, false, false).await.unwrap();
        assert!(!file.exists());
        assert_eq!(
            rm(&file, true, false).await.unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        rm(&file, true, true).await.unwrap();
        let tree = root.join("tree");
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("nested/value"), "value").unwrap();
        rm(&tree, true, false).await.unwrap();
        assert!(!tree.exists());
        std::fs::write(&file, "parent").unwrap();
        assert_eq!(
            rm(&file.join("child"), true, true)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotADirectory
        );
        #[cfg(unix)]
        {
            let target = root.join("target");
            std::fs::create_dir(&target).unwrap();
            std::fs::write(target.join("kept"), "keep").unwrap();
            let link = root.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            rm(&link, true, false).await.unwrap();
            assert!(std::fs::symlink_metadata(&link).is_err());
            assert_eq!(
                std::fs::read_to_string(target.join("kept")).unwrap(),
                "keep"
            );
            std::os::unix::fs::symlink(root.join("missing"), &link).unwrap();
            rm(&link, true, false).await.unwrap();
            assert!(std::fs::symlink_metadata(&link).is_err());
        }
    }
}
