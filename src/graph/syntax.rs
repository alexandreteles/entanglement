use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

/// An index from tree-local identities to tree-sitter nodes.
///
/// Rust captures retain numeric identities before normalization; nodes provide
/// their own structural links without duplicating the tree's relationships.
#[derive(Default)]
pub(crate) struct SyntaxGraph<'tree> {
    nodes: HashMap<usize, Node<'tree>>,
    source: &'tree [u8],
}

impl<'tree> SyntaxGraph<'tree> {
    /// Index every node below `root` for later identity-based lookup.
    pub(crate) fn from_tree(root: Node<'tree>, source: &'tree [u8]) -> Self {
        let mut graph = Self {
            nodes: HashMap::new(),
            source,
        };
        let mut cursor = root.walk();
        loop {
            let node = cursor.node();
            graph.nodes.insert(node.id(), node);
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return graph;
                }
            }
        }
    }

    /// Return the tree-sitter node with this tree-local identity.
    pub(crate) fn node(&self, id: usize) -> Option<Node<'tree>> {
        self.nodes.get(&id).copied()
    }

    /// Return the grammar-provided node kind.
    pub(crate) fn kind(&self, id: usize) -> Option<&str> {
        self.node(id).map(|node| node.kind())
    }

    /// Return the node's half-open byte range.
    pub(crate) fn range(&self, id: usize) -> Option<Range<usize>> {
        self.node(id).map(|node| node.start_byte()..node.end_byte())
    }

    /// Return the source text covered by a node.
    pub(crate) fn text(&self, id: usize) -> Option<String> {
        let range = self.range(id)?;
        Some(String::from_utf8_lossy(self.source.get(range)?).into_owned())
    }

    /// Return a node's parent identity.
    pub(crate) fn parent(&self, id: usize) -> Option<usize> {
        self.node(id)?.parent().map(|node| node.id())
    }

    /// Return the child identity assigned to a grammar-provided field name.
    pub(crate) fn child_by_field(&self, id: usize, field: &str) -> Option<usize> {
        self.node(id)?
            .child_by_field_name(field)
            .map(|node| node.id())
    }

    /// Test whether the node is the child assigned to a parent field.
    pub(crate) fn is_field(&self, id: usize, field: &str) -> bool {
        self.parent(id)
            .and_then(|parent| self.child_by_field(parent, field))
            == Some(id)
    }

    /// Iterate from a node through its ancestors, including the node itself.
    pub(crate) fn ancestors(&self, id: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(id), |current| self.parent(*current))
    }

    /// Find the nearest node satisfying an adapter-supplied predicate.
    pub(crate) fn nearest(
        &self,
        id: usize,
        mut predicate: impl FnMut(usize, Node<'tree>) -> bool,
    ) -> Option<usize> {
        self.ancestors(id).find(|candidate| {
            self.node(*candidate)
                .is_some_and(|node| predicate(*candidate, node))
        })
    }

    /// Return the source bytes used to build this index.
    pub(crate) fn source(&self) -> &'tree [u8] {
        self.source
    }
}

#[cfg(test)]
#[path = "../../tests/unit/graph_syntax.rs"]
mod tests;
