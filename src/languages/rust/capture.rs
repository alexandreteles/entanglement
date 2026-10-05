use tree_sitter::{Language, Query};

use super::node_text;
use super::references::raw_reference;
use super::{
    ANALYSIS_QUERY, Analyzer, CaptureFacts, Captures, InjectionProperties, InjectionRequest,
    LocalScopeKind, NodeGraph, RawDefinition, RawLocal, RawPath,
};
use crate::Result;
use crate::metrics::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};
use crate::model::{DefinitionKind, ReferenceKind};

impl Analyzer {
    pub(in crate::languages) fn new(language: &Language) -> Result<Self> {
        let source = format!(
            "{}\n{}\n{}",
            tree_sitter_rust::TAGS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            ANALYSIS_QUERY
        );
        let query = Query::new(language, &source)?;
        let id = |name| {
            query
                .capture_index_for_name(name)
                .expect("required capture")
        };
        let roles = [
            ("syntax.node", SyntaxRole::Node),
            ("comment", SyntaxRole::Comment),
            ("metric.function", SyntaxRole::Function),
            ("metric.cognitive.if", SyntaxRole::CognitiveIf),
            (
                "metric.cognitive.condition_boundary",
                SyntaxRole::CognitiveConditionBoundary,
            ),
            ("metric.cognitive.else", SyntaxRole::CognitiveElse),
            ("metric.cognitive.else_if", SyntaxRole::CognitiveElseIf),
            ("metric.cognitive.loop", SyntaxRole::CognitiveLoop),
            ("metric.cognitive.let_else", SyntaxRole::CognitiveLetElse),
            ("metric.cognitive.multiway", SyntaxRole::CognitiveMultiway),
            (
                "metric.cognitive.logical_expression",
                SyntaxRole::CognitiveLogicalExpression,
            ),
            (
                "metric.cognitive.logical_and",
                SyntaxRole::CognitiveLogicalAnd,
            ),
            (
                "metric.cognitive.logical_or",
                SyntaxRole::CognitiveLogicalOr,
            ),
            (
                "metric.cognitive.parentheses",
                SyntaxRole::CognitiveParentheses,
            ),
            ("metric.cognitive.closure", SyntaxRole::CognitiveClosure),
            (
                "metric.cognitive.labeled_jump",
                SyntaxRole::CognitiveLabeledJump,
            ),
            ("metric.condition", SyntaxRole::Condition),
            ("metric.logical_condition", SyntaxRole::LogicalCondition),
            ("metric.multiway", SyntaxRole::Multiway),
            ("metric.case", SyntaxRole::Case),
            ("import", SyntaxRole::Import),
            ("reference.path", SyntaxRole::Reference),
            ("reference.call", SyntaxRole::Reference),
            ("reference.implementation", SyntaxRole::Reference),
            ("reference.value", SyntaxRole::Reference),
            ("reference.type", SyntaxRole::Reference),
            ("injection.content", SyntaxRole::InjectionContent),
        ]
        .into_iter()
        .map(|(name, role)| (id(name), role))
        .collect();
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
        let definition_ids = definitions.iter().map(|(capture, _)| *capture).collect();
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
            roles,
            definition_ids,
            local_scopes,
            reference_kinds,
            syntax_node: id("syntax.node"),
            metric_function: id("metric.function"),
            import: id("import"),
            reference_path: id("reference.path"),
            local_pattern: id("local.pattern"),
            injection_content: id("injection.content"),
            injection_language: query.capture_index_for_name("injection.language"),
            name: id("name"),
            definitions,
            reference_call: id("reference.call"),
            reference_implementation: id("reference.implementation"),
            injection_patterns: (0..query.pattern_count())
                .filter_map(|index| {
                    let mut properties = InjectionProperties::default();
                    let mut found = false;
                    for property in query.property_settings(index) {
                        match property.key.as_ref() {
                            "injection.language" => {
                                found = true;
                                properties.language = property.value.as_deref().map(str::to_owned);
                                properties.language_capture =
                                    property.capture_id.map(|id| id as u32);
                            }
                            "injection.include-children" => {
                                found = true;
                                properties.include_children =
                                    property.value.as_deref() != Some("false");
                            }
                            "injection.priority" => {
                                found = true;
                                properties.priority = property
                                    .value
                                    .as_deref()
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or_default();
                            }
                            _ => {}
                        }
                    }
                    found.then_some((index, properties))
                })
                .collect(),
        };
        Ok(Self { query, captures })
    }
}

impl<'a> CaptureFacts<'a> {
    pub(super) fn record_syntax(
        &mut self,
        query: &Captures,
        captures: &[tree_sitter::QueryCapture<'a>],
        source: &[u8],
    ) {
        for capture in captures {
            let node = capture.node;
            let parent = node.parent().map(|parent| parent.id());
            if capture.index == query.syntax_node {
                self.parents.insert(node.id(), parent);
                self.graph.nodes.insert(node.id(), node);
            }
            if let Some(role) = query.roles.get(&capture.index) {
                self.events.push(event(*role, node));
            }
            if query.definition_ids.contains(&capture.index) {
                self.events.push(event(SyntaxRole::Definition, node));
            }
            if capture.index == query.metric_function
                && let Some(name) = node.child_by_field_name("name")
            {
                self.functions.push(FunctionScope {
                    name: node_text(name, source),
                    range: node.start_byte()..node.end_byte(),
                    line: node.start_position().row + 1,
                });
            }
            if capture.index == query.reference_path {
                self.paths.push(RawPath {
                    node_id: node.id(),
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
                self.import_roots.push(node.id());
            }
            if let Some(scope) = query.local_scopes.get(&capture.index) {
                self.locals.push(raw_local(captures, node, query, *scope));
            }
        }
    }

    pub(super) fn record_tags(
        &mut self,
        query: &Captures,
        captures: &[tree_sitter::QueryCapture<'_>],
        source: &[u8],
    ) {
        for name in captures.iter().filter(|item| item.index == query.name) {
            let name_node = name.node;
            for (definition_id, kind) in &query.definitions {
                if let Some(definition) = captures.iter().find(|item| {
                    item.index == *definition_id
                        && item.node.start_byte() <= name_node.start_byte()
                        && name_node.end_byte() <= item.node.end_byte()
                }) {
                    self.definitions
                        .push(raw_definition(definition.node, name_node, kind, source));
                }
            }
            for (capture_id, kind) in [
                (query.reference_call, None),
                (query.reference_implementation, Some(ReferenceKind::Type)),
            ] {
                if captures.iter().any(|item| item.index == capture_id) {
                    let kind = kind.unwrap_or(if name_node.kind() == "field_identifier" {
                        ReferenceKind::Method
                    } else {
                        ReferenceKind::Call
                    });
                    self.references.push(raw_reference(name_node, kind, source));
                }
            }
        }
    }

    pub(super) fn record_injection(
        &mut self,
        query: &Captures,
        pattern_index: usize,
        captures: &[tree_sitter::QueryCapture<'_>],
        source: &[u8],
    ) {
        let Some(content) = captures
            .iter()
            .find(|item| item.index == query.injection_content)
        else {
            return;
        };
        let has_language_capture = query
            .injection_language
            .is_some_and(|id| captures.iter().any(|capture| capture.index == id));
        if has_language_capture || query.injection_patterns.contains_key(&pattern_index) {
            self.events
                .push(event(SyntaxRole::InjectionLanguage, content.node));
        }
        let mut properties = query
            .injection_patterns
            .get(&pattern_index)
            .cloned()
            .unwrap_or_default();
        if properties.language.is_none() && properties.language_capture.is_none() {
            properties.language_capture = query.injection_language.filter(|_| has_language_capture);
        }
        if let Some(injection) = injection_from_match(&properties, captures, content.node, source) {
            self.injections.push(injection);
        }
    }
}

fn event(role: SyntaxRole, node: tree_sitter::Node<'_>) -> SyntaxEvent {
    SyntaxEvent {
        role,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_row: node.start_position().row,
        end_row: node.end_position().row,
        end_column: node.end_position().column,
        node_id: node.id(),
        parent_id: node.parent().map(|parent| parent.id()),
        terminal: node.child_count() == 0,
    }
}

fn raw_definition(
    node: tree_sitter::Node<'_>,
    name: tree_sitter::Node<'_>,
    kind: &DefinitionKind,
    source: &[u8],
) -> RawDefinition {
    RawDefinition {
        node_id: node.id(),
        range: node.start_byte()..node.end_byte(),
        name_range: name.start_byte()..name.end_byte(),
        name: node_text(name, source),
        kind: if is_method(node) {
            DefinitionKind::Method
        } else if *kind == DefinitionKind::Method {
            DefinitionKind::Function
        } else {
            definition_kind(kind, node.kind())
        },
        body: node
            .child_by_field_name("body")
            .map(|body| body.start_byte()..body.end_byte()),
        inline_module: node.kind() == "mod_item" && node.child_by_field_name("body").is_some(),
        external_module: node.kind() == "mod_item" && node.child_by_field_name("body").is_none(),
    }
}

fn raw_local(
    captures: &[tree_sitter::QueryCapture<'_>],
    node: tree_sitter::Node<'_>,
    query: &Captures,
    scope: LocalScopeKind,
) -> RawLocal {
    RawLocal {
        owner: node.id(),
        pattern: capture_node_id(captures, query.local_pattern),
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

pub(super) fn has_visibility(node_id: usize, graph: &NodeGraph<'_>) -> bool {
    let Some(node) = graph.nodes.get(&node_id) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == "visibility_modifier")
}

fn capture_node_id(captures: &[tree_sitter::QueryCapture<'_>], id: u32) -> Option<usize> {
    captures
        .iter()
        .find(|capture| capture.index == id)
        .map(|capture| capture.node.id())
}

fn injection_from_match(
    properties: &InjectionProperties,
    captures: &[tree_sitter::QueryCapture<'_>],
    content: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<InjectionRequest> {
    let language = properties.language.clone().or_else(|| {
        properties.language_capture.and_then(|id| {
            captures
                .iter()
                .find(|capture| capture.index == id)
                .map(|capture| node_text(capture.node, source))
        })
    })?;
    let child_ranges = if properties.include_children {
        Vec::new()
    } else {
        let mut cursor = content.walk();
        content
            .named_children(&mut cursor)
            .map(|child| child.range())
            .collect()
    };
    Some(InjectionRequest {
        language,
        range: content.start_byte()..content.end_byte(),
        child_ranges,
        start_point: content.start_position(),
        end_point: content.end_position(),
        include_children: properties.include_children,
        priority: properties.priority,
    })
}
