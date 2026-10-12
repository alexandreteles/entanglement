use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

use crate::metrics::{SyntaxEvent, SyntaxRole};

#[derive(Clone, Debug)]
pub(super) struct ContextParameters {
    /// The `context(...)` prefix, including its keyword and closing delimiter.
    pub range: Range<usize>,
    /// First non-trivia byte after the prefix, usually a modifier or annotation.
    pub declaration_start: usize,
    /// The declaration keyword after any recognized modifiers.
    pub declaration_keyword: usize,
    pub parameters: Vec<ContextParameter>,
}

#[derive(Clone, Debug)]
pub(super) struct ContextParameter {
    pub name: Option<String>,
    pub name_range: Option<Range<usize>>,
    pub type_name: String,
    pub type_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum RecoveredTypeKind {
    Class,
    Interface,
    Object,
    CompanionObject,
}

#[derive(Clone, Debug)]
pub(super) struct RecoveredType {
    pub error_range: Range<usize>,
    pub declaration_range: Range<usize>,
    pub keyword_range: Range<usize>,
    pub name_range: Option<Range<usize>>,
    pub body_range: Range<usize>,
    pub kind: RecoveredTypeKind,
}

/// Recover type declaration boundaries when the pinned grammar wraps a
/// compact class or object body in one `ERROR` node. Only direct CST tokens
/// participate: nested functions, strings, and comments remain opaque.
pub(super) fn recovered_types(root: Node<'_>, source: &[u8]) -> Vec<RecoveredType> {
    let mut recovered = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "ERROR" {
            recover_error_types(node, source, &mut recovered);
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    recovered.sort_by_key(|declaration| {
        (
            declaration.declaration_range.start,
            declaration.declaration_range.end,
            declaration.kind,
        )
    });
    recovered.dedup_by(|left, right| {
        left.declaration_range == right.declaration_range && left.kind == right.kind
    });
    recovered
}

pub(super) fn recovered_type_for<'types>(
    node: Node<'_>,
    recovered: &'types [RecoveredType],
) -> Option<&'types RecoveredType> {
    recovered
        .iter()
        .filter(|declaration| {
            declaration.declaration_range.start <= node.start_byte()
                && node.end_byte() <= declaration.declaration_range.end
        })
        .min_by_key(|declaration| declaration.declaration_range.len())
}

fn recover_error_types(error: Node<'_>, source: &[u8], recovered: &mut Vec<RecoveredType>) {
    let children = (0..error.child_count())
        .filter_map(|index| error.child(index))
        .collect::<Vec<_>>();
    for (keyword_index, keyword) in children.iter().copied().enumerate() {
        let (kind, header_end) = match keyword.kind() {
            "class" => (RecoveredTypeKind::Class, keyword_index),
            "interface" => (RecoveredTypeKind::Interface, keyword_index),
            "object" => (RecoveredTypeKind::Object, keyword_index),
            "companion" => {
                let Some((object_index, _)) = children
                    .iter()
                    .enumerate()
                    .skip(keyword_index + 1)
                    .find(|(_, child)| !is_comment(child.kind()))
                    .filter(|(_, child)| child.kind() == "object")
                else {
                    continue;
                };
                (RecoveredTypeKind::CompanionObject, object_index)
            }
            _ => continue,
        };
        if keyword.kind() == "object"
            && children
                .iter()
                .take(keyword_index)
                .rev()
                .find(|child| !is_comment(child.kind()))
                .is_some_and(|child| child.kind() == "companion")
        {
            continue;
        }
        let Some(body_index) = children
            .iter()
            .enumerate()
            .skip(header_end + 1)
            .find(|(_, child)| matches!(child.kind(), "{" | "class_body" | "enum_class_body"))
            .map(|(index, _)| index)
        else {
            continue;
        };
        if !valid_recovered_header(&children, header_end, body_index) {
            continue;
        }
        let name_range = children
            .iter()
            .take(body_index)
            .skip(header_end + 1)
            .find(|child| {
                matches!(
                    child.kind(),
                    "type_identifier" | "simple_identifier" | "identifier"
                ) && source
                    .get(child.start_byte()..child.end_byte())
                    .is_some_and(|bytes| !bytes.is_empty())
            })
            .map(Node::byte_range);
        if matches!(
            kind,
            RecoveredTypeKind::Class | RecoveredTypeKind::Interface | RecoveredTypeKind::Object
        ) && name_range.is_none()
        {
            continue;
        }

        let body_node = children[body_index];
        let opening_brace = if body_node.kind() == "{" {
            body_node
        } else {
            let Some(brace) = leaf_nodes(body_node)
                .into_iter()
                .find(|child| child.kind() == "{")
            else {
                continue;
            };
            brace
        };
        let closing_end = if matches!(body_node.kind(), "class_body" | "enum_class_body") {
            if !leaf_nodes(body_node)
                .into_iter()
                .any(|child| child.kind() == "}")
            {
                continue;
            }
            body_node.end_byte()
        } else {
            let mut braces = leaf_nodes(error)
                .into_iter()
                .filter(|child| matches!(child.kind(), "{" | "}"))
                .collect::<Vec<_>>();
            braces.sort_by_key(|child| child.start_byte());
            let mut depth = 0usize;
            let mut closing_brace = None;
            for child in braces
                .into_iter()
                .filter(|child| child.start_byte() >= opening_brace.start_byte())
            {
                match child.kind() {
                    "{" => depth += 1,
                    "}" => {
                        let Some(next_depth) = depth.checked_sub(1) else {
                            break;
                        };
                        depth = next_depth;
                        if depth == 0 {
                            closing_brace = Some(child);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(closing_brace) = closing_brace else {
                continue;
            };
            closing_brace.end_byte()
        };
        let declaration_range = keyword.start_byte()..closing_end;
        let body_range = opening_brace.start_byte()..closing_end;
        if declaration_range.end > error.end_byte()
            || declaration_range.start < error.start_byte()
            || body_range.end <= body_range.start
        {
            continue;
        }
        recovered.push(RecoveredType {
            error_range: error.byte_range(),
            declaration_range,
            keyword_range: keyword.byte_range(),
            name_range,
            body_range,
            kind,
        });
    }
}

fn leaf_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut leaves = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if matches!(
            node.kind(),
            "string_literal" | "character_literal" | "line_comment" | "multiline_comment"
        ) {
            continue;
        }
        if node.child_count() == 0 {
            if !node.is_missing() {
                leaves.push(node);
            }
            continue;
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    leaves
}

fn valid_recovered_header(children: &[Node<'_>], header_end: usize, body_index: usize) -> bool {
    children
        .iter()
        .take(body_index)
        .skip(header_end + 1)
        .all(|child| {
            if is_comment(child.kind()) {
                return true;
            }
            if child.is_named() {
                return matches!(
                    child.kind(),
                    "type_identifier"
                        | "simple_identifier"
                        | "identifier"
                        | "type_parameters"
                        | "primary_constructor"
                        | "type_constraints"
                        | "delegation_specifier"
                        | "delegation_specifiers"
                        | "modifiers"
                        | "annotation"
                );
            }
            matches!(child.kind(), ":" | "," | "." | "where")
        })
        && header_end < body_index
}

fn is_comment(kind: &str) -> bool {
    matches!(kind, "line_comment" | "multiline_comment")
}

/// Find declaration-prefixed context clauses in Kotlin source.
///
/// This scanner only recognizes `context(...)` when its closing delimiter is
/// followed by a declaration keyword. It skips nested comments, quoted
/// literals, and backtick identifiers, so ordinary calls and string contents
/// remain unchanged.
pub(super) fn context_parameters(source: &[u8]) -> Vec<ContextParameters> {
    let mut contexts = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        if let Some(end) = skip_comment(source, offset) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset);
            continue;
        }
        if keyword_at(source, offset, b"context") {
            let open = skip_trivia(source, offset + b"context".len());
            if source.get(open) == Some(&b'(')
                && (offset == 0 || !is_identifier_byte(source[offset - 1]))
                && let Some(close) = matching_parenthesis(source, open)
                && let Some((declaration_start, declaration_keyword)) =
                    declaration_after(source, close + 1)
            {
                contexts.push(ContextParameters {
                    range: offset..close + 1,
                    declaration_start,
                    declaration_keyword,
                    parameters: parse_context_parameters(source, open + 1..close),
                });
                offset = close + 1;
                continue;
            }
        }
        offset += utf8_width(source[offset]);
    }
    contexts
}

pub(super) fn context_parameters_at(source: &[u8], base: usize) -> Vec<ContextParameters> {
    let mut contexts = context_parameters(source);
    if base == 0 {
        return contexts;
    }
    for context in &mut contexts {
        context.range.start += base;
        context.range.end += base;
        context.declaration_start += base;
        context.declaration_keyword += base;
        for parameter in &mut context.parameters {
            if let Some(range) = &mut parameter.name_range {
                range.start += base;
                range.end += base;
            }
            parameter.type_range.start += base;
            parameter.type_range.end += base;
        }
    }
    contexts
}

/// Return source with recognized context prefixes replaced by equal-length
/// whitespace. Tree-sitter offsets remain aligned with the user's source.
pub(super) fn mask_context_parameters(source: &[u8]) -> Option<Vec<u8>> {
    let contexts = context_parameters(source);
    if contexts.is_empty() {
        return None;
    }
    let mut normalized = source.to_vec();
    for context in contexts {
        for byte in &mut normalized[context.range] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    Some(normalized)
}

pub(super) fn context_for_declaration<'a>(
    node: Node<'_>,
    contexts: &'a [ContextParameters],
) -> Option<&'a ContextParameters> {
    contexts.iter().find(|context| {
        context.range.end <= node.start_byte()
            && context.declaration_start <= node.start_byte()
            && node.start_byte() <= context.declaration_keyword
    })
}

pub(super) fn context_start(node: Node<'_>, contexts: &[ContextParameters]) -> usize {
    context_for_declaration(node, contexts).map_or(node.start_byte(), |context| context.range.start)
}

/// Add syntax terminals omitted by context-prefix masking. NLOC therefore
/// still sees code on context declaration lines, while comment-only lines in
/// a multiline clause remain excluded.
pub(super) fn append_context_terminals(
    source: &[u8],
    contexts: &[ContextParameters],
    events: &mut Vec<SyntaxEvent>,
) {
    for context in contexts {
        for range in context_terminal_ranges(source, context) {
            let (start_row, _) = source_position(source, range.start);
            let (end_row, end_column) = source_position(source, range.end);
            events.push(SyntaxEvent {
                role: SyntaxRole::Node,
                start_byte: range.start,
                end_byte: range.end,
                start_row,
                end_row,
                end_column,
                node_id: range.start,
                parent_id: None,
                terminal: true,
            });
        }
    }
}

/// Add Kotlin-specific branches that are not explicit in the parse tree.
/// A `when` without `else` may be exhaustive by type information, but that
/// cannot be established from syntax alone. Count a conservative implicit
/// fall-through arm for source-only analysis.
pub(super) fn append_kotlin_decision_events(
    root: Node<'_>,
    events: &mut Vec<SyntaxEvent>,
    parents: &mut HashMap<usize, Option<usize>>,
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "when_expression" {
            let entries = (0..node.child_count())
                .filter_map(|index| node.child(index))
                .filter(|child| child.kind() == "when_entry")
                .collect::<Vec<_>>();
            let has_else = entries.iter().any(|entry| {
                (0..entry.child_count())
                    .filter_map(|index| entry.child(index))
                    .any(|child| child.kind() == "else")
            });
            if !has_else {
                if let Some(entry) = entries.first() {
                    parents.insert(entry.id(), Some(node.id()));
                    let position = node.end_position();
                    events.push(SyntaxEvent {
                        role: SyntaxRole::Case,
                        start_byte: node.end_byte(),
                        end_byte: node.end_byte(),
                        start_row: position.row,
                        end_row: position.row,
                        end_column: position.column,
                        node_id: entry.id(),
                        parent_id: Some(node.id()),
                        terminal: true,
                    });
                } else {
                    let start = node.start_position();
                    let end = node.end_position();
                    events.push(SyntaxEvent {
                        role: SyntaxRole::Condition,
                        start_byte: node.start_byte(),
                        end_byte: node.end_byte(),
                        start_row: start.row,
                        end_row: end.row,
                        end_column: end.column,
                        node_id: node.id(),
                        parent_id: node.parent().map(|parent| parent.id()),
                        terminal: false,
                    });
                }
            }
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
}

pub(super) fn context_terminal_ranges(
    source: &[u8],
    context: &ContextParameters,
) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut offset = context.range.start;
    while offset < context.range.end {
        if source[offset].is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= context.range.end) {
            offset = end;
            continue;
        }
        let end = if let Some(end) = skip_quoted(source, offset) {
            end.min(context.range.end)
        } else if source[offset] == b'`' {
            skip_backtick(source, offset).min(context.range.end)
        } else if is_identifier_start(source[offset]) {
            consume_identifier(source, offset).min(context.range.end)
        } else if source[offset].is_ascii_digit() {
            consume_number(source, offset).min(context.range.end)
        } else {
            consume_punctuation(source, offset).min(context.range.end)
        };
        if end <= offset {
            offset += 1;
        } else {
            result.push(offset..end);
            offset = end;
        }
    }
    result
}

/// Remove an `else` point when the branch is an `else if`; the nested `if`
/// event already represents that branch in cognitive complexity.
pub(super) fn remove_else_events_for_else_if(root: Node<'_>, events: &mut Vec<SyntaxEvent>) {
    let mut pending = vec![root];
    let mut else_if_bodies = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "control_structure_body"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "if_expression")
            && (0..node.named_child_count()).any(|index| {
                node.named_child(index as u32)
                    .is_some_and(|child| child.kind() == "if_expression")
            })
        {
            else_if_bodies.push(node.id());
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    events.retain(|event| {
        event.role != SyntaxRole::CognitiveElse || !else_if_bodies.contains(&event.node_id)
    });
}

fn parse_context_parameters(source: &[u8], content: Range<usize>) -> Vec<ContextParameter> {
    split_top_level(source, content, b',')
        .into_iter()
        .filter_map(|segment| {
            let segment = trim_space(source, segment);
            if segment.is_empty() {
                return None;
            }
            let colon = top_level_colon(source, segment.clone());
            let parsed_name = colon.and_then(|colon| {
                let name_start = skip_trivia(source, segment.start);
                let (name, name_end) = read_identifier(source, name_start)?;
                (skip_trivia(source, name_end) == colon)
                    .then(|| (name.to_owned(), name_start..name_end, colon))
            });
            let (name, name_range, type_range) =
                if let Some((name, name_range, colon)) = parsed_name {
                    (
                        Some(name),
                        Some(name_range),
                        trim_trivia(source, colon + 1..segment.end),
                    )
                } else {
                    (None, None, trim_trivia(source, segment))
                };
            if type_range.is_empty() {
                return None;
            }
            Some(ContextParameter {
                name,
                name_range,
                type_name: text_without_comments(source, type_range.clone()),
                type_range,
            })
        })
        .collect()
}

fn split_top_level(source: &[u8], range: Range<usize>, delimiter: u8) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut start = range.start;
    let mut offset = range.start;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut angles = 0usize;
    while offset < range.end {
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset).min(range.end);
            continue;
        }
        match source[offset] {
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'{' => braces += 1,
            b'}' => braces = braces.saturating_sub(1),
            b'<' => angles += 1,
            b'>' => angles = angles.saturating_sub(1),
            byte if byte == delimiter
                && parens == 0
                && brackets == 0
                && braces == 0
                && angles == 0 =>
            {
                parts.push(start..offset);
                start = offset + 1;
            }
            _ => {}
        }
        offset += utf8_width(source[offset]);
    }
    parts.push(start..range.end);
    parts
}

fn top_level_colon(source: &[u8], range: Range<usize>) -> Option<usize> {
    let mut offset = range.start;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut angles = 0usize;
    while offset < range.end {
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset).min(range.end);
            continue;
        }
        match source[offset] {
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'{' => braces += 1,
            b'}' => braces = braces.saturating_sub(1),
            b'<' => angles += 1,
            b'>' => angles = angles.saturating_sub(1),
            b':' if parens == 0
                && brackets == 0
                && braces == 0
                && angles == 0
                && source.get(offset.wrapping_sub(1)) != Some(&b':')
                && source.get(offset + 1) != Some(&b':') =>
            {
                return Some(offset);
            }
            _ => {}
        }
        offset += utf8_width(source[offset]);
    }
    None
}

fn trim_space(source: &[u8], mut range: Range<usize>) -> Range<usize> {
    while range.start < range.end && source[range.start].is_ascii_whitespace() {
        range.start += 1;
    }
    while range.start < range.end && source[range.end - 1].is_ascii_whitespace() {
        range.end -= 1;
    }
    range
}

fn trim_trivia(source: &[u8], range: Range<usize>) -> Range<usize> {
    let mut offset = range.start;
    let mut last_significant = offset;
    while offset < range.end {
        if source[offset].is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        let end = if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            end
        } else if source[offset] == b'`' {
            skip_backtick(source, offset).min(range.end)
        } else {
            offset + utf8_width(source[offset])
        };
        offset = end.max(offset + 1);
        last_significant = offset;
    }
    range.start..last_significant
}

fn text_without_comments(source: &[u8], range: Range<usize>) -> String {
    let mut text = Vec::with_capacity(range.len());
    let mut offset = range.start;
    while offset < range.end {
        if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            text.extend_from_slice(&source[offset..end]);
            offset = end;
        } else if source[offset] == b'`' {
            let end = skip_backtick(source, offset).min(range.end);
            text.extend_from_slice(&source[offset..end]);
            offset = end;
        } else if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            text.push(b' ');
            offset = end;
        } else {
            text.push(source[offset]);
            offset += 1;
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}

fn declaration_after(source: &[u8], after: usize) -> Option<(usize, usize)> {
    let mut offset = skip_trivia(source, after);
    let declaration_start = offset;
    loop {
        if source.get(offset) == Some(&b'@') {
            offset = skip_annotation(source, offset)?;
            offset = skip_trivia(source, offset);
            continue;
        }
        let (word, end) = read_identifier(source, offset)?;
        if word == "fun" {
            let next = skip_trivia(source, end);
            if read_identifier(source, next).is_some_and(|(next_word, _)| next_word == "interface")
            {
                offset = next;
            } else {
                return Some((declaration_start, offset));
            }
        } else if matches!(word, "enum" | "annotation" | "value") || is_modifier(word) {
            offset = skip_trivia(source, end);
        } else if matches!(
            word,
            "class" | "interface" | "object" | "val" | "var" | "constructor"
        ) {
            return Some((declaration_start, offset));
        } else {
            return None;
        }
    }
}

fn skip_annotation(source: &[u8], start: usize) -> Option<usize> {
    let mut offset = skip_trivia(source, start + 1);
    let (_, end) = read_identifier(source, offset)?;
    offset = skip_trivia(source, end);
    if source.get(offset) == Some(&b':') {
        offset = skip_trivia(source, offset + 1);
        if source.get(offset) == Some(&b'[') {
            return matching_delimiter(source, offset, b'[', b']').map(|close| close + 1);
        }
        let (_, end) = read_identifier(source, offset)?;
        offset = end;
    }
    loop {
        if source.get(offset) != Some(&b'.') {
            break;
        }
        let (_, end) = read_identifier(source, skip_trivia(source, offset + 1))?;
        offset = end;
    }
    offset = skip_trivia(source, offset);
    if source.get(offset) == Some(&b'(') {
        offset = matching_parenthesis(source, offset)? + 1;
    }
    Some(offset)
}

fn is_modifier(word: &str) -> bool {
    matches!(
        word,
        "public"
            | "private"
            | "protected"
            | "internal"
            | "expect"
            | "actual"
            | "final"
            | "open"
            | "abstract"
            | "sealed"
            | "const"
            | "external"
            | "override"
            | "lateinit"
            | "tailrec"
            | "suspend"
            | "inline"
            | "noinline"
            | "crossinline"
            | "operator"
            | "infix"
            | "data"
            | "inner"
            | "companion"
            | "reified"
    )
}

fn matching_parenthesis(source: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut offset = open;
    while offset < source.len() {
        if let Some(end) = skip_comment(source, offset) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset);
            continue;
        }
        match source[offset] {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
        offset += utf8_width(source[offset]);
    }
    None
}

fn matching_delimiter(source: &[u8], open: usize, opening: u8, closing: u8) -> Option<usize> {
    let mut depth = 0usize;
    let mut offset = open;
    while offset < source.len() {
        if let Some(end) = skip_comment(source, offset) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset);
            continue;
        }
        if source[offset] == opening {
            depth += 1;
        } else if source[offset] == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(offset);
            }
        }
        offset += utf8_width(source[offset]);
    }
    None
}

fn skip_trivia(source: &[u8], mut offset: usize) -> usize {
    loop {
        while source.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        if let Some(end) = skip_comment(source, offset) {
            offset = end;
        } else {
            return offset;
        }
    }
}

fn skip_comment(source: &[u8], offset: usize) -> Option<usize> {
    if source.get(offset..offset + 2) == Some(b"//") {
        let mut end = offset + 2;
        while end < source.len() && !matches!(source[end], b'\n' | b'\r') {
            end += 1;
        }
        return Some(end);
    }
    if source.get(offset..offset + 2) != Some(b"/*") {
        return None;
    }
    let mut depth = 1usize;
    let mut end = offset + 2;
    while end < source.len() {
        if source.get(end..end + 2) == Some(b"/*") {
            depth += 1;
            end += 2;
        } else if source.get(end..end + 2) == Some(b"*/") {
            depth -= 1;
            end += 2;
            if depth == 0 {
                return Some(end);
            }
        } else {
            end += utf8_width(source[end]);
        }
    }
    Some(end)
}

fn skip_quoted(source: &[u8], offset: usize) -> Option<usize> {
    let quote = *source.get(offset)?;
    if !matches!(quote, b'"' | b'\'') {
        return None;
    }
    let triple = quote == b'"' && source.get(offset..offset + 3) == Some(b"\"\"\"");
    let delimiter_length = if triple { 3 } else { 1 };
    let mut end = offset + delimiter_length;
    while end < source.len() {
        if triple && source.get(end..end + 3) == Some(b"\"\"\"") {
            return Some(end + 3);
        }
        if !triple && source[end] == quote {
            return Some(end + 1);
        }
        if !triple && source[end] == b'\\' {
            end = (end + 2).min(source.len());
        } else {
            end += utf8_width(source[end]);
        }
    }
    Some(end)
}

fn skip_backtick(source: &[u8], offset: usize) -> usize {
    let mut end = offset + 1;
    while end < source.len() {
        if source[end] == b'`' {
            return end + 1;
        }
        if matches!(source[end], b'\n' | b'\r') {
            return end;
        }
        end += utf8_width(source[end]);
    }
    end
}

fn keyword_at(source: &[u8], offset: usize, keyword: &[u8]) -> bool {
    source.get(offset..offset + keyword.len()) == Some(keyword)
        && source
            .get(offset + keyword.len())
            .is_none_or(|byte| !is_identifier_byte(*byte))
}

fn read_identifier(source: &[u8], offset: usize) -> Option<(&str, usize)> {
    if source.get(offset) == Some(&b'`') {
        let end = skip_backtick(source, offset);
        return std::str::from_utf8(source.get(offset + 1..end.checked_sub(1)?)?)
            .ok()
            .map(|text| (text, end));
    }
    if !source
        .get(offset)
        .is_some_and(|byte| is_identifier_start(*byte))
    {
        return None;
    }
    let end = consume_identifier(source, offset);
    std::str::from_utf8(source.get(offset..end)?)
        .ok()
        .map(|text| (text, end))
}

fn consume_identifier(source: &[u8], mut offset: usize) -> usize {
    while source
        .get(offset)
        .is_some_and(|byte| is_identifier_byte(*byte))
    {
        offset += utf8_width(source[offset]);
    }
    offset
}

fn consume_number(source: &[u8], mut offset: usize) -> usize {
    while source
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
    {
        offset += 1;
    }
    offset
}

fn consume_punctuation(source: &[u8], offset: usize) -> usize {
    for symbol in [b"::".as_slice(), b"->", b"?.", b"!!", b"?:", b"..", b"<>"] {
        if source.get(offset..offset + symbol.len()) == Some(symbol) {
            return offset + symbol.len();
        }
    }
    offset + utf8_width(source[offset])
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_identifier_byte(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit() || byte == b'$'
}

fn utf8_width(byte: u8) -> usize {
    if byte < 0x80 {
        1
    } else if byte & 0xe0 == 0xc0 {
        2
    } else if byte & 0xf0 == 0xe0 {
        3
    } else if byte & 0xf8 == 0xf0 {
        4
    } else {
        1
    }
}

fn source_position(source: &[u8], offset: usize) -> (usize, usize) {
    let before = &source[..offset.min(source.len())];
    let row = before.iter().filter(|byte| **byte == b'\n').count();
    let column = before
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(before.len(), |newline| before.len() - newline - 1);
    (row, column)
}
