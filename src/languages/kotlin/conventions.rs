//! Statically represented Kotlin convention calls.
//!
//! This module only emits member-call facts when the grammar proves a Kotlin
//! convention expression. The resolver still needs a known receiver type and
//! a matching `operator` or `infix` declaration before it can resolve a call.

use std::ops::Range;

use tree_sitter::Node;

use crate::graph::{BindingType, CallArgumentFact, CallConvention, CallFacts, ReceiverScope};
use crate::model::{ModulePath, Reference, ReferenceKind, SourceRange};

use super::syntax::ContextParameters;

#[derive(Default)]
pub(super) struct ConventionFacts {
    pub references: Vec<Reference>,
    pub calls: Vec<CallFacts>,
}

pub(super) fn capture(
    root: Node<'_>,
    source: &[u8],
    contexts: &[ContextParameters],
    bindings: &[BindingType],
    receivers: &[ReceiverScope],
) -> ConventionFacts {
    let mut facts = ConventionFacts::default();
    let nodes = all_nodes(root);

    for node in nodes.iter().copied() {
        match node.kind() {
            "infix_expression" => {
                if let Some((left, right, operator)) = infix_parts(node)
                    && let Some(name) = simple_identifier(operator, source)
                {
                    push_convention(
                        &mut facts,
                        root,
                        source,
                        contexts,
                        bindings,
                        receivers,
                        node,
                        operator.byte_range(),
                        name,
                        CallConvention::Infix,
                        left,
                        vec![right],
                    );
                }
            }
            "additive_expression"
            | "multiplicative_expression"
            | "range_expression"
            | "comparison_expression"
            | "equality_expression"
            | "check_expression" => {
                if let Some((left, right)) = binary_operands(node)
                    && let Some((operator, token)) = operator_token(node, left, right, source)
                    && let Some((name, receiver, argument)) =
                        binary_convention(node.kind(), &operator, left, right)
                {
                    push_convention(
                        &mut facts,
                        root,
                        source,
                        contexts,
                        bindings,
                        receivers,
                        node,
                        token,
                        name,
                        CallConvention::Operator,
                        receiver,
                        argument.into_iter().collect(),
                    );
                }
            }
            "prefix_expression" | "postfix_expression" => {
                if let Some((operand, token, operator)) = unary_parts(node, source)
                    && let Some(name) = unary_convention(&operator, node.kind())
                {
                    push_convention(
                        &mut facts,
                        root,
                        source,
                        contexts,
                        bindings,
                        receivers,
                        node,
                        token,
                        name,
                        CallConvention::Operator,
                        operand,
                        Vec::new(),
                    );
                }
            }
            "assignment" => {
                if let Some((left, right, operator, token)) = assignment_parts(node, source) {
                    if let Some(indexed) = unwrap_indexing(left) {
                        if operator == "="
                            && let Some((receiver, indices, bracket_range)) =
                                indexing_parts(indexed)
                        {
                            let mut arguments = indices;
                            arguments.push(right);
                            push_convention(
                                &mut facts,
                                root,
                                source,
                                contexts,
                                bindings,
                                receivers,
                                node,
                                bracket_range,
                                "set".to_owned(),
                                CallConvention::Operator,
                                receiver,
                                arguments,
                            );
                        }
                    } else if let Some(name) = assignment_convention(&operator) {
                        push_convention(
                            &mut facts,
                            root,
                            source,
                            contexts,
                            bindings,
                            receivers,
                            node,
                            token,
                            name,
                            CallConvention::Operator,
                            left,
                            vec![right],
                        );
                    }
                }
            }
            "indexing_expression" => {
                if !is_assignment_target(node)
                    && let Some((receiver, indices, bracket_range)) = indexing_parts(node)
                {
                    push_convention(
                        &mut facts,
                        root,
                        source,
                        contexts,
                        bindings,
                        receivers,
                        node,
                        bracket_range,
                        "get".to_owned(),
                        CallConvention::Operator,
                        receiver,
                        indices,
                    );
                }
            }
            "call_expression" => {
                if let Some((callee, arguments)) = invocation_parts(node) {
                    let Some(receiver_type) = expression_type(callee, source, bindings, receivers)
                    else {
                        continue;
                    };
                    if receiver_type.contains("->") || receiver_type.starts_with('(') {
                        continue;
                    }
                    push_convention_with_receiver(
                        &mut facts,
                        root,
                        source,
                        contexts,
                        bindings,
                        receivers,
                        node,
                        callee.byte_range(),
                        "invoke".to_owned(),
                        CallConvention::Operator,
                        arguments,
                        Some(receiver_type),
                    );
                }
            }
            _ => {}
        }
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

#[allow(clippy::too_many_arguments)]
fn push_convention(
    facts: &mut ConventionFacts,
    root: Node<'_>,
    source: &[u8],
    contexts: &[ContextParameters],
    bindings: &[BindingType],
    receivers: &[ReceiverScope],
    expression: Node<'_>,
    reference_range: Range<usize>,
    name: String,
    convention: CallConvention,
    receiver: Node<'_>,
    arguments: Vec<Node<'_>>,
) {
    let receiver_type = expression_type(receiver, source, bindings, receivers);
    push_convention_with_receiver(
        facts,
        root,
        source,
        contexts,
        bindings,
        receivers,
        expression,
        reference_range,
        name,
        convention,
        arguments,
        receiver_type,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_convention_with_receiver(
    facts: &mut ConventionFacts,
    root: Node<'_>,
    source: &[u8],
    contexts: &[ContextParameters],
    bindings: &[BindingType],
    receivers: &[ReceiverScope],
    expression: Node<'_>,
    reference_range: Range<usize>,
    name: String,
    convention: CallConvention,
    arguments: Vec<Node<'_>>,
    receiver_type: Option<String>,
) {
    let call_owner = if is_default_argument(expression, source) {
        None
    } else {
        super::scopes::metric_scope_range_for(root, expression, source, contexts)
    };
    let lexical_scope = call_owner
        .clone()
        .unwrap_or_else(|| lexical_scope(expression, root));
    let call = CallFacts {
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
                type_name: expression_type(argument, source, bindings, receivers),
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
    facts.calls.push(call);
}

fn binary_convention<'tree>(
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

fn infix_parts(node: Node<'_>) -> Option<(Node<'_>, Node<'_>, Node<'_>)> {
    let children = named_children(node);
    let [left, operator, right] = children.as_slice() else {
        return None;
    };
    (operator.kind() == "simple_identifier").then_some((*left, *right, *operator))
}

fn binary_operands(node: Node<'_>) -> Option<(Node<'_>, Node<'_>)> {
    let children = named_children(node);
    Some((*children.first()?, *children.last()?))
}

fn operator_token(
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

fn unary_parts<'tree>(
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

fn unary_convention(operator: &str, kind: &str) -> Option<String> {
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

fn assignment_parts<'tree>(
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

fn assignment_convention(operator: &str) -> Option<String> {
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

fn unwrap_indexing(node: Node<'_>) -> Option<Node<'_>> {
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

fn indexing_parts(node: Node<'_>) -> Option<(Node<'_>, Vec<Node<'_>>, Range<usize>)> {
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

fn is_assignment_target(node: Node<'_>) -> bool {
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

fn invocation_parts(node: Node<'_>) -> Option<(Node<'_>, Vec<Node<'_>>)> {
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

fn expression_type(
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

fn is_expression(kind: &str) -> bool {
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

fn simple_identifier(node: Node<'_>, source: &[u8]) -> Option<String> {
    (node.kind() == "simple_identifier")
        .then(|| node_text(node, source).trim_matches('`').to_owned())
}

fn lexical_scope(node: Node<'_>, root: Node<'_>) -> Range<usize> {
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

fn enclosing_type_name(node: Node<'_>, source: &[u8]) -> Option<String> {
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

fn is_default_argument(node: Node<'_>, source: &[u8]) -> bool {
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

fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    std::iter::successors(Some(node), |node| node.parent())
}

fn all_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        result.push(node);
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    result.sort_by_key(|node| (node.start_byte(), node.end_byte()));
    result
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .collect()
}

fn node_text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map(String::from_utf8_lossy)
        .map(|value| value.into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{all_nodes, capture};
    use crate::graph::{BindingType, CallConvention, ReceiverScope};

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn operator_and_infix_syntax_emit_typed_member_call_facts() {
        let source = r#"
            class Box {
                operator fun plus(other: Box): Box = this + other
                infix fun work(other: Box): Box = this work other
                fun text() = "this + other and this work other"
            }
        "#;
        let tree = parse(source);
        let root = tree.root_node();
        let class_body = all_nodes(root)
            .into_iter()
            .find(|node| node.kind() == "class_body")
            .unwrap();
        let receivers = [ReceiverScope {
            start_byte: class_body.start_byte(),
            end_byte: class_body.end_byte(),
            receiver_type: "Box".into(),
            implicit: true,
            dsl_markers: Vec::new(),
            label: None,
        }];
        let contexts = super::super::syntax::context_parameters(source.as_bytes());
        let facts = capture(root, source.as_bytes(), &contexts, &[], &receivers);

        assert_eq!(facts.calls.len(), 2);
        assert!(facts.calls.iter().any(|call| {
            call.convention == Some(CallConvention::Operator)
                && call.receiver_type.as_deref() == Some("Box")
        }));
        assert!(facts.calls.iter().any(|call| {
            call.convention == Some(CallConvention::Infix)
                && call.receiver_type.as_deref() == Some("Box")
        }));
        let names = facts
            .references
            .iter()
            .map(|reference| source[reference.start_byte..reference.end_byte].to_owned())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "+"));
        assert!(names.iter().any(|name| name == "work"));
        assert_eq!(
            names.len(),
            2,
            "string contents must not become conventions"
        );

        let fallback = capture(root, source.as_bytes(), &contexts, &[], &[]);
        assert!(
            fallback
                .calls
                .iter()
                .all(|call| { call.receiver_type.as_deref() == Some("Box") })
        );
    }

    #[test]
    fn unknown_receiver_is_recorded_without_inventing_a_type() {
        let source = "fun combine(left: Any, right: Any) = left + right";
        let tree = parse(source);
        let contexts = super::super::syntax::context_parameters(source.as_bytes());
        let facts = capture(tree.root_node(), source.as_bytes(), &contexts, &[], &[]);
        assert_eq!(facts.calls.len(), 1);
        assert_eq!(facts.calls[0].convention, Some(CallConvention::Operator));
        assert_eq!(facts.calls[0].receiver_type, None);
    }

    #[test]
    fn infix_operator_is_selected_between_identifier_operands() {
        let source = "fun combine(value: Box, other: Box) = value merge other";
        let tree = parse(source);
        let value_start = source.find("value:").unwrap();
        let binding = BindingType {
            name: "value".into(),
            name_start_byte: value_start,
            name_end_byte: value_start + "value".len(),
            scope_start: 0,
            scope_end: source.len(),
            type_name: Some("Box".into()),
            alias_target: None,
            callable_reference: false,
            immutable: true,
        };
        let contexts = super::super::syntax::context_parameters(source.as_bytes());
        let facts = capture(
            tree.root_node(),
            source.as_bytes(),
            &contexts,
            &[binding],
            &[],
        );

        assert_eq!(facts.references.len(), 1);
        assert_eq!(
            &source[facts.references[0].start_byte..facts.references[0].end_byte],
            "merge"
        );
        assert_eq!(facts.calls.len(), 1);
        assert_eq!(facts.calls[0].convention, Some(CallConvention::Infix));
        assert_eq!(facts.calls[0].receiver_type.as_deref(), Some("Box"));
    }

    #[test]
    fn this_invocation_uses_its_enclosing_class_type() {
        let source = "class Action { operator fun invoke() { this() } }";
        let tree = parse(source);
        let contexts = super::super::syntax::context_parameters(source.as_bytes());
        let facts = capture(tree.root_node(), source.as_bytes(), &contexts, &[], &[]);

        assert_eq!(facts.references.len(), 1);
        assert_eq!(facts.references[0].path, ["invoke"]);
        assert_eq!(facts.calls.len(), 1);
        assert_eq!(facts.calls[0].convention, Some(CallConvention::Operator));
        assert_eq!(facts.calls[0].receiver_type.as_deref(), Some("Action"));
    }

    #[test]
    fn typed_index_and_object_call_emit_get_set_and_invoke() {
        let source = "fun use(box: Box, action: Action, value: Int) { val item = box[0]; box[1] = value; action(value) }";
        let tree = parse(source);
        let contexts = super::super::syntax::context_parameters(source.as_bytes());
        let box_start = source.find("box:").unwrap();
        let action_start = source.find("action:").unwrap();
        let bindings = [
            BindingType {
                name: "box".into(),
                name_start_byte: box_start,
                name_end_byte: box_start + 3,
                scope_start: 0,
                scope_end: source.len(),
                type_name: Some("Box".into()),
                alias_target: None,
                callable_reference: false,
                immutable: true,
            },
            BindingType {
                name: "action".into(),
                name_start_byte: action_start,
                name_end_byte: action_start + 6,
                scope_start: 0,
                scope_end: source.len(),
                type_name: Some("Action".into()),
                alias_target: None,
                callable_reference: false,
                immutable: true,
            },
        ];
        let facts = capture(
            tree.root_node(),
            source.as_bytes(),
            &contexts,
            &bindings,
            &[],
        );
        for (name, receiver) in [("get", "Box"), ("set", "Box"), ("invoke", "Action")] {
            assert!(
                facts.calls.iter().any(|call| {
                    call.convention == Some(CallConvention::Operator)
                        && call.receiver_type.as_deref() == Some(receiver)
                        && facts.references.iter().any(|reference| {
                            reference.path.last().is_some_and(|path| path == name)
                                && call.start_byte <= reference.start_byte
                                && reference.end_byte <= call.end_byte
                        })
                }),
                "expected a {name} call fact on {receiver}"
            );
        }
    }
}
