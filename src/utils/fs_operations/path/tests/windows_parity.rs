use super::super::windows_syntax;
use std::path::Path;

#[test]
fn windows_names_match_official_bun_returned_oracle() {
    // Original Windows Bun 1.4.2 output from windows-path-results.zip, one-bun.jsonl.
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("windows_bun_names.json")).unwrap();
    assert_eq!(rows.len(), 160);
    for row in rows {
        let input = Path::new(row["args"][0].as_str().unwrap());
        let actual = match row["op"].as_str().unwrap() {
            "dirname" => serde_json::json!(windows_syntax::dirname(input).to_string_lossy()),
            "basename" => serde_json::json!(windows_syntax::basename(input).to_string_lossy()),
            "isAbsolute" => serde_json::json!(windows_syntax::is_absolute(input)),
            _ => unreachable!(),
        };
        assert_eq!(actual, row["value"], "{row}");
    }
}

#[test]
fn windows_root_preparation_matches_official_scans() {
    // Node win32 join's leading-separator guard and UNC root scan, before normalization.
    assert_eq!(
        windows_syntax::join_input(Path::new("/"), Path::new("a/.")),
        Path::new(r"\a/.")
    );
    assert_eq!(
        windows_syntax::join_input(Path::new("//server"), Path::new("share")),
        Path::new("//server\\share")
    );
    assert_eq!(
        windows_syntax::canonical_unc(Path::new("//a//b/")),
        Some(Path::new("\\\\a\\b/").to_owned())
    );
}

#[cfg(windows)]
#[test]
fn windows_unc_dependency_apis_match_official_bun() {
    use super::super::{SugarPath, SugarPathBuf};
    let malformed = Path::new("//a//b/");
    assert_eq!(
        malformed.normalize().as_os_str(),
        Path::new("\\\\a\\b\\").as_os_str()
    );
    assert_eq!(
        malformed.to_owned().into_normalized().as_os_str(),
        Path::new("\\\\a\\b\\").as_os_str()
    );
    assert_eq!(
        malformed.try_absolutize().unwrap().as_os_str(),
        Path::new("\\\\a\\b\\").as_os_str()
    );
    assert_eq!(
        Path::new("/child").absolutize_with(malformed).as_os_str(),
        Path::new(r"\\a\b\child").as_os_str()
    );
    assert_eq!(
        malformed.relative(Path::new(r"C:\base")).as_os_str(),
        Path::new("\\\\a\\b\\").as_os_str()
    );
    assert_eq!(
        Path::new("/child")
            .relative_with(".", Path::new("//a//b/base"))
            .as_os_str(),
        Path::new(r"..\child").as_os_str()
    );
}

#[cfg(windows)]
#[test]
fn windows_unc_repair_preserves_native_surrogates() {
    use super::super::{SugarPath, SugarPathBuf};
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let input: Vec<u16> = "//server//"
        .encode_utf16()
        .chain([0xd800])
        .chain("/".encode_utf16())
        .collect();
    let path = std::path::PathBuf::from(OsString::from_wide(&input));
    let expected: Vec<u16> = "\\\\server\\"
        .encode_utf16()
        .chain([0xd800])
        .chain("\\".encode_utf16())
        .collect();
    assert_eq!(
        path.normalize()
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        path.into_normalized()
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        expected
    );
}
