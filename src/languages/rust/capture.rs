use tree_sitter::Language;

use super::references::raw_reference;
use super::{
    ANALYSIS_QUERY, Analyzer, CaptureFacts, Captures, LocalScopeKind, RawDefinition, RawLocal,
    RawPath,
};
use crate::Result;
use crate::languages::query::QueryAnalyzer;
use crate::model::{DefinitionKind, ReferenceKind};

impl Analyzer {
    pub(in crate::languages) fn new(language: &Language) -> Result<Self> {
        let source = format!(
            "{}\n{}\n{}",
            tree_sitter_rust::TAGS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            ANALYSIS_QUERY
        );
        let query = QueryAnalyzer::new(language, &source)?;
        let id = |name| query.capture_id(name).expect("required capture");
        let definitions = [
            ("definition.function", DefinitionKind::Function),
            ("definition.method", DefinitionKind::Method),
            ("definition.class", DefinitionKind::Other),
            ("definition.interface", DefinitionKind::Trait),
            ("definition.module", DefinitionKind::Module),
            ("definition.macro", DefinitionKind::Macro),
            ("definition.constant", DefinitionKind::Constant),
            ("definition.static", DefinitionKind::Static),
        ]
        .into_iter()
        .map(|(name, kind)| (id(name), kind))
        .collect::<Vec<_>>();
        let local_scopes = [
            ("local.declaration", LocalScopeKind::Block),
            ("local.parameter", LocalScopeKind::Function),
            ("local.loop", LocalScopeKind::Loop),
            ("local.condition", LocalScopeKind::Condition),
            ("local.match", LocalScopeKind::Match),
            ("local.closure", LocalScopeKind::Closure),
        ]
        .into_iter()
        .map(|(name, scope)| (id(name), scope))
        .collect();
        let reference_kinds = [
            ("reference.value", ReferenceKind::Value),
            ("reference.type", ReferenceKind::Type),
        ]
        .into_iter()
        .map(|(name, kind)| (id(name), kind))
        .collect();
        let captures = Captures {
            local_scopes,
            reference_kinds,
            import: id("import"),
            reference_path: id("reference.path"),
            local_pattern: id("local.pattern"),
            name: id("name"),
            definitions,
            reference_call: id("reference.call"),
            reference_implementation: id("reference.implementation"),
        };
        Ok(Self { query, captures })
    }
}

impl<'a> CaptureFacts<'a> {
    pub(super) fn record_semantics(
        &mut self,
        query: &Captures,
        captures: &[tree_sitter::QueryCapture<'a>],
        source: &[u8],
    ) {
        for capture in captures {
            let node = capture.node;
            if capture.index == query.reference_path {
                self.paths.push(RawPath {
                    node,
                    kind: if node.kind() == "scoped_type_identifier" {
                        ReferenceKind::Type
                    } else {
                        ReferenceKind::Qualified
                    },
                });
            }
            if let Some(kind) = query.reference_kinds.get(&capture.index) {
                self.references.push(raw_reference(node, *kind, source));
            }
            if capture.index == query.import {
                self.import_roots.push(node);
            }
            if let Some(scope) = query.local_scopes.get(&capture.index) {
                self.locals
                    .push(raw_local(captures, node, query, *scope, source));
            }
        }
    }

    pub(super) fn record_tags(
        &mut self,
        query: &Captures,
        captures: &[tree_sitter::QueryCapture<'a>],
        source: &[u8],
    ) {
        let call = captures
            .iter()
            .any(|item| item.index == query.reference_call);
        let implementation = captures
            .iter()
            .any(|item| item.index == query.reference_implementation);
        for name in captures.iter().filter(|item| item.index == query.name) {
            let name_node = name.node;
            self.definitions.extend(query.definitions.iter().filter_map(
                |(definition_id, kind)| {
                    captures
                        .iter()
                        .find(|item| {
                            item.index == *definition_id
                                && item.node.start_byte() <= name_node.start_byte()
                                && name_node.end_byte() <= item.node.end_byte()
                        })
                        .map(|definition| raw_definition(definition.node, name_node, kind))
                },
            ));
            if call {
                let kind = if name_node.kind() == "field_identifier" {
                    ReferenceKind::Method
                } else {
                    ReferenceKind::Call
                };
                self.references.push(raw_reference(name_node, kind, source));
            }
            if implementation {
                self.references
                    .push(raw_reference(name_node, ReferenceKind::Type, source));
            }
        }
    }
}

fn raw_definition<'tree>(
    node: tree_sitter::Node<'tree>,
    name: tree_sitter::Node<'tree>,
    kind: &DefinitionKind,
) -> RawDefinition<'tree> {
    RawDefinition {
        node,
        name,
        kind: if is_method(node) {
            DefinitionKind::Method
        } else if *kind == DefinitionKind::Method {
            DefinitionKind::Function
        } else {
            definition_kind(kind, node.kind())
        },
    }
}

fn raw_local<'tree>(
    captures: &[tree_sitter::QueryCapture<'tree>],
    node: tree_sitter::Node<'tree>,
    query: &Captures,
    scope: LocalScopeKind,
    source: &[u8],
) -> RawLocal<'tree> {
    RawLocal {
        owner: node,
        names: captures
            .iter()
            .find(|capture| capture.index == query.local_pattern)
            .map_or_else(
                || vec![("self".to_owned(), node.byte_range())],
                |capture| super::scopes::binding_names(capture.node, source),
            ),
        scope,
    }
}

fn definition_kind(kind: &DefinitionKind, node_kind: &str) -> DefinitionKind {
    if *kind != DefinitionKind::Other {
        return kind.clone();
    }
    match node_kind {
        "struct_item" => DefinitionKind::Struct,
        "enum_item" => DefinitionKind::Enum,
        "union_item" => DefinitionKind::Union,
        "type_item" => DefinitionKind::TypeAlias,
        _ => DefinitionKind::Other,
    }
}

fn is_method(node: tree_sitter::Node<'_>) -> bool {
    node.parent().is_some_and(|declarations| {
        declarations.kind() == "declaration_list"
            && declarations
                .parent()
                .is_some_and(|owner| matches!(owner.kind(), "impl_item" | "trait_item"))
    })
}

pub(super) fn has_visibility(node: tree_sitter::Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == "visibility_modifier")
}
