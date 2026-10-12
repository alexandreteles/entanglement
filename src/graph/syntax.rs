use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::Node;

/// An owned index of one tree-sitter tree's generic syntax relationships.
///
/// The index stores nodes and their structural links only. It deliberately
/// assigns no meaning to grammar-specific kinds; adapters supply the
/// predicates that identify scopes, owners, declarations, and other roles.
#[derive(Default)]
pub(crate) struct SyntaxGraph<'tree> {
    nodes: HashMap<usize, Node<'tree>>,
    parents: HashMap<usize, Option<usize>>,
    children: HashMap<usize, Vec<usize>>,
    source: &'tree [u8],
}

impl<'tree> SyntaxGraph<'tree> {
    /// Index every node below `root`, retaining generic parent and child links.
    pub(crate) fn from_tree(root: Node<'tree>, source: &'tree [u8]) -> Self {
        let mut graph = Self {
            nodes: HashMap::new(),
            parents: HashMap::new(),
            children: HashMap::new(),
            source,
        };
        let mut pending = vec![(root, None)];
        while let Some((node, parent)) = pending.pop() {
            let id = node.id();
            graph.nodes.insert(id, node);
            graph.parents.insert(id, parent);
            if let Some(parent) = parent {
                graph.children.entry(parent).or_default().push(id);
            }
            let mut cursor = node.walk();
            pending.extend(
                node.children(&mut cursor)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(|child| (child, Some(id))),
            );
        }
        graph
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
        self.parents.get(&id).copied().flatten()
    }

    /// Return child identities in source order.
    pub(crate) fn children(&self, id: usize) -> &[usize] {
        self.children.get(&id).map_or(&[], Vec::as_slice)
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

    /// Resolve an adapter-defined semantic owner by walking generic ancestry.
    pub(crate) fn owner(
        &self,
        id: usize,
        predicate: impl FnMut(usize, Node<'tree>) -> bool,
    ) -> Option<usize> {
        self.nearest(id, predicate)
    }

    /// Return the source bytes used to build this index.
    pub(crate) fn source(&self) -> &'tree [u8] {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxGraph;

    #[test]
    fn indexed_children_keep_tree_order_and_parent_links() {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).expect("Rust grammar loads");
        let source = b"fn first() {}\nfn second() {}";
        let tree = parser.parse(source, None).expect("source parses");
        let root = tree.root_node();
        let graph = SyntaxGraph::from_tree(root, source);

        let mut cursor = root.walk();
        let expected = root
            .children(&mut cursor)
            .map(|node| node.id())
            .collect::<Vec<_>>();
        assert_eq!(graph.children(root.id()), expected);
        assert!(
            expected
                .iter()
                .all(|child| graph.parent(*child) == Some(root.id()))
        );
        assert!(
            graph
                .children(root.id())
                .windows(2)
                .all(|pair| graph.range(pair[0]).unwrap().start
                    < graph.range(pair[1]).unwrap().start)
        );
    }
}
