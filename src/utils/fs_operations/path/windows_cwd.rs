//! Maps to Node v24.14.0 win32.resolve: =X: environment, then process cwd,
//! then a drive-root fallback if the selected cwd belongs to another drive.
use super::windows_lexical::{bytes, native};
use std::{io, path::PathBuf};

pub(crate) fn select(
    drive: Option<&[u8]>,
    mut environment: impl FnMut(&[u8]) -> Option<PathBuf>,
    mut cwd: impl FnMut() -> io::Result<PathBuf>,
) -> io::Result<PathBuf> {
    let Some(drive) = drive else {
        return cwd();
    };
    let candidate = match environment(drive).filter(|p| !p.as_os_str().is_empty()) {
        Some(value) => value,
        None => cwd()?,
    };
    let value = bytes(&candidate);
    if !value
        .get(..2)
        .is_some_and(|v| v.eq_ignore_ascii_case(drive))
        && value.get(2) == Some(&b'\\')
    {
        let mut root = drive.to_vec();
        root.push(b'\\');
        return Ok(native(root));
    }
    Ok(candidate)
}

#[cfg(windows)]
pub(crate) fn get(drive: Option<&[u8]>) -> io::Result<PathBuf> {
    select(drive, drive_environment, std::env::current_dir)
}

#[cfg(windows)]
fn drive_environment(drive: &[u8]) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetEnvironmentVariableW(name: *const u16, buffer: *mut u16, size: u32) -> u32;
    }
    let name = [b'=' as u16, drive[0] as u16, drive[1] as u16, 0];
    let mut buffer = vec![0u16; 256];
    loop {
        // SAFETY: name is terminated; writable buffer has exactly size elements.
        let len = unsafe {
            GetEnvironmentVariableW(name.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32)
        } as usize;
        if len == 0 {
            return None;
        }
        if len < buffer.len() {
            return Some(std::ffi::OsString::from_wide(&buffer[..len]).into());
        }
        buffer.resize(len, 0);
    }
}
