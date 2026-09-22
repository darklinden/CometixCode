//! Windows lexical resolution, independent of std::path's native prefix parser.
//! Maps to Node lib/path.js win32.resolve's right-to-left device/root/tail scan.
//! Node is the compatibility authority, including namespace device roots.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

pub(super) fn sep(b: u8) -> bool {
    b == b'/' || b == b'\\'
}
pub(super) fn bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}
pub(super) fn native(value: Vec<u8>) -> PathBuf {
    // SAFETY: only ASCII separators and whole native-encoded components are
    // inserted/removed. ASCII boundaries cannot split an encoded code point.
    PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(value) })
}

pub(super) struct Root {
    pub(super) device: Vec<u8>,
    pub(super) end: usize,
    pub(super) absolute: bool,
    pub(super) namespace: bool,
}

pub(super) fn root(value: &[u8]) -> Root {
    let mut result = Root {
        device: Vec::new(),
        end: 0,
        absolute: false,
        namespace: false,
    };
    if value.first().is_some_and(|b| sep(*b)) {
        result.absolute = true;
        result.end = 1;
        if value.len() > 2 && sep(value[1]) && !sep(value[2]) {
            let server_end = (2..value.len())
                .find(|&i| sep(value[i]))
                .unwrap_or(value.len());
            let share_start = (server_end..value.len()).find(|&i| !sep(value[i]));
            if let Some(start) = share_start {
                let end = (start..value.len())
                    .find(|&i| sep(value[i]))
                    .unwrap_or(value.len());
                result.device.extend_from_slice(b"\\\\");
                result.device.extend_from_slice(&value[2..server_end]);
                result.device.push(b'\\');
                result.device.extend_from_slice(&value[start..end]);
                result.end = end;
                result.namespace = matches!(&value[2..server_end], b"?" | b".");
                if result.namespace {
                    // Node treats the namespace marker itself as the device;
                    // C:, pipe and UNC/server/share are ordinary tail components.
                    result.device.truncate(3);
                    result.end = 4;
                }
            }
        }
    } else if value.len() >= 2 && value[0].is_ascii_alphabetic() && value[1] == b':' {
        result.device.extend_from_slice(&value[..2]);
        result.end = 2;
        if value.get(2).is_some_and(|b| sep(*b)) {
            result.end = 3;
            result.absolute = true;
        }
    }
    result
}

/// Explicit cwd must carry both an absolute root and a device. Rust's native
/// parser rejects repeated UNC separators that Node accepts; use the same root
/// grammar as resolution, while rejecting drive-relative/root-relative cwd.
pub(super) fn is_fully_qualified(path: &Path) -> bool {
    let parsed = root(bytes(path));
    parsed.absolute && !parsed.device.is_empty()
}

pub(super) fn normalized_tail(value: &[u8], allow_above_root: bool) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for part in value.split(|b| sep(*b)) {
        match part {
            b"" | b"." => {}
            b".." => {
                if parts.last().is_some_and(|p| *p != b"..") {
                    parts.pop();
                } else if allow_above_root {
                    parts.push(part);
                }
            }
            _ => parts.push(part),
        }
    }
    parts.join(&b'\\')
}

pub(crate) fn is_namespace(path: &Path) -> bool {
    root(bytes(path)).namespace
}

/// Namespace entry shared by borrowed/owned normalization and relative callers.
pub(crate) fn normalize_namespace(path: &Path, preserve_trailing: bool) -> Option<PathBuf> {
    if !is_namespace(path) {
        return None;
    }
    Some(if preserve_trailing {
        super::windows_normalize::normalize(path)
    } else {
        resolve_with(&[path], |_| unreachable!("namespace path is absolute")).unwrap()
    })
}

/// Resolve each argument before concatenating tails, so a partial UNC argument
/// cannot acquire a share from the next argument. The callback supplies only cwd
/// state; user-provided tails (including NUL) never reach an OS path resolver.
pub(crate) fn resolve_with(
    args: &[&Path],
    mut cwd: impl FnMut(Option<&[u8]>) -> io::Result<PathBuf>,
) -> io::Result<PathBuf> {
    let mut device = Vec::new();
    let mut tail = Vec::new();
    let mut absolute = false;
    for index in (0..=args.len()).rev() {
        let ambient;
        let input = if index > 0 {
            bytes(args[index - 1])
        } else {
            ambient = cwd(if device.is_empty() {
                None
            } else {
                Some(&device)
            })?;
            bytes(&ambient)
        };
        if input.is_empty() {
            continue;
        }
        let r = root(input);
        if !r.device.is_empty() {
            if !device.is_empty() && !device.eq_ignore_ascii_case(&r.device) {
                continue;
            }
            if device.is_empty() {
                device = r.device.clone();
            }
        }
        if absolute {
            if !device.is_empty() {
                break;
            }
        } else {
            let mut combined = input[r.end..].to_vec();
            combined.push(b'\\');
            combined.extend_from_slice(&tail);
            tail = combined;
            absolute = r.absolute;
            if absolute && !device.is_empty() {
                break;
            }
        }
    }
    let tail = normalized_tail(&tail, !absolute);
    if absolute {
        device.push(b'\\');
    }
    device.extend_from_slice(&tail);
    if device.is_empty() {
        device.push(b'.');
    }
    Ok(native(device))
}

#[cfg(windows)]
pub(crate) fn resolve(args: &[&Path]) -> io::Result<PathBuf> {
    resolve_with(args, super::windows_cwd::get)
}

/// Node relative compares resolved lexical components after the namespace marker,
/// rather than declaring VerbatimDisk and VerbatimUNC to be different volumes.
pub(crate) fn relative_namespace(base: &Path, target: &Path) -> Option<PathBuf> {
    let a = bytes(base);
    let b = bytes(target);
    if !is_namespace(base) || !is_namespace(target) || a[2] != b[2] {
        return None;
    }
    let from = normalize_namespace(base, false)?;
    let to = normalize_namespace(target, false)?;
    Some(super::windows_relative::from_resolved(&from, &to))
}
