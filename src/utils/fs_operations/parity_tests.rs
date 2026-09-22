//! Source-derived capability matrix: CC utils/fsOperations.ts:384-631.
use super::*;
use futures::StreamExt;
use std::{io, path::Path, sync::Arc};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn native_methods_matches_official_files_directories_and_links() {
    let fs = NodeFsOperations;
    let root = std::env::temp_dir().join(format!("fs-interface-{}", uuid::Uuid::new_v4()));
    fs.mkdir(&root, Some(0o700)).await.unwrap();
    assert!(fs.exists_sync(&root));
    assert_eq!(fs.cwd().unwrap(), std::env::current_dir().unwrap());
    assert!(fs.is_dir_empty_sync(&root).unwrap());
    let path = root.join("a");
    fs.append_file_sync(&path, &"abc".into(), Some(0o600))
        .unwrap();
    fs.append_file_sync(&path, &"def".into(), Some(0o777))
        .unwrap();
    #[cfg(unix)]
    assert_eq!(fs.stat_sync(&path).unwrap().mode & 0o777, 0o600);
    assert_eq!(
        fs.read_file_sync(&path, BufferEncoding::Utf8).unwrap(),
        "abcdef"
    );
    assert_eq!(
        fs.read_file(&path, BufferEncoding::Hex).await.unwrap(),
        "616263646566"
    );
    assert_eq!(fs.read_file_bytes_sync(&path).unwrap(), b"abcdef");
    assert_eq!(fs.read_file_bytes(&path, Some(3.)).await.unwrap(), b"abc");
    assert_eq!(fs.read_file_bytes(&path, None).await.unwrap(), b"abcdef");
    assert_eq!(fs.stat(&path).await.unwrap().len(), 6);
    let read = fs.read_sync(&path, 9).unwrap();
    assert_eq!(read.bytes_read, 6);
    assert_eq!(read.buffer, b"abcdef\0\0\0");
    let copy = root.join("b");
    fs.copy_file_sync(&path, &copy).unwrap();
    fs.copy_file_sync(&path, &path).unwrap();
    assert_eq!(fs.read_file_bytes_sync(&path).unwrap(), b"abcdef");
    fs.rename_sync(&copy, &root.join("c")).unwrap();
    fs.rename(&root.join("c"), &copy).await.unwrap();
    let hard = root.join("hard");
    fs.link_sync(&path, &hard).unwrap();
    fs.append_file_sync(&hard, &"!".into(), None).unwrap();
    assert_eq!(fs.stat_sync(&path).unwrap().len(), 7);
    #[cfg(unix)]
    {
        let link = root.join("link");
        fs.symlink_sync(Path::new("a"), &link, Some(SymlinkType::File))
            .unwrap();
        assert!(fs.lstat_sync(&link).unwrap().file_type().is_symlink());
        assert!(fs.stat_sync(&link).unwrap().is_file());
        assert_eq!(fs.readlink_sync(&link).unwrap(), Path::new("a"));
        assert_eq!(
            fs.realpath_sync(&link).unwrap(),
            fs.realpath_sync(&path).unwrap()
        );
        fs.unlink_sync(&link).unwrap();
    }
    assert_eq!(fs.readdir_string_sync(&root).unwrap(), ["a", "b", "hard"]);
    assert_eq!(fs.readdir_sync(&root).unwrap().len(), 3);
    assert_eq!(fs.readdir(&root).await.unwrap().len(), 3);
    fs.unlink(&copy).await.unwrap();
    fs.rm_sync(&hard, RmOptions::default()).unwrap();
    fs.mkdir_sync(&root.join("empty"), None).unwrap();
    fs.rmdir_sync(&root.join("empty")).unwrap();
    fs.mkdir(&root.join("empty"), None).await.unwrap();
    fs.rmdir(&root.join("empty")).await.unwrap();
    let mut stream = fs.create_write_stream(&path);
    stream.write_all(b"stream").await.unwrap();
    stream.shutdown().await.unwrap();
    assert_eq!(fs.read_file_bytes_sync(&path).unwrap(), b"stream");
    fs.rm(
        &root,
        RmOptions {
            recursive: true,
            force: false,
        },
    )
    .await
    .unwrap();
    fs.rm(
        &root,
        RmOptions {
            recursive: true,
            force: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        fs.stat_sync(&root).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[tokio::test]
async fn async_operations_matches_official_eager_promise_side_effects() {
    let path = std::env::temp_dir().join(format!("fs-eager-{}", uuid::Uuid::new_v4()));
    drop(NodeFsOperations.mkdir(&path, None));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !path.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    NodeFsOperations.rmdir(&path).await.unwrap();
}

#[tokio::test]
async fn reverse_lines_matches_official_raw_utf8_chunks_and_empty_lines() {
    let path = std::env::temp_dir().join(format!("fs-reverse-{}", uuid::Uuid::new_v4()));
    let first = format!("{}😀", "a".repeat(4093));
    std::fs::write(&path, format!("{first}\n\nsecond\r\nlast\n")).unwrap();
    let lines: Vec<_> = read_lines_reverse(&path).collect().await;
    assert_eq!(
        lines.into_iter().collect::<io::Result<Vec<_>>>().unwrap(),
        ["last".to_owned(), "second\r".to_owned(), first]
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn windows_realpath_matches_official_win32_spelling_without_weakening_inputs() {
    assert_eq!(
        native::node_windows_realpath(r"\\?\E:\work\hello.txt"),
        r"E:\work\hello.txt"
    );
    assert_eq!(
        native::node_windows_realpath(r"\\?\UNC\host\share\file"),
        r"\\host\share\file"
    );
    assert_eq!(
        native::node_windows_realpath(r"E:\work\hello.txt"),
        r"E:\work\hello.txt"
    );
    // Raw user-supplied UNC/device paths remain rejected before any filesystem access.
    let raw = Path::new(r"\\?\E:\work\hello.txt");
    assert!(!safe_resolve_path(get_fs_implementation().as_ref(), raw).is_canonical);
}

#[test]
fn reset_matches_official_original_object_identity_and_preserves_cwd() {
    set_original_fs_implementation();
    let original = get_fs_implementation();
    let cwd = std::env::current_dir().unwrap();
    set_fs_implementation(Arc::new(NodeFsOperations));
    assert!(!Arc::ptr_eq(&original, &get_fs_implementation()));
    set_original_fs_implementation();
    assert!(Arc::ptr_eq(&original, &get_fs_implementation()));
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn encodings_matches_official_buffer_decoding() {
    assert_eq!(BufferEncoding::Utf8.decode(&[0xe2, 0x82]), "�");
    assert_eq!(BufferEncoding::Ascii.decode(&[0x80, 0xff]), "\0\u{7f}");
    assert_eq!(BufferEncoding::Latin1.decode(&[0x80, 0xff]), "\u{80}ÿ");
    assert_eq!(BufferEncoding::Base64.decode(&[0xfb, 0xff]), "+/8=");
    assert_eq!(BufferEncoding::Base64Url.decode(&[0xfb, 0xff]), "-_8");
    assert_eq!(BufferEncoding::Utf16Le.decode(&[0x61, 0, 0x62]), "a");
}

#[test]
fn utf16_read_matches_official_lone_surrogate_code_units() {
    let text = BufferEncoding::Utf16Le.decode_text(&[0x00, 0xd8, 0x61, 0x00]);
    assert_eq!(text.code_units(), &[0xd800, 0x61]);
    assert!(text.into_string().is_err());
}

#[test]
fn realpath_matches_official_lexical_dotdot_before_symlink_resolution() {
    let root = std::env::temp_dir().join(format!("fs-realpath-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("target/inside")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("target/inside"), root.join("link")).unwrap();
        assert_eq!(
            NodeFsOperations
                .realpath_sync(&root.join("link/.."))
                .unwrap(),
            NodeFsOperations.realpath_sync(&root).unwrap()
        );
    }
    assert_eq!(
        NodeFsOperations
            .realpath_sync(&root.join("missing/.."))
            .unwrap(),
        NodeFsOperations.realpath_sync(&root).unwrap()
    );
    assert_eq!(
        NodeFsOperations.realpath_sync(Path::new("")).unwrap(),
        NodeFsOperations
            .realpath_sync(&std::env::current_dir().unwrap())
            .unwrap()
    );
    assert_eq!(
        NodeFsOperations
            .mkdir_sync(Path::new(""), None)
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_permission_candidates_matches_official_ordinary_files_are_safe() {
    use crate::utils::permissions::filesystem::{
        PathSafetyForAutoEdit, check_path_safety_for_auto_edit,
    };
    let root = std::env::temp_dir().join(format!("fs-permission-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("existing.txt"), "text").unwrap();
    for path in [root.join("existing.txt"), root.join("new.txt")] {
        let candidates = get_paths_for_permission_check(&path);
        assert!(
            candidates
                .iter()
                .all(|path| !path.to_string_lossy().starts_with(r"\\?\"))
        );
        assert!(matches!(
            check_path_safety_for_auto_edit(&path.to_string_lossy(), None),
            PathSafetyForAutoEdit::Safe
        ));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn copy_file_directory_to_itself_fails() {
    let root = std::env::temp_dir().join(format!("fs-parity-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let fs = NodeFsOperations;
    assert!(fs.copy_file_sync(&root, &root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn write_stream_open_error_retains_node_fields() {
    let root = std::env::temp_dir().join(format!("fs-parity-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("missing/file");
    let mut stream = NodeFsOperations.create_write_stream(&path);
    let error = stream.write_all(b"x").await.unwrap_err();
    let error = error.get_ref().unwrap().downcast_ref::<FsError>().unwrap();
    assert_eq!(error.code, "ENOENT");
    assert_eq!(error.syscall, Some("open"));
    assert_eq!(error.path.as_deref(), Some(path.as_path()));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn lexical_path_dependencies_match_official_bun() {
    // CC fsOperations.ts imports native path.join/dirname/resolve. Bun 1.3.14
    // oracle: docs/research/sugar-path-bun-2026-09-21/evidence.tar.gz.
    // Compare spelling: Path equality itself ignores a trailing separator.
    assert_eq!(
        native::join_path(Path::new("a/"), Path::new("")).as_os_str(),
        "a/"
    );
    assert_eq!(
        native::join_path(Path::new("a/../"), Path::new("")).as_os_str(),
        "./"
    );
    assert_eq!(
        native::join_path(Path::new("a"), Path::new("/b")).as_os_str(),
        "a/b"
    );
    for root in ["//", "///"] {
        assert_eq!(native::dirname(Path::new(root)).as_os_str(), "/");
    }
    assert_eq!(native::dirname(Path::new("//a")).as_os_str(), "//");
    assert_eq!(
        native::resolve_path(Path::new("/a"), Path::new("b/"))
            .unwrap()
            .as_os_str(),
        "/a/b"
    );
}

#[cfg(windows)]
#[test]
fn windows_path_consumers_match_official_bun() {
    use super::native;
    use std::path::Path;
    for (base, target, expected) in [
        (r"C:\base", "C:foo", r"C:\base\foo"),
        (r"C:\base", "c:", r"c:\base"),
        ("//a//b/", "/child", r"\\a\b\child"),
        ("//a//b/", r"\child", r"\\a\b\child"),
    ] {
        assert_eq!(
            native::resolve_path(Path::new(base), Path::new(target))
                .unwrap()
                .as_os_str(),
            Path::new(expected).as_os_str()
        );
    }
    assert_eq!(
        native::join_path(Path::new("/"), Path::new("a/.")).as_os_str(),
        Path::new(r"\a").as_os_str()
    );
    assert_eq!(
        native::join_path(Path::new("//server"), Path::new("share")).as_os_str(),
        Path::new("\\\\server\\share\\").as_os_str()
    );
    assert_eq!(
        native::dirname(Path::new(r"\\server\share")).as_os_str(),
        Path::new(r"\\server\share").as_os_str()
    );
    assert_eq!(native::basename(Path::new("C:foo")), "foo");
    assert!(native::is_absolute(Path::new(r"\foo")));
    // Source expandPath must choose normalize for root-relative input, not resolve.
    assert_eq!(
        crate::utils::path::expand_path(r"\foo", Some(Path::new(r"C:\base")))
            .unwrap()
            .as_os_str(),
        Path::new(r"\foo").as_os_str()
    );
}
