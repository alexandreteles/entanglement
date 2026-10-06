mod paths;
mod scopes;

use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::model::{ModulePath, Reference, ReferenceKind};

use super::syntax::{field, nodes, range, text};

pub(super) fn collect(root: Node<'_>, source: &[u8], excluded: &[Range<usize>]) -> Vec<Reference> {
    let tree = nodes(root);
    let mut capture = References {
        root,
        source,
        excluded,
        claimed: HashSet::new(),
        result: Vec::new(),
    };
    capture.calls(&tree);
    capture.qualified(&tree);
    capture.values(&tree);
    capture.finish()
}

struct References<'tree, 'source> {
    root: Node<'tree>,
    source: &'source [u8],
    excluded: &'source [Range<usize>],
    claimed: HashSet<(usize, usize)>,
    result: Vec<Reference>,
}

impl References<'_, '_> {
    fn calls(&mut self, tree: &[Node<'_>]) {
        for call in tree.iter().copied().filter(|node| node.kind() == "call") {
            if let Some(target) = field(call, "function") {
                self.add_path(target, ReferenceKind::Call, Some(call));
            }
        }
    }

    fn qualified(&mut self, tree: &[Node<'_>]) {
        for node in tree {
            let is_path = node.kind() == "dotted_name"
                || node.kind() == "attribute" && paths::is_outer_attribute(*node);
            if is_path && !paths::is_store_target(*node) {
                self.add_path(*node, paths::kind_for(*node), None);
            }
        }
    }

    fn values(&mut self, tree: &[Node<'_>]) {
        for node in tree
            .iter()
            .copied()
            .filter(|node| node.kind() == "identifier")
        {
            let span = range(node);
            if self
                .excluded
                .iter()
                .any(|item| paths::contains(item, &span))
                || self
                    .claimed
                    .iter()
                    .any(|(start, end)| *start <= span.start && span.end <= *end)
                || paths::is_label(node)
                || paths::is_attribute_label(node)
            {
                continue;
            }
            self.record(
                node,
                vec![text(node, self.source)],
                span,
                paths::kind_for(node),
                None,
            );
        }
    }

    fn add_path(&mut self, node: Node<'_>, kind: ReferenceKind, call: Option<Node<'_>>) {
        let span = range(node);
        if self
            .excluded
            .iter()
            .any(|item| paths::contains(item, &span))
            || !self.claimed.insert((span.start, span.end))
        {
            return;
        }
        let path = paths::path_segments(node, self.source);
        if !path.is_empty() {
            self.record(node, path, span, kind, call);
        }
    }

    fn record(
        &mut self,
        node: Node<'_>,
        path: Vec<String>,
        span: Range<usize>,
        kind: ReferenceKind,
        call: Option<Node<'_>>,
    ) {
        let scope = scopes::containing_scope(node, self.root);
        self.result.push(Reference {
            path,
            module: ModulePath::default(),
            kind,
            start_byte: span.start,
            end_byte: span.end,
            scope_start: scope.start,
            scope_end: scope.end,
            call_owner: call.and_then(|node| scopes::call_owner(node, &span)),
            context_id: 0,
        });
    }

    fn finish(mut self) -> Vec<Reference> {
        let mut seen = HashSet::new();
        self.result
            .retain(|item| seen.insert((item.path.clone(), item.start_byte, item.end_byte)));
        self.result
    }
}
