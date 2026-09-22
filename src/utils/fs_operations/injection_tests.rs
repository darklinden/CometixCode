//! Pure virtual backend tests; no host Metadata/DirEntry fabrication.
use super::*;
use futures::future::BoxFuture;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
struct VirtualFs {
    calls: Mutex<Vec<&'static str>>,
}
impl VirtualFs {
    fn record(&self, method: &'static str) {
        self.calls.lock().unwrap().push(method);
    }
}
#[allow(unused_variables)]
impl FsOperations for VirtualFs {
    fn cwd(&self) -> io::Result<PathBuf> {
        self.record("cwd");
        Ok(PathBuf::from("/virtual"))
    }
    fn exists_sync(&self, path: &Path) -> bool {
        self.record("exists_sync");
        path == Path::new("/virtual/file")
    }
    fn stat(&self, path: &Path) -> BoxFuture<'static, io::Result<FsStats>> {
        self.record("stat");
        if path.file_name().is_some_and(|name| name == "async-deleted") {
            return Box::pin(async { Err(io::ErrorKind::NotFound.into()) });
        }
        if path.file_name().is_some_and(|name| name == "async-pending") {
            return Box::pin(futures::future::pending());
        }
        if path.file_name().is_some_and(|name| name == "large.pdf") {
            return Box::pin(async {
                Ok(FsStats {
                    size: (crate::constants::api_limits::PDF_AT_MENTION_INLINE_THRESHOLD + 1)
                        * 100
                        * 1024,
                    ..FsStats::default()
                })
            });
        }
        if path.file_name().is_some_and(|name| name == "async-cached") {
            return Box::pin(async {
                Ok(FsStats {
                    mtime_ms: 42.0,
                    ..FsStats::default()
                })
            });
        }
        if path.file_name().is_some_and(|name| name == "async-mtime") {
            return Box::pin(async {
                Ok(FsStats {
                    mtime_ms: 1.25,
                    ..FsStats::default()
                })
            });
        }
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "virtual backend",
            ))
        })
    }
    fn readdir(&self, path: &Path) -> BoxFuture<'static, io::Result<Vec<FsDirent>>> {
        self.record("readdir");
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "virtual backend",
            ))
        })
    }
    fn unlink(&self, path: &Path) -> BoxFuture<'static, io::Result<()>> {
        self.record("unlink");
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "virtual backend",
            ))
        })
    }
    fn rmdir(&self, path: &Path) -> BoxFuture<'static, io::Result<()>> {
        self.record("rmdir");
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "virtual backend",
            ))
        })
    }
    fn rm(&self, path: &Path, options: RmOptions) -> BoxFuture<'static, io::Result<()>> {
        self.record("rm");
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "virtual backend",
            ))
        })
    }
    fn mkdir(&self, path: &Path, mode: Option<u32>) -> BoxFuture<'static, io::Result<()>> {
        self.record("mkdir");
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "virtual backend",
            ))
        })
    }
    fn read_file(
        &self,
        path: &Path,
        encoding: BufferEncoding,
    ) -> BoxFuture<'static, io::Result<FsText>> {
        self.record("read_file");
        Box::pin(async { Ok(FsText::from("virtual")) })
    }
    fn rename(&self, source: &Path, target: &Path) -> BoxFuture<'static, io::Result<()>> {
        self.record("rename");
        Box::pin(async {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "virtual backend",
            ))
        })
    }
    fn stat_sync(&self, path: &Path) -> io::Result<FsStats> {
        self.record("stat_sync");
        Ok(FsStats {
            kind: FsFileType::File,
            size: 6,
            ..FsStats::default()
        })
    }
    fn lstat_sync(&self, path: &Path) -> io::Result<FsStats> {
        self.record("lstat_sync");
        #[cfg(windows)]
        if path.starts_with(r"C:\node-join-fixture") {
            return if path == Path::new(r"C:\node-join-fixture") {
                Ok(FsStats {
                    kind: FsFileType::Symlink,
                    ..FsStats::default()
                })
            } else {
                Err(io::ErrorKind::NotFound.into())
            };
        }
        Ok(FsStats {
            kind: FsFileType::File,
            size: 6,
            ..FsStats::default()
        })
    }
    fn read_file_sync(&self, path: &Path, encoding: BufferEncoding) -> io::Result<FsText> {
        self.record("read_file_sync");
        Ok(FsText::from("virtual"))
    }
    fn read_file_bytes_sync(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.record("read_file_bytes_sync");
        Ok(b"virtual".to_vec())
    }
    fn read_sync(&self, path: &Path, length: usize) -> io::Result<ReadSyncResult> {
        self.record("read_sync");
        if path.file_name().is_some_and(|name| name == "probe-denied") {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if path.file_name().is_some_and(|name| name == "probe-missing") {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(ReadSyncResult {
            buffer: vec![0; length],
            bytes_read: 0,
        })
    }
    fn append_file_sync(&self, path: &Path, data: &FsText, mode: Option<u32>) -> io::Result<()> {
        self.record("append_file_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn copy_file_sync(&self, source: &Path, target: &Path) -> io::Result<()> {
        self.record("copy_file_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn unlink_sync(&self, path: &Path) -> io::Result<()> {
        self.record("unlink_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn rename_sync(&self, source: &Path, target: &Path) -> io::Result<()> {
        self.record("rename_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn link_sync(&self, target: &Path, path: &Path) -> io::Result<()> {
        self.record("link_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn symlink_sync(
        &self,
        target: &Path,
        path: &Path,
        kind: Option<SymlinkType>,
    ) -> io::Result<()> {
        self.record("symlink_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn readlink_sync(&self, path: &Path) -> io::Result<PathBuf> {
        self.record("readlink_sync");
        #[cfg(windows)]
        if path == Path::new(r"C:\node-join-fixture") {
            return Ok(PathBuf::from("C:\\"));
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn realpath_sync(&self, path: &Path) -> io::Result<PathBuf> {
        self.record("realpath_sync");
        #[cfg(windows)]
        if path == Path::new(r"C:\node-join-fixture") {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(path.to_owned())
    }
    fn mkdir_sync(&self, path: &Path, mode: Option<u32>) -> io::Result<()> {
        self.record("mkdir_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn readdir_sync(&self, path: &Path) -> io::Result<Vec<FsDirent>> {
        self.record("readdir_sync");
        if path.as_os_str() == std::ffi::OsStr::new(".") {
            return Ok(vec![
                FsDirent {
                    name: "a.ts".into(),
                    parent_path: "/wrong".into(),
                    kind: FsFileType::File,
                },
                FsDirent {
                    name: "a.js".into(),
                    parent_path: "/wrong".into(),
                    kind: FsFileType::File,
                },
            ]);
        }
        Ok(vec![FsDirent {
            name: "file".into(),
            parent_path: path.to_owned(),
            kind: FsFileType::File,
        }])
    }
    fn readdir_string_sync(&self, path: &Path) -> io::Result<Vec<std::ffi::OsString>> {
        self.record("readdir_string_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn is_dir_empty_sync(&self, path: &Path) -> io::Result<bool> {
        self.record("is_dir_empty_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn rmdir_sync(&self, path: &Path) -> io::Result<()> {
        self.record("rmdir_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn rm_sync(&self, path: &Path, options: RmOptions) -> io::Result<()> {
        self.record("rm_sync");
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "virtual backend",
        ))
    }
    fn create_write_stream(&self, path: &Path) -> FsWriteStream {
        self.record("create_write_stream");
        Box::pin(tokio::io::sink())
    }
    fn read_file_bytes(
        &self,
        path: &Path,
        max_bytes: Option<f64>,
    ) -> BoxFuture<'static, io::Result<Vec<u8>>> {
        self.record("read_file_bytes");
        Box::pin(async { Ok(b"virtual".to_vec()) })
    }
}
#[test]
fn injected_backend_matches_official_resolution_and_file_read_consumers() {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            set_original_fs_implementation();
        }
    }
    let _reset = Reset;
    let fs = Arc::new(VirtualFs {
        calls: Mutex::new(Vec::new()),
    });
    set_fs_implementation(fs.clone());
    let result =
        crate::utils::file_read::read_file_sync_with_metadata(Path::new("/virtual/file")).unwrap();
    assert_eq!(result.content, "virtual");
    assert_eq!(
        *fs.calls.lock().unwrap(),
        ["lstat_sync", "realpath_sync", "read_sync", "read_file_sync"]
    );
    fs.calls.lock().unwrap().clear();
    assert_eq!(
        get_paths_for_permission_check(Path::new("/virtual/file")),
        [PathBuf::from("/virtual/file")]
    );
    assert_eq!(
        *fs.calls.lock().unwrap(),
        ["exists_sync", "lstat_sync", "lstat_sync", "realpath_sync"]
    );
    let mut paths = std::collections::HashSet::new();
    assert!(!is_duplicate_path(
        fs.as_ref(),
        Path::new("/virtual/file"),
        &mut paths
    ));
    assert!(is_duplicate_path(
        fs.as_ref(),
        Path::new("/virtual/file"),
        &mut paths
    ));
    assert_eq!(
        fs.readdir_sync(Path::new("/virtual")).unwrap()[0].file_name(),
        "file"
    );
}

#[test]
fn expand_path_matches_official_session_cwd_before_injected_fs() {
    // CC utils/path.ts:34 and utils/cwd.ts:19-30; explicit tool cwd override
    // precedes bootstrap session cwd, which precedes the FsOperations fallback.
    struct Reset(PathBuf);
    impl Drop for Reset {
        fn drop(&mut self) {
            set_original_fs_implementation();
            crate::bootstrap::state::set_original_cwd(self.0.clone());
        }
    }
    let _reset = Reset(crate::bootstrap::state::get_original_cwd());
    let cwd = std::env::current_dir().unwrap().join("session-base");
    crate::bootstrap::state::set_original_cwd(&cwd);
    let fs = Arc::new(VirtualFs {
        calls: Mutex::new(Vec::new()),
    });
    set_fs_implementation(fs.clone());
    assert_eq!(
        crate::utils::path::expand_path("a", None).unwrap(),
        cwd.join("a")
    );
    let override_cwd = cwd.join("agent");
    assert_eq!(
        crate::utils::path::expand_path("a", Some(&override_cwd)).unwrap(),
        override_cwd.join("a")
    );
    assert!(fs.calls.lock().unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn ancestor_symlink_tail_matches_official_node_variadic_join() {
    // CC fsOperations.ts:253 joins the returned link target and all missing
    // segments in one call; Node reserved names make repeated join observably different.
    let fs = VirtualFs {
        calls: Mutex::new(Vec::new()),
    };
    let actual =
        resolve_deepest_existing_ancestor_sync(&fs, Path::new(r"C:\node-join-fixture\a\CON:"))
            .unwrap()
            .unwrap();
    assert_eq!(actual.as_os_str(), "C:\\\\a\\CON:");
    assert_eq!(
        *fs.calls.lock().unwrap(),
        [
            "lstat_sync",
            "lstat_sync",
            "lstat_sync",
            "realpath_sync",
            "readlink_sync"
        ]
    );
}

struct RestoreFs;
impl Drop for RestoreFs {
    fn drop(&mut self) {
        set_original_fs_implementation();
    }
}
fn install_virtual_fs() -> (RestoreFs, Arc<VirtualFs>) {
    let fs = Arc::new(VirtualFs {
        calls: Mutex::new(Vec::new()),
    });
    set_fs_implementation(fs.clone());
    (RestoreFs, fs)
}

#[tokio::test]
async fn injected_pdf_matches_official_async_stat_endpoint() {
    let (_reset, fs) = install_virtual_fs();
    let path = Path::new("virtual.pdf");
    let read = crate::utils::pdf::read_pdf(path);
    let extract = crate::utils::pdf::extract_pdf_pages(path, None);
    assert_eq!(*fs.calls.lock().unwrap(), ["stat", "stat"]);
    set_original_fs_implementation();
    let read = read.await.unwrap_err();
    let extract = extract.await.unwrap_err();
    assert_eq!(read.message, "virtual backend");
    assert_eq!(extract.message, "virtual backend");
    // statSync succeeds in this backend; selecting it would reach native read
    // or poppler. Both source functions must instead preserve async failure.
    assert_eq!(*fs.calls.lock().unwrap(), ["stat", "stat"]);
}

#[tokio::test]
async fn injected_sed_preview_matches_official_read_order_and_probe_errors() {
    let (_reset, fs) = install_virtual_fs();
    use crate::components::permissions::sed_edit_permission_request::read_sed_file_content;
    let read = read_sed_file_content("raw/../virtual");
    assert_eq!(*fs.calls.lock().unwrap(), ["read_sync", "read_file"]);
    set_original_fs_implementation();
    assert_eq!(read.await.unwrap().old_content, "virtual");
    set_fs_implementation(fs.clone());
    fs.calls.lock().unwrap().clear();
    let denied = read_sed_file_content("probe-denied").await.unwrap_err();
    assert_eq!(denied.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(*fs.calls.lock().unwrap(), ["read_sync"]);
    fs.calls.lock().unwrap().clear();
    let missing = read_sed_file_content("probe-missing").await.unwrap();
    assert!(!missing.file_exists);
    assert_eq!(*fs.calls.lock().unwrap(), ["read_sync"]);
}

#[tokio::test]
async fn injected_grep_matches_official_all_settled_and_fractional_mtime() {
    let (_reset, fs) = install_virtual_fs();
    use crate::tools::grep_tool::file_match_mtimes;
    let paths = vec!["async-mtime".into(), "missing".into()];
    assert_eq!(file_match_mtimes(&paths).await, vec![1.25, 0.0]);
    assert_eq!(*fs.calls.lock().unwrap(), ["stat", "stat"]);
    fs.calls.lock().unwrap().clear();
    let paths = vec!["async-pending".into(), "missing".into()];
    let pending = file_match_mtimes(&paths);
    futures::pin_mut!(pending);
    assert!(futures::poll!(pending.as_mut()).is_pending());
    // The second stat starts even while the first remains unresolved.
    assert_eq!(*fs.calls.lock().unwrap(), ["stat", "stat"]);
}

#[test]
fn injected_find_similar_matches_official_dirname_join_and_raw_comparison() {
    let (_reset, fs) = install_virtual_fs();
    assert_eq!(
        crate::utils::file::find_similar_file(Path::new("a.ts")),
        Some("a.js".into())
    );
    assert_eq!(
        crate::utils::file::find_similar_file(Path::new("./a.ts")),
        Some("a.ts".into())
    );
    assert_eq!(*fs.calls.lock().unwrap(), ["readdir_sync", "readdir_sync"]);
}

#[tokio::test]
async fn injected_plan_resume_matches_official_eager_backend_capture() {
    let _plan_lock = crate::utils::plans::test_plan_state_lock();
    let (_reset, fs) = install_virtual_fs();
    let session = uuid::Uuid::new_v4().to_string();
    let slug = format!("virtual-plan-{session}");
    let messages = [serde_json::json!({"type": "user", "slug": slug})];
    let copying = crate::utils::plans::copy_plan_for_resume(&messages, Some(&session));
    assert!(fs.calls.lock().unwrap().contains(&"read_file"));
    set_original_fs_implementation();
    assert!(copying.await);
    crate::utils::plans::clear_plan_slug(Some(&session));
}

#[tokio::test]
async fn injected_attachment_matches_official_distinct_stat_branches() {
    use crate::utils::attachments::{
        FileAttachmentMode, GenerateFileAttachmentOptions, generate_file_attachment,
    };
    // Context construction loads settings/tool/plugin metadata. Complete that
    // independent startup against the native backend before recording the
    // attachment's filesystem calls.
    crate::utils::process_runtime::initialize_test_process_runtime();
    let mut context = crate::tool::ToolUseContext::default();
    let mut limits = crate::tools::file_read_tool::limits::get_default_file_reading_limits();
    let _ = crate::utils::plugins::plugin_loader::load_all_plugins_cache_only().await;
    let (_reset, fs) = install_virtual_fs();
    limits.max_size_bytes = 0.0;
    assert!(
        generate_file_attachment(
            "async-mtime",
            &mut context,
            FileAttachmentMode::AtMention,
            GenerateFileAttachmentOptions::default(),
            limits
        )
        .await
        .is_none()
    );
    assert_eq!(*fs.calls.lock().unwrap(), ["stat_sync", "stat"]);

    fs.calls.lock().unwrap().clear();
    limits.max_size_bytes = 100.0;
    let reference = generate_file_attachment(
        "large.pdf",
        &mut context,
        FileAttachmentMode::AtMention,
        GenerateFileAttachmentOptions::default(),
        limits,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&reference.attachment).unwrap()["type"],
        "pdf_reference"
    );
    assert_eq!(*fs.calls.lock().unwrap(), ["stat_sync", "stat"]);

    fs.calls.lock().unwrap().clear();
    context
        .read_file_state
        .set_entry(crate::utils::query_helpers::ReadFileStateEntry {
            path: "async-cached".into(),
            content: Some("cached content".into()),
            timestamp_ms: Some(42),
            offset: None,
            limit: None,
            is_partial_view: false,
            source: crate::utils::query_helpers::ReadFileStateSource::Read,
        });
    let cached = generate_file_attachment(
        "async-cached",
        &mut context,
        FileAttachmentMode::AtMention,
        GenerateFileAttachmentOptions::default(),
        limits,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&cached.attachment).unwrap()["type"],
        "already_read_file"
    );
    assert_eq!(*fs.calls.lock().unwrap(), ["stat_sync", "stat"]);
}

#[tokio::test]
async fn injected_changed_files_matches_official_concurrent_stats_and_eviction() {
    crate::utils::process_runtime::initialize_test_process_runtime();
    let mut context = crate::tool::ToolUseContext::default();
    let _ = crate::utils::plugins::plugin_loader::load_all_plugins_cache_only().await;
    let (_reset, fs) = install_virtual_fs();
    for name in ["async-pending", "async-deleted", "async-mtime"] {
        context
            .read_file_state
            .set_entry(crate::utils::query_helpers::ReadFileStateEntry {
                path: name.into(),
                content: Some("cached".into()),
                timestamp_ms: Some(50),
                offset: None,
                limit: None,
                is_partial_view: false,
                source: crate::utils::query_helpers::ReadFileStateSource::Read,
            });
    }
    let cache = context.read_file_state.clone();
    let pending = crate::utils::attachments::get_changed_files(&mut context);
    futures::pin_mut!(pending);
    assert!(futures::poll!(pending.as_mut()).is_pending());
    assert_eq!(*fs.calls.lock().unwrap(), ["stat", "stat", "stat"]);
    assert!(cache.get(Path::new("async-pending")).is_some());
    assert!(cache.get(Path::new("async-deleted")).is_none());
    assert!(cache.get(Path::new("async-mtime")).is_some());
}
