//! Tree-sitter Bash analysis helpers.
//!
//! Maps to: CC `utils/bash/treeSitterAnalysis.ts`.
//! This module owns quote context, compound structure, actual-operator, and
//! dangerous-pattern projections. Security policy remains with its consumers.

use tree_sitter::Node;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuoteContext {
    pub with_double_quotes: String,
    pub fully_unquoted: String,
    pub unquoted_keep_quote_chars: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompoundStructure {
    pub has_compound_operators: bool,
    pub has_pipeline: bool,
    pub has_subshell: bool,
    pub has_command_group: bool,
    pub operators: Vec<String>,
    pub segments: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DangerousPatterns {
    pub has_command_substitution: bool,
    pub has_process_substitution: bool,
    pub has_parameter_expansion: bool,
    pub has_heredoc: bool,
    pub has_comment: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeSitterAnalysis {
    pub quote_context: QuoteContext,
    pub compound_structure: CompoundStructure,
    pub has_actual_operator_nodes: bool,
    pub dangerous_patterns: DangerousPatterns,
}

type Span = (usize, usize);

#[derive(Default)]
struct QuoteSpans {
    raw: Vec<Span>,
    ansi_c: Vec<Span>,
    double: Vec<Span>,
    heredoc: Vec<Span>,
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn node_text<'a>(node: Node<'_>, command: &'a str) -> &'a str {
    command
        .get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
}

fn quoted_heredoc(node: Node<'_>, command: &str) -> bool {
    children(node).into_iter().any(|child| {
        child.kind() == "heredoc_start"
            && node_text(child, command)
                .chars()
                .next()
                .is_some_and(|first| matches!(first, '\'' | '"' | '\\'))
    })
}

fn collect_quote_spans(node: Node<'_>, command: &str, spans: &mut QuoteSpans, in_double: bool) {
    match node.kind() {
        "raw_string" => {
            spans.raw.push((node.start_byte(), node.end_byte()));
            return;
        }
        "ansi_c_string" => {
            spans.ansi_c.push((node.start_byte(), node.end_byte()));
            return;
        }
        "string" => {
            if !in_double {
                spans.double.push((node.start_byte(), node.end_byte()));
            }
            for child in children(node) {
                collect_quote_spans(child, command, spans, true);
            }
            return;
        }
        "heredoc_redirect" if quoted_heredoc(node, command) => {
            spans.heredoc.push((node.start_byte(), node.end_byte()));
            return;
        }
        _ => {}
    }
    for child in children(node) {
        collect_quote_spans(child, command, spans, in_double);
    }
}

fn outermost_spans(spans: &[Span]) -> Vec<Span> {
    spans
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, span)| {
            (!spans.iter().enumerate().any(|(other_index, other)| {
                index != other_index
                    && other.0 <= span.0
                    && other.1 >= span.1
                    && (other.0 < span.0 || other.1 > span.1)
            }))
            .then_some(span)
        })
        .collect()
}

fn from_utf16_lossy(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied())
        .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

// `bashParser.ts` exposes UTF-8 byte offsets while these CC helpers use JS
// `String.slice()`/indexing (UTF-16 code units). Preserve that observable
// behavior, including its non-ASCII offset semantics.
fn remove_spans(command: &str, spans: &[Span]) -> String {
    let mut sorted = outermost_spans(spans);
    sorted.sort_unstable_by_key(|right| std::cmp::Reverse(right.0));
    let mut result = command.encode_utf16().collect::<Vec<_>>();
    for (start, end) in sorted {
        let start = start.min(result.len());
        let end = end.min(result.len());
        if start <= end {
            result.drain(start..end);
        }
    }
    from_utf16_lossy(&result)
}

fn replace_spans_keep_quotes(
    command: &str,
    spans: &[(usize, usize, &'static str, &'static str)],
) -> String {
    let plain = spans
        .iter()
        .map(|(start, end, _, _)| (*start, *end))
        .collect::<Vec<_>>();
    let outer = outermost_spans(&plain);
    let mut filtered = spans
        .iter()
        .copied()
        .filter(|(start, end, _, _)| outer.contains(&(*start, *end)))
        .collect::<Vec<_>>();
    filtered.sort_unstable_by_key(|right| std::cmp::Reverse(right.0));
    let mut result = command.encode_utf16().collect::<Vec<_>>();
    for (start, end, open, close) in filtered {
        let start = start.min(result.len());
        let end = end.min(result.len());
        if start <= end {
            result.splice(start..end, open.encode_utf16().chain(close.encode_utf16()));
        }
    }
    from_utf16_lossy(&result)
}

/// Maps to CC `extractQuoteContext(rootNode, command)`.
pub fn extract_quote_context(root: Node<'_>, command: &str) -> QuoteContext {
    let mut spans = QuoteSpans::default();
    collect_quote_spans(root, command, &mut spans, false);

    let literal_spans = spans
        .raw
        .iter()
        .chain(&spans.ansi_c)
        .chain(&spans.heredoc)
        .copied()
        .collect::<Vec<_>>();
    let command_units = command.encode_utf16().collect::<Vec<_>>();
    let mut excluded = vec![false; command_units.len()];
    for (start, end) in &literal_spans {
        for position in *start..(*end).min(excluded.len()) {
            excluded[position] = true;
        }
    }
    for (start, end) in &spans.double {
        if *start < excluded.len() {
            excluded[*start] = true;
        }
        if *end > 0 && *end - 1 < excluded.len() {
            excluded[*end - 1] = true;
        }
    }
    let with_double_quotes = from_utf16_lossy(
        &command_units
            .iter()
            .enumerate()
            .filter_map(|(position, unit)| (!excluded[position]).then_some(*unit))
            .collect::<Vec<_>>(),
    );

    let all_spans = spans
        .raw
        .iter()
        .chain(&spans.ansi_c)
        .chain(&spans.double)
        .chain(&spans.heredoc)
        .copied()
        .collect::<Vec<_>>();
    let fully_unquoted = remove_spans(command, &all_spans);

    let mut with_delimiters = Vec::new();
    with_delimiters.extend(
        spans
            .raw
            .iter()
            .map(|(start, end)| (*start, *end, "'", "'")),
    );
    with_delimiters.extend(
        spans
            .ansi_c
            .iter()
            .map(|(start, end)| (*start, *end, "$'", "'")),
    );
    with_delimiters.extend(
        spans
            .double
            .iter()
            .map(|(start, end)| (*start, *end, "\"", "\"")),
    );
    with_delimiters.extend(
        spans
            .heredoc
            .iter()
            .map(|(start, end)| (*start, *end, "", "")),
    );
    let unquoted_keep_quote_chars = replace_spans_keep_quotes(command, &with_delimiters);

    QuoteContext {
        with_double_quotes,
        fully_unquoted,
        unquoted_keep_quote_chars,
    }
}

struct CompoundAccumulator {
    operators: Vec<String>,
    segments: Vec<String>,
    has_pipeline: bool,
    has_subshell: bool,
    has_command_group: bool,
}

fn command_segment_text(node: Node<'_>, command: &str) -> String {
    let redirect_start = children(node)
        .into_iter()
        .filter(|child| {
            matches!(
                child.kind(),
                "file_redirect" | "heredoc_redirect" | "herestring_redirect"
            )
        })
        .map(|child| child.start_byte())
        .min()
        .unwrap_or(node.end_byte());
    command
        .get(node.start_byte()..redirect_start)
        .unwrap_or_else(|| node_text(node, command))
        .trim()
        .to_string()
}

fn walk_compound(node: Node<'_>, command: &str, output: &mut CompoundAccumulator) {
    match node.kind() {
        "program" => {
            for child in children(node) {
                walk_compound(child, command, output);
            }
        }
        "list" => {
            for child in children(node) {
                match child.kind() {
                    "&&" | "||" => output.operators.push(child.kind().to_string()),
                    _ => walk_compound(child, command, output),
                }
            }
        }
        ";" => output.operators.push(";".to_string()),
        "pipeline" => {
            output.has_pipeline = true;
            output.segments.push(node_text(node, command).to_string());
        }
        "subshell" => {
            output.has_subshell = true;
            output.segments.push(node_text(node, command).to_string());
        }
        "compound_statement" => {
            output.has_command_group = true;
            output.segments.push(node_text(node, command).to_string());
        }
        "command" | "declaration_command" | "variable_assignment" => {
            output.segments.push(command_segment_text(node, command));
        }
        "redirected_statement" => {
            let inner = children(node)
                .into_iter()
                .filter(|inner| inner.kind() != "file_redirect")
                .collect::<Vec<_>>();
            if inner.is_empty() {
                output.segments.push(node_text(node, command).to_string());
            } else {
                for inner in inner {
                    walk_compound(inner, command, output);
                }
            }
        }
        "negated_command" => {
            output.segments.push(node_text(node, command).to_string());
            for child in children(node) {
                walk_compound(child, command, output);
            }
        }
        "if_statement"
        | "while_statement"
        | "for_statement"
        | "case_statement"
        | "function_definition" => {
            output.segments.push(node_text(node, command).to_string());
            for child in children(node) {
                walk_compound(child, command, output);
            }
        }
        _ => {}
    }
}

/// Maps to CC `extractCompoundStructure(rootNode, command)`.
pub fn extract_compound_structure(root: Node<'_>, command: &str) -> CompoundStructure {
    let mut output = CompoundAccumulator {
        operators: Vec::new(),
        segments: Vec::new(),
        has_pipeline: false,
        has_subshell: false,
        has_command_group: false,
    };
    walk_compound(root, command, &mut output);
    if output.segments.is_empty() {
        output.segments.push(command.to_string());
    }
    CompoundStructure {
        has_compound_operators: !output.operators.is_empty(),
        has_pipeline: output.has_pipeline,
        has_subshell: output.has_subshell,
        has_command_group: output.has_command_group,
        operators: output.operators,
        segments: output.segments,
    }
}

/// Maps to CC `hasActualOperatorNodes(rootNode)`.
pub fn has_actual_operator_nodes(root: Node<'_>) -> bool {
    if matches!(root.kind(), ";" | "&&" | "||" | "list") {
        return true;
    }
    children(root).into_iter().any(has_actual_operator_nodes)
}

fn collect_dangerous_patterns(node: Node<'_>, patterns: &mut DangerousPatterns) {
    match node.kind() {
        "command_substitution" => patterns.has_command_substitution = true,
        "process_substitution" => patterns.has_process_substitution = true,
        "expansion" => patterns.has_parameter_expansion = true,
        "heredoc_redirect" => patterns.has_heredoc = true,
        "comment" => patterns.has_comment = true,
        _ => {}
    }
    for child in children(node) {
        collect_dangerous_patterns(child, patterns);
    }
}

/// Maps to CC `extractDangerousPatterns(rootNode)`.
pub fn extract_dangerous_patterns(root: Node<'_>) -> DangerousPatterns {
    let mut patterns = DangerousPatterns::default();
    collect_dangerous_patterns(root, &mut patterns);
    patterns
}

/// Maps to CC `analyzeCommand(rootNode, command)`.
pub fn analyze_command(root: Node<'_>, command: &str) -> TreeSitterAnalysis {
    TreeSitterAnalysis {
        quote_context: extract_quote_context(root, command),
        compound_structure: extract_compound_structure(root, command),
        has_actual_operator_nodes: has_actual_operator_nodes(root),
        dangerous_patterns: extract_dangerous_patterns(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_tree<R>(command: &str, check: impl FnOnce(Node<'_>) -> R) -> R {
        let tree = crate::utils::bash::bash_parser::parse(command)
            .expect("parser")
            .expect("tree");
        check(tree.root_node())
    }

    #[test]
    fn analysis_keeps_source_shaped_quote_compound_and_dangerous_projections() {
        let command = "printf '%s' \"$(echo x)\" | cat && echo ${HOME} # note";
        let analysis = with_tree(command, |root| analyze_command(root, command));
        assert!(analysis.compound_structure.has_pipeline);
        assert!(analysis.compound_structure.has_compound_operators);
        assert!(analysis.has_actual_operator_nodes);
        assert!(analysis.dangerous_patterns.has_command_substitution);
        assert!(analysis.dangerous_patterns.has_parameter_expansion);
        assert!(analysis.dangerous_patterns.has_comment);
        assert!(!analysis.quote_context.fully_unquoted.contains("'%s'"));
    }

    #[test]
    fn redirected_and_utf16_analysis_matches_cc_2_1_88() {
        let unicode = "printf '—' | cat > out";
        let analysis = with_tree(unicode, |root| analyze_command(root, unicode));
        // CC consumes UTF-8 parser offsets with JS UTF-16 String indexing in
        // this projection; preserve that observable non-ASCII result exactly.
        assert_eq!(
            analysis.quote_context.with_double_quotes,
            "printf  cat > out"
        );
        assert_eq!(analysis.quote_context.fully_unquoted, "printf  cat > out");
        assert_eq!(
            analysis.quote_context.unquoted_keep_quote_chars,
            "printf '' cat > out"
        );
        assert!(analysis.compound_structure.has_pipeline);
        assert_eq!(analysis.compound_structure.segments, ["printf '—' | cat"]);

        let subshell = "(cd /tmp && pwd) > out";
        let analysis = with_tree(subshell, |root| analyze_command(root, subshell));
        assert!(analysis.compound_structure.has_subshell);
        assert!(!analysis.compound_structure.has_compound_operators);
        assert_eq!(analysis.compound_structure.operators, Vec::<String>::new());
        assert_eq!(analysis.compound_structure.segments, ["(cd /tmp && pwd)"]);

        let heredoc = "cat <<'EOF'\n$(literal)\nEOF";
        let analysis = with_tree(heredoc, |root| analyze_command(root, heredoc));
        assert_eq!(analysis.compound_structure.segments, ["cat"]);
        assert!(analysis.dangerous_patterns.has_heredoc);
        assert!(!analysis.dangerous_patterns.has_command_substitution);
    }

    #[test]
    fn escaped_find_semicolon_is_not_an_actual_operator() {
        let command = "find . -exec echo {} \\;";
        assert!(!with_tree(command, has_actual_operator_nodes));
    }
}
