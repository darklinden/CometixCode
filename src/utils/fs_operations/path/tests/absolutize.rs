use super::super::SugarPath;
use super::test_utils::{assert_eq_str, pb};
use std::path::PathBuf;
#[cfg(any(target_family = "unix", target_family = "windows"))]
use std::{borrow::Cow, path::Path};

fn get_cwd() -> PathBuf {
    std::env::current_dir().unwrap()
}

#[cfg(target_family = "unix")]
#[test]
fn unix() {
    assert_eq_str!("/var/lib/../file/".absolutize(), "/var/file");
    assert_eq!("a/b/c/../../..".absolutize(), get_cwd());
    assert_eq!(".".absolutize(), get_cwd());
    assert_eq!("".absolutize(), get_cwd());
    assert_eq!("a".absolutize(), get_cwd().join("a"));
    assert_eq_str!("/absolute/".absolutize(), "/absolute");
    assert_eq_str!(
        "/foo/tmp.3/../tmp.3/cycles/root.js".absolutize(),
        "/foo/tmp.3/cycles/root.js"
    );
    assert_eq_str!("/../file/".absolutize(), "/file");
}

#[cfg(target_family = "unix")]
#[test]
fn unix_absolute_inputs_preserve_cow_contract() {
    let clean = Path::new("/some/file");
    let clean_output = clean.absolutize();
    assert!(matches!(clean_output, Cow::Borrowed(_)));
    assert_eq!(clean_output.as_os_str(), clean.as_os_str());

    let dirty = Path::new("/some/../file/");
    let dirty_output = dirty.absolutize();
    assert!(matches!(dirty_output, Cow::Owned(_)));
    assert_eq!(dirty_output.as_os_str(), Path::new("/file").as_os_str());
}

#[test]
fn make_sure_dots_are_resolved() {
    assert_eq!("./main".absolutize(), get_cwd().join("main"));
}

#[cfg(target_family = "windows")]
#[test]
fn windows() {
    assert_eq!(".".absolutize(), get_cwd());
    assert_eq!("".absolutize(), get_cwd());
    // Fixed Node win32.resolve oracle: ambient per-drive state is covered separately.
    for (input, expected) in [
        ("c:../a", r"c:\base\a"),
        ("c:./a", r"c:\base\deep\a"),
        ("../a", r"C:\base\a"),
        ("./a", r"C:\base\deep\a"),
    ] {
        assert_eq_str!(input.absolutize_with(r"C:\base\deep"), expected);
    }
    assert_eq!("a".absolutize(), get_cwd().join("a"));

    assert_eq_str!("c:/ignore".absolutize(), "c:\\ignore");
    assert_eq_str!("c:\\some\\file".absolutize(), "c:\\some\\file");
    assert_eq!(
        "some/dir//".absolutize(),
        get_cwd().join("some").join("dir")
    );
    assert_eq_str!(
        "//server/share/../relative\\".absolutize(),
        "\\\\server\\share\\relative"
    );
    {
        let mut right = get_cwd();
        right.pop();
        right = right.join(pb!("tmp.3\\cycles\\root.js"));
        assert_eq!("..\\tmp.3\\cycles\\root.js".absolutize(), right);
    }
}

#[cfg(target_family = "windows")]
#[test]
fn windows_absolute_inputs_preserve_cow_contract() {
    let clean = Path::new("C:\\some\\file");
    let clean_output = clean.absolutize();
    assert!(matches!(clean_output, Cow::Borrowed(_)));
    assert_eq!(clean_output.as_os_str(), clean.as_os_str());

    let dirty = Path::new("c:/some/../file");
    let dirty_output = dirty.absolutize();
    assert!(matches!(dirty_output, Cow::Owned(_)));
    assert_eq!(dirty_output.as_os_str(), Path::new("c:\\file").as_os_str());
}
