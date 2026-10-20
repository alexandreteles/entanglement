use std::ops::Range;

use std::collections::HashSet;

use tree_sitter::{Node, QueryCapture};

use crate::model::{ModulePath, Reference, ReferenceKind};

use super::namespaces::namespace_reference;
use super::scope::{enclosing_function, scope_range, text};
use super::{CaptureFacts, Captures};

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
                self.references.extend(capture_reference(
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

    pub(super) fn finish_references(&mut self) {
        let aliases = self
            .semantic
            .imports
            .iter()
            .filter_map(|item| item.alias.as_deref())
            .collect::<HashSet<_>>();
        self.namespaces
            .retain(|item| aliases.contains(item.reference.path[0].as_str()));
        self.references
            .extend(self.namespaces.iter().map(|item| item.reference.clone()));
        let ranges = self
            .import_ranges
            .iter()
            .chain(&self.export_clause_ranges)
            .chain(&self.definition_names)
            .collect::<Vec<_>>();
        self.references.retain(|reference| {
            let span = reference.start_byte..reference.end_byte;
            !ranges.iter().any(|range| contains(range, &span))
                && !self
                    .semantic
                    .locals
                    .iter()
                    .any(|local| local.start_byte <= span.start && span.end <= local.end_byte)
                && !self.namespaces.iter().any(|item| {
                    (span == item.object || span == item.property)
                        && (item.reference.start_byte..item.reference.end_byte) != span
                })
        });
        self.semantic.references.append(&mut self.references);
    }
}

fn capture_reference(
    node: Node<'_>,
    kind: ReferenceKind,
    source: &[u8],
    root: Range<usize>,
) -> Option<Reference> {
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
    // A JSX tag selects a component. It does not call the component here.
    let call_owner = (jsx.is_none() && matches!(kind, ReferenceKind::Call | ReferenceKind::Method))
        .then(|| enclosing_function(node))
        .flatten();
    Some(Reference {
        path: if jsx.is_some() {
            name.split('.').map(str::to_owned).collect()
        } else {
            vec![name]
        },
        module: ModulePath::default(),
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        kind: if jsx.is_some() && node.kind() == "member_expression" {
            ReferenceKind::Qualified
        } else {
            kind
        },
        scope_start,
        scope_end,
        call_owner,
        execution_owner: call_owner,
        access: None,
        context_id: 0,
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
