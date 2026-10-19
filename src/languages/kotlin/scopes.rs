use std::{collections::HashMap, ops::Range};

use tree_sitter::Node;

use super::syntax::{self, ContextParameters};

use crate::metrics::cyclomatic::FunctionScope;

pub(super) struct ScopeIndex<'tree> {
    root: Node<'tree>,
    source: &'tree [u8],
    contexts: &'tree [ContextParameters],
    recovered_types: Vec<syntax::RecoveredType>,
    nodes: Vec<Node<'tree>>,
    executable: HashMap<usize, ScopeIdentity>,
    script: Option<ScriptScope>,
}

struct ScriptScope {
    range: Range<usize>,
    line: usize,
}

struct ScopeIdentity {
    kind: ScopeKind,
    range: Range<usize>,
}

enum ScopeKind {
    Declaration(&'static str),
    Named(&'static str),
    Anonymous,
    PrimaryConstructor,
    SecondaryConstructor,
    Property,
    Destructuring,
}

impl<'tree> ScopeIndex<'tree> {
    pub(super) fn new(
        root: Node<'tree>,
        source: &'tree [u8],
        contexts: &'tree [ContextParameters],
    ) -> Self {
        let nodes = super::all_nodes(root);
        let recovered_types = syntax::recovered_types(root, source);
        let executable = nodes
            .iter()
            .filter_map(|node| {
                let kind = executable_scope_kind(*node, &recovered_types)?;
                let range = syntax::context_start(*node, contexts)..node.end_byte();
                Some((node.id(), ScopeIdentity { kind, range }))
            })
            .collect();
        let mut script_nodes = (0..root.named_child_count())
            .filter_map(|index| root.named_child(index as u32))
            .filter(|child| is_top_level_executable(child.kind()))
            .filter(|child| !recovered_error(*child, &recovered_types));
        let script = script_nodes.next().map(|first| {
            let last = script_nodes.next_back().unwrap_or(first);
            ScriptScope {
                range: first.start_byte()..last.end_byte(),
                line: first.start_position().row + 1,
            }
        });
        Self {
            root,
            source,
            contexts,
            recovered_types,
            nodes,
            executable,
            script,
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

    pub(super) fn nodes(&self) -> &[Node<'tree>] {
        &self.nodes
    }

    pub(super) fn recovered_types(&self) -> &[syntax::RecoveredType] {
        &self.recovered_types
    }

    pub(super) fn metric_scope_range_for(&self, node: Node<'_>) -> Option<Range<usize>> {
        metric_scope_range_for(self, node)
    }
}

/// Return every syntactically executable Kotlin range. Bodyless declarations
/// stay out so expect/abstract/external APIs do not receive a synthetic
/// complexity baseline.
pub(in crate::languages::kotlin) fn capture(
    index: &ScopeIndex<'_>,
    query_functions: Vec<FunctionScope>,
) -> Vec<FunctionScope> {
    let source = index.source();
    let mut scopes = index
        .nodes()
        .iter()
        .copied()
        .filter_map(|node| {
            let identity = index.executable.get(&node.id())?;
            let name = scope_name(&identity.kind, node, index, &query_functions);
            Some(scope(
                name,
                identity.range.start,
                identity.range.end,
                source,
            ))
        })
        .collect::<Vec<_>>();
    scopes.extend(index.script.iter().map(|script| {
        scope(
            format!("script@{}", script.line),
            script.range.start,
            script.range.end,
            source,
        )
    }));

    scopes.sort_by_key(|item| (item.range.start, item.range.end, item.name.clone()));
    scopes.dedup_by(|left, right| left.range == right.range && left.name == right.name);
    scopes
}
/// Return the nearest captured metric scope containing `node`.
///
/// The returned identity is exactly the byte range used by `FunctionScope`.
/// Consumers attaching semantic calls must still exclude source that executes
/// at call sites (such as default arguments) from body-call ownership.
pub(super) fn metric_scope_range_for(
    index: &ScopeIndex<'_>,
    node: Node<'_>,
) -> Option<Range<usize>> {
    let root = index.root();
    if let Some(identity) =
        super::ancestors(node).find_map(|candidate| index.executable.get(&candidate.id()))
    {
        return Some(identity.range.clone());
    }

    let mut direct_child = node;
    while let Some(parent) = direct_child.parent() {
        if parent.id() == root.id() {
            break;
        }
        direct_child = parent;
    }
    if direct_child
        .parent()
        .is_none_or(|parent| parent.id() != root.id())
        || !is_top_level_executable(direct_child.kind())
    {
        return None;
    }

    index.script.as_ref().map(|script| script.range.clone())
}

pub(in crate::languages::kotlin) fn property_initializer_range(
    property: Node<'_>,
    contexts: &[ContextParameters],
) -> Option<Range<usize>> {
    (property.kind() == "property_declaration"
        && has_property_initializer(property)
        && bound_function_value(property).is_none())
    .then(|| syntax::context_start(property, contexts)..property.end_byte())
}

/// Classify executable nodes once, before query-dependent names are available.
fn executable_scope_kind(
    node: Node<'_>,
    recovered_types: &[syntax::RecoveredType],
) -> Option<ScopeKind> {
    match node.kind() {
        "function_declaration" => {
            has_named_child(node, "function_body").then_some(ScopeKind::Declaration("function"))
        }
        "getter" => has_named_child(node, "function_body").then_some(ScopeKind::Named("get")),
        "setter" => has_named_child(node, "function_body").then_some(ScopeKind::Named("set")),
        "lambda_literal" => Some(ScopeKind::Anonymous),
        "anonymous_function" => {
            has_named_child(node, "function_body").then_some(ScopeKind::Anonymous)
        }
        "primary_constructor" => (!has_bodyless_api_primary_constructor(node, recovered_types))
            .then_some(ScopeKind::PrimaryConstructor),
        "secondary_constructor" => {
            has_direct_child(node, "{").then_some(ScopeKind::SecondaryConstructor)
        }
        "property_declaration" => (has_property_initializer(node)
            && bound_function_value(node).is_none())
        .then_some(ScopeKind::Property),
        "destructuring_declaration" => {
            has_direct_child(node, "=").then_some(ScopeKind::Destructuring)
        }
        "anonymous_initializer" => Some(ScopeKind::Named("init")),
        "enum_entry" => {
            has_named_child(node, "value_arguments").then_some(ScopeKind::Declaration("enum_entry"))
        }
        _ => None,
    }
}

fn scope_name(
    kind: &ScopeKind,
    node: Node<'_>,
    index: &ScopeIndex<'_>,
    query_functions: &[FunctionScope],
) -> String {
    let source = index.source();
    match kind {
        ScopeKind::Declaration(fallback) => declaration_name(node, source, fallback),
        ScopeKind::Named(name) => (*name).to_owned(),
        ScopeKind::Anonymous => bound_lambda_name(node, source)
            .or_else(|| query_name(query_functions, node))
            .unwrap_or_else(|| format!("anonymous@{}", node.start_position().row + 1)),
        ScopeKind::PrimaryConstructor => {
            primary_constructor_name(node, source, index.recovered_types())
        }
        ScopeKind::SecondaryConstructor => format!(
            "{}.constructor",
            enclosing_type_name(node, source, index.recovered_types())
        ),
        ScopeKind::Property => property_name(node, source),
        ScopeKind::Destructuring => format!("destructuring@{}", node.start_position().row + 1),
    }
}
pub(super) fn bound_lambda_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut parent = node.parent()?;
    if parent.kind() == "annotated_lambda" {
        parent = parent.parent()?;
    }
    (parent.kind() == "property_declaration"
        && bound_function_value(parent).is_some_and(|value| value.id() == node.id()))
    .then(|| property_name(parent, source))
}
pub(super) fn declaration_name(node: Node<'_>, source: &[u8], fallback: &str) -> String {
    let candidate = match node.kind() {
        "enum_entry" => direct_named_child(node, "simple_identifier"),
        "function_declaration" => direct_named_child(node, "simple_identifier"),
        _ => node.child_by_field_name("name"),
    };
    candidate
        .map(|name| normalized_identifier(name, source))
        .unwrap_or_else(|| format!("{fallback}@{}", node.start_position().row + 1))
}
pub(super) fn property_name(node: Node<'_>, source: &[u8]) -> String {
    direct_named_child(node, "variable_declaration")
        .and_then(|declaration| direct_named_child(declaration, "simple_identifier"))
        .map(|name| normalized_identifier(name, source))
        .unwrap_or_else(|| format!("property@{}", node.start_position().row + 1))
}
pub(super) fn primary_constructor_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> String {
    node.parent()
        .filter(|parent| parent.kind() == "class_declaration")
        .and_then(|class| direct_named_child(class, "type_identifier"))
        .map(|name| normalized_identifier(name, source))
        .or_else(|| {
            syntax::recovered_type_for(node, recovered_types)
                .and_then(|recovered| recovered.name_range.as_ref())
                .map(|range| normalized_source_identifier(range, source))
        })
        .unwrap_or_else(|| format!("constructor@{}", node.start_position().row + 1))
}
pub(super) fn has_bodyless_api_primary_constructor(
    node: Node<'_>,
    recovered_types: &[syntax::RecoveredType],
) -> bool {
    if node
        .parent()
        .filter(|parent| matches!(parent.kind(), "class_declaration" | "object_declaration"))
        .and_then(|declaration| direct_named_child(declaration, "modifiers"))
        .is_some_and(|modifiers| {
            has_descendant_kind(modifiers, "expect") || has_descendant_kind(modifiers, "external")
        })
    {
        return true;
    }
    let Some(recovered) = syntax::recovered_type_for(node, recovered_types) else {
        return false;
    };
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .find(|ancestor| {
            ancestor.kind() == "ERROR" && ancestor.byte_range() == recovered.error_range
        })
        .and_then(|error| {
            (0..error.child_count())
                .filter_map(|index| error.child(index))
                .find(|child| {
                    child.kind() == "modifiers" && child.end_byte() <= recovered.keyword_range.start
                })
        })
        .is_some_and(|modifiers| {
            has_descendant_kind(modifiers, "expect") || has_descendant_kind(modifiers, "external")
        })
}
pub(super) fn has_descendant_kind(node: Node<'_>, kind: &str) -> bool {
    super::all_nodes(node)
        .iter()
        .any(|candidate| candidate.kind() == kind)
}

pub(super) fn enclosing_type_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> String {
    if let Some(parent) = super::ancestors(node)
        .skip(1)
        .find(|parent| matches!(parent.kind(), "class_declaration" | "object_declaration"))
    {
        return direct_named_child(parent, "type_identifier")
            .map(|name| normalized_identifier(name, source))
            .unwrap_or_else(|| "anonymous".to_owned());
    }
    syntax::recovered_type_for(node, recovered_types)
        .and_then(|recovered| recovered.name_range.as_ref())
        .map(|range| normalized_source_identifier(range, source))
        .unwrap_or_else(|| "constructor".to_owned())
}
pub(super) fn recovered_error(node: Node<'_>, recovered_types: &[syntax::RecoveredType]) -> bool {
    recovered_types
        .iter()
        .any(|recovered| recovered.error_range == node.byte_range())
}

pub(super) fn query_name(functions: &[FunctionScope], node: Node<'_>) -> Option<String> {
    functions
        .iter()
        .find(|function| {
            function.range.start == node.start_byte() && function.range.end == node.end_byte()
        })
        .map(|function| function.name.clone())
}
pub(super) fn scope(name: String, start: usize, end: usize, source: &[u8]) -> FunctionScope {
    FunctionScope {
        name,
        range: start..end,
        line: source[..start.min(source.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1,
    }
}
pub(super) fn has_named_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.named_child_count()).any(|index| {
        node.named_child(index as u32)
            .is_some_and(|child| child.kind() == kind)
    })
}
pub(super) fn has_direct_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count()).any(|index| node.child(index).is_some_and(|child| child.kind() == kind))
}
pub(super) fn has_property_initializer(node: Node<'_>) -> bool {
    has_named_child(node, "property_delegate") || has_direct_child(node, "=")
}
pub(super) fn bound_function_value(node: Node<'_>) -> Option<Node<'_>> {
    direct_named_child(node, "lambda_literal")
        .or_else(|| direct_named_child(node, "anonymous_function"))
        .or_else(|| {
            let annotated = direct_named_child(node, "annotated_lambda")?;
            direct_named_child(annotated, "lambda_literal")
        })
}
pub(super) fn normalized_source_identifier(range: &Range<usize>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[range.clone()])
        .trim_matches('`')
        .to_owned()
}
pub(super) fn direct_named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .find(|child| child.kind() == kind)
}
pub(super) fn normalized_identifier(node: Node<'_>, source: &[u8]) -> String {
    let raw = String::from_utf8_lossy(&source[node.byte_range()]);
    raw.strip_prefix('`')
        .and_then(|inner| inner.strip_suffix('`'))
        .unwrap_or(&raw)
        .to_owned()
}
pub(super) fn is_top_level_executable(kind: &str) -> bool {
    !matches!(
        kind,
        "shebang_line"
            | "file_annotation"
            | "package_header"
            | "import_list"
            | "import_header"
            | "line_comment"
            | "multiline_comment"
            | "class_declaration"
            | "object_declaration"
            | "function_declaration"
            | "property_declaration"
            | "destructuring_declaration"
            | "getter"
            | "setter"
            | "type_alias"
            | "enum_entry"
    )
}

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_scopes.rs"]
mod tests;
