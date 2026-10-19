//! Kotlin operator, infix, indexing and invoke convention facts.

use std::ops::Range;

use tree_sitter::Node;

use crate::graph::{BindingType, CallArgumentFact, CallConvention, CallFacts, ReceiverScope};
use crate::model::{ModulePath, Reference, ReferenceKind, SourceRange};

use super::{all_nodes, ancestors, named_children};

#[derive(Default)]
pub(in crate::languages::kotlin) struct ConventionFacts {
    pub references: Vec<Reference>,
    pub calls: Vec<CallFacts>,
}

pub(super) struct CaptureContext<'borrow, 'tree> {
    pub(super) scope_index: &'borrow super::scopes::ScopeIndex<'tree>,
    pub(super) bindings: &'borrow [BindingType],
    pub(super) receivers: &'borrow [ReceiverScope],
}

pub(in crate::languages::kotlin) fn capture(
    bindings: &[BindingType],
    receivers: &[ReceiverScope],
    scope_index: &super::scopes::ScopeIndex<'_>,
) -> ConventionFacts {
    let mut facts = ConventionFacts::default();
    let context = CaptureContext {
        scope_index,
        bindings,
        receivers,
    };
    let nodes = scope_index.nodes();
    let source = scope_index.source();

    for node in nodes.iter().copied() {
        capture_node(&mut facts, &context, node, source);
    }

    facts.references.sort_by_key(|reference| {
        (
            reference.start_byte,
            reference.end_byte,
            reference.path.clone(),
        )
    });
    facts
        .calls
        .sort_by_key(|call| (call.start_byte, call.end_byte));
    facts
}

fn capture_node(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    node: Node<'_>,
    source: &[u8],
) {
    let Some(call) = convention_call(node, source) else {
        return;
    };
    let receiver_type = expression_type(call.receiver, source, context.bindings, context.receivers);
    if node.kind() == "call_expression"
        && receiver_type
            .as_ref()
            .is_none_or(|ty| ty.contains("->") || ty.starts_with('('))
    {
        return;
    }
    emit_convention_with_receiver(facts, context, call, receiver_type);
}

pub(super) fn convention_call<'tree>(
    node: Node<'tree>,
    source: &[u8],
) -> Option<ConventionCall<'tree>> {
    let mut convention = CallConvention::Operator;
    let (reference_range, name, receiver, arguments) = match node.kind() {
        "infix_expression" => {
            let (left, right, operator) = infix_parts(node)?;
            convention = CallConvention::Infix;
            (
                operator.byte_range(),
                simple_identifier(operator, source)?,
                left,
                vec![right],
            )
        }
        "additive_expression"
        | "multiplicative_expression"
        | "range_expression"
        | "comparison_expression"
        | "equality_expression"
        | "check_expression" => {
            let (left, right) = binary_operands(node)?;
            let (operator, token) = operator_token(node, left, right, source)?;
            let (name, receiver, argument) =
                binary_convention(node.kind(), &operator, left, right)?;
            (token, name, receiver, argument.into_iter().collect())
        }
        "prefix_expression" | "postfix_expression" => {
            let (operand, token, operator) = unary_parts(node, source)?;
            (
                token,
                unary_convention(&operator, node.kind())?,
                operand,
                Vec::new(),
            )
        }
        "assignment" => {
            let (left, right, operator, token) = assignment_parts(node, source)?;
            if let Some(indexed) = unwrap_indexing(left) {
                if operator != "=" {
                    return None;
                }
                let (receiver, mut arguments, reference_range) = indexing_parts(indexed)?;
                arguments.push(right);
                (reference_range, "set".to_owned(), receiver, arguments)
            } else {
                (token, assignment_convention(&operator)?, left, vec![right])
            }
        }
        "indexing_expression" if !is_assignment_target(node) => {
            let (receiver, arguments, reference_range) = indexing_parts(node)?;
            (reference_range, "get".to_owned(), receiver, arguments)
        }
        "call_expression" => {
            let (callee, arguments) = invocation_parts(node)?;
            (callee.byte_range(), "invoke".to_owned(), callee, arguments)
        }
        _ => return None,
    };
    Some(ConventionCall {
        expression: node,
        reference_range,
        name,
        convention,
        receiver,
        arguments,
    })
}

pub(super) struct ConventionCall<'tree> {
    pub(super) expression: Node<'tree>,
    pub(super) reference_range: Range<usize>,
    pub(super) name: String,
    pub(super) convention: CallConvention,
    pub(super) receiver: Node<'tree>,
    pub(super) arguments: Vec<Node<'tree>>,
}

pub(super) fn emit_convention_with_receiver(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    call: ConventionCall<'_>,
    receiver_type: Option<String>,
) {
    let scope_index = context.scope_index;
    let root = scope_index.root();
    let source = scope_index.source();
    let call_owner = if is_default_argument(call.expression, source) {
        None
    } else {
        scope_index.metric_scope_range_for(call.expression)
    };
    let lexical_scope = call_owner
        .clone()
        .unwrap_or_else(|| lexical_scope(call.expression, root));
    let ConventionCall {
        expression,
        reference_range,
        name,
        convention,
        arguments,
        ..
    } = call;
    let call_facts = CallFacts {
        start_byte: expression.start_byte(),
        end_byte: expression.end_byte(),
        target: Some(SourceRange {
            start_byte: reference_range.start,
            end_byte: reference_range.end,
        }),
        convention: Some(convention),
        receiver_type,
        arguments: arguments
            .into_iter()
            .map(|argument| CallArgumentFact {
                start_byte: argument.start_byte(),
                end_byte: argument.end_byte(),
                type_name: expression_type(argument, source, context.bindings, context.receivers),
                ..CallArgumentFact::default()
            })
            .collect(),
    };
    facts.references.push(Reference {
        path: vec![name],
        module: ModulePath::default(),
        kind: ReferenceKind::Method,
        start_byte: reference_range.start,
        end_byte: reference_range.end,
        scope_start: lexical_scope.start,
        scope_end: lexical_scope.end,
        call_owner: call_owner.as_ref().map(|owner| owner.start),
        execution_owner: call_owner.map(|owner| owner.start),
        access: None,
        context_id: 0,
    });
    facts.calls.push(call_facts);
}

pub(super) fn binary_convention<'tree>(
    kind: &str,
    operator: &str,
    left: Node<'tree>,
    right: Node<'tree>,
) -> Option<(String, Node<'tree>, Option<Node<'tree>>)> {
    let name = match (kind, operator) {
        ("additive_expression", "+") => "plus",
        ("additive_expression", "-") => "minus",
        ("multiplicative_expression", "*") => "times",
        ("multiplicative_expression", "/") => "div",
        ("multiplicative_expression", "%") => "rem",
        ("range_expression", "..") => "rangeTo",
        ("range_expression", "..<") => "rangeUntil",
        ("comparison_expression", "<" | ">" | "<=" | ">=") => "compareTo",
        ("equality_expression", "==" | "!=") => "equals",
        ("check_expression", "in" | "!in") => {
            return Some(("contains".to_owned(), right, Some(left)));
        }
        _ => return None,
    };
    Some((name.to_owned(), left, Some(right)))
}

pub(super) fn infix_parts(node: Node<'_>) -> Option<(Node<'_>, Node<'_>, Node<'_>)> {
    let children = named_children(node);
    let [left, operator, right] = children.as_slice() else {
        return None;
    };
    (operator.kind() == "simple_identifier").then_some((*left, *right, *operator))
}

pub(super) fn binary_operands(node: Node<'_>) -> Option<(Node<'_>, Node<'_>)> {
    let children = named_children(node);
    Some((*children.first()?, *children.last()?))
}

pub(super) fn operator_token(
    node: Node<'_>,
    left: Node<'_>,
    right: Node<'_>,
    source: &[u8],
) -> Option<(String, Range<usize>)> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| {
            !child.is_named()
                && left.end_byte() <= child.start_byte()
                && child.end_byte() <= right.start_byte()
        })
        .find_map(|child| {
            let token = source.get(child.byte_range())?;
            let token = std::str::from_utf8(token).ok()?.trim();
            matches!(
                token,
                "+" | "-"
                    | "*"
                    | "/"
                    | "%"
                    | ".."
                    | "..<"
                    | "<"
                    | ">"
                    | "<="
                    | ">="
                    | "=="
                    | "!="
                    | "in"
                    | "!in"
            )
            .then(|| (token.to_owned(), child.byte_range()))
        })
}

pub(super) fn unary_parts<'tree>(
    node: Node<'tree>,
    source: &[u8],
) -> Option<(Node<'tree>, Range<usize>, String)> {
    let operand = named_children(node)
        .into_iter()
        .find(|child| !matches!(child.kind(), "annotation" | "label"))?;
    let token = (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| !child.is_named() && child.end_byte() <= node.end_byte())
        .find(|child| {
            let token = source.get(child.byte_range()).unwrap_or_default();
            matches!(token, b"+" | b"-" | b"!" | b"++" | b"--")
        })?;
    let operator = std::str::from_utf8(source.get(token.byte_range())?)
        .ok()?
        .to_owned();
    Some((operand, token.byte_range(), operator))
}

pub(super) fn unary_convention(operator: &str, kind: &str) -> Option<String> {
    let name = match (kind, operator) {
        ("prefix_expression", "+") => "unaryPlus",
        ("prefix_expression", "-") => "unaryMinus",
        ("prefix_expression", "!") => "not",
        ("prefix_expression" | "postfix_expression", "++") => "inc",
        ("prefix_expression" | "postfix_expression", "--") => "dec",
        _ => return None,
    };
    Some(name.to_owned())
}

pub(super) fn assignment_parts<'tree>(
    node: Node<'tree>,
    source: &[u8],
) -> Option<(Node<'tree>, Node<'tree>, String, Range<usize>)> {
    let children = named_children(node);
    let left = *children.first()?;
    let right = *children.last()?;
    let operator = (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| !child.is_named() && left.end_byte() <= child.start_byte())
        .find(|child| child.end_byte() <= right.start_byte())?;
    let value = std::str::from_utf8(source.get(operator.byte_range())?)
        .ok()?
        .to_owned();
    Some((left, right, value, operator.byte_range()))
}

pub(super) fn assignment_convention(operator: &str) -> Option<String> {
    let name = match operator {
        "+=" => "plusAssign",
        "-=" => "minusAssign",
        "*=" => "timesAssign",
        "/=" => "divAssign",
        "%=" => "remAssign",
        _ => return None,
    };
    Some(name.to_owned())
}

pub(super) fn unwrap_indexing(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "indexing_expression" {
        return Some(node);
    }
    if node.kind() == "directly_assignable_expression"
        && all_nodes(node)
            .iter()
            .any(|child| child.kind() == "indexing_suffix")
    {
        return Some(node);
    }
    None
}

pub(super) fn indexing_parts(node: Node<'_>) -> Option<(Node<'_>, Vec<Node<'_>>, Range<usize>)> {
    let children = named_children(node);
    let receiver = *children.first()?;
    let suffix = children
        .iter()
        .copied()
        .find(|child| child.kind() == "indexing_suffix")?;
    let arguments = named_children(suffix)
        .into_iter()
        .filter(|child| is_expression(child.kind()))
        .collect();
    Some((receiver, arguments, suffix.byte_range()))
}

pub(super) fn is_assignment_target(node: Node<'_>) -> bool {
    for parent in ancestors(node) {
        if parent.kind() == "assignment" {
            return parent.named_child(0).is_some_and(|left| {
                left.start_byte() <= node.start_byte() && node.end_byte() <= left.end_byte()
            });
        }
        if matches!(parent.kind(), "statements" | "source_file") {
            return false;
        }
    }
    false
}

pub(super) fn invocation_parts(node: Node<'_>) -> Option<(Node<'_>, Vec<Node<'_>>)> {
    let children = named_children(node);
    let callee = *children.first()?;
    if !matches!(callee.kind(), "simple_identifier" | "this_expression") {
        return None;
    }
    let suffix = children
        .iter()
        .copied()
        .find(|child| child.kind() == "call_suffix")?;
    let arguments = all_nodes(suffix)
        .into_iter()
        .filter(|child| child.kind() == "value_argument")
        .filter_map(|argument| {
            named_children(argument)
                .into_iter()
                .find(|child| is_expression(child.kind()))
        })
        .collect();
    Some((callee, arguments))
}

pub(super) fn is_expression(kind: &str) -> bool {
    matches!(
        kind,
        "simple_identifier"
            | "type_identifier"
            | "this_expression"
            | "super_expression"
            | "call_expression"
            | "navigation_expression"
            | "infix_expression"
            | "additive_expression"
            | "multiplicative_expression"
            | "range_expression"
            | "comparison_expression"
            | "equality_expression"
            | "prefix_expression"
            | "postfix_expression"
            | "indexing_expression"
            | "parenthesized_expression"
            | "literal_constant"
    )
}

pub(super) fn expression_type(
    node: Node<'_>,
    source: &[u8],
    bindings: &[BindingType],
    receivers: &[ReceiverScope],
) -> Option<String> {
    if node.kind() == "this_expression" {
        if node_text(node, source).contains('@') {
            return None;
        }
        return receivers
            .iter()
            .filter(|receiver| {
                receiver.start_byte <= node.start_byte() && node.end_byte() <= receiver.end_byte
            })
            .min_by_key(|receiver| receiver.end_byte - receiver.start_byte)
            .map(|receiver| receiver.receiver_type.clone())
            .or_else(|| enclosing_type_name(node, source));
    }
    if node.kind() != "simple_identifier" {
        return None;
    }
    let name = node_text(node, source);
    let name = name.trim_matches('`');
    bindings
        .iter()
        .filter(|binding| {
            binding.name == name
                && binding.name_start_byte <= node.start_byte()
                && binding.scope_start <= node.start_byte()
                && node.end_byte() <= binding.scope_end
        })
        .min_by_key(|binding| {
            (
                binding.scope_end - binding.scope_start,
                usize::MAX - binding.name_start_byte,
            )
        })
        .and_then(|binding| binding.type_name.clone())
}

pub(super) fn simple_identifier(node: Node<'_>, source: &[u8]) -> Option<String> {
    (node.kind() == "simple_identifier")
        .then(|| node_text(node, source).trim_matches('`').to_owned())
}

pub(super) fn lexical_scope(node: Node<'_>, root: Node<'_>) -> Range<usize> {
    ancestors(node)
        .find(|ancestor| {
            matches!(
                ancestor.kind(),
                "statements" | "class_body" | "enum_class_body" | "lambda_literal"
            )
        })
        .map(|scope| scope.byte_range())
        .unwrap_or_else(|| root.byte_range())
}

pub(super) fn enclosing_type_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut owners = ancestors(node)
        .filter(|ancestor| {
            matches!(
                ancestor.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
        .map(|declaration| {
            named_children(declaration)
                .into_iter()
                .find(|child| child.kind() == "type_identifier")
                .map(|name| node_text(name, source).trim_matches('`').to_owned())
                .unwrap_or_else(|| "Companion".to_owned())
        })
        .collect::<Vec<_>>();
    owners.reverse();
    (!owners.is_empty()).then(|| owners.join("."))
}

pub(super) fn is_default_argument(node: Node<'_>, source: &[u8]) -> bool {
    for ancestor in ancestors(node) {
        if matches!(
            ancestor.kind(),
            "function_declaration" | "anonymous_function" | "lambda_literal"
        ) {
            return false;
        }
        if matches!(
            ancestor.kind(),
            "parameter" | "function_value_parameter" | "class_parameter"
        ) && source
            .get(ancestor.byte_range())
            .is_some_and(|parameter| parameter.contains(&b'='))
        {
            return true;
        }
    }
    false
}

pub(super) fn node_text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map(String::from_utf8_lossy)
        .map(|value| value.into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_conventions.rs"]
mod tests;
