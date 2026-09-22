#[cfg(target_family = "windows")]
use super::windows::push_windows_relative_component;
#[cfg(target_family = "windows")]
use super::windows::windows_standalone_relative_bytes_are_representable;
use smallvec::SmallVec;
use std::ffi::OsStr;
#[cfg(target_family = "windows")]
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

pub(super) type OsStrVec<'a> = SmallVec<[&'a OsStr; 16]>;

#[derive(Clone, Copy)]
pub(super) struct LexicalRelativeShape {
    pub(super) unresolved_parents: usize,
    pub(super) surviving_normals: usize,
    pub(super) max_normal_depth: usize,
}

fn classify_lexical_relative(path: &Path) -> Option<LexicalRelativeShape> {
    let mut unresolved_parents = 0;
    let mut surviving_normals = 0;
    let mut max_normal_depth = 0;

    for component in path.components() {
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

    Some(LexicalRelativeShape {
        unresolved_parents,
        surviving_normals,
        max_normal_depth,
    })
}

fn collect_lexical_normals<'a>(path: &'a Path, shape: LexicalRelativeShape) -> OsStrVec<'a> {
    let mut normals = OsStrVec::with_capacity(shape.max_normal_depth);
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normals.pop();
            }
            Component::Normal(normal) => normals.push(normal),
            Component::Prefix(_) | Component::RootDir => {
                unreachable!("classified lexical relative paths have no prefix or root")
            }
        }
    }
    debug_assert_eq!(normals.len(), shape.surviving_normals);
    normals
}

pub(super) fn try_relative_lexically(target: &Path, base: &Path) -> Option<PathBuf> {
    let target_shape = classify_lexical_relative(target)?;
    let base_shape = classify_lexical_relative(base)?;
    if target_shape.unresolved_parents != base_shape.unresolved_parents {
        return None;
    }

    let target = collect_lexical_normals(target, target_shape);
    let base = collect_lexical_normals(base, base_shape);

    let common_len = target
        .iter()
        .zip(&base)
        .take_while(|(target, base)| {
            #[cfg(target_family = "windows")]
            {
                target.eq_ignore_ascii_case(base)
            }
            #[cfg(not(target_family = "windows"))]
            {
                target == base
            }
        })
        .count();

    let up_len = base.len() - common_len;
    let target_suffix = &target[common_len..];
    #[cfg(target_family = "windows")]
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
    #[cfg(target_family = "windows")]
    {
        let mut relative = OsString::with_capacity(capacity);
        for _ in 0..up_len {
            push_windows_relative_component(&mut relative, OsStr::new(".."));
        }
        for component in target_suffix {
            push_windows_relative_component(&mut relative, component);
        }
        Some(PathBuf::from(relative))
    }
    #[cfg(not(target_family = "windows"))]
    let mut relative = PathBuf::with_capacity(capacity);
    #[cfg(not(target_family = "windows"))]
    for _ in 0..up_len {
        relative.push(Component::ParentDir);
    }
    #[cfg(not(target_family = "windows"))]
    for component in target_suffix {
        relative.push(component);
    }
    #[cfg(not(target_family = "windows"))]
    Some(relative)
}
