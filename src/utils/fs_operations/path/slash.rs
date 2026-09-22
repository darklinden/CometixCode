use memchr::memchr;
use std::path::PathBuf;

fn replace_main_separator_in_owned(mut string: String) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        string
    } else {
        let mut offset = 0;
        while let Some(position) = memchr(
            std::path::MAIN_SEPARATOR as u8,
            &string.as_bytes()[offset..],
        ) {
            let separator = offset + position;
            string.replace_range(separator..=separator, "/");
            offset = separator + 1;
        }
        string
    }
}

pub(super) fn try_path_buf_into_slash(path: PathBuf) -> Result<String, PathBuf> {
    match path.into_os_string().into_string() {
        Ok(string) => Ok(replace_main_separator_in_owned(string)),
        Err(path) => Err(PathBuf::from(path)),
    }
}

pub(super) fn path_buf_into_slash(path: PathBuf) -> String {
    try_path_buf_into_slash(path).expect("path is not valid Unicode")
}

pub(super) fn path_buf_into_slash_lossy(path: PathBuf) -> String {
    match try_path_buf_into_slash(path) {
        Ok(string) => string,
        Err(path) => replace_main_separator_in_owned(path.to_string_lossy().into_owned()),
    }
}

pub(super) fn replace_main_separator(input: &str) -> Option<String> {
    let sep = std::path::MAIN_SEPARATOR;
    let mut replaced: Option<String> = None;
    let mut segment_start = 0;

    for (idx, ch) in input.char_indices() {
        if ch == sep {
            let buf = replaced.get_or_insert_with(|| String::with_capacity(input.len()));
            buf.push_str(&input[segment_start..idx]);
            buf.push('/');
            segment_start = idx + ch.len_utf8();
        }
    }

    if let Some(mut buf) = replaced {
        buf.push_str(&input[segment_start..]);
        Some(buf)
    } else {
        None
    }
}
