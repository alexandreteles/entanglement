mod calls;
mod imports;
mod paths;

pub(super) use paths::child_field;
use paths::path_segments;

use std::ops::Range;

use super::scopes::{binding_names, contains, lexical_scope, relative_module};
use super::{CaptureFacts, ModuleSpan, RawReference};
use crate::model::{Reference, ReferenceKind};

impl<'a> CaptureFacts<'a> {
    fn use_ranges(&self) -> Vec<Range<usize>> {
        self.import_roots
            .iter()
            .filter_map(|id| self.graph.nodes.get(id).map(tree_sitter::Node::byte_range))
            .collect()
    }

    fn binding_ranges(&self) -> Vec<Range<usize>> {
        self.locals
            .iter()
            .flat_map(|local| {
                local.pattern.map_or_else(
                    || {
                        self.graph
                            .nodes
                            .get(&local.owner)
                            .map(|node| vec![node.byte_range()])
                            .unwrap_or_default()
                    },
                    |pattern| {
                        binding_names(pattern, &self.graph)
                            .into_iter()
                            .map(|(_, range)| range)
                            .collect()
                    },
                )
            })
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
            .map(|definition| definition.name_range.clone())
            .collect::<Vec<_>>();
        let call_ranges = self
            .references
            .iter()
            .filter(|reference| {
                matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
            })
            .map(|reference| reference.range.clone())
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
            .map(|reference| reference.range.clone())
            .collect::<Vec<_>>();
        references.retain(|reference| {
            !matches!(reference.kind, ReferenceKind::Value | ReferenceKind::Type)
                || !definition_ranges
                    .iter()
                    .chain(&use_ranges)
                    .chain(&binding_ranges)
                    .chain(&call_ranges)
                    .any(|range| contains(range, &reference.range))
                    && !qualified_ranges
                        .iter()
                        .any(|range| range != &reference.range && contains(range, &reference.range))
        });
        references
            .into_iter()
            .map(|reference| {
                let module = relative_module(reference.range.start, modules);
                let (scope_start, scope_end) =
                    lexical_scope(reference.node_id, false, &self.graph, root);
                Reference {
                    path: reference.path,
                    module,
                    kind: reference.kind,
                    start_byte: reference.range.start,
                    end_byte: reference.range.end,
                    scope_start,
                    scope_end,
                    call_owner: self.call_owner(
                        reference.node_id,
                        &reference.range,
                        reference.kind,
                    ),
                    context_id: 0,
                }
            })
            .collect()
    }

    fn qualified_paths(&self, use_ranges: &[Range<usize>]) -> Vec<RawReference> {
        let candidates = self
            .paths
            .iter()
            .filter_map(|path| {
                let node = self.graph.nodes.get(&path.node_id)?;
                let segments = path_segments(path.node_id, &self.graph);
                (!segments.is_empty()).then_some(RawReference {
                    node_id: path.node_id,
                    path: segments,
                    range: node.byte_range(),
                    kind: path.kind,
                })
            })
            .filter(|reference| {
                !use_ranges
                    .iter()
                    .any(|range| contains(range, &reference.range))
            })
            .collect::<Vec<_>>();
        let ranges = candidates
            .iter()
            .map(|reference| reference.range.clone())
            .collect::<Vec<_>>();
        candidates
            .into_iter()
            .filter(|inner| {
                !ranges
                    .iter()
                    .any(|outer| outer != &inner.range && contains(outer, &inner.range))
            })
            .collect()
    }
}

pub(super) fn raw_reference(
    node: tree_sitter::Node<'_>,
    kind: ReferenceKind,
    source: &[u8],
) -> RawReference {
    RawReference {
        node_id: node.id(),
        path: vec![super::node_text(node, source)],
        range: node.start_byte()..node.end_byte(),
        kind,
    }
}
