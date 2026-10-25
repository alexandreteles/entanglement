//! Static resource facts from host and embedded syntax trees.

use tree_sitter::{Node, Tree};

use super::model::{Evidence, Relationship, RelationshipKind, ResolutionStatus};
use crate::model::{FileFacts, Resolution, SemanticStore};

/// One literal URL with its source range in a semantic context.
#[derive(Clone, Debug)]
pub(crate) struct ResourceFact {
    pub source: String,
    pub start: usize,
    pub end: usize,
    pub dynamic: bool,
}

/// Capture literal resource URLs. Dynamic expressions remain outside proven edges.
pub(crate) fn capture(tree: &Tree, source: &[u8], language: &str, facts: &mut SemanticStore) {
    let css = language.contains("css");
    let markup = ["html", "astro", "svelte", "angular", "jsx", "tsx"]
        .iter()
        .any(|name| language.contains(name));
    if !css && !markup {
        return;
    }
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        if let Some(value) = resource_value(node, source, css) {
            let raw = text(value, source).trim();
            let raw = if value.kind() == "arguments" {
                raw.trim_start_matches('(').trim_end_matches(')').trim()
            } else {
                raw
            };
            let url = raw.trim_matches(['\'', '"']);
            if !url.is_empty() {
                facts.semantic.structure_resources.push(ResourceFact {
                    source: url.to_owned(),
                    start: value.start_byte(),
                    end: value.end_byte(),
                    dynamic: url.contains(['{', '}', '$', '\\', '\n']),
                });
            }
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
}

fn resource_value<'a>(node: Node<'a>, source: &[u8], css: bool) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    if css {
        return match node.kind() {
            "import_statement" => node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "string_value"),
            "call_expression" if text(node, source).starts_with("url(") => node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "arguments"),
            _ => None,
        };
    }
    if !matches!(node.kind(), "attribute" | "jsx_attribute") {
        return None;
    }
    let children = node.named_children(&mut cursor).collect::<Vec<_>>();
    let name = children.first().map(|node| text(*node, source))?;
    let tag = node
        .parent()
        .and_then(|parent| {
            let mut cursor = parent.walk();
            parent
                .named_children(&mut cursor)
                .find(|child| matches!(child.kind(), "tag_name" | "identifier"))
        })
        .map(|node| text(node, source));
    let resource = name == "src"
        || name == "poster"
        || name == "href" && tag.is_some_and(|tag| tag.eq_ignore_ascii_case("link"));
    resource.then(|| children.get(1).copied()).flatten()
}

fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

/// Resolve resources through the existing file index and retain boundary evidence.
pub(crate) fn relationships(facts: &[FileFacts]) -> Vec<Relationship> {
    crate::resolver::structural_resources(facts)
        .into_iter()
        .map(|(file, context, resource, resolution)| {
            let path = &facts[file].analysis.path;
            let (target, status) = match resolution {
                Resolution::Exact(symbol) => (
                    format!("module:{}", symbol.file),
                    ResolutionStatus::Resolved,
                ),
                Resolution::External => (resource.source.clone(), ResolutionStatus::External),
                Resolution::Ambiguous(_) => (resource.source.clone(), ResolutionStatus::Ambiguous),
                Resolution::Unresolved => (resource.source.clone(), ResolutionStatus::Unresolved),
            };
            Relationship {
                ordered: false,
                source: format!("module:{path}"),
                target,
                kind: RelationshipKind::Resource,
                contract: false,
                resolution: status,
                evidence: vec![Evidence {
                    path: path.clone(),
                    start: resource.start,
                    end: resource.end,
                    owner: format!("{path}#{context}"),
                    detail: if resource.dynamic {
                        "url_dynamic"
                    } else {
                        "url"
                    }
                    .to_owned(),
                }],
            }
        })
        .collect()
}
