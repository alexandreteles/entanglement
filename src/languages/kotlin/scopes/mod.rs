use std::ops::Range;

use tree_sitter::Node;

use super::syntax::{self, ContextParameters};

mod capture;
mod names;
mod nodes;

pub(super) use capture::{capture, property_initializer_range};

pub(super) struct ScopeIndex<'tree> {
    root: Node<'tree>,
    source: &'tree [u8],
    contexts: &'tree [ContextParameters],
    recovered_types: Vec<syntax::RecoveredType>,
}

impl<'tree> ScopeIndex<'tree> {
    pub(super) fn new(
        root: Node<'tree>,
        source: &'tree [u8],
        contexts: &'tree [ContextParameters],
    ) -> Self {
        Self {
            root,
            source,
            contexts,
            recovered_types: syntax::recovered_types(root, source),
        }
    }

    pub(super) fn root(&self) -> Node<'tree> {
        self.root
    }

    pub(super) fn source(&self) -> &'tree [u8] {
        self.source
    }

    pub(super) fn contexts(&self) -> &'tree [ContextParameters] {
        self.contexts
    }

    pub(super) fn recovered_types(&self) -> &[syntax::RecoveredType] {
        &self.recovered_types
    }

    pub(super) fn metric_scope_range_for(&self, node: Node<'_>) -> Option<Range<usize>> {
        capture::metric_scope_range_for(self, node)
    }
}
