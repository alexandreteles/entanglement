use crate::languages::CapturedTree;
use crate::languages::syntax::{named_children, text};
use crate::metrics::{
    SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope, halstead::HalsteadTokenKind,
};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Use physical declaration ranges for initializers, primary constructors and top-level code.
pub(super) fn complete(root: Node<'_>, source: &[u8], captured: &mut CapturedTree) {
    let mut pending = vec![root];
    let mut alternate = HashSet::new();
    let mut names = HashMap::new();
    let mut keywords = HashSet::new();
    while let Some(node) = pending.pop() {
        let field = |name| node.child_by_field_name(name);
        match node.kind() {
            "if_statement"
                if field("alternative").is_some_and(|other| other.kind() == "if_statement") =>
            {
                let mut cursor = node.walk();
                alternate.extend(
                    node.children(&mut cursor)
                        .filter(|item| item.kind() == "else")
                        .map(|item| item.id()),
                );
            }
            "accessor_declaration"
            | "destructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration" => {
                if let Some(name) = callable_name(node, source) {
                    names.insert(node.start_byte(), name);
                }
            }
            "parameter_list"
                if node.parent().is_some_and(|parent| {
                    matches!(
                        parent.kind(),
                        "class_declaration" | "record_declaration" | "struct_declaration"
                    )
                }) =>
            {
                if let Some(name) = node
                    .parent()
                    .and_then(|parent| parent.child_by_field_name("name"))
                {
                    add(node, node, text(name, source), captured);
                }
            }
            "variable_declarator"
                if node.parent().and_then(|p| p.parent()).is_some_and(|p| {
                    matches!(p.kind(), "field_declaration" | "event_field_declaration")
                }) =>
            {
                if let (Some(name), Some(value)) = (field("name"), named_children(node).nth(1))
                    && value.kind() != "bracketed_argument_list"
                {
                    add(
                        node,
                        node,
                        format!("{} initializer", text(name, source)),
                        captured,
                    );
                }
            }
            "property_declaration"
                if field("value").is_some_and(|v| v.kind() != "arrow_expression_clause") =>
            {
                if let (Some(name), Some(value)) = (field("name"), field("value")) {
                    add(
                        value,
                        value,
                        format!("{} initializer", text(name, source)),
                        captured,
                    );
                }
            }
            "predefined_type" | "implicit_type" | "modifier" => {
                keywords.insert(node.start_byte());
            }
            _ => {}
        }
        pending.extend(named_children(node));
    }
    let mut statements = named_children(root).filter(|node| {
        node.kind() == "global_statement"
            && !named_children(*node).any(|item| item.kind() == "local_function_statement")
    });
    if let Some(first) = statements.next() {
        add(
            first,
            statements.last().unwrap_or(first),
            "<Main>".into(),
            captured,
        );
    }
    captured.events.retain(|event| {
        event.role != SyntaxRole::CognitiveElse || !alternate.contains(&event.node_id)
    });
    for function in &mut captured.functions {
        if let Some(name) = names.remove(&function.range.start) {
            function.name = name;
        }
    }
    for token in &mut captured.tokens {
        if keywords.contains(&token.start_byte) {
            token.kind = HalsteadTokenKind::Operator;
        }
    }
}

fn callable_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let name = |node: Node<'_>| text(node, source);
    match node.kind() {
        "accessor_declaration" => {
            let member = node.parent()?.parent()?;
            Some(format!(
                "{} {}",
                name(node.child_by_field_name("name")?),
                member
                    .child_by_field_name("name")
                    .map(name)
                    .unwrap_or_else(|| "this".into())
            ))
        }
        "destructor_declaration" => Some(format!("~{}", name(node.child_by_field_name("name")?))),
        "operator_declaration" => Some(format!(
            "operator {}",
            name(node.child_by_field_name("operator")?)
        )),
        "conversion_operator_declaration" => Some(format!(
            "operator {}",
            name(node.child_by_field_name("type")?)
        )),
        _ => None,
    }
}

fn add(first: Node<'_>, last: Node<'_>, name: String, captured: &mut CapturedTree) {
    let start = first.start_position();
    let end = last.end_position();
    let range = first.start_byte()..last.end_byte();
    captured.functions.push(FunctionScope {
        name,
        range: range.clone(),
        line: start.row + 1,
    });
    captured.events.push(SyntaxEvent {
        role: SyntaxRole::Function,
        start_byte: range.start,
        end_byte: range.end,
        start_row: start.row,
        end_row: end.row,
        end_column: end.column,
        node_id: first.id(),
        parent_id: first.parent().map(|node| node.id()),
        terminal: false,
    });
}
