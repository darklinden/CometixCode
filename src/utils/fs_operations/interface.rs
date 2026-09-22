use super::{FsDirent, FsStats};
// Maps to CC `utils/fsOperations.ts:23-126,384-631`.
use futures::future::BoxFuture;
use std::{
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, LazyLock, RwLock},
};
use tokio::io::AsyncWrite;

pub use super::encoding::{BufferEncoding, FsText};
/// Source rm options; omitted booleans are false.
#[derive(Clone, Copy, Debug, Default)]
pub struct RmOptions {
    pub recursive: bool,
    pub force: bool,
}
/// Source symlinkSync's optional Node type argument.
#[derive(Clone, Copy, Debug)]
pub enum SymlinkType {
    File,
    Dir,
    Junction,
}
/// Source readSync returns the allocated buffer, including its unread zero tail.
#[derive(Debug)]
pub struct ReadSyncResult {
    pub buffer: Vec<u8>,
    pub bytes_read: usize,
}
/// Node WriteStream's ordered byte writes, flush/end and asynchronous errors.
pub type FsWriteStream = Pin<Box<dyn AsyncWrite + Send>>;

/// Object-safe carrier of the complete source FsOperations interface.
/// No method silently falls back to the host when an implementation is installed.
pub trait FsOperations: Send + Sync {
    /// Maps to CC `NodeFsOperations:385`.
    fn cwd(&self) -> io::Result<PathBuf>;
    /// Maps to CC `NodeFsOperations:389`.
    fn exists_sync(&self, path: &Path) -> bool;
    /// Maps to CC `NodeFsOperations:394`.
    fn stat(&self, path: &Path) -> BoxFuture<'static, io::Result<FsStats>>;
    /// Maps to CC `NodeFsOperations:398`.
    fn readdir(&self, path: &Path) -> BoxFuture<'static, io::Result<Vec<FsDirent>>>;
    /// Maps to CC `NodeFsOperations:402`.
    fn unlink(&self, path: &Path) -> BoxFuture<'static, io::Result<()>>;
    /// Maps to CC `NodeFsOperations:406`.
    fn rmdir(&self, path: &Path) -> BoxFuture<'static, io::Result<()>>;
    /// Maps to CC `NodeFsOperations:410`.
    fn rm(&self, path: &Path, options: RmOptions) -> BoxFuture<'static, io::Result<()>>;
    /// Maps to CC `NodeFsOperations:414`.
    fn mkdir(&self, path: &Path, mode: Option<u32>) -> BoxFuture<'static, io::Result<()>>;
    /// Maps to CC `NodeFsOperations:427`.
    fn read_file(
        &self,
        path: &Path,
        encoding: BufferEncoding,
    ) -> BoxFuture<'static, io::Result<FsText>>;
    /// Maps to CC `NodeFsOperations:431`.
    fn rename(&self, source: &Path, target: &Path) -> BoxFuture<'static, io::Result<()>>;
    /// Maps to CC `NodeFsOperations:435`.
    fn stat_sync(&self, path: &Path) -> io::Result<FsStats>;
    /// Maps to CC `NodeFsOperations:440`.
    fn lstat_sync(&self, path: &Path) -> io::Result<FsStats>;
    /// Maps to CC `NodeFsOperations:445`.
    fn read_file_sync(&self, path: &Path, encoding: BufferEncoding) -> io::Result<FsText>;
    /// Maps to CC `NodeFsOperations:450`.
    fn read_file_bytes_sync(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Maps to CC `NodeFsOperations:455`.
    fn read_sync(&self, path: &Path, length: usize) -> io::Result<ReadSyncResult>;
    /// Maps to CC `NodeFsOperations:468`.
    fn append_file_sync(&self, path: &Path, data: &FsText, mode: Option<u32>) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:489`.
    fn copy_file_sync(&self, source: &Path, target: &Path) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:494`.
    fn unlink_sync(&self, path: &Path) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:499`.
    fn rename_sync(&self, source: &Path, target: &Path) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:504`.
    fn link_sync(&self, target: &Path, path: &Path) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:509`.
    fn symlink_sync(&self, target: &Path, path: &Path, kind: Option<SymlinkType>)
    -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:518`.
    fn readlink_sync(&self, path: &Path) -> io::Result<PathBuf>;
    /// Maps to CC `NodeFsOperations:523`.
    fn realpath_sync(&self, path: &Path) -> io::Result<PathBuf>;
    /// Maps to CC `NodeFsOperations:528`.
    fn mkdir_sync(&self, path: &Path, mode: Option<u32>) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:548`.
    fn readdir_sync(&self, path: &Path) -> io::Result<Vec<FsDirent>>;
    /// Maps to CC `NodeFsOperations:553`.
    fn readdir_string_sync(&self, path: &Path) -> io::Result<Vec<std::ffi::OsString>>;
    /// Maps to CC `NodeFsOperations:558`.
    fn is_dir_empty_sync(&self, path: &Path) -> io::Result<bool>;
    /// Maps to CC `NodeFsOperations:564`.
    fn rmdir_sync(&self, path: &Path) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:569`.
    fn rm_sync(&self, path: &Path, options: RmOptions) -> io::Result<()>;
    /// Maps to CC `NodeFsOperations:574`.
    fn create_write_stream(&self, path: &Path) -> FsWriteStream;
    /// Maps to CC `NodeFsOperations:578`.
    fn read_file_bytes(
        &self,
        path: &Path,
        max_bytes: Option<f64>,
    ) -> BoxFuture<'static, io::Result<Vec<u8>>>;
}

/// Maps to CC `NodeFsOperations`.
#[derive(Default)]
pub struct NodeFsOperations;
impl FsOperations for NodeFsOperations {
    fn cwd(&self) -> io::Result<PathBuf> {
        super::native::cwd()
    }
    fn exists_sync(&self, path: &Path) -> bool {
        super::native::exists_sync(path)
    }
    fn stat(&self, path: &Path) -> BoxFuture<'static, io::Result<FsStats>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::stat(&path)
                .await
                .map_err(|error| super::error::native(error, "stat", &path, None))
        })
    }
    fn readdir(&self, path: &Path) -> BoxFuture<'static, io::Result<Vec<FsDirent>>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::readdir(&path)
                .await
                .map_err(|error| super::error::native(error, "scandir", &path, None))
        })
    }
    fn unlink(&self, path: &Path) -> BoxFuture<'static, io::Result<()>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::unlink(&path)
                .await
                .map_err(|error| super::error::native(error, "unlink", &path, None))
        })
    }
    fn rmdir(&self, path: &Path) -> BoxFuture<'static, io::Result<()>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::rmdir(&path)
                .await
                .map_err(|error| super::error::native(error, "rmdir", &path, None))
        })
    }
    fn rm(&self, path: &Path, options: RmOptions) -> BoxFuture<'static, io::Result<()>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::rm(&path, options)
                .await
                .map_err(|error| super::error::native(error, "rm", &path, None))
        })
    }
    fn mkdir(&self, path: &Path, mode: Option<u32>) -> BoxFuture<'static, io::Result<()>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::mkdir(&path, mode)
                .await
                .map_err(|error| super::error::native(error, "mkdir", &path, None))
        })
    }
    fn read_file(
        &self,
        path: &Path,
        encoding: BufferEncoding,
    ) -> BoxFuture<'static, io::Result<FsText>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::read_file(&path, encoding)
                .await
                .map_err(|error| super::error::native(error, "open", &path, None))
        })
    }
    fn rename(&self, source: &Path, target: &Path) -> BoxFuture<'static, io::Result<()>> {
        let source = source.to_owned();
        let target = target.to_owned();
        super::eager::start(async move {
            super::native::rename(&source, &target)
                .await
                .map_err(|error| super::error::native(error, "rename", &source, Some(&target)))
        })
    }
    fn stat_sync(&self, path: &Path) -> io::Result<FsStats> {
        super::native::stat_sync(path)
            .map_err(|error| super::error::native(error, "stat", path, None))
    }
    fn lstat_sync(&self, path: &Path) -> io::Result<FsStats> {
        super::native::lstat_sync(path)
            .map_err(|error| super::error::native(error, "lstat", path, None))
    }
    fn read_file_sync(&self, path: &Path, encoding: BufferEncoding) -> io::Result<FsText> {
        super::native::read_file_sync(path, encoding)
            .map_err(|error| super::error::native(error, "open", path, None))
    }
    fn read_file_bytes_sync(&self, path: &Path) -> io::Result<Vec<u8>> {
        super::native::read_file_bytes_sync(path)
            .map_err(|error| super::error::native(error, "open", path, None))
    }
    fn read_sync(&self, path: &Path, length: usize) -> io::Result<ReadSyncResult> {
        super::native::read_sync(path, length)
            .map_err(|error| super::error::native(error, "read", path, None))
    }
    fn append_file_sync(&self, path: &Path, data: &FsText, mode: Option<u32>) -> io::Result<()> {
        super::native::append_file_sync(path, data, mode)
            .map_err(|error| super::error::native(error, "open", path, None))
    }
    fn copy_file_sync(&self, source: &Path, target: &Path) -> io::Result<()> {
        super::native::copy_file_sync(source, target)
            .map_err(|error| super::error::native(error, "copyfile", source, Some(target)))
    }
    fn unlink_sync(&self, path: &Path) -> io::Result<()> {
        super::native::unlink_sync(path)
            .map_err(|error| super::error::native(error, "unlink", path, None))
    }
    fn rename_sync(&self, source: &Path, target: &Path) -> io::Result<()> {
        super::native::rename_sync(source, target)
            .map_err(|error| super::error::native(error, "rename", source, Some(target)))
    }
    fn link_sync(&self, target: &Path, path: &Path) -> io::Result<()> {
        super::native::link_sync(target, path)
            .map_err(|error| super::error::native(error, "link", target, Some(path)))
    }
    fn symlink_sync(
        &self,
        target: &Path,
        path: &Path,
        kind: Option<SymlinkType>,
    ) -> io::Result<()> {
        super::native::symlink_sync(target, path, kind)
            .map_err(|error| super::error::native(error, "symlink", target, Some(path)))
    }
    fn readlink_sync(&self, path: &Path) -> io::Result<PathBuf> {
        super::native::readlink_sync(path)
            .map_err(|error| super::error::native(error, "readlink", path, None))
    }
    fn realpath_sync(&self, path: &Path) -> io::Result<PathBuf> {
        super::native::realpath_sync(path)
            .map_err(|error| super::error::native(error, "realpath", path, None))
    }
    fn mkdir_sync(&self, path: &Path, mode: Option<u32>) -> io::Result<()> {
        super::native::mkdir_sync(path, mode)
            .map_err(|error| super::error::native(error, "mkdir", path, None))
    }
    fn readdir_sync(&self, path: &Path) -> io::Result<Vec<FsDirent>> {
        super::native::readdir_sync(path)
            .map_err(|error| super::error::native(error, "scandir", path, None))
    }
    fn readdir_string_sync(&self, path: &Path) -> io::Result<Vec<std::ffi::OsString>> {
        super::native::readdir_string_sync(path)
            .map_err(|error| super::error::native(error, "scandir", path, None))
    }
    fn is_dir_empty_sync(&self, path: &Path) -> io::Result<bool> {
        Ok(self.readdir_sync(path)?.is_empty())
    }
    fn rmdir_sync(&self, path: &Path) -> io::Result<()> {
        super::native::rmdir_sync(path)
            .map_err(|error| super::error::native(error, "rmdir", path, None))
    }
    fn rm_sync(&self, path: &Path, options: RmOptions) -> io::Result<()> {
        super::native::rm_sync(path, options)
            .map_err(|error| super::error::native(error, "rm", path, None))
    }
    fn create_write_stream(&self, path: &Path) -> FsWriteStream {
        super::native::create_write_stream(path)
    }
    fn read_file_bytes(
        &self,
        path: &Path,
        max_bytes: Option<f64>,
    ) -> BoxFuture<'static, io::Result<Vec<u8>>> {
        let path = path.to_owned();
        super::eager::start(async move {
            super::native::read_file_bytes(&path, max_bytes)
                .await
                .map_err(|error| super::error::native(error, "open", &path, None))
        })
    }
}

static ORIGINAL_FS: LazyLock<Arc<dyn FsOperations>> = LazyLock::new(|| Arc::new(NodeFsOperations));

static ACTIVE_FS: LazyLock<RwLock<Arc<dyn FsOperations>>> =
    LazyLock::new(|| RwLock::new(ORIGINAL_FS.clone()));
/// Maps to CC `setFsImplementation`; does not change process cwd.
pub fn set_fs_implementation(implementation: Arc<dyn FsOperations>) {
    *ACTIVE_FS.write().unwrap_or_else(|e| e.into_inner()) = implementation;
}
/// Maps to CC `getFsImplementation`; cloned Arc retains source object identity.
pub fn get_fs_implementation() -> Arc<dyn FsOperations> {
    ACTIVE_FS.read().unwrap_or_else(|e| e.into_inner()).clone()
}
/// Maps to CC `setOriginalFsImplementation`; does not change process cwd.
pub fn set_original_fs_implementation() {
    set_fs_implementation(ORIGINAL_FS.clone());
}
