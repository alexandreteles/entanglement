use super::*;

pub(super) fn nearest_declaration(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node).find(|ancestor| {
        matches!(
            ancestor.kind(),
            "function_declaration"
                | "secondary_constructor"
                | "anonymous_function"
                | "lambda_literal"
                | "class_declaration"
                | "object_declaration"
                | "companion_object"
                | "getter"
                | "setter"
                | "anonymous_initializer"
        )
    })
}

pub(super) fn collect_calls(
    nodes: &[Node<'_>],
    source: &[u8],
    bindings: &[LocalBinding],
) -> Vec<CallFacts> {
    nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "call_expression" | "constructor_invocation"))
        .map(|call| {
            let children = named_children(call);
            let suffix = children
                .iter()
                .copied()
                .find(|child| child.kind() == "call_suffix");
            let argument_containers = children
                .iter()
                .copied()
                .chain(suffix.into_iter().flat_map(named_children))
                .filter(|child| child.kind() == "value_arguments")
                .collect::<Vec<_>>();
            let mut arguments = argument_containers
                .into_iter()
                .flat_map(named_children)
                .filter(|argument| argument.kind() == "value_argument")
                .filter_map(|argument| call_argument(argument, source, bindings, false))
                .collect::<Vec<_>>();
            if let Some(suffix) = suffix {
                arguments.extend(
                    named_children(suffix)
                        .into_iter()
                        .filter(|child| child.kind() == "annotated_lambda")
                        .filter_map(|annotated| {
                            all_nodes(annotated)
                                .into_iter()
                                .find(|child| child.kind() == "lambda_literal")
                        })
                        .filter_map(|lambda| call_argument(lambda, source, bindings, true)),
                );
            }
            let target = children
                .iter()
                .copied()
                .find(|child| !matches!(child.kind(), "call_suffix" | "value_arguments"))
                .and_then(|node| reference_target(node, source))
                .map(|(_, target)| SourceRange {
                    start_byte: target.start,
                    end_byte: target.end,
                });
            CallFacts {
                start_byte: call.start_byte(),
                end_byte: call.end_byte(),
                target,
                receiver_type: None,
                arguments,
                ..CallFacts::default()
            }
        })
        .collect()
}

pub(super) fn call_argument(
    node: Node<'_>,
    source: &[u8],
    bindings: &[LocalBinding],
    is_trailing_lambda: bool,
) -> Option<CallArgumentFact> {
    let (lambda, name, expression, start_byte, end_byte) = if node.kind() == "lambda_literal" {
        (Some(node), None, node, node.start_byte(), node.end_byte())
    } else if node.kind() == "value_argument" {
        let children = named_children(node);
        let argument_name = children
            .iter()
            .copied()
            .find(|child| is_argument_label(*child, node, source))
            .map(|name| text(name, source).trim_matches('`').to_owned());
        let expression = children.iter().copied().rfind(|child| {
            child.kind() != "annotation" && !is_argument_label(*child, node, source)
        });
        let lambda = expression.and_then(|expression| match expression.kind() {
            "lambda_literal" => Some(expression),
            "annotated_lambda" => all_nodes(expression)
                .into_iter()
                .find(|child| child.kind() == "lambda_literal"),
            _ => None,
        });
        let expression = expression?;
        (
            lambda,
            argument_name,
            expression,
            lambda.map_or(expression.start_byte(), |lambda| lambda.start_byte()),
            lambda.map_or(expression.end_byte(), |lambda| lambda.end_byte()),
        )
    } else {
        return None;
    };
    let type_name = expression_type(expression, source).or_else(|| {
        lambda
            .is_none()
            .then(|| argument_binding_type(node, source, bindings))
            .flatten()
    });
    let lambda_parameters = lambda.and_then(|lambda| {
        named_children(lambda)
            .into_iter()
            .find(|child| child.kind() == "lambda_parameters")
    });
    let parameters = lambda_parameters.map(named_children).unwrap_or_default();
    let explicit_lambda_parameters = parameters
        .iter()
        .copied()
        .filter(|parameter| parameter.kind() == "variable_declaration")
        .map(|parameter| {
            direct_type_children(parameter)
                .next_back()
                .map(|ty| normalized_type(text(ty, source)))
        })
        .collect();
    let lambda_parameter_names = parameters
        .into_iter()
        .map(|parameter| {
            named_children(parameter)
                .into_iter()
                .find(|child| child.kind() == "simple_identifier")
                .map(|name| text(name, source).trim_matches('`').to_owned())
                .unwrap_or_default()
        })
        .collect();
    Some(CallArgumentFact {
        start_byte,
        end_byte,
        name,
        type_name,
        is_lambda: lambda.is_some(),
        is_trailing_lambda,
        lambda_label: lambda.and_then(|lambda| explicit_lambda_label(lambda, source)),
        lambda_has_explicit_parameters: lambda_parameters.is_some(),
        lambda_parameter_names,
        lambda_parameter_types: explicit_lambda_parameters,
        lambda_receiver_type: None,
    })
}

pub(super) fn explicit_lambda_label(lambda: Node<'_>, source: &[u8]) -> Option<String> {
    let wrapper = lambda.parent()?;
    matches!(wrapper.kind(), "prefix_expression" | "annotated_lambda")
        .then(|| {
            named_children(wrapper)
                .into_iter()
                .find(|child| child.kind() == "label")
        })
        .flatten()
        .map(|label| text(label, source))
        .map(|label| label.trim_end_matches('@').trim().to_owned())
        .filter(|label| !label.is_empty())
}

pub(super) fn expression_type(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "integer_literal" | "hex_literal" | "bin_literal" | "long_literal" | "unsigned_literal" => {
            integer_literal_type(&text(node, source)).map(str::to_owned)
        }
        "real_literal" | "float_literal" => Some(real_literal_type(&text(node, source)).to_owned()),
        "character_literal" => Some("Char".to_owned()),
        "string_literal" | "multi_line_string_literal" => Some("String".to_owned()),
        "boolean_literal" => Some("Boolean".to_owned()),
        _ => None,
    }
}

pub(super) fn real_literal_type(literal: &str) -> &'static str {
    if literal.ends_with(['f', 'F']) {
        "Float"
    } else {
        "Double"
    }
}

pub(super) fn integer_literal_type(literal: &str) -> Option<&'static str> {
    let lowercase = literal.replace('_', "").to_ascii_lowercase();
    let (digits, suffix) = ["ul", "lu", "u", "l"]
        .into_iter()
        .find_map(|suffix| {
            lowercase
                .strip_suffix(suffix)
                .map(|digits| (digits, suffix))
        })
        .unwrap_or((&lowercase, ""));
    let (radix, digits) = [("0x", 16), ("0b", 2)]
        .into_iter()
        .find_map(|(prefix, radix)| digits.strip_prefix(prefix).map(|digits| (radix, digits)))
        .unwrap_or((10, digits));
    let value = u64::from_str_radix(digits, radix).ok()?;
    match suffix {
        "ul" | "lu" => Some("ULong"),
        "u" if value <= u32::MAX as u64 => Some("UInt"),
        "u" => Some("ULong"),
        "l" if value <= i64::MAX as u64 => Some("Long"),
        "l" => None,
        "" if value <= i32::MAX as u64 => Some("Int"),
        "" if value <= i64::MAX as u64 => Some("Long"),
        _ => None,
    }
}

fn argument_binding_type(
    node: Node<'_>,
    source: &[u8],
    bindings: &[LocalBinding],
) -> Option<String> {
    // The former enrichment pass chose the first containing value_argument in
    // source order. Keep that choice for nested calls while avoiding a full-tree scan.
    let argument = super::super::ancestors(node)
        .filter(|node| node.kind() == "value_argument")
        .min_by_key(|node| (node.start_byte(), node.end_byte()))?;
    let expression = named_children(argument).into_iter().rfind(|child| {
        child.kind() != "annotation" && !is_argument_label(*child, argument, source)
    })?;
    if expression.kind() != "simple_identifier" {
        return None;
    }
    let name = text(expression, source).trim_matches('`').to_owned();
    bindings
        .iter()
        .filter(|binding| binding.evidence != BindingEvidence::Lexical)
        .filter(|binding| {
            binding.name == name
                && binding.start_byte < expression.start_byte()
                && binding.scope_start <= expression.start_byte()
                && expression.start_byte() < binding.scope_end
        })
        .max_by_key(|binding| binding.start_byte)
        .and_then(|binding| binding.type_name.clone())
}
