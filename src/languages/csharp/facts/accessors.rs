use crate::graph::DeclarationExecutionFacts;
use crate::languages::syntax::{named_children, text};
use crate::model::SourceRange;
use tree_sitter::Node;

pub(super) fn declaration(node: Node<'_>, source: &[u8]) -> Option<DeclarationExecutionFacts> {
    let range = |n: Node<'_>| SourceRange {
        start_byte: n.start_byte(),
        end_byte: n.end_byte(),
    };
    let mut result = DeclarationExecutionFacts::default();
    if matches!(
        node.kind(),
        "class_declaration" | "record_declaration" | "struct_declaration"
    ) {
        let primary = named_children(node).find(|n| n.kind() == "parameter_list")?;
        result.constructors.push(range(primary));
    } else if matches!(
        node.kind(),
        "property_declaration" | "indexer_declaration" | "event_declaration"
    ) {
        if node
            .child_by_field_name("value")
            .is_some_and(|n| n.kind() == "arrow_expression_clause")
        {
            result.getter = Some(range(node));
        }
        for accessor in node
            .child_by_field_name("accessors")
            .into_iter()
            .flat_map(named_children)
            .filter(|n| n.child_by_field_name("body").is_some())
        {
            match accessor
                .child_by_field_name("name")
                .map(|n| text(n, source))
                .as_deref()
            {
                Some("get" | "remove") => result.getter = Some(range(accessor)),
                Some("set" | "init" | "add") => result.setter = Some(range(accessor)),
                _ => {}
            }
        }
    } else {
        return None;
    }
    Some(result)
}

pub(super) fn value_position(parent: Node<'_>, node: Node<'_>) -> bool {
    match parent.kind() {
        "argument" => parent.child_by_field_name("name") != Some(node),
        "member_access_expression" | "element_access_expression" => {
            parent.child_by_field_name("expression") == Some(node)
        }
        "variable_declarator" => parent.child_by_field_name("name") != Some(node),
        "return_statement"
        | "arrow_expression_clause"
        | "assignment_expression"
        | "binary_expression"
        | "conditional_expression"
        | "prefix_unary_expression"
        | "postfix_unary_expression"
        | "parenthesized_expression"
        | "expression_statement"
        | "throw_statement"
        | "interpolation"
        | "await_expression"
        | "if_statement"
        | "switch_expression"
        | "switch_expression_arm"
        | "yield_statement"
        | "initializer_expression"
        | "collection_element"
        | "ERROR" => true,
        _ => false,
    }
}
