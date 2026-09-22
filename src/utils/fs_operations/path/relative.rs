use super::SugarPath;
use super::lexical::try_relative_lexically;
use super::normalize::normalize_for_resolution;
#[cfg(not(target_family = "windows"))]
use super::relative_string::relative_str;
#[cfg(target_family = "windows")]
use super::windows::{
    push_windows_relative_component, try_relative_drive_lexically, try_relative_windows_absolute,
    try_relative_windows_root_lexically, windows_components_eq_ignore_ascii_case,
    windows_paths_have_different_prefixes, windows_relative_component_has_literal_slash,
    windows_standalone_relative_bytes_are_representable,
};
use std::borrow::Cow;
#[cfg(target_family = "windows")]
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

pub(super) enum RelativeOutcome<'a> {
    BorrowedNative(&'a Path),
    Native(PathBuf),
    Slash(String),
}

impl<'a> RelativeOutcome<'a> {
    pub(super) fn into_path_buf(self) -> PathBuf {
        match self {
            Self::BorrowedNative(path) => path.to_owned(),
            Self::Native(path) => path,
            Self::Slash(path) => {
                #[cfg(target_family = "windows")]
                {
                    PathBuf::from(path.replace('/', "\\"))
                }
                #[cfg(not(target_family = "windows"))]
                {
                    PathBuf::from(path)
                }
            }
        }
    }

    pub(super) fn into_cow_path(self) -> Cow<'a, Path> {
        match self {
            Self::BorrowedNative(path) => Cow::Borrowed(path),
            outcome => Cow::Owned(outcome.into_path_buf()),
        }
    }
}

fn relative_without_cwd<'a>(
    target_path: &'a Path,
    base_path: &Path,
) -> Option<RelativeOutcome<'a>> {
    // Fast path: absolute inputs do not need cwd state. Unix can scan the UTF-8
    // spelling directly. Windows first compares borrowed prefix components, then
    // scans canonical native separators without allocating normalized copies.
    #[cfg(target_family = "windows")]
    if target_path.is_absolute()
        && base_path.is_absolute()
        && let Some(outcome) = try_relative_windows_absolute(target_path, base_path)
    {
        return Some(outcome);
    }

    #[cfg(not(target_family = "windows"))]
    if target_path.is_absolute()
        && base_path.is_absolute()
        && let (Some(target_str), Some(base_str)) = (target_path.to_str(), base_path.to_str())
    {
        return Some(match relative_str(target_str, base_str) {
            Cow::Borrowed(relative) => RelativeOutcome::BorrowedNative(Path::new(relative)),
            Cow::Owned(relative) => RelativeOutcome::Slash(relative),
        });
    }

    #[cfg(target_family = "windows")]
    if let Some(relative) = try_relative_windows_root_lexically(target_path, base_path) {
        return Some(RelativeOutcome::Native(relative));
    }

    #[cfg(target_family = "windows")]
    if let Some(relative) = try_relative_drive_lexically(target_path, base_path) {
        return Some(RelativeOutcome::Native(relative));
    }

    // Plain relative paths with the same number of unresolved leading parents
    // resolve from the same cwd ancestor. Their relative path is therefore
    // independent of the cwd itself.
    if !target_path.has_root()
        && !base_path.has_root()
        && let Some(relative) = try_relative_lexically(target_path, base_path)
    {
        return Some(RelativeOutcome::Native(relative));
    }

    None
}

pub(super) fn relative_from_resolved<'a>(base: PathBuf, target: PathBuf) -> RelativeOutcome<'a> {
    #[cfg(target_family = "windows")]
    if windows_paths_have_different_prefixes(&base, &target) {
        return RelativeOutcome::Native(target);
    }

    if base == target {
        return RelativeOutcome::Native(PathBuf::new());
    }

    let filter_fn = |component: &Component| {
        matches!(
            component,
            Component::Normal(_) | Component::Prefix(_) | Component::RootDir
        )
    };
    let base_components = base.components().filter(filter_fn);
    let target_components = target.components().filter(filter_fn);
    let common_len = base_components
        .clone()
        .zip(target_components.clone())
        .take_while(|(from, to)| {
            #[cfg(target_family = "windows")]
            {
                windows_components_eq_ignore_ascii_case(from, to)
            }
            #[cfg(not(target_family = "windows"))]
            {
                from == to
            }
        })
        .count();
    let up_len = base_components.count().saturating_sub(common_len);
    #[cfg(target_family = "windows")]
    {
        let target_suffix = target_components.clone().skip(common_len);
        if target_suffix
            .clone()
            .any(|component| windows_relative_component_has_literal_slash(&component))
            || (up_len == 0
                && target_suffix.clone().next().is_some_and(|component| {
                    !windows_standalone_relative_bytes_are_representable(
                        component.as_os_str().as_encoded_bytes(),
                    )
                }))
        {
            return RelativeOutcome::Native(target);
        }

        let suffix_count = target_suffix.clone().count();
        let component_count = up_len + suffix_count;
        let capacity = up_len * 2
            + target_suffix
                .clone()
                .map(|component| component.as_os_str().len())
                .sum::<usize>()
            + component_count.saturating_sub(1);
        let mut relative = OsString::with_capacity(capacity);
        for _ in 0..up_len {
            push_windows_relative_component(&mut relative, OsStr::new(".."));
        }
        for component in target_suffix {
            push_windows_relative_component(&mut relative, component.as_os_str());
        }
        RelativeOutcome::Native(PathBuf::from(relative))
    }
    #[cfg(not(target_family = "windows"))]
    let relative = (0..up_len)
        .map(|_| Component::ParentDir)
        .chain(target_components.skip(common_len))
        .collect();
    #[cfg(not(target_family = "windows"))]
    RelativeOutcome::Native(relative)
}

#[cfg(not(windows))]
pub(super) fn try_relative_outcome<'a>(
    target_path: &'a Path,
    base_path: &Path,
) -> io::Result<RelativeOutcome<'a>> {
    if let Some(outcome) = relative_without_cwd(target_path, base_path) {
        return Ok(outcome);
    }

    // Slow path: avoid current_dir() for already-absolute paths.
    let base = if base_path.is_absolute() {
        normalize_for_resolution(base_path).into_owned()
    } else {
        base_path.try_absolutize()?.into_owned()
    };
    let target = if target_path.is_absolute() {
        normalize_for_resolution(target_path).into_owned()
    } else {
        target_path.try_absolutize()?.into_owned()
    };

    Ok(relative_from_resolved(base, target))
}

#[cfg(not(windows))]
pub(super) fn relative_outcome_with<'a, P>(
    target_path: &'a Path,
    base_path: &Path,
    cwd: P,
) -> RelativeOutcome<'a>
where
    P: AsRef<Path> + Into<PathBuf>,
{
    if let Some(outcome) = relative_without_cwd(target_path, base_path) {
        return outcome;
    }

    assert!(
        cwd.as_ref().is_absolute(),
        "explicit current directory must be absolute"
    );

    let base = if base_path.is_absolute() {
        normalize_for_resolution(base_path).into_owned()
    } else {
        base_path.absolutize_with(cwd.as_ref()).into_owned()
    };
    let target = if target_path.is_absolute() {
        normalize_for_resolution(target_path).into_owned()
    } else {
        target_path.absolutize_with(cwd).into_owned()
    };

    if !base.is_absolute() || !target.is_absolute() {
        return RelativeOutcome::Native(normalize_for_resolution(&target).into_owned());
    }

    relative_from_resolved(base, target)
}

#[cfg(windows)]
fn windows_outcome<'a>(target: &'a Path, result: PathBuf) -> RelativeOutcome<'a> {
    let input = target.as_os_str().as_encoded_bytes();
    let value = result.as_os_str().as_encoded_bytes();
    if value.is_empty() {
        // SAFETY: the empty suffix is a valid native string and preserves target ownership.
        return RelativeOutcome::BorrowedNative(Path::new(unsafe {
            OsStr::from_encoded_bytes_unchecked(&input[input.len()..])
        }));
    }
    let mut end = input.len();
    while end > 0 && super::windows_lexical::sep(input[end - 1]) {
        end -= 1;
    }
    if input[..end].ends_with(value) {
        let start = end - value.len();
        if start == 0 || super::windows_lexical::sep(input[start - 1]) {
            // SAFETY: suffix is a complete native string at an ASCII component boundary.
            let suffix = unsafe { OsStr::from_encoded_bytes_unchecked(&input[start..end]) };
            return RelativeOutcome::BorrowedNative(Path::new(suffix));
        }
    }
    RelativeOutcome::Native(result)
}

#[cfg(windows)]
pub(super) fn try_relative_outcome<'a>(
    target: &'a Path,
    base: &Path,
) -> io::Result<RelativeOutcome<'a>> {
    if target.as_os_str() == base.as_os_str() {
        return Ok(windows_outcome(target, PathBuf::new()));
    }
    let from = super::windows_lexical::resolve(&[base])?;
    let to = super::windows_lexical::resolve(&[target])?;
    Ok(windows_outcome(
        target,
        super::windows_relative::from_resolved(&from, &to),
    ))
}

#[cfg(windows)]
pub(super) fn relative_outcome_with<'a, P>(
    target: &'a Path,
    base: &Path,
    cwd: P,
) -> RelativeOutcome<'a>
where
    P: AsRef<Path> + Into<PathBuf>,
{
    if target.as_os_str() == base.as_os_str() {
        return windows_outcome(target, PathBuf::new());
    }
    // sugar_path-only explicit-cwd extension: when the supplied cwd is invalid,
    // retain cwd-independent cases using a common synthetic root. The shape
    // check never supplies the result; Node's Unicode comparison still does.
    let no_prefix = |p: &Path| p.components().all(|c| !matches!(c, Component::Prefix(_)));
    let independent = no_prefix(target)
        && no_prefix(base)
        && ((target.has_root() && base.has_root())
            || (!target.has_root()
                && !base.has_root()
                && try_relative_lexically(target, base).is_some()));
    let cwd = if !super::windows_lexical::is_fully_qualified(cwd.as_ref()) && independent {
        Path::new(r"C:\")
    } else {
        cwd.as_ref()
    };
    let resolve = |path| {
        super::windows_lexical::resolve_with(&[path], |drive| {
            super::windows_cwd::select(
                drive,
                |_| None,
                || {
                    assert!(
                        super::windows_lexical::is_fully_qualified(cwd),
                        "explicit current directory must be absolute"
                    );
                    Ok(cwd.to_owned())
                },
            )
        })
        .expect("explicit cwd resolution is infallible")
    };
    windows_outcome(
        target,
        super::windows_relative::from_resolved(&resolve(base), &resolve(target)),
    )
}
