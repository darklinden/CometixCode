//! Constructible value carriers for Node Stats and Dirent. No host handle is
//! needed to implement FsOperations for a virtual filesystem.
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FsFileType {
    File,
    Directory,
    Symlink,
    Fifo,
    Socket,
    CharacterDevice,
    BlockDevice,
    #[default]
    Unknown,
}
impl FsFileType {
    pub fn is_file(self) -> bool {
        self == Self::File
    }
    pub fn is_dir(self) -> bool {
        self == Self::Directory
    }
    pub fn is_symlink(self) -> bool {
        self == Self::Symlink
    }
    pub fn is_fifo(self) -> bool {
        self == Self::Fifo
    }
    pub fn is_socket(self) -> bool {
        self == Self::Socket
    }
    pub fn is_char_device(self) -> bool {
        self == Self::CharacterDevice
    }
    pub fn is_block_device(self) -> bool {
        self == Self::BlockDevice
    }
}
impl From<std::fs::FileType> for FsFileType {
    fn from(value: std::fs::FileType) -> Self {
        if value.is_file() {
            return Self::File;
        }
        if value.is_dir() {
            return Self::Directory;
        }
        if value.is_symlink() {
            return Self::Symlink;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            if value.is_fifo() {
                return Self::Fifo;
            }
            if value.is_socket() {
                return Self::Socket;
            }
            if value.is_char_device() {
                return Self::CharacterDevice;
            }
            if value.is_block_device() {
                return Self::BlockDevice;
            }
        }
        Self::Unknown
    }
}
/// Maps to the Node Stats value returned by stat/lstat. Times are epoch ms.
#[derive(Clone, Debug, Default)]
pub struct FsStats {
    pub kind: FsFileType,
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u64,
    pub size: u64,
    pub blksize: u64,
    pub blocks: u64,
    pub atime_ms: f64,
    pub mtime_ms: f64,
    pub ctime_ms: f64,
    pub birthtime_ms: f64,
}
impl FsStats {
    pub fn file_type(&self) -> FsFileType {
        self.kind
    }
    pub fn is_file(&self) -> bool {
        self.kind.is_file()
    }
    pub fn is_dir(&self) -> bool {
        self.kind.is_dir()
    }
    pub fn len(&self) -> u64 {
        self.size
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    pub fn modified(&self) -> io::Result<SystemTime> {
        time(self.mtime_ms)
    }
    pub fn accessed(&self) -> io::Result<SystemTime> {
        time(self.atime_ms)
    }
    pub fn created(&self) -> io::Result<SystemTime> {
        time(self.birthtime_ms)
    }
}
fn time(ms: f64) -> io::Result<SystemTime> {
    if !ms.is_finite() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid timestamp",
        ));
    }
    let duration = Duration::try_from_secs_f64(ms.abs() / 1000.).map_err(io::Error::other)?;
    (if ms < 0. {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    })
    .ok_or_else(|| io::Error::other("timestamp out of range"))
}
fn milliseconds(time: io::Result<SystemTime>) -> f64 {
    match time.unwrap_or(UNIX_EPOCH).duration_since(UNIX_EPOCH) {
        Ok(value) => value.as_secs_f64() * 1000.,
        Err(value) => -value.duration().as_secs_f64() * 1000.,
    }
}
impl From<std::fs::Metadata> for FsStats {
    fn from(value: std::fs::Metadata) -> Self {
        let mut result = Self {
            kind: value.file_type().into(),
            size: value.len(),
            atime_ms: milliseconds(value.accessed()),
            mtime_ms: milliseconds(value.modified()),
            birthtime_ms: milliseconds(value.created()),
            ..Self::default()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            result.dev = value.dev();
            result.ino = value.ino();
            result.mode = value.mode();
            result.nlink = value.nlink();
            result.uid = value.uid();
            result.gid = value.gid();
            result.rdev = value.rdev();
            result.blksize = value.blksize();
            result.blocks = value.blocks();
            result.ctime_ms = value.ctime() as f64 * 1000. + value.ctime_nsec() as f64 / 1e6;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            result.mode = if value.is_dir() {
                0o040000 | 0o111
            } else {
                0o100000
            } | if value.permissions().readonly() {
                0o444
            } else {
                0o666
            };
            result.ctime_ms = result.birthtime_ms;
            result.blocks = (value.file_size() + 511) / 512;
            result.blksize = 4096;
        }
        result
    }
}
/// Maps to Node Dirent (name, parentPath and type predicates).
#[derive(Clone, Debug)]
pub struct FsDirent {
    pub name: OsString,
    pub parent_path: PathBuf,
    pub kind: FsFileType,
}
impl FsDirent {
    pub fn file_name(&self) -> OsString {
        self.name.clone()
    }
    pub fn path(&self) -> PathBuf {
        self.parent_path.join(&self.name)
    }
    pub fn file_type(&self) -> io::Result<FsFileType> {
        Ok(self.kind)
    }
    pub(super) fn native(entry: std::fs::DirEntry, parent: &Path) -> io::Result<Self> {
        Ok(Self {
            name: entry.file_name(),
            parent_path: parent.to_owned(),
            kind: entry.file_type()?.into(),
        })
    }
}
