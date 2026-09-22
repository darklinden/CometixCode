use super::super::{windows_lexical, windows_syntax};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture_drive_cwd(cwd: &str, drive: Option<&[u8]>) -> std::io::Result<PathBuf> {
    // Original Windows run: C: is process cwd; other drive observations were roots.
    Ok(match drive {
        Some(drive) if !cwd.as_bytes()[..2].eq_ignore_ascii_case(drive) => {
            PathBuf::from(format!("{}\\", std::str::from_utf8(drive).unwrap()))
        }
        _ => PathBuf::from(cwd),
    })
}

#[test]
fn windows_resolve_matches_official_common_matrix() {
    let rows: Vec<Value> =
        serde_json::from_str(include_str!("windows_common_resolve.json")).unwrap();
    let env: Value = serde_json::from_str(include_str!("windows_common_environment.json")).unwrap();
    assert_eq!(rows.len(), 2641);
    assert_eq!(
        rows.iter().filter(|r| r["was_different"] == true).count(),
        158
    );
    for scenario in ["one", "two"] {
        let cwd = env["cwd"][scenario].as_str().unwrap();
        for row in &rows {
            let args: Vec<&Path> = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| Path::new(p.as_str().unwrap()))
                .collect();
            let actual =
                windows_lexical::resolve_with(&args, |drive| fixture_drive_cwd(cwd, drive))
                    .unwrap();
            assert_eq!(
                actual.as_os_str(),
                Path::new(row[scenario].as_str().unwrap()).as_os_str(),
                "{scenario}: {row}"
            );
        }
    }
}

#[test]
fn windows_namespace_operations_match_official_common_matrix() {
    for (op, fixture, expected_fixed) in [
        ("join", include_str!("windows_common_join.json"), 46),
        ("relative", include_str!("windows_common_relative.json"), 4),
    ] {
        let rows: Vec<Value> = serde_json::from_str(fixture).unwrap();
        for scenario in ["one", "two"] {
            let mut fixed = 0;
            for row in &rows {
                let a = Path::new(row["args"][0].as_str().unwrap());
                let b = Path::new(row["args"][1].as_str().unwrap());
                let actual = if op == "join" {
                    windows_lexical::normalize_namespace(&windows_syntax::join_input(a, b), true)
                } else {
                    windows_lexical::relative_namespace(a, b)
                };
                if row["was_different"] == true {
                    assert!(actual.is_some(), "Missed regression: {row}");
                    fixed += 1;
                }
                if let Some(actual) = actual {
                    assert_eq!(
                        actual.as_os_str(),
                        Path::new(row[scenario].as_str().unwrap()).as_os_str(),
                        "{scenario}/{op}: {row}"
                    );
                }
            }
            assert_eq!(fixed, expected_fixed);
        }
    }
}

#[test]
fn windows_resolve_matches_official_cwd_laziness() {
    fn fail(_: Option<&[u8]>) -> std::io::Result<PathBuf> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cwd unavailable",
        ))
    }
    assert_eq!(
        windows_lexical::resolve_with(&[Path::new("ignored"), Path::new(r"C:\a")], fail)
            .unwrap()
            .as_os_str(),
        r"C:\a"
    );
    assert!(windows_lexical::resolve_with(&[Path::new("a")], fail).is_err());
    let mut requested = Vec::new();
    let result = windows_lexical::resolve_with(&[Path::new("D:a\0b")], |drive| {
        requested.push(drive.unwrap().to_vec());
        Ok(PathBuf::from(r"D:\per-drive"))
    })
    .unwrap();
    assert_eq!(requested, [b"D:".to_vec()]);
    assert_eq!(result.as_os_str(), "D:\\per-drive\\a\0b");
}

#[test]
fn windows_namespace_relative_matches_official_mixed_separators() {
    for (base, target) in [("//?/C:/a", r"\\?\C:\b"), (r"\\?\C:\a", "//?/C:/b")] {
        assert_eq!(
            windows_lexical::relative_namespace(Path::new(base), Path::new(target))
                .unwrap()
                .as_os_str(),
            r"..\b"
        );
    }
}

#[test]
fn windows_namespace_normalize_matches_official_empty_tail() {
    for (input, expected) in [
        (r"\\.\PIPE\foo\..\", r"\\.\PIPE\"),
        (r"\\?\Volume{abc}\foo\..\", r"\\?\Volume{abc}\"),
    ] {
        assert_eq!(
            windows_lexical::normalize_namespace(Path::new(input), true)
                .unwrap()
                .as_os_str(),
            expected
        );
    }
    // This bare namespace drive differs between Node/Bun: preserve the existing
    // Node-shaped no-root-slash spelling, without changing the compatibility choice.
    assert_eq!(
        windows_lexical::normalize_namespace(Path::new(r"\\?\C:"), true)
            .unwrap()
            .as_os_str(),
        r"\\?\C:"
    );
    assert_eq!(
        windows_lexical::resolve_with(&[Path::new(r"\\?\C:")], |_| panic!("no cwd needed"))
            .unwrap()
            .as_os_str(),
        r"\\?\C:"
    );
}

#[cfg(windows)]
#[test]
fn windows_native_consumers_match_official_common_matrix() {
    // Exercise the real Windows entry points too: pure helpers alone do not
    // prove that normalization / relative fast paths reach the fixed owner.
    use super::super::{SugarPath, SugarPathBuf};
    use crate::utils::fs_operations::native;
    let p = Path::new(r"\\?\UNC\server\share\a/../../a");
    assert_eq!(p.normalize().as_os_str(), r"\\?\UNC\server\a");
    assert_eq!(
        p.to_owned().into_normalized().as_os_str(),
        r"\\?\UNC\server\a"
    );
    assert_eq!(
        native::resolve_path(Path::new("/"), Path::new("C:foo"))
            .unwrap()
            .as_os_str(),
        r"C:\foo"
    );
    assert_eq!(
        native::join_path(Path::new(r"\\?\C:\"), Path::new("a/.")).as_os_str(),
        r"\\?\C:\a"
    );
    assert_eq!(
        Path::new(r"\\?\UNC\server\share\a")
            .relative(Path::new(r"\\?\C:\"))
            .as_os_str(),
        r"..\UNC\server\share\a"
    );
    assert!(
        Path::new("C:a\0b")
            .try_absolutize()
            .unwrap()
            .as_os_str()
            .as_encoded_bytes()
            .contains(&0)
    );
}

#[cfg(windows)]
#[test]
fn windows_lexical_preserves_native_surrogates() {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let target = PathBuf::from(OsString::from_wide(&[b'C' as u16, b':' as u16, 0xd800]));
    let actual = windows_lexical::resolve_with(&[Path::new(r"C:\base"), &target], |_| {
        panic!("absolute explicit base must not read cwd")
    })
    .unwrap();
    let expected: Vec<u16> = "C:\\base\\".encode_utf16().chain([0xd800]).collect();
    assert_eq!(
        actual.as_os_str().encode_wide().collect::<Vec<_>>(),
        expected
    );
    let input: Vec<u16> = "\\\\?\\C:\\a/"
        .encode_utf16()
        .chain([0xd800])
        .chain("/.".encode_utf16())
        .collect();
    let p = PathBuf::from(OsString::from_wide(&input));
    let normalized = windows_lexical::normalize_namespace(&p, true).unwrap();
    let expected: Vec<u16> = "\\\\?\\C:\\a\\".encode_utf16().chain([0xd800]).collect();
    assert_eq!(
        normalized.as_os_str().encode_wide().collect::<Vec<_>>(),
        expected
    );
}
