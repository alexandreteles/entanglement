use crate::languages::syntax::{ancestors, named_children, text};
use tree_sitter::Node;

/// A function declarator, including the parameter types that identify an overload.
pub(crate) struct Callable {
    pub path: Vec<String>,
    pub parameters: Vec<String>,
    pub minimum: usize,
    pub variadic: bool,
    pub qualifiers: String,
}

impl Callable {
    pub fn display(&self, cpp: bool) -> String {
        let name = self.path.join("::");
        if cpp {
            format!("{name}({}){}", self.parameters.join(", "), self.qualifiers)
        } else {
            name
        }
    }
}

/// Read the binding from the inside out. Parentheses distinguish a function
/// returning a pointer from a variable that holds a pointer to a function.
pub(crate) fn callable(mut node: Node<'_>, source: &[u8]) -> Option<Callable> {
    let owner = owners(node, source);
    let mut function = None;
    loop {
        match node.kind() {
            "function_declarator" => function = Some(node),
            "pointer_declarator" | "reference_declarator" | "array_declarator" => function = None,
            "operator_cast" => {
                function = node.child_by_field_name("declarator");
                break;
            }
            "qualified_identifier"
            | "identifier"
            | "field_identifier"
            | "operator_name"
            | "destructor_name"
            | "template_function"
            | "template_method" => break,
            _ => {}
        }
        node = nested(node)?;
    }
    let conversion = node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "operator_cast");
    let function = conversion
        .and_then(|name| name.child_by_field_name("declarator"))
        .or(function)?;
    let parameters = function.child_by_field_name("parameters")?;
    let mut path = owner;
    let spelling = if conversion.is_some() || node.kind() == "operator_cast" {
        String::from_utf8_lossy(&source[node.start_byte()..parameters.start_byte()])
            .trim()
            .to_owned()
    } else {
        text(node, source)
    };
    path.extend(
        spelling
            .split("::")
            .filter(|part| !part.is_empty())
            .map(|part| part.trim().to_owned()),
    );
    let variadic = text(parameters, source).contains("...");
    let minimum = named_children(parameters)
        .filter(|child| child.kind() == "parameter_declaration" && text(*child, source) != "void")
        .count();
    let parameters = named_children(parameters)
        .filter(|parameter| parameter.kind() != "comment")
        .map(|parameter| parameter_type(parameter, source))
        .filter(|name| name != "void")
        .collect();
    let qualifiers = named_children(function)
        .filter(|child| {
            matches!(
                child.kind(),
                "type_qualifier" | "ref_qualifier" | "requires_clause"
            )
        })
        .map(|child| format!(" {}", text(child, source)))
        .collect();
    Some(Callable {
        path,
        parameters,
        minimum,
        variadic,
        qualifiers,
    })
}

pub(crate) fn owners(node: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut owners: Vec<_> = ancestors(node)
        .skip(1)
        .filter(|parent| {
            matches!(
                parent.kind(),
                "namespace_definition" | "class_specifier" | "struct_specifier" | "union_specifier"
            )
        })
        .filter_map(|parent| parent.child_by_field_name("name"))
        .map(|name| text(name, source))
        .collect();
    owners.reverse();
    owners
}

pub(crate) fn identifier(mut node: Node<'_>) -> Option<Node<'_>> {
    while !matches!(
        node.kind(),
        "identifier"
            | "field_identifier"
            | "type_identifier"
            | "qualified_identifier"
            | "structured_binding_declarator"
    ) {
        node = nested(node)?;
    }
    Some(node)
}

fn nested(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("declarator").or_else(|| {
        named_children(node).find(|child| {
            child.kind().contains("declarator")
                || matches!(
                    child.kind(),
                    "identifier" | "field_identifier" | "type_identifier"
                )
        })
    })
}

pub(super) fn parameter_type(node: Node<'_>, source: &[u8]) -> String {
    let name = node.child_by_field_name("declarator").and_then(identifier);
    let end = (0..node.child_count())
        .filter_map(|index| node.child(index))
        .find(|child| child.kind() == "=")
        .map_or(node.end_byte(), |child| child.start_byte());
    let bytes = &source[node.start_byte()..end];
    let written = name.map_or_else(
        || bytes.to_vec(),
        |name| {
            [
                &source[node.start_byte()..name.start_byte()],
                &source[name.end_byte()..end],
            ]
            .concat()
        },
    );
    normalize(&String::from_utf8_lossy(&written))
}

pub(crate) fn normalize(text: &str) -> String {
    let mut result = String::new();
    for word in text.split_whitespace() {
        let ident = |character: char| character.is_alphanumeric() || character == '_';
        if result.ends_with(ident) && word.starts_with(ident) {
            result.push(' ');
        }
        result.push_str(word);
    }
    result
}
