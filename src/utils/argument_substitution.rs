//! Maps to: CC `utils/argumentSubstitution.ts`.
//! ECMAScript regex/string operations use the existing regress UTF-16 carrier.

use crate::utils::bash::shell_quote::{ParseEntry, try_parse_shell_command};
use regress::{Match, Regex};
use serde_json::Value;
use std::sync::LazyLock;

/// Maps to: CC `parseArguments` (argumentSubstitution.ts:24–40).
pub fn parse_arguments(args: &str) -> Vec<String> {
    if args.is_empty() || NON_WHITESPACE.find(args).is_none() {
        return Vec::new();
    }
    // Canonical shellQuote already preserves variables as literal `$KEY`.
    match try_parse_shell_command(args) {
        Ok(tokens) => tokens
            .into_iter()
            .filter_map(|token| match token {
                ParseEntry::String(value) => Some(value),
                _ => None,
            })
            .collect(),
        Err(_) => NON_WHITESPACE
            .find_iter(args)
            .map(|m| args[m.range()].to_owned())
            .collect(),
    }
}

/// Maps to: CC `parseArgumentNames` (argumentSubstitution.ts:50–68).
pub fn parse_argument_names(argument_names: Option<&Value>) -> Vec<String> {
    let valid = |name: &str| {
        NON_WHITESPACE.find(name).is_some() && !name.bytes().all(|ch| ch.is_ascii_digit())
    };
    match argument_names {
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .filter(|name| valid(name))
            .map(str::to_owned)
            .collect(),
        Some(Value::String(names)) => NON_WHITESPACE
            .find_iter(names)
            .map(|m| &names[m.range()])
            .filter(|name| valid(name))
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Maps to: CC `generateProgressiveArgumentHint` (argumentSubstitution.ts:76–83).
pub fn generate_progressive_argument_hint(
    arg_names: &[String],
    typed_args: &[String],
) -> Option<String> {
    let remaining = arg_names.get(typed_args.len()..).unwrap_or_default();
    if remaining.is_empty() {
        return None;
    }
    Some(
        remaining
            .iter()
            .map(|name| format!("[{name}]"))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Maps to: CC `substituteArguments` (argumentSubstitution.ts:94–145).
/// L1: a thrown dynamic RegExp construction error becomes `Result::Err`.
/// Intermediate strings retain UTF-16 units across all four replacement passes.
/// Existing String boundary: a final lone surrogate is projected to U+FFFD;
/// Rust prompt strings cannot represent JavaScript's unpaired surrogate value.
pub fn substitute_arguments(
    content: &str,
    args: Option<&str>,
    append_if_no_placeholder: bool,
    argument_names: &[String],
) -> anyhow::Result<String> {
    let Some(args) = args else {
        return Ok(content.to_owned());
    };
    let parsed_args = parse_arguments(args);
    let original_content = content.encode_utf16().collect::<Vec<_>>();
    let mut content = original_content.clone();
    for (index, name) in argument_names.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        // Deliberately do not escape the name: CC inserts it into new RegExp.
        let pattern = format!(r"\${name}(?![\[\w])");
        let regex = Regex::from_unicode(
            pattern.encode_utf16().map(u32::from),
            regress::Flags::default(),
        )
        .map_err(|error| anyhow::anyhow!("Invalid regular expression: {error}"))?;
        let replacement = parsed_args
            .get(index)
            .map_or("", String::as_str)
            .encode_utf16()
            .collect::<Vec<_>>();
        content = replace_all(&content, &regex, |m| {
            expand_replacement(&content, m, &replacement)
        });
    }
    for regex in [&*INDEXED_ARGUMENT, &*SHORTHAND_ARGUMENT] {
        content = replace_all(&content, regex, |m| {
            let digits =
                String::from_utf16_lossy(&content[m.group(1).expect("fixed index capture")]);
            digits
                .parse::<usize>()
                .ok()
                .and_then(|index| parsed_args.get(index))
                .map(|value| value.encode_utf16().collect())
                .unwrap_or_default()
        });
    }
    let replacement = args.encode_utf16().collect::<Vec<_>>();
    content = replace_all(&content, &FULL_ARGUMENTS, |m| {
        expand_replacement(&content, m, &replacement)
    });
    if content == original_content && append_if_no_placeholder && !args.is_empty() {
        content.extend("\n\nARGUMENTS: ".encode_utf16());
        content.extend(args.encode_utf16());
    }
    Ok(String::from_utf16_lossy(&content))
}

// L1 built-in adapters for source /\s+/, String.replace and replaceAll.
// regress replacement strings follow a different contract ($0/${name}); only
// its ECMAScript matcher is reused here, never its replace_all expansion.
static NON_WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\S+").expect("fixed whitespace regex"));
static INDEXED_ARGUMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$ARGUMENTS\[(\d+)\]").expect("fixed index regex"));
static SHORTHAND_ARGUMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$(\d+)(?!\w)").expect("fixed shorthand regex"));
static FULL_ARGUMENTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$ARGUMENTS").expect("fixed arguments regex"));

/// L1 standard built-in representation, not an argumentSubstitution.ts export.
/// Shared String.replace(/pattern/g, replacementString) semantics for source
/// consumers; this does not share the argument-substitution business algorithm.
pub(crate) fn replace_all_with_string(content: &str, pattern: &Regex, replacement: &str) -> String {
    let content = content.encode_utf16().collect::<Vec<_>>();
    let replacement = replacement.encode_utf16().collect::<Vec<_>>();
    String::from_utf16_lossy(&replace_all(&content, pattern, |matched| {
        expand_replacement(&content, matched, &replacement)
    }))
}

/// L1: the UTF-16 form of String.prototype.replace/replaceAll match traversal.
fn replace_all(
    content: &[u16],
    regex: &Regex,
    replacement: impl Fn(&Match) -> Vec<u16>,
) -> Vec<u16> {
    let mut output = Vec::new();
    let mut end = 0;
    for matched in regex.find_from_ucs2(content, 0) {
        output.extend_from_slice(&content[end..matched.start()]);
        output.extend(replacement(&matched));
        end = matched.end();
    }
    output.extend_from_slice(&content[end..]);
    output
}

/// L1: ECMAScript GetSubstitution for the string replacement arguments in
/// argumentSubstitution.ts:116–119,136. Callback replacements skip this step.
fn expand_replacement(content: &[u16], matched: &Match, replacement: &[u16]) -> Vec<u16> {
    let mut output = Vec::new();
    let mut cursor = 0;
    while cursor < replacement.len() {
        if replacement[cursor] != u16::from(b'$') || cursor + 1 == replacement.len() {
            output.push(replacement[cursor]);
            cursor += 1;
            continue;
        }
        let next = replacement[cursor + 1];
        match next {
            0x24 => output.push(u16::from(b'$')),
            0x26 => output.extend_from_slice(&content[matched.range()]),
            0x60 => output.extend_from_slice(&content[..matched.start()]),
            0x27 => output.extend_from_slice(&content[matched.end()..]),
            0x3c if matched.named_groups().next().is_some() => {
                if let Some(end) = replacement[cursor + 2..]
                    .iter()
                    .position(|c| *c == u16::from(b'>'))
                {
                    let end = cursor + 2 + end;
                    let name = String::from_utf16_lossy(&replacement[cursor + 2..end]);
                    if let Some(range) = matched.named_group(&name) {
                        output.extend_from_slice(&content[range]);
                    }
                    cursor = end + 1;
                    continue;
                }
                output.push(u16::from(b'$'));
                cursor += 1;
                continue;
            }
            0x30..=0x39 => {
                let first = usize::from(next - u16::from(b'0'));
                let mut index = first;
                let mut digits = 1;
                if let Some(second @ 0x30..=0x39) = replacement.get(cursor + 2).copied() {
                    let two = first * 10 + usize::from(second - u16::from(b'0'));
                    if two > 0 && two <= matched.captures.len() {
                        index = two;
                        digits = 2;
                    }
                }
                if index > 0 && index <= matched.captures.len() {
                    if let Some(range) = matched.group(index) {
                        output.extend_from_slice(&content[range]);
                    }
                    cursor += 1 + digits;
                    continue;
                }
                output.push(u16::from(b'$'));
                cursor += 1;
                continue;
            }
            _ => {
                output.push(u16::from(b'$'));
                cursor += 1;
                continue;
            }
        }
        cursor += 2;
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_arguments_matches_official_core_forms() {
        let names = vec!["file".to_string(), "mode".to_string()];
        let rendered = substitute_arguments(
            "Full=$ARGUMENTS first=$ARGUMENTS[0] zero=$0 name=$file mode=$mode keep=$modeX",
            Some("src/main.rs 'dry run'"),
            true,
            &names,
        )
        .unwrap();
        assert_eq!(
            rendered,
            "Full=src/main.rs 'dry run' first=src/main.rs zero=src/main.rs name=src/main.rs mode=dry run keep=$modeX"
        );

        assert_eq!(
            substitute_arguments("No placeholder", Some("abc"), true, &[]).unwrap(),
            "No placeholder\n\nARGUMENTS: abc"
        );
    }

    /// Expected values captured by direct Bun imports in the source oracle.
    #[test]
    fn argument_substitution_matches_official_bun_oracle() {
        let fixture: Value = serde_json::from_str(r####"{"parses":[{"input":"","output":[]},{"input":"  ","output":[]},{"input":"one \"two three\" '' four","output":["one","two three","","four"]},{"input":"foo | bar && baz > out","output":["foo","bar","baz","out"]},{"input":"foo *.rs #comment","output":["foo"]},{"input":"$HOME ${NAME} \"$USER\"","output":["$HOME","$NAME","$USER"]},{"input":"${bad","output":["${bad"]},{"input":"\"unclosed word","output":["unclosed","word"]},{"input":"a b","output":["a","b"]},{"input":"ab","output":["ab"]},{"input":"﻿","output":[]},{"input":"中文 \"两个 字\" 🌙","output":["中文","两个 字","🌙"]}],"names":[{"output":[]},{"input":null,"output":[]},{"input":"","output":[]},{"input":"one 22  x﻿y","output":["one","x","y"]},{"input":[" one "," 22 ","22",""," ","零",4,null],"output":[" one "," 22 ","零"]},{"input":["","﻿"],"output":[""]},{"input":{"x":1},"output":[]}],"hints":[{"names":["a"," b ","c"],"typed":[],"output":"[a] [ b ] [c]"},{"names":["a","b"],"typed":["v"],"output":"[b]"},{"names":["a"],"typed":["v","w"]}],"substitutions":[{"content":"中文 🌙 $0 与 $ARGUMENTS[1]","args":"测试 \"两个 字\"","append":true,"names":[],"output":"中文 🌙 测试 与 两个 字"},{"content":"x","append":true,"names":["["],"output":"x"},{"content":"x","args":"","append":true,"names":["["],"error":{"name":"SyntaxError","message":"Invalid regular expression: unmatched parentheses"}},{"content":"$foo $fooX $foo[0] $foo_ $foo中","args":"ok","append":false,"names":["foo"],"output":"ok $fooX $foo[0] $foo_ ok中"},{"content":"$ foo  $ 22  $22","args":"one two","append":false,"names":[" foo "," 22 "],"output":"one two "},{"content":"$a $b $axb","args":"one two","append":false,"names":["a|b","x"],"output":"one $one onexone"},{"content":"$axb $a.b","args":"one","append":false,"names":["a.b"],"output":"one one"},{"content":"$x $x","args":"'$$-$&-$`-$'-${x}-$1-$0'","append":false,"names":["x"],"output":"$-$x--$-$x--$$-$&-$`-$-$x-$1-$0 $-$x-$x -$-$x--$$-$&-$`-$-$x-$1-$0"},{"content":"A$xB $x Z","args":"'$$-$&-$`-$'-${x}-$1-$0'","append":false,"names":["x"],"output":"A$xB $-$x-A$xB -$-$x--$$-$&-$`-$-$x-$1-$0 Z"},{"content":"$xy $x","args":"'$1/$01/$10/$99/$0/$<n>'","append":false,"names":["(?<n>x)(y)?"],"output":"x/x/x0//$1/$01/$10/$99/$0/$<n>/x x/x/x0//$1/$01/$10/$99/$0/$<n>/x"},{"content":"$xy $x","args":"'$1/$2/$20/$99/$0/$<none>'","append":false,"names":["(x)(y)?"],"output":"x/y/y0//$1/$2/$20/$99/$0/$<none>/$<none> x//0//$1/$2/$20/$99/$0/$<none>/$<none>"},{"content":"$0 $00 $1 $01 $10 $0x $0_ $0中 $123x","args":"A B","append":true,"names":[],"output":"A A B B  $0x $0_ A中 $123x"},{"content":"$ARGUMENTS[999999999999999999999999] $999999999999999999999999","args":"a","append":true,"names":[],"output":" "},{"content":"$ARGUMENTS/$ARGUMENTS","args":"$&","append":false,"names":[],"output":"$ARGUMENTS/$ARGUMENTS"},{"content":"before $ARGUMENTS after","args":"$`/$'/$$/$&/$1","append":false,"names":[],"output":"before before / after/$/$ARGUMENTS/$1 after"},{"content":"$x $0 $ARGUMENTS","args":"'$ARGUMENTS'","append":false,"names":["x"],"output":"'$ARGUMENTS' '$ARGUMENTS' '$ARGUMENTS'"},{"content":"same","args":"args","append":true,"names":[],"output":"same\n\nARGUMENTS: args"},{"content":"same","args":"","append":true,"names":[],"output":"same"},{"content":"$x","args":"$x","append":true,"names":["x"],"output":"$x\n\nARGUMENTS: $x"},{"content":"$ARGUMENTS","args":"$ARGUMENTS","append":true,"names":[],"output":"$ARGUMENTS\n\nARGUMENTS: $ARGUMENTS"},{"content":"$x","args":"arg","append":false,"names":["["],"error":{"name":"SyntaxError","message":"Invalid regular expression: unmatched parentheses"}},{"content":"$x","args":"arg","append":false,"names":["("],"error":{"name":"SyntaxError","message":"Invalid regular expression: missing )"}},{"content":"$x","args":"arg","append":false,"names":[""],"output":"$x"}]}"####).unwrap();
        for case in fixture["parses"].as_array().unwrap() {
            assert_eq!(
                serde_json::json!(parse_arguments(case["input"].as_str().unwrap())),
                case["output"],
                "{case}"
            );
        }
        for case in fixture["names"].as_array().unwrap() {
            assert_eq!(
                serde_json::json!(parse_argument_names(case.get("input"))),
                case["output"],
                "{case}"
            );
        }
        for case in fixture["hints"].as_array().unwrap() {
            let names: Vec<String> = serde_json::from_value(case["names"].clone()).unwrap();
            let typed: Vec<String> = serde_json::from_value(case["typed"].clone()).unwrap();
            assert_eq!(
                generate_progressive_argument_hint(&names, &typed).as_deref(),
                case["output"].as_str(),
                "{case}"
            );
        }
        for case in fixture["substitutions"].as_array().unwrap() {
            let names: Vec<String> = serde_json::from_value(case["names"].clone()).unwrap();
            let result = substitute_arguments(
                case["content"].as_str().unwrap(),
                case.get("args").and_then(Value::as_str),
                case["append"].as_bool().unwrap(),
                &names,
            );
            if case.get("error").is_some() {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .starts_with("Invalid regular expression:"),
                    "{case}"
                );
            } else {
                assert_eq!(result.unwrap(), case["output"].as_str().unwrap(), "{case}");
            }
        }
    }

    #[test]
    fn final_lone_surrogate_records_existing_string_boundary() {
        // Bun source: "arg\udf19!". RegExp without u matches one UTF-16 unit.
        // Rust's existing String prompt carrier projects that final unit to U+FFFD.
        assert_eq!(
            substitute_arguments("$🌙!", Some("arg"), false, &[".".into()]).unwrap(),
            "arg\u{fffd}!"
        );
    }
}
