use super::{Argument, Expression};
use crate::languages::syntax::{named_children, text};
use tree_sitter::Node;

pub(super) fn simple(node: Node<'_>, source: &[u8]) -> String {
    text(node, source).trim_start_matches('@').to_owned()
}

pub(super) fn expression(node: Node<'_>, source: &[u8]) -> Expression {
    let field = |name| node.child_by_field_name(name);
    let literal = match node.kind() {
        "string_literal"
        | "verbatim_string_literal"
        | "raw_string_literal"
        | "interpolated_string_expression" => "string",
        "character_literal" => "char",
        "boolean_literal" => "bool",
        "null_literal" => "null",
        "integer_literal" => {
            let token = text(node, source).to_ascii_lowercase();
            match (
                token.ends_with('u') || token.ends_with("ul"),
                token.ends_with('l') || token.ends_with("lu"),
            ) {
                (true, true) => "ulong",
                (true, false) => "uint",
                (false, true) => "long",
                _ => "int",
            }
        }
        "real_literal" => {
            let token = text(node, source).to_ascii_lowercase();
            if token.ends_with('f') {
                "float"
            } else if token.ends_with('m') {
                "decimal"
            } else {
                "double"
            }
        }
        "object_creation_expression" | "cast_expression" | "default_expression" => {
            return field("type")
                .map_or(Expression::Unknown, |n| Expression::Type(text(n, source)));
        }
        "identifier" | "generic_name" | "this" | "base" | "this_expression" | "base_expression" => {
            return Expression::Name(simple(node, source));
        }
        "invocation_expression" => {
            return field("function")
                .and_then(callee)
                .map_or(Expression::Unknown, |n| Expression::Call(n.start_byte()));
        }
        "member_access_expression" => {
            return field("expression").zip(field("name")).map_or(
                Expression::Unknown,
                |(receiver, name)| {
                    Expression::Member(Box::new(expression(receiver, source)), simple(name, source))
                },
            );
        }
        "parenthesized_expression" | "await_expression" | "argument" => {
            return named_children(node)
                .last()
                .map_or(Expression::Unknown, |n| expression(n, source));
        }
        _ => return Expression::Unknown,
    };
    Expression::Type(literal.into())
}

pub(super) fn callee(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "identifier" | "generic_name" => Some(node),
        "member_access_expression" | "member_binding_expression" => {
            node.child_by_field_name("name")
        }
        "conditional_access_expression" => named_children(node).last().and_then(callee),
        _ => None,
    }
}

pub(super) fn arguments(list: Option<Node<'_>>, source: &[u8]) -> Vec<Argument> {
    list.into_iter()
        .flat_map(named_children)
        .filter(|n| n.kind() == "argument")
        .map(|node| {
            let mut cursor = node.walk();
            let mode = node
                .children(&mut cursor)
                .find(|v| matches!(v.kind(), "in" | "out" | "ref"))
                .map(|v| text(v, source))
                .unwrap_or_default();
            Argument {
                name: node.child_by_field_name("name").map(|n| simple(n, source)),
                mode,
                value: expression(node, source),
            }
        })
        .collect()
}
