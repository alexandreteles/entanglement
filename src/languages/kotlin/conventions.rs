//! Kotlin operator, infix, indexing and invoke convention facts.

use std::ops::Range;

use tree_sitter::Node;

use crate::graph::{CallArgumentFact, CallConvention, CallFacts, ReceiverScope};
use crate::model::{
    BindingEvidence, LocalBinding, ModulePath, Reference, ReferenceKind, SourceRange,
};

use super::{all_nodes, ancestors, named_children};

mod syntax;
mod types;

use syntax::*;
use types::*;

#[derive(Default)]
pub(in crate::languages::kotlin) struct ConventionFacts {
    pub references: Vec<Reference>,
    pub calls: Vec<CallFacts>,
}

pub(super) struct CaptureContext<'borrow, 'tree> {
    pub(super) scope_index: &'borrow super::scopes::ScopeIndex<'tree>,
    pub(super) bindings: &'borrow [LocalBinding],
    pub(super) receivers: &'borrow [ReceiverScope],
}

pub(in crate::languages::kotlin) fn capture(
    bindings: &[LocalBinding],
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
    let call_owner = if super::scopes::is_default_argument(call.expression, source, |kind| {
        matches!(
            kind,
            "function_declaration" | "anonymous_function" | "lambda_literal"
        )
    }) {
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

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_conventions.rs"]
mod tests;
