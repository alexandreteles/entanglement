//! Read declared names from C and C++ declarator chains.
//!
//! A declarator nests the declared name inside pointer, array, function, and
//! reference wrappers, so `int (*f(void))[3]` declares a function named `f`.

use tree_sitter::Node;

use crate::languages::syntax::{named_children, text};

const WRAPPERS: &[&str] = &[
    "init_declarator",
    "pointer_declarator",
    "reference_declarator",
    "array_declarator",
    "function_declarator",
    "parenthesized_declarator",
    "attributed_declarator",
    "variadic_declarator",
];

const NAMES: &[&str] = &[
    "identifier",
    "field_identifier",
    "type_identifier",
    "qualified_identifier",
    "destructor_name",
    "operator_name",
    "operator_cast",
    "template_function",
    "structured_binding_declarator",
];

pub(super) fn is_declarator(kind: &str) -> bool {
    WRAPPERS.contains(&kind) || NAMES.contains(&kind)
}

/// The wrappers from the outermost declarator to the declared name, inclusive.
pub(super) fn chain(declarator: Node<'_>) -> Vec<Node<'_>> {
    let mut chain = vec![declarator];
    let mut current = declarator;
    while WRAPPERS.contains(&current.kind()) {
        let Some(inner) = inner(current) else { break };
        chain.push(inner);
        current = inner;
    }
    chain
}

/// The declarator nested inside one wrapper.
pub(super) fn inner(wrapper: Node<'_>) -> Option<Node<'_>> {
    wrapper
        .child_by_field_name("declarator")
        .or_else(|| named_children(wrapper).find(|child| is_declarator(child.kind())))
}

pub(super) fn name(declarator: Node<'_>) -> Option<Node<'_>> {
    chain(declarator)
        .pop()
        .filter(|node| NAMES.contains(&node.kind()))
}

/// The function declarator nearest to the name owns the function parameters.
/// Outer ones describe a returned function pointer.
pub(super) fn function_declarator(declarator: Node<'_>) -> Option<Node<'_>> {
    chain(declarator)
        .into_iter()
        .rev()
        .find(|node| node.kind() == "function_declarator")
}

/// `int f(void)` declares a function, but `int (*f)(void)` declares a pointer.
pub(super) fn declares_function(declarator: Node<'_>) -> bool {
    chain(declarator)
        .into_iter()
        .rev()
        .skip(1)
        .find(|node| {
            !matches!(
                node.kind(),
                "parenthesized_declarator" | "attributed_declarator"
            )
        })
        .is_some_and(|node| node.kind() == "function_declarator")
}

/// A possibly qualified name split into its explicit scope and final name.
pub(super) struct QualifiedName {
    pub scope: Vec<String>,
    pub name: String,
    /// A leading `::` starts lookup at the global namespace.
    pub global: bool,
}

pub(super) fn qualified_name(node: Node<'_>, source: &[u8]) -> Option<QualifiedName> {
    let mut scope = Vec::new();
    let mut global = false;
    let mut current = node;
    while current.kind() == "qualified_identifier" {
        match current.child_by_field_name("scope") {
            Some(part) => scope.push(simple_name(part, source)?),
            None => global |= scope.is_empty(),
        }
        current = current.child_by_field_name("name")?;
    }
    Some(QualifiedName {
        scope,
        name: simple_name(current, source)?,
        global,
    })
}

/// Template arguments are not part of the name. Dependent names have no static spelling.
fn simple_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" | "namespace_identifier" => {
            Some(text(node, source))
        }
        "template_type" | "template_function" | "template_method" => {
            simple_name(node.child_by_field_name("name")?, source)
        }
        "destructor_name" | "operator_name" | "operator_cast" => Some(
            text(node, source)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => None,
    }
}
