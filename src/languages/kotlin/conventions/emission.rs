use std::ops::Range;

use tree_sitter::Node;

use crate::graph::{CallArgumentFact, CallConvention, CallFacts};
use crate::model::{ModulePath, Reference, ReferenceKind, SourceRange};

use super::capture::{CaptureContext, ConventionFacts};
use super::convention_typing::{expression_type, is_default_argument, lexical_scope};

pub(super) struct ConventionCall<'tree> {
    pub(super) expression: Node<'tree>,
    pub(super) reference_range: Range<usize>,
    pub(super) name: String,
    pub(super) convention: CallConvention,
    pub(super) receiver: Node<'tree>,
    pub(super) arguments: Vec<Node<'tree>>,
}

pub(super) fn emit_convention(
    facts: &mut ConventionFacts,
    context: &CaptureContext<'_, '_>,
    call: ConventionCall<'_>,
) {
    let receiver_type = expression_type(
        call.receiver,
        context.scope_index.source(),
        context.bindings,
        context.receivers,
    );
    emit_convention_with_receiver(facts, context, call, receiver_type);
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
