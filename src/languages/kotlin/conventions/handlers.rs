use tree_sitter::Node;

use crate::graph::CallConvention;

use super::capture::{CaptureContext, ConventionFacts};
use super::convention_syntax::*;
use super::convention_typing::{expression_type, simple_identifier};
use super::emission::{ConventionCall, emit_convention, emit_convention_with_receiver};

pub(super) fn capture_infix(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    node: Node<'_>,
    source: &[u8],
) {
    let Some((left, right, operator)) = infix_parts(node) else {
        return;
    };
    let Some(name) = simple_identifier(operator, source) else {
        return;
    };
    emit_convention(
        facts,
        context,
        ConventionCall {
            expression: node,
            reference_range: operator.byte_range(),
            name,
            convention: CallConvention::Infix,
            receiver: left,
            arguments: vec![right],
        },
    );
}

pub(super) fn capture_binary(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    node: Node<'_>,
    source: &[u8],
) {
    let Some((left, right)) = binary_operands(node) else {
        return;
    };
    let Some((operator, token)) = operator_token(node, left, right, source) else {
        return;
    };
    let Some((name, receiver, argument)) = binary_convention(node.kind(), &operator, left, right)
    else {
        return;
    };
    emit_convention(
        facts,
        context,
        ConventionCall {
            expression: node,
            reference_range: token,
            name,
            convention: CallConvention::Operator,
            receiver,
            arguments: argument.into_iter().collect(),
        },
    );
}

pub(super) fn capture_unary(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    node: Node<'_>,
    source: &[u8],
) {
    let Some((operand, token, operator)) = unary_parts(node, source) else {
        return;
    };
    let Some(name) = unary_convention(&operator, node.kind()) else {
        return;
    };
    emit_convention(
        facts,
        context,
        ConventionCall {
            expression: node,
            reference_range: token,
            name,
            convention: CallConvention::Operator,
            receiver: operand,
            arguments: Vec::new(),
        },
    );
}

pub(super) fn capture_assignment(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    node: Node<'_>,
    source: &[u8],
) {
    let Some((left, right, operator, token)) = assignment_parts(node, source) else {
        return;
    };
    if let Some(indexed) = unwrap_indexing(left) {
        capture_indexed_assignment(facts, context, node, indexed, right, &operator);
        return;
    }
    let Some(name) = assignment_convention(&operator) else {
        return;
    };
    emit_convention(
        facts,
        context,
        ConventionCall {
            expression: node,
            reference_range: token,
            name,
            convention: CallConvention::Operator,
            receiver: left,
            arguments: vec![right],
        },
    );
}

pub(super) fn capture_indexed_assignment(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    expression: Node<'_>,
    indexed: Node<'_>,
    value: Node<'_>,
    operator: &str,
) {
    if operator != "=" {
        return;
    }
    let Some((receiver, mut arguments, reference_range)) = indexing_parts(indexed) else {
        return;
    };
    arguments.push(value);
    emit_convention(
        facts,
        context,
        ConventionCall {
            expression,
            reference_range,
            name: "set".to_owned(),
            convention: CallConvention::Operator,
            receiver,
            arguments,
        },
    );
}

pub(super) fn capture_indexing(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    node: Node<'_>,
) {
    if is_assignment_target(node) {
        return;
    }
    let Some((receiver, arguments, reference_range)) = indexing_parts(node) else {
        return;
    };
    emit_convention(
        facts,
        context,
        ConventionCall {
            expression: node,
            reference_range,
            name: "get".to_owned(),
            convention: CallConvention::Operator,
            receiver,
            arguments,
        },
    );
}

pub(super) fn capture_invoke(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    node: Node<'_>,
    source: &[u8],
) {
    let Some((callee, arguments)) = invocation_parts(node) else {
        return;
    };
    let Some(receiver_type) = expression_type(callee, source, context.bindings, context.receivers)
    else {
        return;
    };
    if receiver_type.contains("->") || receiver_type.starts_with('(') {
        return;
    }
    emit_convention_with_receiver(
        facts,
        context,
        ConventionCall {
            expression: node,
            reference_range: callee.byte_range(),
            name: "invoke".to_owned(),
            convention: CallConvention::Operator,
            receiver: callee,
            arguments,
        },
        Some(receiver_type),
    );
}
