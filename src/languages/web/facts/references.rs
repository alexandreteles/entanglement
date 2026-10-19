use std::ops::Range;

use std::collections::HashSet;

use tree_sitter::{Node, QueryCapture};

use crate::model::{ModulePath, Reference, ReferenceKind};

use super::namespaces::namespace_reference;
use super::scope::{enclosing_function, scope_range, text};
use super::{CaptureFacts, Captures};

#[derive(Clone)]
pub(super) struct RawReference {
    pub(super) path: Vec<String>,
    pub(super) range: Range<usize>,
    pub(super) kind: ReferenceKind,
    pub(super) scope: Range<usize>,
    pub(super) call_owner: Option<usize>,
}

impl CaptureFacts {
    pub(super) fn record_references(
        &mut self,
        query: &Captures,
        captures: &[QueryCapture<'_>],
        source: &[u8],
    ) {
        for capture in captures {
            if let Some((_, kind)) = query
                .references
                .iter()
                .find(|(capture_id, _)| *capture_id == capture.index)
            {
                self.references.extend(raw_reference(
                    capture.node,
                    *kind,
                    source,
                    self.root.clone(),
                ));
            }
            if Some(capture.index) == query.namespace
                && jsx_name(capture.node).is_none()
                && let Some(namespace) =
                    namespace_reference(capture.node, source, self.root.clone())
            {
                self.namespaces.push(namespace);
            }
        }
    }

    pub(super) fn resolve_namespace_members(&mut self) {
        let aliases = self
            .semantic
            .imports
            .iter()
            .filter_map(|item| item.alias.clone())
            .collect::<HashSet<_>>();
        for item in &self.namespaces {
            if aliases.contains(&item.alias) {
                let kind = if item.is_call {
                    ReferenceKind::Call
                } else {
                    ReferenceKind::Qualified
                };
                self.references.push(RawReference {
                    path: vec![item.alias.clone(), item.member.clone()],
                    range: item.range.clone(),
                    kind,
                    scope: item.scope.clone(),
                    call_owner: item.call_owner,
                });
            }
        }
    }

    pub(super) fn materialize_references(&mut self) {
        let ranges = self
            .import_ranges
            .iter()
            .chain(&self.export_clause_ranges)
            .chain(&self.definition_names)
            .cloned()
            .collect::<Vec<_>>();
        let namespaces = self
            .namespaces
            .iter()
            .filter(|item| {
                self.semantic
                    .imports
                    .iter()
                    .any(|import| import.alias.as_deref() == Some(&item.alias))
            })
            .cloned()
            .collect::<Vec<_>>();
        for reference in &self.references {
            if ranges.iter().any(|range| contains(range, &reference.range))
                || self.semantic.locals.iter().any(|local| {
                    local.start_byte <= reference.range.start
                        && reference.range.end <= local.end_byte
                })
                || namespaces.iter().any(|item| {
                    (reference.range == item.object || reference.range == item.property)
                        && item.range != reference.range
                })
            {
                continue;
            }
            self.semantic.references.push(Reference {
                path: reference.path.clone(),
                module: ModulePath::default(),
                kind: reference.kind,
                start_byte: reference.range.start,
                end_byte: reference.range.end,
                scope_start: reference.scope.start,
                scope_end: reference.scope.end,
                call_owner: reference.call_owner,
                execution_owner: reference.call_owner,
                access: None,
                context_id: 0,
            });
        }
    }
}

fn raw_reference(
    node: Node<'_>,
    kind: ReferenceKind,
    source: &[u8],
    root: Range<usize>,
) -> Option<RawReference> {
    let jsx = jsx_name(node);
    let node = jsx.unwrap_or(node);
    let name = text(node, source);
    if jsx.is_some()
        && (node
            .parent()
            .is_some_and(|parent| parent.kind() == "jsx_closing_element")
            || name.contains(':')
            || (!name.contains('.')
                && name.starts_with(|character: char| character.is_ascii_lowercase())))
    {
        return None;
    }
    let (scope_start, scope_end) = scope_range(node, root, false);
    Some(RawReference {
        path: if jsx.is_some() {
            name.split('.').map(str::to_owned).collect()
        } else {
            vec![name]
        },
        range: node.byte_range(),
        kind: if jsx.is_some() && node.kind() == "member_expression" {
            ReferenceKind::Qualified
        } else {
            kind
        },
        scope: scope_start..scope_end,
        // A JSX tag selects a component. It does not call the component here.
        call_owner: (jsx.is_none() && matches!(kind, ReferenceKind::Call | ReferenceKind::Method))
            .then(|| enclosing_function(node))
            .flatten(),
    })
}

fn jsx_name(mut node: Node<'_>) -> Option<Node<'_>> {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "member_expression" | "jsx_namespace_name" => node = parent,
            "jsx_opening_element" | "jsx_self_closing_element" | "jsx_closing_element" => {
                return parent
                    .child_by_field_name("name")
                    .filter(|name| name.id() == node.id());
            }
            _ => return None,
        }
    }
    None
}

fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
