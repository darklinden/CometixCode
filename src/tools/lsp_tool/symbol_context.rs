//! Port of official `tools/LSPTool/symbolContext.ts`.

const MAX_READ_BYTES: usize = 64 * 1024;

/// Character classes of CC's `/[\w$'!]+|[+\-*/%&|^~<>=]+/g`. Both alternatives
/// are ASCII-only, so UTF-16 units outside ASCII never open a run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SymbolClass {
    Word,
    Operator,
}

fn symbol_class(unit: u16) -> Option<SymbolClass> {
    let byte = u8::try_from(unit).ok()?;
    match byte {
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$' | b'\'' | b'!' => {
            Some(SymbolClass::Word)
        }
        b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'~' | b'<' | b'>' | b'=' => {
            Some(SymbolClass::Operator)
        }
        _ => None,
    }
}

fn read_head(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    let result = crate::utils::fs_operations::get_fs_implementation()
        .read_sync(path, MAX_READ_BYTES)?;
    let mut buffer = result.buffer;
    buffer.truncate(result.bytes_read);
    Ok(buffer)
}

/// Maps to: CC `tools/LSPTool/symbolContext.ts:21-90` `getSymbolAtPosition`.
///
/// `line` and `character` are 0-indexed; `character` counts UTF-16 units the
/// way the JavaScript regex offsets do.
pub(crate) fn get_symbol_at_position(file_path: &str, line: i64, character: i64) -> Option<String> {
    let absolute_path = match crate::utils::path::expand_path(file_path, None) {
        Ok(path) => path,
        Err(error) => {
            crate::utils::debug::log_for_debugging_with_level(
                &format!("Symbol extraction failed for {file_path}:{line}:{character}: {error}"),
                crate::utils::debug::DebugLogLevel::Warn,
            );
            return None;
        }
    };
    let buffer = match read_head(&absolute_path) {
        Ok(buffer) => buffer,
        Err(error) => {
            crate::utils::debug::log_for_debugging_with_level(
                &format!("Symbol extraction failed for {file_path}:{line}:{character}: {error}"),
                crate::utils::debug::DebugLogLevel::Warn,
            );
            return None;
        }
    };
    let bytes_read = buffer.len();
    let content = String::from_utf8_lossy(&buffer);
    let lines = content.split('\n').collect::<Vec<_>>();

    let index = usize::try_from(line).ok()?;
    if index >= lines.len() {
        return None;
    }
    // A full buffer means the file continues past the window, so the trailing
    // split element may be a partial line.
    if bytes_read == MAX_READ_BYTES && index == lines.len() - 1 {
        return None;
    }

    let units = lines[index].encode_utf16().collect::<Vec<_>>();
    let character = usize::try_from(character).ok()?;
    if units.is_empty() || character >= units.len() {
        return None;
    }

    let mut position = 0usize;
    while position < units.len() {
        let Some(class) = symbol_class(units[position]) else {
            position += 1;
            continue;
        };
        let start = position;
        while position < units.len() && symbol_class(units[position]) == Some(class) {
            position += 1;
        }
        if character >= start && character < position {
            let symbol = String::from_utf16_lossy(&units[start..position]);
            return Some(crate::utils::truncate::truncate(&symbol, 30, false));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cometix-symbol-context-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn extracts_word_and_operator_runs_at_the_official_offsets() {
        let path = temp_file("main.rs", b"fn compute(a: i32) -> i32 { a += 1 }\n");
        let file = path.display().to_string();
        assert_eq!(
            super::get_symbol_at_position(&file, 0, 3),
            Some("compute".to_string())
        );
        assert_eq!(
            super::get_symbol_at_position(&file, 0, 30),
            Some("+=".to_string())
        );
        assert_eq!(super::get_symbol_at_position(&file, 0, 10), None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn returns_none_outside_the_line_and_character_range() {
        let path = temp_file("short.rs", b"ab\n");
        let file = path.display().to_string();
        assert_eq!(super::get_symbol_at_position(&file, 5, 0), None);
        assert_eq!(super::get_symbol_at_position(&file, 0, 9), None);
        assert_eq!(super::get_symbol_at_position(&file, -1, 0), None);
        assert_eq!(super::get_symbol_at_position(&file, 0, -1), None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn counts_utf16_units_like_the_javascript_regex() {
        let path = temp_file("emoji.rs", "let \u{1F600} = value\n".as_bytes());
        let file = path.display().to_string();
        // "let " is 4 units, the emoji occupies 2, " = " lands on 6..9.
        assert_eq!(
            super::get_symbol_at_position(&file, 0, 9),
            Some("value".to_string())
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn truncates_long_symbols_to_thirty_columns() {
        let long = "a".repeat(60);
        let path = temp_file("long.rs", format!("{long}\n").as_bytes());
        let file = path.display().to_string();
        let symbol = super::get_symbol_at_position(&file, 0, 0).unwrap();
        assert_eq!(symbol, format!("{}…", "a".repeat(29)));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
