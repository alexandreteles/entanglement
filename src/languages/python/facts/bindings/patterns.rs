use std::ops::Range;

use tree_sitter::Node;

use super::super::SemanticFacts;
use super::super::syntax::{children, field, range, target_names, text};
use super::scope::{add_names, push_local};

pub(super) fn parameter_names(parameter: Node<'_>, source: &[u8]) -> Vec<(String, Range<usize>)> {
    match parameter.kind() {
        "identifier" => vec![(text(parameter, source), range(parameter))],
        "default_parameter" | "typed_default_parameter" => field(parameter, "name")
            .map(|name| target_names(name, source))
            .unwrap_or_default(),
        "typed_parameter" => children(parameter)
            .into_iter()
            .filter(|child| {
                matches!(
                    child.kind(),
                    "identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
                )
            })
            .flat_map(|child| target_names(child, source))
            .collect(),
        "list_splat_pattern" | "dictionary_splat_pattern" | "tuple_pattern" => {
            target_names(parameter, source)
        }
        _ => Vec::new(),
    }
}

pub(super) fn collect_exception_target(
    clause: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    let aliases = field(clause, "alias").into_iter().collect::<Vec<_>>();
    let mut pending = children(clause);
    let mut aliases = aliases;
    while let Some(node) = pending.pop() {
        if node.kind() == "as_pattern" {
            aliases.extend(field(node, "alias"));
        } else {
            pending.extend(children(node));
        }
    }
    let names = aliases
        .into_iter()
        .flat_map(|alias| target_names(alias, source))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return;
    }
    excluded.extend(names.iter().map(|(_, span)| span.clone()));
    add_names(clause, names, root, facts, clause.end_byte());
}

pub(super) fn collect_case_targets(
    clause: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    for pattern in children(clause)
        .into_iter()
        .filter(|child| child.kind() == "case_pattern")
    {
        let names = pattern_captures(pattern, source);
        excluded.extend(names.iter().map(|(_, span)| span.clone()));
        add_names(clause, names, root, facts, clause.end_byte());
    }
}

fn pattern_captures(pattern: Node<'_>, source: &[u8]) -> Vec<(String, Range<usize>)> {
    let mut captures = Vec::new();
    let mut pending = vec![pattern];
    while let Some(node) = pending.pop() {
        match node.kind() {
            "identifier" if text(node, source) != "_" => {
                if !node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "class_pattern")
                {
                    captures.push((text(node, source), range(node)));
                }
            }
            "dotted_name" => {
                let parts = children(node);
                if let [part] = parts.as_slice()
                    && part.kind() == "identifier"
                    && text(*part, source) != "_"
                {
                    captures.push((text(*part, source), range(*part)));
                }
            }
            "keyword_identifier" | "string" | "integer" | "float" => {}
            "keyword_pattern" => pending.extend(children(node).into_iter().skip(1)),
            "class_pattern" => pending.extend(children(node).into_iter().skip(1)),
            "dict_pattern" => pending.extend(dict_values(node)),
            _ => pending.extend(children(node)),
        }
    }
    captures
}

fn dict_values(pattern: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = pattern.walk();
    let mut values: Vec<_> = pattern
        .children_by_field_name("value", &mut cursor)
        .collect();
    values.extend(
        children(pattern)
            .into_iter()
            .filter(|child| child.kind() == "splat_pattern"),
    );
    values
}

pub(super) fn bind_parameters(
    function: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    bind_parameter_scope(function, source, facts, excluded);
}

pub(super) fn bind_lambda_parameters(
    lambda: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    bind_parameter_scope(lambda, source, facts, excluded);
}

fn bind_parameter_scope(
    callable: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    let Some(parameters) = field(callable, "parameters") else {
        return;
    };
    let scope = field(callable, "body")
        .map(range)
        .unwrap_or_else(|| range(callable));
    for parameter in children(parameters) {
        for (name, name_range) in parameter_names(parameter, source) {
            excluded.push(name_range.clone());
            push_local(facts, name, name_range, scope.clone());
        }
    }
}
