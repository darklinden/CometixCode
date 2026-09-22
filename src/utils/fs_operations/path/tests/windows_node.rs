//! Oracle: native Windows Node 24.14.0, returned run-EPyqC5.
//! CC utils/fsOperations.ts and utils/path.ts delegate these operations to node:path.
use super::super::{
    windows_cwd, windows_lexical, windows_normalize, windows_relative, windows_syntax,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn resolve(args: &[&Path], cwd: &str) -> PathBuf {
    windows_lexical::resolve_with(args, |drive| {
        windows_cwd::select(drive, |_| None, || Ok(PathBuf::from(cwd)))
    })
    .unwrap()
}
#[test]
fn windows_paths_match_official_node_complete_matrix() {
    let environments: Value =
        serde_json::from_str(include_str!("windows_node_environment.json")).unwrap();
    let fixtures = [
        include_str!("windows_node_normalize.json"),
        include_str!("windows_node_join.json"),
        include_str!("windows_node_resolve.json"),
        include_str!("windows_node_relative.json"),
        include_str!("windows_node_dirname.json"),
        include_str!("windows_node_basename.json"),
        include_str!("windows_node_isAbsolute.json"),
    ];
    for scene in ["one", "two"] {
        let cwd = environments[scene].as_str().unwrap();
        let mut count = 0;
        for fixture in fixtures {
            let rows: Vec<Value> = serde_json::from_str(fixture).unwrap();
            for row in rows {
                let args: Vec<_> = row["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| Path::new(p.as_str().unwrap()))
                    .collect();
                let actual = match row["op"].as_str().unwrap() {
                    "isAbsolute" => serde_json::json!(windows_syntax::is_absolute(args[0])),
                    op => {
                        let value = match op {
                            "normalize" => windows_normalize::normalize(args[0]),
                            "join" => windows_normalize::join(args[0], args[1]),
                            "resolve" => resolve(&args, cwd),
                            "relative" => windows_relative::from_resolved(
                                &resolve(&args[..1], cwd),
                                &resolve(&args[1..], cwd),
                            ),
                            "dirname" => windows_syntax::dirname(args[0]),
                            "basename" => PathBuf::from(windows_syntax::basename(args[0])),
                            _ => unreachable!(),
                        };
                        serde_json::json!(value.to_str().unwrap())
                    }
                };
                assert_eq!(actual, row[scene], "{scene}: {row}");
                count += 1;
            }
        }
        assert_eq!(count, 8750);
    }
}

#[test]
fn windows_drive_cwd_matches_official_node_priority_and_fallback() {
    // Node lib/path.js resolve reads process.env['=D:'] before process.cwd().
    let selected = windows_cwd::select(
        Some(b"D:"),
        |_| Some(PathBuf::from(r"D:\saved")),
        || panic!("cwd must remain lazy"),
    )
    .unwrap();
    assert_eq!(selected.as_os_str(), r"D:\saved");
    for env in [None, Some(PathBuf::new()), Some(PathBuf::from(r"C:\wrong"))] {
        let selected = windows_cwd::select(
            Some(b"D:"),
            |_| env.clone(),
            || Ok(PathBuf::from(r"C:\cwd")),
        )
        .unwrap();
        assert_eq!(selected.as_os_str(), r"D:\");
    }
    assert!(
        windows_cwd::select(
            Some(b"D:"),
            |_| None,
            || Err(std::io::Error::other("cwd failure"))
        )
        .is_err()
    );
}

#[test]
fn windows_reserved_names_match_official_node_utf16_slice() {
    // Node win32.normalize's final reserved-name check uses slice(0, -1).
    for (input, expected) in [
        ("CONé", ".\\CONé"),
        ("COM1é", ".\\COM1é"),
        ("NUL中", ".\\NUL中"),
        ("CON𐀀", "CON𐀀"),
        ("CONx", ".\\CONx"),
    ] {
        assert_eq!(
            windows_normalize::normalize(Path::new(input)).as_os_str(),
            expected
        );
    }
    assert_eq!(
        windows_relative::from_resolved(Path::new(r"C:\Ä\a"), Path::new(r"c:\ä\b")).as_os_str(),
        r"..\b"
    );
}

#[cfg(windows)]
#[test]
fn windows_native_entrypoints_match_official_node_disputed_oracle() {
    use super::super::{SugarPath, SugarPathBuf};
    use crate::utils::fs_operations::native;
    assert_eq!(
        native::join_path(Path::new(".."), Path::new("C:")).as_os_str(),
        r".\..\C:"
    );
    assert_eq!(
        native::resolve_path(Path::new(r"\\.\pipe\name"), Path::new("/"))
            .unwrap()
            .as_os_str(),
        r"\\.\"
    );
    assert_eq!(
        Path::new(r"\\?\C:\").try_absolutize().unwrap().as_os_str(),
        r"\\?\C:"
    );
    assert_eq!(
        Path::new(r"C:\base\C:foo").relative(r"C:\base").as_os_str(),
        "C:foo"
    );
    assert_eq!(
        PathBuf::from(r"\\?\C:\a\..\..").normalize().as_os_str(),
        r"\\?\"
    );
}

#[test]
fn windows_variadic_join_matches_official_node_single_normalization() {
    let args = [Path::new("C:\\"), Path::new("a"), Path::new("CON:")];
    assert_eq!(
        windows_normalize::join_many(&args).as_os_str(),
        "C:\\\\a\\CON:"
    );
    #[cfg(windows)]
    assert_eq!(
        crate::utils::fs_operations::native::join_paths(&args).as_os_str(),
        "C:\\\\a\\CON:"
    );
}

#[test]
fn windows_explicit_cwd_uses_node_root_grammar() {
    for cwd in [
        r"C:\base",
        "C:/base",
        "//a//b/base",
        r"\\a\b",
        r"\\?\C:\base",
    ] {
        assert!(
            windows_lexical::is_fully_qualified(Path::new(cwd)),
            "{cwd:?}"
        );
    }
    for cwd in ["", ".", "C:base", r"\base", "//server", "///a/b"] {
        assert!(
            !windows_lexical::is_fully_qualified(Path::new(cwd)),
            "{cwd:?}"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_explicit_unc_cwd_matches_official_node() {
    use super::super::SugarPath;
    let cwd = Path::new("//a//b/base");
    assert_eq!(
        Path::new("/child").relative_with(".", cwd).as_os_str(),
        r"..\child"
    );
    assert_eq!(
        Path::new("/child").absolutize_with(cwd).as_os_str(),
        r"\\a\b\child"
    );
    assert_eq!(
        Path::new("child").absolutize_with(cwd).as_os_str(),
        r"\\a\b\base\child"
    );
}
