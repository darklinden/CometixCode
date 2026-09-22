#[cfg(target_family = "windows")]
use super::windows::windows_drive_spelling;
#[cfg(target_family = "windows")]
use super::windows::windows_prefix_is_verbatim;
#[cfg(target_family = "windows")]
use super::windows::windows_standalone_relative_bytes_are_representable;
use memchr::memchr;
use memchr::memrchr;
use std::borrow::Cow;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::iter::Peekable;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

enum OwnedNormalizeOutcome {
    Unchanged,
    CurrentDirectory,
    Owned(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TrailingSeparator {
    Preserve,
    Strip,
}

fn normalize_owned_path_buf_with(mut path: PathBuf, trailing: TrailingSeparator) -> PathBuf {
    let outcome = match normalize_path(&path, trailing) {
        Cow::Borrowed(normalized) if std::ptr::eq(normalized, path.as_path()) => {
            OwnedNormalizeOutcome::Unchanged
        }
        Cow::Borrowed(normalized) if normalized.as_os_str() == OsStr::new(".") => {
            OwnedNormalizeOutcome::CurrentDirectory
        }
        Cow::Borrowed(normalized) => OwnedNormalizeOutcome::Owned(normalized.to_owned()),
        Cow::Owned(normalized) => OwnedNormalizeOutcome::Owned(normalized),
    };

    match outcome {
        OwnedNormalizeOutcome::Unchanged => path,
        OwnedNormalizeOutcome::CurrentDirectory => {
            path.clear();
            path.push(".");
            path
        }
        OwnedNormalizeOutcome::Owned(normalized) => normalized,
    }
}

pub(super) fn normalize_owned_path_buf(path: PathBuf) -> PathBuf {
    normalize_owned_path_buf_with(path, TrailingSeparator::Preserve)
}

pub(super) fn normalize_owned_for_resolution(path: PathBuf) -> PathBuf {
    normalize_owned_path_buf_with(path, TrailingSeparator::Strip)
}

#[cfg(windows)]
pub(super) fn normalize_path(path: &Path, trailing: TrailingSeparator) -> Cow<'_, Path> {
    if path.as_os_str().is_empty() {
        return Cow::Borrowed(Path::new("."));
    }
    let normalized =
        if trailing == TrailingSeparator::Strip && super::windows_lexical::is_namespace(path) {
            super::windows_lexical::normalize_namespace(path, false).unwrap()
        } else {
            let mut normalized = super::windows_normalize::normalize(path);
            if trailing == TrailingSeparator::Strip {
                // Resolution strips a tail separator but retains true root separators.
                let raw = normalized.as_os_str().as_encoded_bytes();
                let root = super::windows_lexical::root(raw);
                if raw.last() == Some(&b'\\') && raw.len() > root.end + 1 {
                    normalized = super::windows_lexical::native(raw[..raw.len() - 1].to_vec());
                }
            }
            normalized
        };
    return if normalized.as_os_str() == path.as_os_str() {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(normalized)
    };
}

#[cfg(not(windows))]
pub(super) fn normalize_path(path: &Path, trailing: TrailingSeparator) -> Cow<'_, Path> {
    if !needs_normalization(path, trailing) {
        return Cow::Borrowed(path);
    }
    normalize_inner(
        path.components().peekable(),
        path.as_os_str().len(),
        trailing == TrailingSeparator::Preserve && has_trailing_separator(path),
        {
            #[cfg(not(target_family = "windows"))]
            {
                None
            }
        },
    )
}

pub(super) fn normalize_for_resolution(path: &Path) -> Cow<'_, Path> {
    normalize_path(path, TrailingSeparator::Strip)
}

#[inline]
fn has_trailing_separator(path: &Path) -> bool {
    let Some(last) = path.as_os_str().as_encoded_bytes().last() else {
        return false;
    };
    if *last == std::path::MAIN_SEPARATOR as u8 {
        return true;
    }

    #[cfg(target_family = "windows")]
    {
        *last == b'/'
            && !matches!(
              path.components().next(),
              Some(Component::Prefix(prefix))
                if windows_prefix_is_verbatim(prefix.kind())
            )
    }
    #[cfg(not(target_family = "windows"))]
    {
        false
    }
}

/// Check whether a path needs normalization. Returns `false` for already-clean
/// paths, allowing `normalize()` to return `Cow::Borrowed` with zero allocation.
#[inline]
#[cfg(not(target_family = "windows"))]
fn needs_normalization(path: &Path, trailing: TrailingSeparator) -> bool {
    let Some(s) = path.to_str() else {
        return true;
    };
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return true;
    }
    if bytes == b"." || (trailing == TrailingSeparator::Preserve && bytes == b"./") {
        return false;
    }
    // A leading `.` needs normalization. A leading run of `..` can be clean when
    // every remaining component is normal (`...` and `.foo` are normal names).
    if bytes[0] == b'.' {
        if bytes.len() == 1 || bytes[1] == b'/' {
            return true;
        }
        if bytes[1] == b'.' && (bytes.len() == 2 || bytes[2] == b'/') {
            return !leading_parent_path_is_normalized(
                bytes,
                b'/',
                trailing == TrailingSeparator::Preserve,
            );
        }
    }
    // Trailing `/` (unless path is exactly `/`)
    if trailing == TrailingSeparator::Strip && bytes.len() > 1 && bytes[bytes.len() - 1] == b'/' {
        return true;
    }
    // memchr scan for `//`, `/.`, `/..`
    let mut offset = 0;
    while let Some(pos) = memchr(b'/', &bytes[offset..]) {
        let slash = offset + pos;
        let next = slash + 1;
        if next < bytes.len() {
            let b = bytes[next];
            // `//` — consecutive slashes
            if b == b'/' {
                return true;
            }
            // `/.` — could be `/.` or `/..`
            if b == b'.' {
                let after_dot = next + 1;
                // "/." at end or "/./"
                if after_dot >= bytes.len() || bytes[after_dot] == b'/' {
                    return true;
                }
                // "/.." at end or "/../"
                if bytes[after_dot] == b'.'
                    && (after_dot + 1 >= bytes.len() || bytes[after_dot + 1] == b'/')
                {
                    return true;
                }
            }
        }
        offset = next;
    }
    false
}

/// Check whether a path needs normalization (Windows variant).
#[inline]
#[cfg(target_family = "windows")]
fn needs_normalization(path: &Path, trailing: TrailingSeparator) -> bool {
    let Some(s) = path.to_str() else {
        return true;
    };
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return true;
    }
    if bytes == b"." || (trailing == TrailingSeparator::Preserve && bytes == b".\\") {
        return false;
    }
    // Any forward slash means normalization is needed (gets converted to `\`)
    if memchr(b'/', bytes).is_some() {
        return true;
    }
    // UNC prefix `\\` at start — always bail out to normalizer
    if bytes.len() >= 2 && bytes[0] == b'\\' && bytes[1] == b'\\' {
        return true;
    }
    // A bare drive `X:` normalizes to `X:.`. Other drive-relative paths keep
    // their drive spelling and lexical form.
    if bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return true; // bare `C:`
    }
    // `C:.` and `C:.\` are canonical drive-relative current-directory
    // spellings, but the same component before another segment is redundant.
    if bytes.len() > 4
        && bytes[1] == b':'
        && bytes[0].is_ascii_alphabetic()
        && bytes[2] == b'.'
        && bytes[3] == b'\\'
    {
        return true;
    }
    // A leading `.` needs normalization. A leading run of `..` can be clean when
    // every remaining component is normal (`...` and `.foo` are normal names).
    if bytes[0] == b'.' {
        if bytes.len() == 1 || bytes[1] == b'\\' {
            return true;
        }
        if bytes[1] == b'.' && (bytes.len() == 2 || bytes[2] == b'\\') {
            return !leading_parent_path_is_normalized(
                bytes,
                b'\\',
                trailing == TrailingSeparator::Preserve,
            );
        }
    }
    // Trailing `\` (unless path is `\` alone or `X:\`)
    if trailing == TrailingSeparator::Strip && bytes[bytes.len() - 1] == b'\\' {
        // `\` alone is clean
        if bytes.len() == 1 {
            return false;
        }
        // `X:\` is clean
        if bytes.len() == 3 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
            return false;
        }
        return true;
    }
    // memchr scan for `\\` (consecutive), `\.`, `\..`
    let mut offset = 0;
    while let Some(pos) = memchr(b'\\', &bytes[offset..]) {
        let slash = offset + pos;
        let next = slash + 1;
        if next < bytes.len() {
            let b = bytes[next];
            // `\\` — consecutive separators
            if b == b'\\' {
                return true;
            }
            // `\.` — could be `\.` or `\..`
            if b == b'.' {
                let after_dot = next + 1;
                // "\." at end or "\.\"
                if after_dot >= bytes.len() || bytes[after_dot] == b'\\' {
                    return true;
                }
                // "\.." at end or "\..\"
                if bytes[after_dot] == b'.'
                    && (after_dot + 1 >= bytes.len() || bytes[after_dot + 1] == b'\\')
                {
                    return true;
                }
            }
        }
        offset = next;
    }
    false
}

/// Return whether an unprefixed path starting with a `..` component is already
/// in the exact spelling produced by `normalize_inner`.
///
/// A canonical path may contain one or more leading `..` components followed
/// only by normal components. Empty components, `.`, a `..` after any normal
/// component, and a trailing separator all require normalization.
#[inline]
fn leading_parent_path_is_normalized(bytes: &[u8], separator: u8, preserve_trailing: bool) -> bool {
    debug_assert!(bytes == b".." || bytes.starts_with(&[b'.', b'.', separator]));

    let mut offset = 0;
    let mut saw_normal = false;
    loop {
        let end = memchr(separator, &bytes[offset..])
            .map(|position| offset + position)
            .unwrap_or(bytes.len());
        let component = &bytes[offset..end];

        if component == b".." {
            if saw_normal {
                return false;
            }
        } else if component.is_empty() || component == b"." {
            return false;
        } else {
            saw_normal = true;
        }

        if end == bytes.len() {
            return true;
        }
        offset = end + 1;
        if offset == bytes.len() {
            return preserve_trailing;
        }
    }
}

#[inline]
fn normalize_inner<'a>(
    mut components: Peekable<impl Iterator<Item = Component<'a>>>,
    hint_cap: usize,
    preserve_trailing: bool,
    _drive_spelling: Option<u8>,
) -> Cow<'a, Path> {
    let sep_byte = std::path::MAIN_SEPARATOR as u8;
    let mut buf: Vec<u8> = Vec::with_capacity(hint_cap);
    let mut has_root = false;
    let mut depth: usize = 0; // count of Normal segments currently in buf
    let mut need_sep = false;

    // --- Prefix (Windows only) ---
    #[cfg(target_family = "windows")]
    let prefix_len: usize;
    #[cfg(target_family = "windows")]
    let prefix_only_suffix: Option<u8>;
    #[cfg(target_family = "windows")]
    let prefix_root_is_optional: bool;
    #[cfg(target_family = "windows")]
    {
        if let Some(Component::Prefix(p)) = components.peek() {
            let (suffix, optional_root) = match p.kind() {
                std::path::Prefix::VerbatimDisk(drive) => {
                    buf.extend_from_slice(b"\\\\?\\");
                    buf.push(_drive_spelling.unwrap_or(drive));
                    buf.push(b':');
                    // `\\?\C:` has no RootDir component. A slash would add one, while
                    // a dot would make Rust parse the whole prefix as generic Verbatim.
                    (None, false)
                }
                std::path::Prefix::DeviceNS(device) => {
                    buf.extend_from_slice(b"\\\\.\\");
                    buf.extend_from_slice(device.as_encoded_bytes());
                    (None, true)
                }
                std::path::Prefix::UNC(server, share) => {
                    buf.extend_from_slice(b"\\\\");
                    buf.extend_from_slice(server.as_encoded_bytes());
                    buf.push(b'\\');
                    buf.extend_from_slice(share.as_encoded_bytes());
                    (Some(b'\\'), false)
                }
                std::path::Prefix::Disk(drive) => {
                    buf.push(_drive_spelling.unwrap_or(drive));
                    buf.push(b':');
                    (Some(b'.'), false)
                }
                std::path::Prefix::Verbatim(_) | std::path::Prefix::VerbatimUNC(_, _) => {
                    buf.extend_from_slice(p.as_os_str().as_encoded_bytes());
                    (None, true)
                }
            };
            prefix_only_suffix = suffix;
            prefix_root_is_optional = optional_root;
            components.next();
        } else {
            prefix_only_suffix = None;
            prefix_root_is_optional = false;
        }
        prefix_len = buf.len();
    }

    // --- RootDir ---
    if matches!(components.peek(), Some(Component::RootDir)) {
        #[cfg(target_family = "windows")]
        let prefix_has_synthetic_root = buf.len() == hint_cap && prefix_root_is_optional;
        #[cfg(not(target_family = "windows"))]
        let prefix_has_synthetic_root = false;
        if !prefix_has_synthetic_root {
            buf.push(sep_byte);
        }
        has_root = true;
        components.next();
    }

    let root_end = buf.len();

    // --- Remaining components ---
    for component in components {
        match component {
            Component::Prefix(prefix) => unreachable!("Unexpected prefix for {:?}", prefix),
            Component::RootDir => unreachable!("Unexpected RootDir after initial position"),
            Component::CurDir => {}
            Component::ParentDir => {
                if depth > 0 {
                    // Roll back the last Normal segment using memrchr.
                    let search_region = &buf[root_end..];
                    if let Some(pos) = memrchr(sep_byte, search_region) {
                        buf.truncate(root_end + pos);
                    } else {
                        buf.truncate(root_end);
                    }
                    depth -= 1;
                    need_sep = buf.len() > root_end;
                } else if !has_root {
                    // Relative path going above start: write ".." literally
                    if need_sep {
                        buf.push(sep_byte);
                    }
                    buf.extend_from_slice(b"..");
                    need_sep = true;
                }
                // else: has_root && depth == 0 → ignore (can't go above root)
            }
            Component::Normal(s) => {
                if need_sep {
                    buf.push(sep_byte);
                }
                buf.extend_from_slice(s.as_encoded_bytes());
                depth += 1;
                need_sep = true;
            }
        }
    }

    #[cfg(target_family = "windows")]
    if prefix_root_is_optional && depth == 0 && !preserve_trailing {
        buf.truncate(prefix_len);
    }

    // A normal component such as `C:foo` is only a drive prefix at the start of
    // a standalone Windows path. Keep the minimal `.\` spelling when removing
    // earlier lexical components would otherwise change that component's type.
    #[cfg(target_family = "windows")]
    if prefix_len == 0 && !has_root && !windows_standalone_relative_bytes_are_representable(&buf) {
        let len = buf.len();
        buf.reserve(2);
        buf.resize(len + 2, 0);
        buf.copy_within(0..len, 2);
        buf[0] = b'.';
        buf[1] = sep_byte;
    }

    // --- Empty result → "." ---
    if buf.is_empty() {
        if preserve_trailing {
            let mut current_directory = PathBuf::from(".");
            current_directory.push("");
            return Cow::Owned(current_directory);
        }
        return Cow::Borrowed(Path::new("."));
    }

    // --- Prefix-only: preserve its component semantics or use its canonical suffix ---
    #[cfg(target_family = "windows")]
    if buf.len() == prefix_len
        && prefix_len > 0
        && let Some(suffix) = prefix_only_suffix
    {
        buf.push(suffix);
    }

    if preserve_trailing && buf.last() != Some(&sep_byte) {
        buf.push(sep_byte);
    }

    // SAFETY: `buf` was built entirely from:
    // - encoded bytes of OsStr components (valid platform encoding)
    // - ASCII separator bytes and ASCII '.' characters
    // This preserves the encoding invariants required by OsString.
    Cow::Owned(PathBuf::from(unsafe {
        OsString::from_encoded_bytes_unchecked(buf)
    }))
}
