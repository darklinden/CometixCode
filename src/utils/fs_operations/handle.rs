//! Explicit finally/async-dispose close semantics for source-owned file handles.
use std::{fs::File, io};
pub(super) fn finish<T>(file: File, result: io::Result<T>) -> io::Result<T> {
    close(file)
        .map_err(|error| super::error::native(error, "close", std::path::Path::new(""), None))?;
    result
}
pub(super) fn close(file: File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::IntoRawFd;
        let fd = file.into_raw_fd();
        // SAFETY: ownership was transferred out of File; close exactly once.
        // Never retry EINTR: the fd may have been reused by another thread.
        if unsafe { libc::close(fd) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::IntoRawHandle;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        // SAFETY: this is the single close of the transferred owned handle.
        if unsafe { CloseHandle(file.into_raw_handle()) } != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
