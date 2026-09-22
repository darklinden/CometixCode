use super::lexical::{LexicalRelativeShape, OsStrVec};
use super::normalize::{normalize_for_resolution, normalize_owned_for_resolution};
use super::relative::{RelativeOutcome, relative_from_resolved};
use super::relative_string::relative_str;
use memchr::{memchr, memrchr};
use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

#[cfg(target_family = "windows")]
pub(super) fn classify_drive_relative(path: &Path) -> Option<(u8, LexicalRelativeShape)> {
    use std::path::Prefix;

    if path.has_root() {
        return None;
    }
    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    let Prefix::Disk(parsed_drive) = prefix.kind() else {
        return None;
    };
    let drive = windows_drive_spelling(path).unwrap_or(parsed_drive);

    let mut unresolved_parents = 0;
    let mut surviving_normals = 0;
    let mut max_normal_depth = 0;
    for component in components {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if surviving_normals > 0 {
                    surviving_normals -= 1;
                } else {
                    unresolved_parents += 1;
                }
            }
            Component::Normal(_) => {
                surviving_normals += 1;
                max_normal_depth = max_normal_depth.max(surviving_normals);
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }

    Some((
        drive,
        LexicalRelativeShape {
            unresolved_parents,
            surviving_normals,
            max_normal_depth,
        },
    ))
}

#[cfg(target_family = "windows")]
pub(super) fn windows_drive_spelling(path: &Path) -> Option<u8> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Some(bytes[0]);
    }
    if bytes.len() >= 6
        && matches!(bytes[0], b'/' | b'\\')
        && matches!(bytes[1], b'/' | b'\\')
        && bytes[2] == b'?'
        && matches!(bytes[3], b'/' | b'\\')
        && bytes[5] == b':'
        && bytes[4].is_ascii_alphabetic()
    {
        return Some(bytes[4]);
    }
    None
}

#[cfg(target_family = "windows")]
pub(super) fn push_windows_relative_component(path: &mut OsString, component: &OsStr) {
    if !path.is_empty() {
        path.push("\\");
    }
    path.push(component);
}

#[cfg(target_family = "windows")]
fn push_windows_path_component(
    path: &mut OsString,
    component: &OsStr,
    forward_slash_is_separator: bool,
) {
    let has_separator = matches!(path.as_encoded_bytes().last(), Some(b'\\'))
        || (forward_slash_is_separator && matches!(path.as_encoded_bytes().last(), Some(b'/')));
    if !path.is_empty() && !has_separator {
        path.push("\\");
    }
    path.push(component);
}

#[cfg(target_family = "windows")]
fn collect_drive_relative_normals(path: &Path, shape: LexicalRelativeShape) -> OsStrVec<'_> {
    let mut normals = OsStrVec::with_capacity(shape.max_normal_depth);
    for component in path.components().skip(1) {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normals.pop();
            }
            Component::Normal(normal) => normals.push(normal),
            Component::Prefix(_) | Component::RootDir => {
                unreachable!("classified drive-relative paths have one prefix and no root")
            }
        }
    }
    debug_assert_eq!(normals.len(), shape.surviving_normals);
    normals
}

#[cfg(target_family = "windows")]
pub(super) fn try_relative_drive_lexically(target: &Path, base: &Path) -> Option<PathBuf> {
    let (target_drive, target_shape) = classify_drive_relative(target)?;
    let (base_drive, base_shape) = classify_drive_relative(base)?;
    if !target_drive.eq_ignore_ascii_case(&base_drive)
        || target_shape.unresolved_parents != base_shape.unresolved_parents
    {
        return None;
    }

    let target = collect_drive_relative_normals(target, target_shape);
    let base = collect_drive_relative_normals(base, base_shape);
    let common_len = target
        .iter()
        .zip(&base)
        .take_while(|(target, base)| target.eq_ignore_ascii_case(base))
        .count();
    let up_len = base.len() - common_len;
    let target_suffix = &target[common_len..];
    if up_len == 0
        && target_suffix.first().is_some_and(|component| {
            !windows_standalone_relative_bytes_are_representable(component.as_encoded_bytes())
        })
    {
        return None;
    }
    let component_count = up_len + target_suffix.len();
    let capacity = up_len * 2
        + target_suffix
            .iter()
            .map(|component| component.len())
            .sum::<usize>()
        + component_count.saturating_sub(1);
    let mut relative = OsString::with_capacity(capacity);
    for _ in 0..up_len {
        push_windows_relative_component(&mut relative, OsStr::new(".."));
    }
    for component in target_suffix {
        push_windows_relative_component(&mut relative, component);
    }
    Some(PathBuf::from(relative))
}

#[cfg(target_family = "windows")]
fn windows_absolute_disk_drive(path: &Path) -> Option<(u8, bool)> {
    use std::path::Prefix;

    let Component::Prefix(prefix) = path.components().next()? else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(drive) => Some((windows_drive_spelling(path).unwrap_or(drive), true)),
        Prefix::VerbatimDisk(drive) => Some((windows_drive_spelling(path).unwrap_or(drive), false)),
        _ => None,
    }
}

#[cfg(target_family = "windows")]
fn rebuild_windows_disk_path(path: &Path, drive: u8) -> Option<PathBuf> {
    let mut rebuilt = OsString::with_capacity(path.as_os_str().len().max(3));
    let prefix = [drive, b':', b'\\'];
    rebuilt.push(std::str::from_utf8(&prefix).expect("Windows drive prefix is ASCII"));
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir | Component::Normal(_) => {
                if memchr(b'/', component.as_os_str().as_encoded_bytes()).is_some() {
                    return None;
                }
                push_windows_path_component(&mut rebuilt, component.as_os_str(), true);
            }
        }
    }
    Some(PathBuf::from(rebuilt))
}

#[cfg(target_family = "windows")]
fn rebuild_windows_verbatim_disk_path(path: &Path, drive: u8) -> PathBuf {
    let mut rebuilt = OsString::with_capacity(path.as_os_str().len().max(7));
    let prefix = [b'\\', b'\\', b'?', b'\\', drive, b':', b'\\'];
    rebuilt.push(std::str::from_utf8(&prefix).expect("Windows verbatim drive prefix is ASCII"));
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir | Component::Normal(_) => {
                push_windows_path_component(&mut rebuilt, component.as_os_str(), false);
            }
        }
    }
    PathBuf::from(rebuilt)
}

#[cfg(target_family = "windows")]
pub(super) fn absolutize_drive_relative_with<P>(path: &Path, cwd: P, drive: u8) -> Cow<'_, Path>
where
    P: AsRef<Path> + Into<PathBuf>,
{
    let Some((cwd_drive, ordinary_disk)) = windows_absolute_disk_drive(cwd.as_ref()) else {
        return normalize_for_resolution(path);
    };
    if !cwd_drive.eq_ignore_ascii_case(&drive) {
        return normalize_for_resolution(path);
    }

    let resolved = if cwd_drive == drive {
        cwd.into()
    } else if ordinary_disk {
        rebuild_windows_disk_path(cwd.as_ref(), drive)
            .expect("ordinary Windows disk components contain no literal forward slash")
    } else {
        rebuild_windows_verbatim_disk_path(cwd.as_ref(), drive)
    };
    let mut resolved = resolved.into_os_string();
    for component in path.components().skip(1) {
        push_windows_path_component(&mut resolved, component.as_os_str(), ordinary_disk);
    }
    Cow::Owned(normalize_owned_for_resolution(PathBuf::from(resolved)))
}

#[cfg(target_family = "windows")]
fn windows_absolute_parts(path: &Path) -> Option<(std::path::Prefix<'_>, &Path)> {
    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    if matches!(components.clone().next(), Some(Component::RootDir)) {
        components.next();
    }
    Some((prefix.kind(), components.as_path()))
}

#[cfg(target_family = "windows")]
pub(super) fn windows_prefix_is_verbatim(prefix: std::path::Prefix<'_>) -> bool {
    matches!(
        prefix,
        std::path::Prefix::Verbatim(_)
            | std::path::Prefix::VerbatimDisk(_)
            | std::path::Prefix::VerbatimUNC(_, _)
    )
}

#[cfg(target_family = "windows")]
fn windows_standalone_relative_is_representable(path: &str) -> bool {
    windows_standalone_relative_bytes_are_representable(path.as_bytes())
}

#[cfg(target_family = "windows")]
pub(super) fn windows_standalone_relative_bytes_are_representable(path: &[u8]) -> bool {
    !matches!(path.first(), Some(b'/' | b'\\'))
        && !(path.len() >= 2 && path[0].is_ascii_alphabetic() && path[1] == b':')
}

#[cfg(target_family = "windows")]
pub(super) fn windows_relative_component_has_literal_slash(component: &Component<'_>) -> bool {
    let Component::Normal(component) = component else {
        return false;
    };
    memchr(b'/', component.as_encoded_bytes()).is_some()
}

#[cfg(target_family = "windows")]
fn windows_native_relative_input_is_clean(path: &str) -> bool {
    if memchr(b'/', path.as_bytes()).is_some() {
        return false;
    }

    let path = path.trim_end_matches('\\');
    let mut offset = 0;
    while offset < path.len() {
        let end = memchr(b'\\', &path.as_bytes()[offset..])
            .map(|position| offset + position)
            .unwrap_or(path.len());
        let component = &path[offset..end];
        if component.is_empty() || component == "." || component == ".." {
            return false;
        }
        offset = end.saturating_add(1);
    }
    true
}

#[cfg(target_family = "windows")]
fn relative_windows_native_fast<'a>(target: &'a str, base: &str) -> Option<RelativeOutcome<'a>> {
    if !windows_native_relative_input_is_clean(target)
        || !windows_native_relative_input_is_clean(base)
    {
        return None;
    }

    let target = target.trim_end_matches('\\');
    let base = base.trim_end_matches('\\');
    let common_byte_len = target
        .as_bytes()
        .iter()
        .zip(base.as_bytes())
        .take_while(|(target, base)| target.eq_ignore_ascii_case(base))
        .count();
    let at_boundary = (common_byte_len == target.len() && common_byte_len == base.len())
        || (common_byte_len == target.len()
            && base.as_bytes().get(common_byte_len) == Some(&b'\\'))
        || (common_byte_len == base.len()
            && target.as_bytes().get(common_byte_len) == Some(&b'\\'));
    let common_prefix = if at_boundary {
        common_byte_len
    } else {
        memrchr(b'\\', &target.as_bytes()[..common_byte_len]).unwrap_or(0)
    };

    let base_remaining = &base.as_bytes()[common_prefix..];
    let mut ups = 0usize;
    let mut offset = 0;
    while offset < base_remaining.len() {
        if base_remaining[offset] == b'\\' {
            offset += 1;
            continue;
        }
        ups += 1;
        offset = memchr(b'\\', &base_remaining[offset..])
            .map(|position| offset + position + 1)
            .unwrap_or(base_remaining.len());
    }

    let target_suffix = target[common_prefix..].trim_start_matches('\\');
    if ups == 0 {
        if !windows_standalone_relative_is_representable(target_suffix) {
            return None;
        }
        return Some(RelativeOutcome::BorrowedNative(Path::new(target_suffix)));
    }

    let mut relative = String::with_capacity(ups * 3 + target_suffix.len());
    for _ in 0..ups {
        if !relative.is_empty() {
            relative.push('\\');
        }
        relative.push_str("..");
    }
    if !target_suffix.is_empty() {
        relative.push('\\');
        relative.push_str(target_suffix);
    }
    Some(RelativeOutcome::Native(PathBuf::from(relative)))
}

#[cfg(target_family = "windows")]
pub(super) fn try_relative_windows_absolute<'a>(
    target_path: &'a Path,
    base_path: &Path,
) -> Option<RelativeOutcome<'a>> {
    let (target_prefix, target_rest) = windows_absolute_parts(target_path)?;
    let (base_prefix, base_rest) = windows_absolute_parts(base_path)?;
    if !windows_prefixes_eq_ignore_ascii_case(target_prefix, base_prefix) {
        return Some(RelativeOutcome::Native(
            normalize_for_resolution(target_path).into_owned(),
        ));
    }

    // A forward slash is a literal byte inside a verbatim path component. The
    // normalized-string fallback would reinterpret it as a separator, so let
    // the component fallback retain the native `std::path` meaning instead.
    if windows_prefix_is_verbatim(target_prefix)
        && (memchr(b'/', target_rest.as_os_str().as_encoded_bytes()).is_some()
            || memchr(b'/', base_rest.as_os_str().as_encoded_bytes()).is_some())
    {
        return None;
    }

    let (target_str, base_str) = (target_rest.to_str()?, base_rest.to_str()?);
    if let Some(outcome) = relative_windows_native_fast(target_str, base_str) {
        return Some(outcome);
    }

    // Dirty or mixed-separator paths retain the established normalized-string
    // fallback. Its temporary allocations stay off the canonical Rolldown path.
    let target_fwd = normalize_backslash_cow(target_str);
    let base_fwd = normalize_backslash_cow(base_str);
    let relative = relative_str(&target_fwd, &base_fwd);
    if !windows_standalone_relative_is_representable(&relative) {
        return None;
    }
    Some(match relative {
        Cow::Borrowed(relative) => {
            let target_without_trailing = target_str.trim_end_matches(['/', '\\']);
            debug_assert!(relative.len() <= target_without_trailing.len());
            let original_relative =
                &target_without_trailing[target_without_trailing.len() - relative.len()..];
            if memchr(b'/', original_relative.as_bytes()).is_none() {
                RelativeOutcome::BorrowedNative(Path::new(original_relative))
            } else {
                RelativeOutcome::Slash(relative.to_owned())
            }
        }
        Cow::Owned(relative) => RelativeOutcome::Slash(relative),
    })
}

#[cfg(target_family = "windows")]
pub(super) fn try_relative_windows_root_lexically(target: &Path, base: &Path) -> Option<PathBuf> {
    if !matches!(target.components().next(), Some(Component::RootDir))
        || !matches!(base.components().next(), Some(Component::RootDir))
    {
        return None;
    }

    let target = normalize_for_resolution(target).into_owned();
    let base = normalize_for_resolution(base).into_owned();
    Some(relative_from_resolved(base, target).into_path_buf())
}

#[cfg(target_family = "windows")]
pub(super) fn windows_paths_have_different_prefixes(base: &Path, target: &Path) -> bool {
    match (base.components().next(), target.components().next()) {
        (Some(Component::Prefix(base)), Some(Component::Prefix(target))) => {
            !windows_prefixes_eq_ignore_ascii_case(base.kind(), target.kind())
        }
        (Some(Component::Prefix(_)), _) | (_, Some(Component::Prefix(_))) => true,
        _ => false,
    }
}

#[cfg(target_family = "windows")]
pub(super) fn windows_components_eq_ignore_ascii_case(
    from: &Component<'_>,
    to: &Component<'_>,
) -> bool {
    match (from, to) {
        (Component::Normal(from), Component::Normal(to)) => from
            .as_encoded_bytes()
            .eq_ignore_ascii_case(to.as_encoded_bytes()),
        (Component::Prefix(from), Component::Prefix(to)) => {
            windows_prefixes_eq_ignore_ascii_case(from.kind(), to.kind())
        }
        _ => from == to,
    }
}

#[cfg(target_family = "windows")]
fn windows_prefixes_eq_ignore_ascii_case(
    from: std::path::Prefix<'_>,
    to: std::path::Prefix<'_>,
) -> bool {
    use std::path::Prefix;

    let os_eq_ignore_ascii_case = |from: &std::ffi::OsStr, to: &std::ffi::OsStr| {
        from.as_encoded_bytes()
            .eq_ignore_ascii_case(to.as_encoded_bytes())
    };

    match (from, to) {
        (Prefix::Disk(from), Prefix::Disk(to))
        | (Prefix::VerbatimDisk(from), Prefix::VerbatimDisk(to)) => from.eq_ignore_ascii_case(&to),
        (Prefix::UNC(from_server, from_share), Prefix::UNC(to_server, to_share))
        | (
            Prefix::VerbatimUNC(from_server, from_share),
            Prefix::VerbatimUNC(to_server, to_share),
        ) => {
            os_eq_ignore_ascii_case(from_server, to_server)
                && os_eq_ignore_ascii_case(from_share, to_share)
        }
        (Prefix::DeviceNS(from), Prefix::DeviceNS(to))
        | (Prefix::Verbatim(from), Prefix::Verbatim(to)) => os_eq_ignore_ascii_case(from, to),
        _ => false,
    }
}

/// String-based relative path computation. Dispatches to the fast path when
/// the component spelling is already canonical, otherwise normalizes first.
/// Replace `\` with `/` using memchr SIMD search. Returns the input unchanged
/// (zero allocation) when no backslashes are present.
#[cfg(target_family = "windows")]
fn normalize_backslash_cow(s: &str) -> Cow<'_, str> {
    let bytes = s.as_bytes();
    let Some(first) = memchr(b'\\', bytes) else {
        return Cow::Borrowed(s);
    };
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(&bytes[..first]);
    out.push(b'/');
    let mut offset = first + 1;
    while let Some(pos) = memchr(b'\\', &bytes[offset..]) {
        out.extend_from_slice(&bytes[offset..offset + pos]);
        out.push(b'/');
        offset += pos + 1;
    }
    out.extend_from_slice(&bytes[offset..]);
    // SAFETY: input is valid UTF-8, and we only replaced `\` (single ASCII byte) with `/`
    Cow::Owned(unsafe { String::from_utf8_unchecked(out) })
}
