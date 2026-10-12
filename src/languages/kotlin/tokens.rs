use std::collections::HashSet;

use tree_sitter::Node;

use crate::metrics::halstead::{HalsteadToken, HalsteadTokenKind};

use super::syntax::{self, ContextParameters};

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
        if kind == "string_literal" {
            capture_string(node, source, &mut tokens, &context_names);
            continue;
        }
        if is_atomic_operand(kind) {
            push(
                &mut tokens,
                node.byte_range(),
                literal_text(node, source),
                HalsteadTokenKind::Operand,
                source,
            );
            continue;
        }
        if node.child_count() == 0 {
            let text = node_text(node, source);
            let (text, token_kind) = normalize_leaf(kind, &text, node.is_named(), &context_names);
            push(&mut tokens, node.byte_range(), text, token_kind, source);
            continue;
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }

    for context in contexts {
        for range in syntax::context_terminal_ranges(source, context) {
            let raw = source
                .get(range.clone())
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_default();
            let is_parameter_name = context
                .parameters
                .iter()
                .any(|parameter| parameter.name_range.as_ref() == Some(&range));
            let (text, kind) = if is_parameter_name {
                (normalize_identifier(&raw), HalsteadTokenKind::Operand)
            } else {
                normalize_context_token(&raw)
            };
            push(&mut tokens, range, text, kind, source);
        }
    }
    tokens.sort_by_key(|token| (token.start_byte, token.end_byte));
    tokens
}

fn capture_string(
    node: Node<'_>,
    source: &[u8],
    tokens: &mut Vec<HalsteadToken>,
    context_names: &HashSet<String>,
) {
    let mut pending = Vec::new();
    let mut had_string_content = false;
    let mut had_interpolation = false;
    for index in (0..node.child_count()).rev() {
        if let Some(child) = node.child(index) {
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
                    let (text, kind) = normalize_leaf(
                        child.kind(),
                        &node_text(child, source),
                        true,
                        context_names,
                    );
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
    while let Some(node) = pending.pop() {
        let kind = node.kind();
        if node.is_missing() || is_comment(kind) {
            continue;
        }
        if kind == "string_literal" {
            capture_string(node, source, tokens, context_names);
            continue;
        }
        if is_atomic_operand(kind) {
            push(
                tokens,
                node.byte_range(),
                literal_text(node, source),
                HalsteadTokenKind::Operand,
                source,
            );
            continue;
        }
        if node.child_count() == 0 {
            let raw = node_text(node, source);
            let (text, token_kind) = normalize_leaf(kind, &raw, node.is_named(), context_names);
            push(tokens, node.byte_range(), text, token_kind, source);
            continue;
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
}

fn normalize_leaf(
    kind: &str,
    raw: &str,
    is_named: bool,
    context_names: &HashSet<String>,
) -> (String, HalsteadTokenKind) {
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
        let identifier = raw
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
            .unwrap_or(raw);
        return (identifier.to_owned(), HalsteadTokenKind::Operand);
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
        if is_named {
            HalsteadTokenKind::Operand
        } else {
            HalsteadTokenKind::Operator
        },
    )
}

fn normalize_context_token(raw: &str) -> (String, HalsteadTokenKind) {
    if raw.starts_with('"') || raw.starts_with('\'') {
        return ("<string>".to_owned(), HalsteadTokenKind::Operand);
    }
    if raw.starts_with('`') && raw.ends_with('`') && raw.len() >= 2 {
        return (raw[1..raw.len() - 1].to_owned(), HalsteadTokenKind::Operand);
    }
    if raw
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_' || *byte >= 0x80)
    {
        let kind = if is_keyword(raw) {
            HalsteadTokenKind::Operator
        } else {
            HalsteadTokenKind::Operand
        };
        return (raw.to_owned(), kind);
    }
    if raw.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return (raw.to_owned(), HalsteadTokenKind::Operand);
    }
    (raw.to_owned(), HalsteadTokenKind::Operator)
}

fn normalize_identifier(raw: &str) -> String {
    raw.strip_prefix('`')
        .and_then(|value| value.strip_suffix('`'))
        .unwrap_or(raw)
        .to_owned()
}

fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "context"
            | "in"
            | "out"
            | "suspend"
            | "dynamic"
            | "where"
            | "this"
            | "super"
            | "fun"
            | "val"
            | "var"
            | "constructor"
            | "class"
            | "interface"
            | "object"
            | "enum"
            | "annotation"
            | "value"
            | "data"
            | "final"
            | "open"
            | "abstract"
            | "sealed"
            | "public"
            | "private"
            | "protected"
            | "internal"
            | "expect"
            | "actual"
            | "external"
            | "override"
            | "lateinit"
            | "const"
            | "inline"
            | "noinline"
            | "crossinline"
            | "operator"
            | "infix"
            | "tailrec"
            | "reified"
            | "companion"
            | "inner"
            | "get"
            | "set"
    )
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

fn literal_text(node: Node<'_>, source: &[u8]) -> String {
    node_text(node, source)
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
