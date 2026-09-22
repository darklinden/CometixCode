//! Synchronous text-file reads with encoding and line-ending metadata.
//!
//! Maps to: CC `utils/fileRead.ts:1-102`.
//! This leaf owner intentionally depends only on filesystem/debug primitives;
//! Write/Edit consume it without reimplementing encoding detection.

use crate::utils::fs_operations::{BufferEncoding, get_fs_implementation, safe_resolve_path};
use std::path::Path;

/// Maps to CC `BufferEncoding` values observable in file-tool paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileEncoding {
    Utf8,
    Utf16Le,
}

impl From<FileEncoding> for BufferEncoding {
    fn from(encoding: FileEncoding) -> Self {
        match encoding {
            FileEncoding::Utf8 => Self::Utf8,
            FileEncoding::Utf16Le => Self::Utf16Le,
        }
    }
}

/// Maps to CC `LineEndingType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEndingType {
    Lf,
    CrLf,
}

/// Maps to CC `readFileSyncWithMetadata(...)` result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileReadMetadata {
    pub content: String,
    pub encoding: FileEncoding,
    pub line_endings: LineEndingType,
}

/// Maps to CC `detectEncodingForResolvedPath(resolvedPath)`.
pub fn detect_encoding_for_resolved_path(path: &Path) -> std::io::Result<FileEncoding> {
    let result = get_fs_implementation().read_sync(path, 4096)?;
    let buffer = result.buffer;
    let bytes_read = result.bytes_read;
    if bytes_read >= 2 && buffer[..2] == [0xff, 0xfe] {
        Ok(FileEncoding::Utf16Le)
    } else {
        // Empty files and UTF-8 BOM files both use UTF-8. The BOM is decoded as
        // content, matching Node's `readFileSync(..., { encoding: 'utf8' })`.
        Ok(FileEncoding::Utf8)
    }
}

/// Maps to CC `detectLineEndingsForString(content)`.
pub fn detect_line_endings_for_string(content: &str) -> LineEndingType {
    let bytes = content.as_bytes();
    let mut crlf_count = 0usize;
    let mut lf_count = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            if index > 0 && bytes[index - 1] == b'\r' {
                crlf_count += 1;
            } else {
                lf_count += 1;
            }
        }
    }
    if crlf_count > lf_count {
        LineEndingType::CrLf
    } else {
        LineEndingType::Lf
    }
}

fn first_utf16_code_units(value: &str, maximum_units: usize) -> String {
    let mut units = 0usize;
    value
        .chars()
        .take_while(|character| {
            let width = character.len_utf16();
            if units + width > maximum_units {
                return false;
            }
            units += width;
            true
        })
        .collect()
}

/// Maps to CC `readFileSyncWithMetadata(filePath)`.
pub fn read_file_sync_with_metadata(file_path: &Path) -> std::io::Result<FileReadMetadata> {
    let fs = get_fs_implementation();
    let resolved = safe_resolve_path(fs.as_ref(), file_path);
    if resolved.is_symlink {
        crate::utils::debug::log_for_debugging(&format!(
            "Reading through symlink: {} -> {}",
            file_path.display(),
            resolved.resolved_path.display()
        ));
    }

    let encoding = detect_encoding_for_resolved_path(&resolved.resolved_path)?;
    let raw = fs.read_file_sync(&resolved.resolved_path, encoding.into())?;
    // JS `raw.slice(0, 4096)` counts UTF-16 code units, not Unicode scalar
    // values. Preserve that boundary before line-ending detection.
    let raw = raw.to_string_lossy();
    let sample = first_utf16_code_units(&raw, 4096);
    let line_endings = detect_line_endings_for_string(&sample);
    Ok(FileReadMetadata {
        content: raw.replace("\r\n", "\n"),
        encoding,
        line_endings,
    })
}

/// Maps to CC `readFileSync(filePath)`.
pub fn read_file_sync(file_path: &Path) -> std::io::Result<String> {
    read_file_sync_with_metadata(file_path).map(|metadata| metadata.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_read_matches_official_encoding_and_line_normalization() {
        let root = std::env::temp_dir().join(format!(
            "cometix-file-read-metadata-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let utf8 = root.join("utf8.txt");
        std::fs::write(&utf8, b"a\r\nb\r\n").unwrap();
        let metadata = read_file_sync_with_metadata(&utf8).unwrap();
        assert_eq!(metadata.encoding, FileEncoding::Utf8);
        assert_eq!(metadata.line_endings, LineEndingType::CrLf);
        assert_eq!(metadata.content, "a\nb\n");

        let utf16 = root.join("utf16.txt");
        let mut bytes = vec![0xff, 0xfe];
        for unit in "a\r\nb\r\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(&utf16, bytes).unwrap();
        let metadata = read_file_sync_with_metadata(&utf16).unwrap();
        assert_eq!(metadata.encoding, FileEncoding::Utf16Le);
        assert_eq!(metadata.line_endings, LineEndingType::CrLf);
        assert_eq!(metadata.content, "\u{feff}a\nb\n");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_sample_boundary_counts_javascript_utf16_code_units() {
        let raw = format!("{}{}", "😀".repeat(2_050), "\r\n".repeat(8));
        let sample = first_utf16_code_units(&raw, 4_096);
        assert_eq!(sample.encode_utf16().count(), 4_096);
        assert!(!sample.contains('\n'));
        assert_eq!(detect_line_endings_for_string(&sample), LineEndingType::Lf);
    }

    #[test]
    fn mixed_line_endings_use_strict_crlf_majority_like_official() {
        assert_eq!(
            detect_line_endings_for_string("a\r\nb\nc\r\n"),
            LineEndingType::CrLf
        );
        assert_eq!(
            detect_line_endings_for_string("a\r\nb\n"),
            LineEndingType::Lf
        );
    }
}
