use std::collections::HashSet;

use tree_sitter::Node;

use crate::metrics::halstead::{HalsteadToken, HalsteadTokenKind};

use super::syntax::ContextParameters;

/// Capture Kotlin terminal tokens while treating text segments as normalized
/// string operands and retaining code tokens inside string interpolation.
pub(super) fn capture(
    root: Node<'_>,
    source: &[u8],
    contexts: &[ContextParameters],
) -> Vec<HalsteadToken> {
    let mut tokens = Vec::new();
    let context_names = contexts
        .iter()
        .flat_map(|context| context.parameters.iter())
        .filter_map(|parameter| parameter.name.as_deref())
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let kind = node.kind();
        if node.is_missing() || is_comment(kind) {
            continue;
        }
        if kind == "string_literal"
            && contexts.iter().any(|context| {
                context.range.start <= node.start_byte() && node.end_byte() <= context.range.end
            })
        {
            push(
                &mut tokens,
                node.byte_range(),
                "<string>".to_owned(),
                HalsteadTokenKind::Operand,
                source,
            );
            continue;
        }
        if kind == "string_literal" {
            capture_string(node, source, &mut tokens, &context_names, &mut pending);
            continue;
        }
        if is_atomic_operand(kind) {
            push(
                &mut tokens,
                node.byte_range(),
                node_text(node, source),
                HalsteadTokenKind::Operand,
                source,
            );
            continue;
        }
        if node.child_count() == 0 {
            let text = node_text(node, source);
            let (text, token_kind) = normalize_leaf(node, &text, &context_names, contexts);
            push(&mut tokens, node.byte_range(), text, token_kind, source);
            continue;
        }
        pending.extend(
            (0..node.child_count())
                .rev()
                .filter_map(|index| node.child(index)),
        );
    }

    tokens.sort_by_key(|token| (token.start_byte, token.end_byte));
    tokens
}

fn capture_string<'tree>(
    node: Node<'tree>,
    source: &[u8],
    tokens: &mut Vec<HalsteadToken>,
    context_names: &HashSet<String>,
    pending: &mut Vec<Node<'tree>>,
) {
    let mut had_string_content = false;
    let mut had_interpolation = false;
    for child in (0..node.child_count())
        .rev()
        .filter_map(|index| node.child(index))
    {
        match child.kind() {
            "string_content" => {
                had_string_content = true;
                push(
                    tokens,
                    child.byte_range(),
                    "<string>".to_owned(),
                    HalsteadTokenKind::Operand,
                    source,
                );
            }
            "interpolated_expression" => {
                had_interpolation = true;
                pending.push(child);
            }
            "interpolated_identifier" => {
                had_interpolation = true;
                let (text, kind) =
                    normalize_leaf(child, &node_text(child, source), context_names, &[]);
                push(tokens, child.byte_range(), text, kind, source);
            }
            "interpolation_expression_start"
            | "interpolation_expression_end"
            | "interpolation_identifier_start" => push(
                tokens,
                child.byte_range(),
                node_text(child, source),
                HalsteadTokenKind::Operator,
                source,
            ),
            _ => {}
        }
    }
    if !had_string_content && !had_interpolation {
        push(
            tokens,
            node.byte_range(),
            "<string>".to_owned(),
            HalsteadTokenKind::Operand,
            source,
        );
    }
}

fn normalize_leaf(
    node: Node<'_>,
    raw: &str,
    context_names: &HashSet<String>,
    contexts: &[ContextParameters],
) -> (String, HalsteadTokenKind) {
    if let Some(context) = contexts.iter().find(|context| {
        context.range.start <= node.start_byte() && node.end_byte() <= context.range.end
    }) {
        let parameter_name = context
            .parameters
            .iter()
            .any(|parameter| parameter.name_range.as_ref() == Some(&node.byte_range()));
        return normalize_context_leaf(raw, parameter_name);
    }
    let kind = node.kind();
    if matches!(
        kind,
        "interpolation_expression_start" | "interpolation_expression_end"
    ) {
        return (raw.to_owned(), HalsteadTokenKind::Operator);
    }
    if matches!(
        kind,
        "simple_identifier" | "type_identifier" | "identifier" | "interpolated_identifier"
    ) {
        return (normalize_identifier(raw), HalsteadTokenKind::Operand);
    }
    let identifier = normalize_identifier(raw);
    if context_names.contains(&identifier) {
        return (identifier, HalsteadTokenKind::Operand);
    }
    if kind == "string_content" {
        return ("<string>".to_owned(), HalsteadTokenKind::Operand);
    }
    (
        raw.to_owned(),
        if node.is_named() {
            HalsteadTokenKind::Operand
        } else {
            HalsteadTokenKind::Operator
        },
    )
}

/// Keep context-prefix token classes compatible with the former source scan.
fn normalize_context_leaf(raw: &str, parameter_name: bool) -> (String, HalsteadTokenKind) {
    const KEYWORDS: &str = "context in out suspend dynamic where this super fun val var constructor
        class interface object enum annotation value data final open abstract sealed public private
        protected internal expect actual external override lateinit const inline noinline crossinline
        operator infix tailrec reified companion inner get set";
    let identifier = raw
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_' || *byte >= 0x80);
    let operand = parameter_name
        || raw.starts_with('`')
        || raw.as_bytes().first().is_some_and(u8::is_ascii_digit)
        || (identifier
            && !KEYWORDS
                .split_ascii_whitespace()
                .any(|keyword| keyword == raw));
    (
        normalize_identifier(raw),
        if operand {
            HalsteadTokenKind::Operand
        } else {
            HalsteadTokenKind::Operator
        },
    )
}

fn normalize_identifier(raw: &str) -> String {
    raw.strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
        .unwrap_or(raw)
        .to_owned()
}

fn is_comment(kind: &str) -> bool {
    matches!(kind, "line_comment" | "multiline_comment")
}

fn is_atomic_operand(kind: &str) -> bool {
    matches!(
        kind,
        "boolean_literal"
            | "integer_literal"
            | "long_literal"
            | "unsigned_literal"
            | "hex_literal"
            | "bin_literal"
            | "real_literal"
            | "character_literal"
            | "null_literal"
    )
}

fn node_text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

fn push(
    tokens: &mut Vec<HalsteadToken>,
    range: std::ops::Range<usize>,
    token: String,
    kind: HalsteadTokenKind,
    source: &[u8],
) {
    if token.is_empty() || range.start >= range.end || range.end > source.len() {
        return;
    }
    let line = source[..range.start]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    tokens.push(HalsteadToken {
        kind,
        token,
        start_byte: range.start,
        end_byte: range.end,
        line,
    });
}
