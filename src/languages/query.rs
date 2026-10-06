mod events;
pub(super) mod handler;
mod injections;
pub(crate) mod ranges;
mod roles;
mod tokens;

use std::collections::HashMap;

use tree_sitter::{Language, Query, QueryCapture, QueryCursor, StreamingIterator, Tree};

use crate::Result;
use crate::metrics::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope, halstead::HalsteadToken};

use super::InjectionRequest;

/// A compiled analysis query with the common capture conventions indexed once.
pub(crate) struct QueryAnalyzer {
    query: Query,
    roles: HashMap<u32, SyntaxRole>,
    syntax_node: Option<u32>,
    function: Option<u32>,
    function_name: Option<u32>,
    injection_content: Option<u32>,
    injection_language: Option<u32>,
    injection_host: Option<u32>,
    injection_patterns: HashMap<usize, InjectionProperties>,
}

#[derive(Clone, Default)]
struct InjectionProperties {
    language: Option<String>,
    language_capture: Option<u32>,
    include_children: bool,
    priority: i32,
    inherit_scope: bool,
    registered_only: bool,
}

/// Common facts emitted by language queries before syntax-specific adapters run.
#[derive(Default)]
pub(crate) struct QueryFacts {
    pub events: Vec<SyntaxEvent>,
    pub parents: HashMap<usize, Option<usize>>,
    pub functions: Vec<FunctionScope>,
    pub tokens: Vec<HalsteadToken>,
    pub injections: Vec<InjectionRequest>,
}

impl QueryAnalyzer {
    /// Compile one query and index standard capture names and injection settings.
    pub(crate) fn new(language: &Language, source: &str) -> Result<Self> {
        let query = Query::new(language, source)?;
        let mut roles = HashMap::new();
        for (index, name) in query.capture_names().iter().enumerate() {
            if let Some(role) = roles::capture_role(name) {
                roles.insert(index as u32, role);
            }
        }
        let capture = |name| query.capture_index_for_name(name);
        let injection_content = capture("injection.content");
        let injection_language = capture("injection.language");
        let injection_host = capture("injection.host");
        let mut injection_patterns = HashMap::new();
        for index in 0..query.pattern_count() {
            let mut properties = InjectionProperties::default();
            let mut found = false;
            for property in query.property_settings(index) {
                match property.key.as_ref() {
                    "injection.language" => {
                        found = true;
                        if let Some(capture_id) = property.capture_id {
                            properties.language_capture = Some(capture_id as u32);
                        } else {
                            properties.language = property.value.as_deref().map(str::to_owned);
                        }
                    }
                    "injection.include-children" => {
                        found = true;
                        properties.include_children = property.value.as_deref() != Some("false");
                    }
                    "injection.priority" => {
                        found = true;
                        properties.priority = property
                            .value
                            .as_deref()
                            .and_then(|value| value.parse().ok())
                            .unwrap_or_default();
                    }
                    "injection.inherit-scope" => {
                        properties.inherit_scope = property.value.as_deref() == Some("true");
                    }
                    "injection.registered-only" => {
                        found = true;
                        properties.registered_only = property.value.as_deref() == Some("true");
                    }
                    _ => {}
                }
            }
            if found {
                injection_patterns.insert(index, properties);
            }
        }
        Ok(Self {
            syntax_node: capture("syntax.node"),
            function: capture("metric.function"),
            function_name: capture("metric.function.name"),
            injection_content,
            injection_language,
            injection_host,
            injection_patterns,
            roles,
            query,
        })
    }

    /// Find a capture ID for a language adapter without exposing the compiled query.
    pub(crate) fn capture_id(&self, name: &str) -> Option<u32> {
        self.query.capture_index_for_name(name)
    }

    /// Capture shared syntax facts and let one language adapter consume each match.
    pub(crate) fn capture_with<'tree>(
        &self,
        tree: &'tree Tree,
        source: &[u8],
        include_tokens: bool,
        mut on_match: impl FnMut(usize, &[QueryCapture<'tree>], &[u8]),
    ) -> QueryFacts {
        let mut facts = QueryFacts::default();
        let included = ranges::included(tree);
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.query, tree.root_node(), source);
        while let Some(query_match) = matches.next() {
            let captures = query_match.captures();
            facts.record_syntax(self, captures, source, &included);
            facts.record_injections(self, query_match.pattern_index, captures, source);
            on_match(query_match.pattern_index, captures, source);
        }
        facts.finish(tree, source, include_tokens, &included);
        facts
    }

    /// Capture common syntax facts when no language-specific adapter is needed.
    pub(crate) fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> QueryFacts {
        self.capture_with(tree, source, include_tokens, |_, _, _| {})
    }
}

impl QueryFacts {
    fn finish(
        &mut self,
        tree: &Tree,
        source: &[u8],
        include_tokens: bool,
        included: &[tree_sitter::Range],
    ) {
        self.deduplicate();
        injections::merge(&mut self.injections);
        if include_tokens {
            self.tokens = tokens::capture(tree.root_node(), source, included);
            let comments = self
                .events
                .iter()
                .filter(|event| event.role == SyntaxRole::Comment)
                .map(|event| event.start_byte..event.end_byte)
                .collect::<Vec<_>>();
            self.tokens.retain(|token| {
                !comments
                    .iter()
                    .any(|range| range.start <= token.start_byte && token.end_byte <= range.end)
            });
        }
    }
}
