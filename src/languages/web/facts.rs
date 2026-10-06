mod capture_schema;
mod definitions;
mod exports;
mod namespaces;
mod references;
mod scope;

use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::{Node, QueryCapture};

use crate::model::{
    Definition, DefinitionKind, Export, Import, LocalBinding, ModulePath, Reference,
};

use super::{exports::parse_export, imports::parse_import, syntax::named_child};

pub(super) use capture_schema::Captures;
pub(super) use scope::binding_names;

#[derive(Default)]
pub(super) struct SemanticFacts {
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
}

pub(super) struct CaptureFacts {
    pub(super) semantic: SemanticFacts,
    pub(super) root: Range<usize>,
    definition_names: Vec<Range<usize>>,
    import_ranges: Vec<Range<usize>>,
    export_clause_ranges: Vec<Range<usize>>,
    references: Vec<references::RawReference>,
    namespaces: Vec<namespaces::NamespaceReference>,
}

impl CaptureFacts {
    pub(super) fn new(root: Range<usize>) -> Self {
        Self {
            semantic: SemanticFacts::default(),
            root,
            definition_names: Vec::new(),
            import_ranges: Vec::new(),
            export_clause_ranges: Vec::new(),
            references: Vec::new(),
            namespaces: Vec::new(),
        }
    }

    pub(super) fn record(
        &mut self,
        query: &Captures,
        captures: &[QueryCapture<'_>],
        source: &[u8],
    ) {
        self.record_imports(query, captures, source);
        self.record_definitions(query, captures, source);
        self.record_references(query, captures, source);
        self.record_locals(query, captures, source);
    }

    fn record_imports(&mut self, query: &Captures, captures: &[QueryCapture<'_>], source: &[u8]) {
        for capture in captures {
            let node = capture.node;
            if Some(capture.index) == query.import {
                self.import_ranges.push(node.byte_range());
                self.semantic.imports.extend(parse_import(node, source));
            }
            if Some(capture.index) == query.export {
                if let Some(clause) = named_child(node, "export_clause") {
                    self.export_clause_ranges.push(clause.byte_range());
                }
                let exports = parse_export(node, source);
                if exports.iter().any(|export| {
                    export.exported_name == "default"
                        && export.local_name.as_deref() == Some("default")
                }) {
                    self.add_default_callable_definition(node);
                }
                self.semantic.exports.extend(exports);
            }
        }
    }

    pub(super) fn finish(mut self, root: Node<'_>) -> SemanticFacts {
        self.root = root.byte_range();
        for import in &mut self.semantic.imports {
            import.module = ModulePath::default();
            import.scope_start = self.root.start;
            import.scope_end = self.root.end;
        }
        for export in &mut self.semantic.exports {
            export.scope_start = self.root.start;
            export.scope_end = self.root.end;
        }
        self.mark_public_definitions();
        self.resolve_namespace_members();
        self.materialize_references();
        self.remove_duplicate_top_level_locals();
        self.deduplicate_facts();
        self.semantic
    }

    fn mark_public_definitions(&mut self) {
        for definition in &mut self.semantic.definitions {
            definition.is_public = self.semantic.exports.iter().any(|export| {
                export.source.is_none()
                    && export.local_name.as_deref() == Some(definition.name.as_str())
            });
        }
    }

    fn remove_duplicate_top_level_locals(&mut self) {
        self.semantic.locals.retain(|local| {
            !self.semantic.definitions.iter().any(|definition| {
                definition.name == local.name
                    && definition.scope_start == local.scope_start
                    && definition.scope_end == local.scope_end
                    && (definition.kind == DefinitionKind::Function
                        || (definition.start_byte <= local.start_byte
                            && local.end_byte <= definition.end_byte))
            })
        });
    }

    fn deduplicate_facts(&mut self) {
        deduplicate(&mut self.semantic.definitions, |item| {
            (item.name.clone(), item.kind.clone(), item.start_byte)
        });
        deduplicate(&mut self.semantic.imports, |item| {
            (
                item.source.clone(),
                item.imported_name.clone(),
                item.alias.clone(),
                item.start_byte,
            )
        });
        deduplicate(&mut self.semantic.exports, |item| {
            (
                item.source.clone(),
                item.imported_name.clone(),
                item.local_name.clone(),
                item.exported_name.clone(),
                item.start_byte,
            )
        });
        deduplicate(&mut self.semantic.locals, |item| {
            (
                item.name.clone(),
                item.start_byte,
                item.scope_start,
                item.scope_end,
            )
        });
        discard_value_duplicates(&mut self.semantic.references);
        deduplicate(&mut self.semantic.references, |item| {
            (item.path.clone(), item.start_byte, item.end_byte)
        });
    }
}

fn discard_value_duplicates(references: &mut Vec<Reference>) {
    let specific_ranges = references
        .iter()
        .filter(|item| item.kind != crate::model::ReferenceKind::Value)
        .map(|item| (item.start_byte, item.end_byte))
        .collect::<HashSet<_>>();
    references.retain(|item| {
        item.kind != crate::model::ReferenceKind::Value
            || !specific_ranges.contains(&(item.start_byte, item.end_byte))
    });
}

fn deduplicate<T, K: Eq + std::hash::Hash>(values: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    values.retain(|item| seen.insert(key(item)));
}
