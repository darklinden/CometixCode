//! Line-oriented, bounded file reads for the Read tool.
//!
//! Maps to: CC `utils/readFileInRange.ts:1-383`.
//!
//! The public function keeps the two official paths: regular files below
//! 10 MiB are decoded and split in memory, while larger/special files are
//! scanned in 512 KiB chunks. Only lines in the requested range are retained
//! on the streaming path. Rust performs the synchronous filesystem work on a
//! `spawn_blocking` worker at the FileReadTool call site.

use crate::tool::AbortController;
use crate::utils::format::format_file_size;
use std::io::Read as _;
use std::path::Path;

/// Maps to CC `FAST_PATH_MAX_SIZE`.
pub const FAST_PATH_MAX_SIZE: u64 = 10 * 1024 * 1024;
const STREAM_CHUNK_SIZE: usize = 512 * 1024;

/// Maps to CC `ReadFileRangeResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadFileRangeResult {
    pub content: String,
    pub line_count: usize,
    pub total_lines: usize,
    pub total_bytes: u64,
    pub read_bytes: u64,
    pub mtime_ms: f64,
    pub truncated_by_bytes: bool,
}

/// Maps to CC `FileTooLargeError`.
#[derive(Clone, Debug, PartialEq)]
pub struct FileTooLargeError {
    pub size_in_bytes: u64,
    pub max_size_bytes: f64,
}

impl std::fmt::Display for FileTooLargeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "File content ({}) exceeds maximum allowed size ({}). Use offset and limit parameters to read specific portions of the file, or search for specific content instead of reading the whole file.",
            format_file_size(self.size_in_bytes),
            format_file_size(self.max_size_bytes),
        )
    }
}

impl std::error::Error for FileTooLargeError {}

#[derive(Debug)]
pub enum ReadFileInRangeError {
    Io(std::io::Error),
    IoAtPath {
        source: std::io::Error,
        operation: &'static str,
        path: std::path::PathBuf,
    },
    FileTooLarge(FileTooLargeError),
    AbortedBeforeIo,
    Aborted,
}

impl ReadFileInRangeError {
    pub fn io_error(&self) -> Option<&std::io::Error> {
        match self {
            Self::Io(error) | Self::IoAtPath { source: error, .. } => Some(error),
            Self::FileTooLarge(_) | Self::AbortedBeforeIo | Self::Aborted => None,
        }
    }

    fn at_path(self, operation: &'static str, path: &Path) -> Self {
        match self {
            Self::Io(source) => Self::IoAtPath {
                source,
                operation,
                path: path.to_path_buf(),
            },
            error => error,
        }
    }
}

impl std::fmt::Display for ReadFileInRangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::IoAtPath {
                source,
                operation,
                path,
            } => f.write_str(&crate::utils::errors::format_native_file_error(
                source,
                operation,
                Some(path),
            )),
            Self::FileTooLarge(error) => error.fmt(f),
            Self::AbortedBeforeIo => f.write_str("This operation was aborted"),
            Self::Aborted => f.write_str("The operation was aborted"),
        }
    }
}

impl std::error::Error for ReadFileInRangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) | Self::IoAtPath { source: error, .. } => Some(error),
            Self::FileTooLarge(error) => Some(error),
            Self::AbortedBeforeIo | Self::Aborted => None,
        }
    }
}

impl From<std::io::Error> for ReadFileInRangeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Maps to CC `readFileInRange(..., options)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadFileRangeOptions {
    pub truncate_on_byte_limit: bool,
}

fn check_aborted_before_io(abort: Option<&AbortController>) -> Result<(), ReadFileInRangeError> {
    if abort.is_some_and(AbortController::is_aborted) {
        Err(ReadFileInRangeError::AbortedBeforeIo)
    } else {
        Ok(())
    }
}

fn check_aborted(abort: Option<&AbortController>) -> Result<(), ReadFileInRangeError> {
    if abort.is_some_and(AbortController::is_aborted) {
        Err(ReadFileInRangeError::Aborted)
    } else {
        Ok(())
    }
}

fn modified_time_ms(metadata: &std::fs::Metadata) -> f64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| {
            duration.as_secs() as f64 * 1_000.0 + f64::from(duration.subsec_nanos()) / 1_000_000.0
        })
        .unwrap_or(0.0)
}

fn open_fast_file(file_path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(file_path)
}

// Mechanical serde_json carriers for the JavaScript `offset + maxLines`
// operator written inline in CC `readFileInRangeFast` (:135) and
// `readFileInRangeStreaming` (:357); range policy remains in those functions.
// ECMAScript Number stringification delegates directly to `ryu-js`.
fn javascript_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value
            .as_f64()
            .map(|value| ryu_js::Buffer::new().format(value).to_string())
            .unwrap_or_else(|| value.to_string()),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(values) => values
            .iter()
            .map(|value| match value {
                serde_json::Value::Null => String::new(),
                value => javascript_value_to_string(value),
            })
            .collect::<Vec<_>>()
            .join(","),
        serde_json::Value::Object(_) => "[object Object]".to_string(),
    }
}

pub(crate) fn javascript_string_to_number(value: &str) -> f64 {
    let value = value.trim_matches(|character: char| {
        matches!(
            character,
            '\u{0009}'
                | '\u{000a}'
                | '\u{000b}'
                | '\u{000c}'
                | '\u{000d}'
                | '\u{0020}'
                | '\u{00a0}'
                | '\u{1680}'
                | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    });
    if value.is_empty() {
        return 0.0;
    }
    match value {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }

    for (lower, upper, radix) in [("0x", "0X", 16_u32), ("0b", "0B", 2), ("0o", "0O", 8)] {
        if let Some(digits) = value
            .strip_prefix(lower)
            .or_else(|| value.strip_prefix(upper))
        {
            if digits.is_empty() {
                return f64::NAN;
            }
            let mut number = 0.0;
            for digit in digits.chars() {
                let Some(digit) = digit.to_digit(radix) else {
                    return f64::NAN;
                };
                number = number * f64::from(radix) + f64::from(digit);
            }
            return number;
        }
    }

    let bytes = value.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'+') | Some(b'-')));
    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let mut has_digit = index > integer_start;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        has_digit |= index > fraction_start;
    }
    if !has_digit {
        return f64::NAN;
    }
    if matches!(bytes.get(index), Some(b'e') | Some(b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+') | Some(b'-')) {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            return f64::NAN;
        }
    }
    if index != bytes.len() {
        return f64::NAN;
    }
    value.parse::<f64>().unwrap_or(f64::NAN)
}

/// Maps to: CC `utils/readFileInRange.ts#readFileInRange` (:73-119).
pub fn read_file_in_range(
    file_path: &Path,
    offset: f64,
    max_lines: Option<serde_json::Value>,
    max_bytes: Option<f64>,
    abort: Option<&AbortController>,
    options: ReadFileRangeOptions,
) -> Result<ReadFileRangeResult, ReadFileInRangeError> {
    check_aborted_before_io(abort)?;
    let metadata = std::fs::metadata(file_path)
        .map_err(|error| ReadFileInRangeError::Io(error).at_path("stat", file_path))?;
    if metadata.is_dir() {
        return Err(ReadFileInRangeError::Io(std::io::Error::new(
            std::io::ErrorKind::IsADirectory,
            format!(
                "EISDIR: illegal operation on a directory, read '{}'",
                file_path.display()
            ),
        )));
    }
    if metadata.is_file() && metadata.len() < FAST_PATH_MAX_SIZE {
        if !options.truncate_on_byte_limit
            && max_bytes.is_some_and(|maximum| metadata.len() as f64 > maximum)
        {
            return Err(ReadFileInRangeError::FileTooLarge(FileTooLargeError {
                size_in_bytes: metadata.len(),
                max_size_bytes: max_bytes.unwrap_or_default(),
            }));
        }
        let mtime_ms = modified_time_ms(&metadata);
        let initial_capacity = metadata.len().min(FAST_PATH_MAX_SIZE);
        let mut file = open_fast_file(file_path)
            .map_err(|error| ReadFileInRangeError::Io(error).at_path("open", file_path))?;
        let mut bytes =
            Vec::with_capacity(usize::try_from(initial_capacity).unwrap_or(STREAM_CHUNK_SIZE));
        let mut chunk = vec![0u8; STREAM_CHUNK_SIZE];
        loop {
            check_aborted(abort)?;
            let read = file
                .read(&mut chunk)
                .map_err(|error| ReadFileInRangeError::Io(error).at_path("read", file_path))?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        check_aborted(abort)?;
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        return Ok(read_file_in_range_fast(
            &raw,
            mtime_ms,
            offset,
            max_lines.as_ref(),
            options
                .truncate_on_byte_limit
                .then_some(max_bytes)
                .flatten(),
        ));
    }

    read_file_in_range_streaming(
        file_path,
        &metadata,
        offset,
        max_lines.as_ref(),
        max_bytes,
        options.truncate_on_byte_limit,
        abort,
    )
}

fn try_push_selected_line(
    selected_lines: &mut Vec<String>,
    selected_bytes: &mut u64,
    truncated_by_bytes: &mut bool,
    truncate_at_bytes: Option<f64>,
    line: &str,
) -> bool {
    if let Some(maximum) = truncate_at_bytes {
        let separator = u64::from(!selected_lines.is_empty());
        let next_bytes = selected_bytes
            .saturating_add(separator)
            .saturating_add(line.len() as u64);
        if next_bytes as f64 > maximum {
            *truncated_by_bytes = true;
            return false;
        }
        *selected_bytes = next_bytes;
    }
    selected_lines.push(line.to_string());
    true
}

/// Maps to: CC `utils/readFileInRange.ts:128-343`
/// `readFileInRangeFast`.
fn read_file_in_range_fast(
    raw: &str,
    mtime_ms: f64,
    offset: f64,
    max_lines: Option<&serde_json::Value>,
    truncate_at_bytes: Option<f64>,
) -> ReadFileRangeResult {
    let end_line = match max_lines {
        None => f64::INFINITY,
        Some(serde_json::Value::Null) => offset,
        Some(serde_json::Value::Bool(value)) => offset + f64::from(u8::from(*value)),
        Some(serde_json::Value::Number(value)) => offset + value.as_f64().unwrap_or(f64::NAN),
        Some(serde_json::Value::String(value)) => javascript_string_to_number(&format!(
            "{}{}",
            ryu_js::Buffer::new().format(offset),
            value
        )),
        Some(value @ (serde_json::Value::Array(_) | serde_json::Value::Object(_))) => {
            javascript_string_to_number(&format!(
                "{}{}",
                ryu_js::Buffer::new().format(offset),
                javascript_value_to_string(value)
            ))
        }
    };
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut selected_lines = Vec::new();
    let mut line_index = 0usize;
    let mut start_pos = 0usize;
    let mut selected_bytes = 0u64;
    let mut truncated_by_bytes = false;

    while let Some(relative) = text[start_pos..].find('\n') {
        let newline_pos = start_pos + relative;
        if (line_index as f64) >= offset && (line_index as f64) < end_line && !truncated_by_bytes {
            let line = text[start_pos..newline_pos]
                .strip_suffix('\r')
                .unwrap_or(&text[start_pos..newline_pos]);
            let _ = try_push_selected_line(
                &mut selected_lines,
                &mut selected_bytes,
                &mut truncated_by_bytes,
                truncate_at_bytes,
                line,
            );
        }
        line_index = line_index.saturating_add(1);
        start_pos = newline_pos.saturating_add(1);
    }

    if (line_index as f64) >= offset && (line_index as f64) < end_line && !truncated_by_bytes {
        let fragment = text[start_pos..]
            .strip_suffix('\r')
            .unwrap_or(&text[start_pos..]);
        let _ = try_push_selected_line(
            &mut selected_lines,
            &mut selected_bytes,
            &mut truncated_by_bytes,
            truncate_at_bytes,
            fragment,
        );
    }
    line_index = line_index.saturating_add(1);

    let content = selected_lines.join("\n");
    ReadFileRangeResult {
        line_count: selected_lines.len(),
        total_lines: line_index,
        total_bytes: text.len() as u64,
        read_bytes: content.len() as u64,
        content,
        mtime_ms,
        truncated_by_bytes,
    }
}

/// Incremental UTF-8 decoder matching Node's replacement-character behavior
/// while retaining incomplete code units between 512 KiB chunks.
fn decode_utf8_chunk(bytes: &[u8], pending: &mut Vec<u8>, eof: bool) -> String {
    let mut joined = Vec::with_capacity(pending.len() + bytes.len());
    joined.extend_from_slice(pending);
    joined.extend_from_slice(bytes);
    pending.clear();

    let mut output = String::new();
    let mut remaining = joined.as_slice();
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                output.push_str(valid);
                remaining = &[];
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    output.push_str(std::str::from_utf8(&remaining[..valid_up_to]).unwrap_or(""));
                }
                match error.error_len() {
                    Some(length) => {
                        output.push('\u{fffd}');
                        remaining = &remaining[valid_up_to.saturating_add(length)..];
                    }
                    None if eof => {
                        output.push_str(&String::from_utf8_lossy(&remaining[valid_up_to..]));
                        remaining = &[];
                    }
                    None => {
                        pending.extend_from_slice(&remaining[valid_up_to..]);
                        remaining = &[];
                    }
                }
            }
        }
    }
    output
}

#[cfg(unix)]
fn open_stream_file(
    file_path: &Path,
    path_metadata: &std::fs::Metadata,
) -> Result<(std::fs::File, bool, Option<std::fs::Metadata>), std::io::Error> {
    use std::os::unix::fs::FileTypeExt as _;
    let file = std::fs::File::open(file_path)?;
    let opened_metadata = file.metadata().ok();
    let is_fifo = opened_metadata
        .as_ref()
        .unwrap_or(path_metadata)
        .file_type()
        .is_fifo();
    Ok((file, is_fifo, opened_metadata))
}

#[cfg(not(unix))]
fn open_stream_file(
    file_path: &Path,
    _path_metadata: &std::fs::Metadata,
) -> Result<(std::fs::File, bool, Option<std::fs::Metadata>), std::io::Error> {
    let file = std::fs::File::open(file_path)?;
    let opened_metadata = file.metadata().ok();
    Ok((file, false, opened_metadata))
}

#[cfg(unix)]
fn read_stream_chunk(
    file: &mut std::fs::File,
    is_fifo: bool,
    buffer: &mut [u8],
    abort: Option<&AbortController>,
) -> Result<usize, ReadFileInRangeError> {
    use std::os::fd::AsRawFd as _;
    if !is_fifo {
        return file.read(buffer).map_err(Into::into);
    }
    loop {
        check_aborted(abort)?;
        let mut descriptor = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        let status = unsafe { libc::poll(&mut descriptor, 1, 50) };
        if status < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if status == 0 {
            continue;
        }
        if descriptor.revents & libc::POLLIN != 0 {
            match file.read(buffer) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                result => return result.map_err(Into::into),
            }
        }
        if descriptor.revents & libc::POLLHUP != 0 {
            return Ok(0);
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
}

#[cfg(not(unix))]
fn read_stream_chunk(
    file: &mut std::fs::File,
    _is_fifo: bool,
    buffer: &mut [u8],
    abort: Option<&AbortController>,
) -> Result<usize, ReadFileInRangeError> {
    check_aborted(abort)?;
    file.read(buffer).map_err(Into::into)
}

/// Maps to: CC `utils/readFileInRange.ts:344-383`
/// `readFileInRangeStreaming`.
fn read_file_in_range_streaming(
    file_path: &Path,
    path_metadata: &std::fs::Metadata,
    offset: f64,
    max_lines: Option<&serde_json::Value>,
    max_bytes: Option<f64>,
    truncate_on_byte_limit: bool,
    abort: Option<&AbortController>,
) -> Result<ReadFileRangeResult, ReadFileInRangeError> {
    check_aborted(abort)?;
    let (mut file, is_fifo, opened_metadata) = open_stream_file(file_path, path_metadata)
        .map_err(|error| ReadFileInRangeError::Io(error).at_path("open", file_path))?;
    // CC resolves descriptor-metadata failure to zero; it never performs a
    // second path stat or substitutes the earlier path metadata.
    let mtime_ms = opened_metadata
        .as_ref()
        .map(modified_time_ms)
        .unwrap_or(0.0);
    let mut end_line = match max_lines {
        None => f64::INFINITY,
        Some(serde_json::Value::Null) => offset,
        Some(serde_json::Value::Bool(value)) => offset + f64::from(u8::from(*value)),
        Some(serde_json::Value::Number(value)) => offset + value.as_f64().unwrap_or(f64::NAN),
        Some(serde_json::Value::String(value)) => javascript_string_to_number(&format!(
            "{}{}",
            ryu_js::Buffer::new().format(offset),
            value
        )),
        Some(value @ (serde_json::Value::Array(_) | serde_json::Value::Object(_))) => {
            javascript_string_to_number(&format!(
                "{}{}",
                ryu_js::Buffer::new().format(offset),
                javascript_value_to_string(value)
            ))
        }
    };
    let mut total_bytes_read = 0u64;
    let mut selected_bytes = 0u64;
    let mut truncated_by_bytes = false;
    let mut current_line_index = 0usize;
    let mut selected_lines = Vec::<String>::new();
    let mut partial = String::new();
    let mut first_decoded_chunk = true;
    let mut pending_utf8 = Vec::new();
    let mut bytes = vec![0u8; STREAM_CHUNK_SIZE];
    loop {
        check_aborted(abort)?;
        let count = read_stream_chunk(&mut file, is_fifo, &mut bytes, abort)
            .map_err(|error| error.at_path("read", file_path))?;
        if count == 0 {
            break;
        }
        let mut chunk = decode_utf8_chunk(&bytes[..count], &mut pending_utf8, false);
        if first_decoded_chunk && !chunk.is_empty() {
            first_decoded_chunk = false;
            if let Some(without_bom) = chunk.strip_prefix('\u{feff}') {
                chunk = without_bom.to_string();
            }
        }
        total_bytes_read = total_bytes_read.saturating_add(chunk.len() as u64);
        if !truncate_on_byte_limit
            && max_bytes.is_some_and(|maximum| total_bytes_read as f64 > maximum)
        {
            return Err(ReadFileInRangeError::FileTooLarge(FileTooLargeError {
                size_in_bytes: total_bytes_read,
                max_size_bytes: max_bytes.unwrap_or_default(),
            }));
        }
        let data = if partial.is_empty() {
            chunk
        } else {
            let mut data = std::mem::take(&mut partial);
            data.push_str(&chunk);
            data
        };
        let mut start_pos = 0usize;
        while let Some(relative) = data[start_pos..].find('\n') {
            let newline_pos = start_pos + relative;
            if (current_line_index as f64) >= offset && (current_line_index as f64) < end_line {
                let raw_line = &data[start_pos..newline_pos];
                let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
                if truncate_on_byte_limit {
                    if let Some(maximum) = max_bytes {
                        let separator = u64::from(!selected_lines.is_empty());
                        let next_bytes = selected_bytes
                            .saturating_add(separator)
                            .saturating_add(line.len() as u64);
                        if next_bytes as f64 > maximum {
                            truncated_by_bytes = true;
                            end_line = current_line_index as f64;
                        } else {
                            selected_bytes = next_bytes;
                            selected_lines.push(line.to_string());
                        }
                    } else {
                        selected_lines.push(line.to_string());
                    }
                } else {
                    selected_lines.push(line.to_string());
                }
            }
            current_line_index = current_line_index.saturating_add(1);
            start_pos = newline_pos.saturating_add(1);
        }

        if start_pos < data.len()
            && (current_line_index as f64) >= offset
            && (current_line_index as f64) < end_line
        {
            let fragment = &data[start_pos..];
            if truncate_on_byte_limit {
                if let Some(maximum) = max_bytes {
                    let separator = u64::from(!selected_lines.is_empty());
                    let fragment_bytes = selected_bytes
                        .saturating_add(separator)
                        .saturating_add(fragment.len() as u64);
                    if fragment_bytes as f64 > maximum {
                        truncated_by_bytes = true;
                        end_line = current_line_index as f64;
                    } else {
                        partial.push_str(fragment);
                    }
                } else {
                    partial.push_str(fragment);
                }
            } else {
                partial.push_str(fragment);
            }
        }
    }

    let final_decoded = decode_utf8_chunk(&[], &mut pending_utf8, true);
    if !final_decoded.is_empty() {
        total_bytes_read = total_bytes_read.saturating_add(final_decoded.len() as u64);
        if !truncate_on_byte_limit
            && max_bytes.is_some_and(|maximum| total_bytes_read as f64 > maximum)
        {
            return Err(ReadFileInRangeError::FileTooLarge(FileTooLargeError {
                size_in_bytes: total_bytes_read,
                max_size_bytes: max_bytes.unwrap_or_default(),
            }));
        }
        if (current_line_index as f64) >= offset && (current_line_index as f64) < end_line {
            partial.push_str(&final_decoded);
        }
    }

    let line = partial.strip_suffix('\r').unwrap_or(&partial);
    if (current_line_index as f64) >= offset && (current_line_index as f64) < end_line {
        if truncate_on_byte_limit {
            if let Some(maximum) = max_bytes {
                let separator = u64::from(!selected_lines.is_empty());
                let next_bytes = selected_bytes
                    .saturating_add(separator)
                    .saturating_add(line.len() as u64);
                if next_bytes as f64 > maximum {
                    truncated_by_bytes = true;
                } else {
                    selected_lines.push(line.to_string());
                }
            } else {
                selected_lines.push(line.to_string());
            }
        } else {
            selected_lines.push(line.to_string());
        }
    }
    current_line_index = current_line_index.saturating_add(1);

    let content = selected_lines.join("\n");
    check_aborted(abort)?;
    Ok(ReadFileRangeResult {
        line_count: selected_lines.len(),
        total_lines: current_line_index,
        total_bytes: total_bytes_read,
        read_bytes: content.len() as u64,
        content,
        mtime_ms,
        truncated_by_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, content: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "cometix-read-range-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.txt");
        std::fs::write(&path, content).unwrap();
        (root, path)
    }

    #[test]
    fn javascript_string_number_conversion_matches_official_number_grammar() {
        assert!(javascript_string_to_number("-inf").is_nan());
        assert!(javascript_string_to_number("infinity").is_nan());
        assert_eq!(javascript_string_to_number("Infinity"), f64::INFINITY);
        assert_eq!(javascript_string_to_number("\u{feff}"), 0.0);
        assert!(javascript_string_to_number("\u{0085}").is_nan());
        assert_eq!(javascript_string_to_number("0x10"), 16.0);
        assert_eq!(javascript_string_to_number(".5"), 0.5);
        assert_eq!(javascript_string_to_number("5."), 5.0);
        assert_eq!(javascript_string_to_number("1e2"), 100.0);
    }

    #[test]
    fn fast_range_matches_official_bom_cr_and_final_fragment() {
        let (root, path) = temp_file("fast", b"\xef\xbb\xbfa\r\nb\rc\r");
        let result = read_file_in_range(
            &path,
            0.0,
            None,
            None,
            None,
            ReadFileRangeOptions::default(),
        )
        .unwrap();
        assert_eq!(result.content, "a\nb\rc");
        assert_eq!(result.line_count, 2);
        assert_eq!(result.total_lines, 2);
        assert_eq!(result.total_bytes, 7);
        assert_eq!(result.read_bytes, 5);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fast_range_counts_trailing_empty_line_like_official() {
        let (root, path) = temp_file("trailing", b"a\nb\n");
        let result = read_file_in_range(
            &path,
            1.0,
            Some(serde_json::json!(1)),
            None,
            None,
            ReadFileRangeOptions::default(),
        )
        .unwrap();
        assert_eq!(result.content, "b");
        assert_eq!(result.line_count, 1);
        assert_eq!(result.total_lines, 3);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fast_range_throws_exact_file_too_large_error() {
        let (root, path) = temp_file("large", b"12345");
        let error = read_file_in_range(
            &path,
            0.0,
            None,
            Some(4.0),
            None,
            ReadFileRangeOptions::default(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "File content (5 bytes) exceeds maximum allowed size (4 bytes). Use offset and limit parameters to read specific portions of the file, or search for specific content instead of reading the whole file."
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fractional_max_bytes_matches_official_number_boundary_and_copy() {
        let (root, path) = temp_file("fractional-limit", b"12345");
        let error = read_file_in_range(
            &path,
            0.0,
            None,
            Some(4.5),
            None,
            ReadFileRangeOptions::default(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "File content (5 bytes) exceeds maximum allowed size (4.5 bytes). Use offset and limit parameters to read specific portions of the file, or search for specific content instead of reading the whole file."
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn truncate_mode_stops_at_last_complete_line_that_fits() {
        let (root, path) = temp_file("truncate", b"aa\nbb\ncc");
        let result = read_file_in_range(
            &path,
            0.0,
            None,
            Some(5.0),
            None,
            ReadFileRangeOptions {
                truncate_on_byte_limit: true,
            },
        )
        .unwrap();
        assert_eq!(result.content, "aa\nbb");
        assert!(result.truncated_by_bytes);
        assert_eq!(result.total_lines, 3);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn native_stat_error_preserves_errno_operation_and_path() {
        let path = std::env::temp_dir().join(format!(
            "cometix-read-range-missing-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let error = read_file_in_range(
            &path,
            0.0,
            None,
            None,
            None,
            ReadFileRangeOptions::default(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "ENOENT: no such file or directory, stat '{}'",
                path.display()
            )
        );
        assert_eq!(
            crate::utils::errors::get_errno_code(&anyhow::Error::new(error)),
            Some("ENOENT")
        );
    }

    #[test]
    fn already_aborted_read_does_no_io() {
        let abort = AbortController::default();
        abort.abort();
        let error = read_file_in_range(
            Path::new("/definitely/not/read"),
            0.0,
            None,
            None,
            Some(&abort),
            ReadFileRangeOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(error, ReadFileInRangeError::AbortedBeforeIo));
        assert_eq!(error.to_string(), "This operation was aborted");
    }

    #[cfg(unix)]
    #[test]
    fn fifo_open_blocks_until_writer_then_observes_abort() {
        use std::ffi::CString;
        let root = std::env::temp_dir().join(format!(
            "cometix-read-range-fifo-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fifo = root.join("input.fifo");
        let c_path = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let abort = AbortController::default();
        let worker_abort = abort.clone();
        let worker_path = fifo.clone();
        let worker = std::thread::spawn(move || {
            read_file_in_range(
                &worker_path,
                0.0,
                None,
                None,
                Some(&worker_abort),
                ReadFileRangeOptions::default(),
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(75));
        abort.abort();
        let writer = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
        drop(writer);
        assert!(matches!(
            worker.join().unwrap(),
            Err(ReadFileInRangeError::Aborted)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
