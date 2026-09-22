//! Node filesystem errors retain code, operation, paths and native cause.
use std::{
    fmt, io,
    path::{Path, PathBuf},
};
#[derive(Debug)]
pub struct FsError {
    pub code: &'static str,
    pub syscall: Option<&'static str>,
    pub path: Option<PathBuf>,
    pub dest: Option<PathBuf>,
    pub message: String,
    pub native: Option<io::Error>,
}
impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for FsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.native.as_ref().map(|error| error as _)
    }
}
pub(super) fn native(
    error: io::Error,
    syscall: &'static str,
    path: &Path,
    dest: Option<&Path>,
) -> io::Error {
    if error.get_ref().is_some_and(|inner| inner.is::<FsError>()) {
        return error;
    }
    let code = crate::utils::errors::io_errno_code(&error).unwrap_or("EIO");
    let path = (!matches!(syscall, "read" | "write" | "fstat" | "close")).then_some(path);
    let mut message = crate::utils::errors::format_native_file_error(&error, syscall, path);
    if let Some(dest) = dest {
        message.push_str(&format!(" -> '{}'", dest.display()));
    }
    io::Error::new(
        error.kind(),
        FsError {
            code,
            syscall: Some(syscall),
            path: path.map(Path::to_owned),
            dest: dest.map(Path::to_owned),
            message,
            native: Some(error),
        },
    )
}
pub(super) fn argument(code: &'static str, message: String) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        FsError {
            code,
            syscall: None,
            path: None,
            dest: None,
            message,
            native: None,
        },
    )
}
