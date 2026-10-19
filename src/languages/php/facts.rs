//! Capture namespace regions, imports, and trait composition from PHP syntax.

use tree_sitter::Node;

use crate::graph::{
    PhpFacts, PhpImport, PhpInclude, PhpNamespace, PhpSymbolKind, PhpTraitAdaptation, PhpTraitUse,
};
use crate::languages::CapturedTree;
use crate::model::SourceRange;

use super::syntax::*;

pub(super) fn capture(root: Node<'_>, source: &[u8], captured: &mut CapturedTree) {
    let nodes = nodes(root);
    let mut php = PhpFacts {
        namespaces: namespaces(root, &nodes, source),
        ..PhpFacts::default()
    };
    let mut capture = super::Capture {
        source,
        php: &mut php,
        captured,
    };
    for node in nodes.iter().copied() {
        match node.kind() {
            "namespace_use_declaration" => imports(node, source, capture.php),
            "use_declaration" => trait_use(node, source, capture.php),
            "include_expression"
            | "include_once_expression"
            | "require_expression"
            | "require_once_expression" => {
                capture.php.includes.push(PhpInclude {
                    conditional: conditional(node),
                    start_byte: node.start_byte(),
                    path: node
                        .named_child(0)
                        .and_then(|value| include_path(value, source)),
                });
            }
            _ => {}
        }
        capture.declaration(node);
        capture.reference(node);
    }
    super::bindings::capture(&nodes, source, captured);
    captured.semantic.php = Some(php);
}

fn namespaces(root: Node<'_>, nodes: &[Node<'_>], source: &[u8]) -> Vec<PhpNamespace> {
    let definitions = nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "namespace_definition")
        .collect::<Vec<_>>();
    let mut regions = vec![PhpNamespace {
        range: range(root),
        name: Vec::new(),
    }];
    regions.extend(definitions.iter().enumerate().map(|(index, node)| {
        PhpNamespace {
            range: SourceRange {
                start_byte: node.start_byte(),
                end_byte: node.child_by_field_name("body").map_or_else(
                    || {
                        definitions
                            .get(index + 1)
                            .map_or(root.end_byte(), Node::start_byte)
                    },
                    |body| body.end_byte(),
                ),
            },
            name: node
                .child_by_field_name("name")
                .map_or_else(Vec::new, |name| name_parts(text(name, source))),
        }
    }));
    regions
}

fn imports(node: Node<'_>, source: &[u8], php: &mut PhpFacts) {
    let namespace = namespace(&php.namespaces, node);
    let namespace_start = namespace.range.start_byte;
    let kind = import_kind(node, source).unwrap_or(PhpSymbolKind::Class);
    let prefix = children(node)
        .find(|child| child.kind() == "namespace_name")
        .map_or_else(Vec::new, |prefix| name_parts(text(prefix, source)));
    for clause in children(node)
        .flat_map(|child| std::iter::once(child).chain(children(child)))
        .filter(|child| child.kind() == "namespace_use_clause")
    {
        let Some(name) = children(clause).find(|child| is_name(*child)) else {
            continue;
        };
        let path = prefix
            .iter()
            .cloned()
            .chain(name_parts(text(name, source)))
            .collect::<Vec<_>>();
        let alias = clause
            .child_by_field_name("alias")
            .map(|alias| text(alias, source).to_owned())
            .or_else(|| path.last().cloned())
            .unwrap_or_default();
        php.imports.push(PhpImport {
            range: range(clause),
            namespace_start,
            kind: import_kind(clause, source).unwrap_or(kind),
            path,
            alias,
        });
    }
}

fn import_kind(node: Node<'_>, source: &[u8]) -> Option<PhpSymbolKind> {
    node.child_by_field_name("type")
        .map(|kind| match text(kind, source) {
            "function" => PhpSymbolKind::Function,
            "const" => PhpSymbolKind::Constant,
            _ => PhpSymbolKind::Class,
        })
}

fn trait_use(node: Node<'_>, source: &[u8], php: &mut PhpFacts) {
    let Some(owner) = class_name(node, source, &php.namespaces) else {
        return;
    };
    let traits = children(node)
        .filter(|child| is_name(*child))
        .map(|child| text(child, source).to_owned())
        .collect();
    let adaptations = nodes(node)
        .into_iter()
        .filter(|child| matches!(child.kind(), "use_as_clause" | "use_instead_of_clause"))
        .filter_map(|clause| adaptation(clause, source))
        .collect();
    php.traits.push(PhpTraitUse {
        start_byte: node.start_byte(),
        owner,
        traits,
        adaptations,
    });
}

fn adaptation(node: Node<'_>, source: &[u8]) -> Option<PhpTraitAdaptation> {
    let children = children(node).collect::<Vec<_>>();
    let target = *children.first()?;
    let (trait_name, method) = if target.kind() == "class_constant_access_expression" {
        let parts = super::syntax::children(target).collect::<Vec<_>>();
        (
            parts.first().map(|node| text(*node, source).to_owned()),
            text(*parts.last()?, source).to_owned(),
        )
    } else {
        (None, text(target, source).to_owned())
    };
    let names = children
        .iter()
        .skip(1)
        .filter(|child| is_name(**child))
        .map(|child| text(*child, source).to_owned())
        .collect::<Vec<_>>();
    Some(PhpTraitAdaptation {
        is_final: modifier(node, "final_modifier"),
        method,
        trait_name,
        alias: (node.kind() == "use_as_clause")
            .then(|| names.first().cloned())
            .flatten(),
        visibility: children
            .iter()
            .any(|child| child.kind() == "visibility_modifier")
            .then(|| visibility(node, source)),
        instead_of: if node.kind() == "use_instead_of_clause" {
            names
        } else {
            Vec::new()
        },
    })
}

fn include_path(node: Node<'_>, source: &[u8]) -> Option<String> {
    let node = unwrap(node);
    if let Some(path) = literal(node, source) {
        return Some(path);
    }
    if node.kind() == "binary_expression"
        && node
            .child_by_field_name("operator")
            .is_some_and(|operator| text(operator, source) == ".")
    {
        let left = node.child_by_field_name("left")?;
        let right = node.child_by_field_name("right")?;
        let prefix = match text(left, source) {
            "__DIR__" => "@dir:".to_owned(),
            "__FILE__" => "@file:".to_owned(),
            _ => include_path(left, source)?,
        };
        return Some(prefix + &include_path(right, source)?);
    }
    None
}
