use crate::structure::types::TypeExpression;
use tree_sitter::Node;

/// A Go method signature without parameter names or its receiver.
#[derive(Clone, Debug)]
pub(crate) struct MethodSignature {
    pub parameters: Vec<TypeExpression>,
    pub results: Vec<TypeExpression>,
    pub variadic: bool,
    pub generic: bool,
}

pub(super) fn capture(node: Node<'_>, source: &[u8]) -> Option<MethodSignature> {
    if !matches!(node.kind(), "method_declaration" | "method_elem") {
        return None;
    }
    let parameters = node.child_by_field_name("parameters")?;
    let result = node.child_by_field_name("result");
    Some(MethodSignature {
        parameters: parameter_types(parameters, source),
        results: result.map_or_else(Vec::new, |result| {
            if result.kind() == "parameter_list" {
                parameter_types(result, source)
            } else {
                vec![TypeExpression::from_node(result, source)]
            }
        }),
        variadic: {
            let mut cursor = parameters.walk();
            parameters
                .named_children(&mut cursor)
                .any(|parameter| parameter.kind() == "variadic_parameter_declaration")
        },
        generic: node.child_by_field_name("type_parameters").is_some(),
    })
}

fn parameter_types(list: Node<'_>, source: &[u8]) -> Vec<TypeExpression> {
    let mut cursor = list.walk();
    list.named_children(&mut cursor)
        .flat_map(|parameter| {
            let Some(parameter_type) = parameter.child_by_field_name("type") else {
                return Vec::new();
            };
            let mut cursor = parameter.walk();
            let count = parameter
                .children_by_field_name("name", &mut cursor)
                .count()
                .max(1);
            std::iter::repeat_n(TypeExpression::from_node(parameter_type, source), count).collect()
        })
        .collect()
}
