mod calls;
mod imports;
mod paths;

use calls::call_owner;
use paths::path_segments;

use std::ops::Range;

use super::scopes::{contains, lexical_scope, relative_module};
use super::{CaptureFacts, ModuleSpan, RawReference};
use crate::model::{Reference, ReferenceKind};

impl<'a> CaptureFacts<'a> {
    fn use_ranges(&self) -> Vec<Range<usize>> {
        self.import_roots
            .iter()
            .map(|node| node.byte_range())
            .collect()
    }

    fn binding_ranges(&self) -> Vec<Range<usize>> {
        self.locals
            .iter()
            .flat_map(|local| local.names.iter().map(|(_, range)| range.clone()))
            .collect()
    }

    pub(super) fn normalize_references(
        &mut self,
        modules: &[ModuleSpan],
        root: &Range<usize>,
    ) -> Vec<Reference> {
        let use_ranges = self.use_ranges();
        let binding_ranges = self.binding_ranges();
        let definition_ranges = self
            .definitions
            .iter()
            .map(|definition| definition.name.byte_range())
            .collect::<Vec<_>>();
        let call_ranges = self
            .references
            .iter()
            .filter(|reference| {
                matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
            })
            .map(|reference| reference.node.byte_range())
            .collect::<Vec<_>>();
        let mut references = std::mem::take(&mut self.references);
        references.extend(self.qualified_paths(&use_ranges));
        let qualified_ranges = references
            .iter()
            .filter(|reference| {
                matches!(
                    reference.kind,
                    ReferenceKind::Qualified | ReferenceKind::Type
                )
            })
            .map(|reference| reference.node.byte_range())
            .collect::<Vec<_>>();
        references.retain(|reference| {
            !matches!(reference.kind, ReferenceKind::Value | ReferenceKind::Type)
                || !definition_ranges
                    .iter()
                    .chain(&use_ranges)
                    .chain(&binding_ranges)
                    .chain(&call_ranges)
                    .any(|range| contains(range, &reference.node.byte_range()))
                    && !qualified_ranges.iter().any(|range| {
                        range != &reference.node.byte_range()
                            && contains(range, &reference.node.byte_range())
                    })
        });
        references
            .into_iter()
            .map(|reference| {
                let module = relative_module(reference.node.start_byte(), modules);
                let (scope_start, scope_end) = lexical_scope(reference.node, false, root);
                let call_owner =
                    call_owner(reference.node, &reference.node.byte_range(), reference.kind);
                Reference {
                    path: reference.path,
                    module,
                    kind: reference.kind,
                    start_byte: reference.node.start_byte(),
                    end_byte: reference.node.end_byte(),
                    scope_start,
                    scope_end,
                    call_owner,
                    execution_owner: call_owner,
                    access: None,
                    context_id: 0,
                }
            })
            .collect()
    }

    fn qualified_paths(&self, use_ranges: &[Range<usize>]) -> Vec<RawReference<'a>> {
        let candidates = self
            .paths
            .iter()
            .filter_map(|path| {
                let segments = path_segments(path.node, self.source);
                (!segments.is_empty()).then_some(RawReference {
                    node: path.node,
                    path: segments,
                    kind: path.kind,
                })
            })
            .filter(|reference| {
                !use_ranges
                    .iter()
                    .any(|range| contains(range, &reference.node.byte_range()))
            })
            .collect::<Vec<_>>();
        let ranges = candidates
            .iter()
            .map(|reference| reference.node.byte_range())
            .collect::<Vec<_>>();
        candidates
            .into_iter()
            .filter(|inner| {
                !ranges.iter().any(|outer| {
                    outer != &inner.node.byte_range() && contains(outer, &inner.node.byte_range())
                })
            })
            .collect()
    }
}

pub(super) fn raw_reference<'tree>(
    node: tree_sitter::Node<'tree>,
    kind: ReferenceKind,
    source: &[u8],
) -> RawReference<'tree> {
    RawReference {
        node,
        path: vec![super::node_text(node, source)],
        kind,
    }
}
