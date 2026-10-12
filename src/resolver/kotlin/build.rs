//! Conservative, compiler-free extraction of Gradle project relationships.
//!
//! Kotlin DSL scripts are parsed with the repository's tree-sitter grammar and
//! indexed through `SyntaxGraph`. A small lexer handles the corresponding
//! Groovy DSL forms, since this repository does not carry a Groovy grammar.
//! Both paths only accept literal project paths and source-set names.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use crate::graph::SyntaxGraph;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum DependencyScope {
    /// `implementation`, `api`, `compileOnly`, `runtimeOnly`, or a `main*` form.
    Production,
    /// A test source-set configuration such as `testImplementation`,
    /// `commonTestImplementation`, or `androidTestImplementation`.
    Test(String),
    /// A named variant/source set such as `debugImplementation` or
    /// `commonMainApi`.
    Variant(String),
    /// A statically named but unfamiliar configuration. The configuration is
    /// retained so callers can keep it uncertain instead of treating it as a
    /// production edge.
    Other,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ProjectDependency {
    pub path: String,
    pub configuration: String,
    pub scope: DependencyScope,
    pub exposure: DependencyExposure,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum DependencyExposure {
    /// The dependency is available to this module's compile classpath and is
    /// re-exported to consumers of this module.
    Api,
    /// Available to this module's compile classpath, but not re-exported.
    Implementation,
    /// Available to this module's compile classpath only.
    CompileOnly,
    /// Runtime classpath only; it does not provide compile-time visibility.
    Runtime,
    /// Unknown configuration; retain it without granting visibility.
    Other,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
    line: usize,
}

#[derive(Clone, Debug)]
enum TokenKind {
    Identifier(String),
    String(Option<String>),
    Symbol(char),
}

#[derive(Default)]
struct KotlinSyntax {
    valid: bool,
    identifiers: HashSet<(usize, usize)>,
    strings: HashSet<(usize, usize)>,
    control_ranges: Vec<(usize, usize)>,
}

impl KotlinSyntax {
    fn parse(source: &str) -> Self {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
        if parser.set_language(&language).is_err() {
            return Self::default();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Self::default();
        };
        let root = tree.root_node();
        let graph = SyntaxGraph::from_tree(root, source.as_bytes());
        let mut result = Self {
            valid: !root.has_error(),
            ..Self::default()
        };
        let mut pending = vec![root.id()];
        while let Some(id) = pending.pop() {
            let Some(node) = graph.node(id) else {
                continue;
            };
            let range = (node.start_byte(), node.end_byte());
            match graph.kind(id) {
                Some("simple_identifier") => {
                    result.identifiers.insert(range);
                }
                Some("string_literal") => {
                    result.strings.insert(range);
                }
                Some(
                    "if_expression" | "when_expression" | "for_statement" | "while_statement"
                    | "do_while_statement" | "try_expression",
                ) => result.control_ranges.push(range),
                _ => {}
            }
            pending.extend(graph.children(id).iter().copied());
        }
        result
    }

    fn permits_identifier(&self, token: &Token) -> bool {
        !self.valid || self.identifiers.contains(&(token.start, token.end))
    }

    fn permits_string(&self, token: &Token) -> bool {
        !self.valid || self.strings.contains(&(token.start, token.end))
    }

    fn outside_control_flow(&self, token: &Token) -> bool {
        !self
            .control_ranges
            .iter()
            .any(|(start, end)| *start <= token.start && token.end <= *end)
    }
}

/// Extract statically named included modules from a settings script.
///
/// Calls are accepted only as top-level statements. In particular, literals
/// inside comments, other strings, or nested blocks do not declare modules.
pub(super) fn parse_includes(source: &str) -> BTreeSet<String> {
    let tokens = lex(source);
    let syntax = KotlinSyntax::parse(source);
    let mut result = BTreeSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if !ident(&tokens[index], "include")
            || !syntax.permits_identifier(&tokens[index])
            || !syntax.outside_control_flow(&tokens[index])
            || !block_ancestry(&tokens, index, &syntax).is_empty()
            || !statement_start(&tokens, index)
        {
            index += 1;
            continue;
        }

        if symbol_at(&tokens, index + 1, '(')
            && let Some(close) = matching(&tokens, index + 1, '(', ')')
        {
            for (start, end) in split_arguments(&tokens, index + 2, close) {
                if let Some(path) = literal_path(&tokens, start, end, &syntax) {
                    result.insert(path);
                }
            }
            index = close + 1;
            continue;
        }

        // Groovy also commonly writes `include ':app', ':lib'`.
        let end = statement_end(&tokens, index + 1);
        for (start, end) in split_arguments(&tokens, index + 1, end) {
            if let Some(path) = literal_path(&tokens, start, end, &syntax) {
                result.insert(path);
            }
        }
        index += 1;
    }
    result
}

/// Extract direct project dependencies and retain their configuration
/// context. An unfamiliar configuration stays `Other` so downstream code
/// cannot accidentally grant it production visibility.
pub(super) fn parse_project_dependency_details(source: &str) -> BTreeSet<ProjectDependency> {
    let tokens = lex(source);
    let syntax = KotlinSyntax::parse(source);
    let mut result = BTreeSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if !ident(&tokens[index], "dependencies")
            || !syntax.permits_identifier(&tokens[index])
            || !syntax.outside_control_flow(&tokens[index])
        {
            index += 1;
            continue;
        }
        let Some(context_scope) = dependency_scope_context(&tokens, index, &syntax) else {
            index += 1;
            continue;
        };
        let dotted = symbol_at(&tokens, index.saturating_sub(1), '.');
        if (!dotted && !statement_start(&tokens, index))
            || (dotted && context_scope == DependencyScope::Production)
        {
            index += 1;
            continue;
        }
        let mut open = index + 1;
        if symbol_at(&tokens, open, '(') {
            let Some(close) = matching(&tokens, open, '(', ')') else {
                index += 1;
                continue;
            };
            open = close + 1;
        }
        if !symbol_at(&tokens, open, '{') {
            index += 1;
            continue;
        }
        let Some(close) = matching(&tokens, open, '{', '}') else {
            index += 1;
            continue;
        };
        collect_dependency_block(&tokens, &syntax, open, close, &context_scope, &mut result);
        index = close + 1;
    }
    result
}

/// Map statically configured Gradle project directories from a settings
/// script. Dynamic expressions and absolute paths are not resolved here.
pub(super) fn parse_project_directories(source: &str) -> BTreeMap<String, PathBuf> {
    let tokens = lex(source);
    let syntax = KotlinSyntax::parse(source);
    let mut result = BTreeMap::new();
    let mut index = 0;
    while index < tokens.len() {
        if !ident(&tokens[index], "project")
            || !syntax.permits_identifier(&tokens[index])
            || !syntax.outside_control_flow(&tokens[index])
            || !block_ancestry(&tokens, index, &syntax).is_empty()
            || !statement_start(&tokens, index)
            || (index > 0 && symbol_at(&tokens, index - 1, '.'))
            || !symbol_at(&tokens, index + 1, '(')
        {
            index += 1;
            continue;
        }
        let Some(project_close) = matching(&tokens, index + 1, '(', ')') else {
            index += 1;
            continue;
        };
        let Some(path) = literal_path(&tokens, index + 2, project_close, &syntax) else {
            index = project_close + 1;
            continue;
        };
        let assignment = project_close + 1;
        if !(symbol_at(&tokens, assignment, '.')
            && ident_at(&tokens, assignment + 1, "projectDir")
            && symbol_at(&tokens, assignment + 2, '='))
        {
            index = project_close + 1;
            continue;
        }
        let file_call = assignment + 3;
        if !ident_at(&tokens, file_call, "file") || !symbol_at(&tokens, file_call + 1, '(') {
            index = project_close + 1;
            continue;
        }
        if let Some(file_close) = matching(&tokens, file_call + 1, '(', ')') {
            if let Some(directory) = literal_string(&tokens, file_call + 2, file_close, &syntax) {
                let relative = PathBuf::from(directory);
                if !relative.is_absolute() {
                    result.insert(path, relative);
                }
            }
            index = file_close + 1;
        } else {
            index = project_close + 1;
        }
    }
    result
}

/// Collect explicit source-set `dependsOn` edges from a Gradle build script.
///
/// Only direct declarations in a `sourceSets` block and the lambda attached
/// to a recognized source-set declaration are considered. This keeps a
/// nested, unrelated `dependsOn` call from being attributed to another set.
pub(super) fn parse_source_edges(source: &str, edges: &mut BTreeMap<String, BTreeSet<String>>) {
    let tokens = lex(source);
    let syntax = KotlinSyntax::parse(source);
    let mut index = 0;
    while index < tokens.len() {
        if !ident(&tokens[index], "sourceSets")
            || !syntax.permits_identifier(&tokens[index])
            || !syntax.outside_control_flow(&tokens[index])
            || !statement_start(&tokens, index)
            || (index > 0 && symbol_at(&tokens, index - 1, '.'))
            || !matches!(
                block_ancestry(&tokens, index, &syntax).as_slice(),
                [GradleBlock::Kotlin] | [GradleBlock::Android]
            )
        {
            index += 1;
            continue;
        }
        let mut open = index + 1;
        if symbol_at(&tokens, open, '(') {
            let Some(close) = matching(&tokens, open, '(', ')') else {
                index += 1;
                continue;
            };
            open = close + 1;
        }
        if !symbol_at(&tokens, open, '{') {
            index += 1;
            continue;
        }
        let Some(close) = matching(&tokens, open, '{', '}') else {
            index += 1;
            continue;
        };
        collect_source_set_block(&tokens, &syntax, open, close, edges);
        index = close + 1;
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GradleBlock {
    Kotlin,
    Android,
    SourceSets,
    BuildTypes,
    ProductFlavors,
    SourceSet(String),
    Variant(String),
    Other,
}

/// Return a source-set scope only when the dependency block is either a
/// top-level project block or nested through a small set of literal Gradle
/// DSL containers. Arbitrary lambdas (including conditionals) remain unknown.
fn dependency_scope_context(
    tokens: &[Token],
    index: usize,
    syntax: &KotlinSyntax,
) -> Option<DependencyScope> {
    let ancestry = block_ancestry(tokens, index, syntax);
    match ancestry.as_slice() {
        [] => Some(DependencyScope::Production),
        [
            GradleBlock::Kotlin,
            GradleBlock::SourceSets,
            GradleBlock::SourceSet(name),
        ] => Some(DependencyScope::Variant(name.clone())),
        [
            GradleBlock::Android,
            GradleBlock::SourceSets,
            GradleBlock::SourceSet(name),
        ] => Some(DependencyScope::Variant(name.clone())),
        [
            GradleBlock::Android,
            GradleBlock::BuildTypes,
            GradleBlock::Variant(name),
        ]
        | [
            GradleBlock::Android,
            GradleBlock::ProductFlavors,
            GradleBlock::Variant(name),
        ] => Some(DependencyScope::Variant(name.clone())),
        [GradleBlock::Kotlin, GradleBlock::SourceSets]
            if symbol_at(tokens, index.saturating_sub(1), '.') =>
        {
            let name = token_identifier(tokens, index.saturating_sub(2))?;
            syntax
                .permits_identifier(&tokens[index.saturating_sub(2)])
                .then(|| DependencyScope::Variant(name.to_owned()))
        }
        _ => None,
    }
}

fn block_ancestry(tokens: &[Token], end: usize, syntax: &KotlinSyntax) -> Vec<GradleBlock> {
    let mut stack = Vec::new();
    for (index, token) in tokens.iter().enumerate().take(end) {
        match token.kind {
            TokenKind::Symbol('}') => {
                stack.pop();
            }
            TokenKind::Symbol('{') => {
                stack.push(classify_gradle_block(tokens, index, syntax, &stack));
            }
            _ => {}
        }
    }
    stack
}

fn classify_gradle_block(
    tokens: &[Token],
    open: usize,
    syntax: &KotlinSyntax,
    ancestry: &[GradleBlock],
) -> GradleBlock {
    let previous = open.saturating_sub(1);
    if token_identifier(tokens, previous).is_some_and(|name| name == "kotlin")
        || call_name_before_brace(tokens, open).is_some_and(|name| name == "kotlin")
    {
        return GradleBlock::Kotlin;
    }
    if token_identifier(tokens, previous).is_some_and(|name| name == "android") {
        return GradleBlock::Android;
    }
    if token_identifier(tokens, previous).is_some_and(|name| name == "sourceSets") {
        return GradleBlock::SourceSets;
    }
    if token_identifier(tokens, previous).is_some_and(|name| name == "buildTypes") {
        return GradleBlock::BuildTypes;
    }
    if token_identifier(tokens, previous).is_some_and(|name| name == "productFlavors") {
        return GradleBlock::ProductFlavors;
    }

    match ancestry.last() {
        Some(GradleBlock::SourceSets) => source_set_block_name(tokens, open, syntax)
            .map(GradleBlock::SourceSet)
            .unwrap_or(GradleBlock::Other),
        Some(GradleBlock::BuildTypes | GradleBlock::ProductFlavors) => {
            static_named_block(tokens, open, syntax)
                .map(GradleBlock::Variant)
                .unwrap_or(GradleBlock::Other)
        }
        _ => GradleBlock::Other,
    }
}

fn source_set_block_name(tokens: &[Token], open: usize, syntax: &KotlinSyntax) -> Option<String> {
    let name = if matches!(
        token_identifier(tokens, open.checked_sub(1)?),
        Some("getting" | "creating")
    ) {
        // Kotlin DSL: `val commonMain by getting { ... }` or `creating`.
        let name_index = open.checked_sub(3)?;
        if !ident_at(tokens, open.checked_sub(4)?, "val")
            || !ident_at(tokens, open.checked_sub(2)?, "by")
            || !syntax.permits_identifier(&tokens[name_index])
        {
            return None;
        }
        token_identifier(tokens, name_index).map(str::to_owned)
    } else {
        static_named_block(tokens, open, syntax)
    };
    name.filter(|name| valid_source_set_name(name))
}

fn static_named_block(tokens: &[Token], open: usize, syntax: &KotlinSyntax) -> Option<String> {
    let close = open.checked_sub(1)?;
    if !symbol_at(tokens, close, ')') {
        // Groovy/Kotlin DSL: `commonMain { ... }`.
        let name = token_identifier(tokens, close)?;
        if matches!(name, "getting" | "creating") {
            return None;
        }
        return (syntax.permits_identifier(&tokens[close]) && valid_source_set_name(name))
            .then(|| name.to_owned());
    }
    let paren = matching_open(tokens, close, '(', ')')?;
    let callee = token_identifier(tokens, paren.checked_sub(1)?)?;
    if !matches!(callee, "getByName" | "named" | "create" | "maybeCreate") {
        return None;
    }
    let name = literal_string(tokens, paren + 1, close, syntax)?;
    valid_source_set_name(&name).then_some(name)
}

fn call_name_before_brace(tokens: &[Token], open: usize) -> Option<&str> {
    let close = open.checked_sub(1)?;
    if !symbol_at(tokens, close, ')') {
        return None;
    }
    let paren = matching_open(tokens, close, '(', ')')?;
    token_identifier(tokens, paren.checked_sub(1)?)
}

fn matching_open(tokens: &[Token], close: usize, left: char, right: char) -> Option<usize> {
    if !symbol_at(tokens, close, right) {
        return None;
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(close + 1).rev() {
        match token.kind {
            TokenKind::Symbol(character) if character == right => depth += 1,
            TokenKind::Symbol(character) if character == left => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_dependency_block(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    open: usize,
    close: usize,
    context_scope: &DependencyScope,
    result: &mut BTreeSet<ProjectDependency>,
) {
    let depths = brace_depths(tokens);
    let direct_depth = depths.get(open).copied().unwrap_or_default() + 1;
    let mut index = open + 1;
    while index < close {
        if depths.get(index).copied() != Some(direct_depth)
            || !statement_start(tokens, index)
            || !is_identifier(&tokens[index])
            || !syntax.outside_control_flow(&tokens[index])
        {
            index += 1;
            continue;
        }
        let TokenKind::Identifier(configuration) = &tokens[index].kind else {
            unreachable!();
        };
        if !syntax.permits_identifier(&tokens[index]) {
            index += 1;
            continue;
        }
        let (scope, exposure) = classify_configuration(configuration);
        let scope = if matches!(context_scope, DependencyScope::Production) {
            scope
        } else {
            context_scope.clone()
        };
        let after = index + 1;
        let mut next = after;
        let paths = if symbol_at(tokens, after, '(') {
            if let Some(end) = matching(tokens, after, '(', ')') {
                next = end + 1;
                split_arguments(tokens, after + 1, end)
                    .into_iter()
                    .filter_map(|(start, end)| {
                        project_dependency_argument(tokens, syntax, start, end)
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        } else {
            // Groovy's `implementation project(':lib')` form.
            let end = statement_end(tokens, after);
            next = end;
            project_dependency_argument(tokens, syntax, after, end)
                .into_iter()
                .collect()
        };
        for path in paths {
            result.insert(ProjectDependency {
                path,
                configuration: configuration.clone(),
                scope: scope.clone(),
                exposure: exposure.clone(),
            });
        }
        index = next.max(index + 1);
    }
}

fn project_dependency_argument(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    start: usize,
    end: usize,
) -> Option<String> {
    if !ident_at(tokens, start, "project") || !symbol_at(tokens, start + 1, '(') {
        return None;
    }
    let close = matching(tokens, start + 1, '(', ')')?;
    if close + 1 != end {
        return None;
    }
    literal_path(tokens, start + 2, close, syntax)
}

fn classify_configuration(configuration: &str) -> (DependencyScope, DependencyExposure) {
    const SUFFIXES: [(&str, DependencyExposure); 6] = [
        ("implementation", DependencyExposure::Implementation),
        ("api", DependencyExposure::Api),
        ("compileonly", DependencyExposure::CompileOnly),
        ("runtimeonly", DependencyExposure::Runtime),
        ("compile", DependencyExposure::Implementation),
        ("runtime", DependencyExposure::Runtime),
    ];
    let normalized = configuration.to_ascii_lowercase();
    let Some((suffix, exposure)) = SUFFIXES
        .iter()
        .find(|(suffix, _)| normalized.ends_with(*suffix))
    else {
        return (DependencyScope::Other, DependencyExposure::Other);
    };
    let prefix = configuration
        .get(..configuration.len().saturating_sub(suffix.len()))
        .unwrap_or_default();
    let scope = if prefix.is_empty() || prefix.eq_ignore_ascii_case("main") {
        DependencyScope::Production
    } else if prefix == "test" || prefix.ends_with("Test") {
        DependencyScope::Test(prefix.to_owned())
    } else {
        DependencyScope::Variant(prefix.to_owned())
    };
    (scope, exposure.clone())
}

fn collect_source_set_block(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    open: usize,
    close: usize,
    edges: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let mut index = open + 1;
    while index < close {
        if symbol_at(tokens, index, '{') {
            index = matching(tokens, index, '{', '}').map_or(index + 1, |end| end + 1);
            continue;
        }
        if !statement_start(tokens, index) {
            index += 1;
            continue;
        }

        if ident_at(tokens, index, "val")
            && syntax.outside_control_flow(&tokens[index])
            && let Some(name) = declared_source_set(tokens, syntax, index, close)
        {
            if let Some((body_open, body_close)) = name.body {
                collect_depends_calls(tokens, syntax, &name.name, body_open, body_close, edges);
                index = body_close + 1;
                continue;
            }
            index = name.next;
            continue;
        }

        if let Some((name, body_open, body_close)) = named_source_set(tokens, syntax, index, close)
            && syntax.outside_control_flow(&tokens[index])
        {
            collect_depends_calls(tokens, syntax, &name, body_open, body_close, edges);
            index = body_close + 1;
            continue;
        }

        if let Some((child, _body_open, body_close)) = dotted_depends(tokens, syntax, index, close)
            && syntax.outside_control_flow(&tokens[index])
        {
            if let Some(parent) = first_source_set_argument(tokens, syntax, index + 4, body_close) {
                edges.entry(child).or_default().insert(parent);
            }
            index = body_close + 1;
            continue;
        }
        index += 1;
    }
}

struct DeclaredSourceSet {
    name: String,
    body: Option<(usize, usize)>,
    next: usize,
}

fn declared_source_set(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    index: usize,
    block_close: usize,
) -> Option<DeclaredSourceSet> {
    let name = token_identifier(tokens, index + 1)?.to_owned();
    if !syntax.permits_identifier(&tokens[index + 1]) || !ident_at(tokens, index + 2, "by") {
        return None;
    }
    let mut delegate = index + 3;
    while delegate < block_close {
        if symbol_at(tokens, delegate, '{') {
            let body_close = matching(tokens, delegate, '{', '}')?;
            return Some(DeclaredSourceSet {
                name,
                body: Some((delegate, body_close)),
                next: body_close + 1,
            });
        }
        if symbol_at(tokens, delegate, ';') || tokens[delegate].line > tokens[index].line {
            break;
        }
        if symbol_at(tokens, delegate, '(') {
            delegate = matching(tokens, delegate, '(', ')')? + 1;
        } else {
            delegate += 1;
        }
    }
    Some(DeclaredSourceSet {
        name,
        body: None,
        next: delegate,
    })
}

fn named_source_set(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    index: usize,
    block_close: usize,
) -> Option<(String, usize, usize)> {
    if !(ident_at(tokens, index, "getByName") || ident_at(tokens, index, "named"))
        || !syntax.permits_identifier(&tokens[index])
        || !symbol_at(tokens, index + 1, '(')
    {
        return None;
    }
    let args_close = matching(tokens, index + 1, '(', ')')?;
    let name = literal_string(tokens, index + 2, args_close, syntax)?;
    if !valid_source_set_name(&name) {
        return None;
    }
    let body_open = args_close + 1;
    if !symbol_at(tokens, body_open, '{') {
        return None;
    }
    let body_close = matching(tokens, body_open, '{', '}')?;
    (body_close < block_close).then_some((name, body_open, body_close))
}

fn dotted_depends(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    index: usize,
    block_close: usize,
) -> Option<(String, usize, usize)> {
    let child = token_identifier(tokens, index)?.to_owned();
    if !syntax.permits_identifier(&tokens[index])
        || !symbol_at(tokens, index + 1, '.')
        || !ident_at(tokens, index + 2, "dependsOn")
        || !syntax.permits_identifier(&tokens[index + 2])
        || !symbol_at(tokens, index + 3, '(')
    {
        return None;
    }
    let end = matching(tokens, index + 3, '(', ')')?;
    (end < block_close).then_some((child, index + 3, end))
}

fn collect_depends_calls(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    child: &str,
    body_open: usize,
    body_close: usize,
    edges: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let mut index = body_open + 1;
    while index < body_close {
        if symbol_at(tokens, index, '{') {
            index = matching(tokens, index, '{', '}').map_or(index + 1, |end| end + 1);
            continue;
        }
        if ident_at(tokens, index, "dependsOn")
            && syntax.permits_identifier(&tokens[index])
            && syntax.outside_control_flow(&tokens[index])
            && symbol_at(tokens, index + 1, '(')
            && let Some(end) = matching(tokens, index + 1, '(', ')')
        {
            if let Some(parent) = first_source_set_argument(tokens, syntax, index + 2, end) {
                edges.entry(child.to_owned()).or_default().insert(parent);
            }
            index = end + 1;
            continue;
        }
        index += 1;
    }
}

fn first_source_set_argument(
    tokens: &[Token],
    syntax: &KotlinSyntax,
    start: usize,
    end: usize,
) -> Option<String> {
    if let Some(name) = token_identifier(tokens, start)
        && syntax.permits_identifier(&tokens[start])
        && valid_source_set_name(name)
    {
        return Some(name.to_owned());
    }
    let name = literal_string(tokens, start, end, syntax)?;
    valid_source_set_name(&name).then_some(name)
}

fn valid_source_set_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn literal_path(
    tokens: &[Token],
    start: usize,
    end: usize,
    syntax: &KotlinSyntax,
) -> Option<String> {
    let path = literal_string(tokens, start, end, syntax)?;
    valid_gradle_path(&path).then_some(path)
}

fn literal_string(
    tokens: &[Token],
    start: usize,
    end: usize,
    syntax: &KotlinSyntax,
) -> Option<String> {
    if end != start + 1 || !syntax.permits_string(&tokens[start]) {
        return None;
    }
    match &tokens[start].kind {
        TokenKind::String(Some(value)) => Some(value.clone()),
        _ => None,
    }
}

fn valid_gradle_path(path: &str) -> bool {
    path.starts_with(':')
        && path.strip_prefix(':').is_some_and(|rest| {
            !rest.is_empty()
                && rest.split(':').all(|part| {
                    !part.is_empty()
                        && part.chars().all(|character| {
                            character.is_ascii_alphanumeric()
                                || matches!(character, '_' | '-' | '.')
                        })
                })
        })
}

fn statement_start(tokens: &[Token], index: usize) -> bool {
    index == 0
        || symbol_at(tokens, index - 1, '{')
        || symbol_at(tokens, index - 1, '}')
        || symbol_at(tokens, index - 1, ';')
        || tokens[index - 1].line < tokens[index].line
}

fn statement_end(tokens: &[Token], start: usize) -> usize {
    let line = tokens.get(start).map_or(usize::MAX, |token| token.line);
    let mut index = start;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    while index < tokens.len() {
        if tokens[index].line > line && parens == 0 && brackets == 0 {
            break;
        }
        match tokens[index].kind {
            TokenKind::Symbol('(') => parens += 1,
            TokenKind::Symbol(')') => parens = parens.saturating_sub(1),
            TokenKind::Symbol('[') => brackets += 1,
            TokenKind::Symbol(']') => brackets = brackets.saturating_sub(1),
            TokenKind::Symbol(';') | TokenKind::Symbol('}') if parens == 0 && brackets == 0 => {
                break;
            }
            _ => {}
        }
        index += 1;
    }
    index
}

fn brace_depths(tokens: &[Token]) -> Vec<usize> {
    let mut depth = 0usize;
    let mut result = Vec::with_capacity(tokens.len());
    for token in tokens {
        if matches!(token.kind, TokenKind::Symbol('}')) {
            depth = depth.saturating_sub(1);
        }
        result.push(depth);
        if matches!(token.kind, TokenKind::Symbol('{')) {
            depth += 1;
        }
    }
    result
}

fn split_arguments(tokens: &[Token], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut argument_start = start;
    let mut parens = 0usize;
    let mut braces = 0usize;
    let mut brackets = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(start) {
        match token.kind {
            TokenKind::Symbol('(') => parens += 1,
            TokenKind::Symbol(')') => parens = parens.saturating_sub(1),
            TokenKind::Symbol('{') => braces += 1,
            TokenKind::Symbol('}') => braces = braces.saturating_sub(1),
            TokenKind::Symbol('[') => brackets += 1,
            TokenKind::Symbol(']') => brackets = brackets.saturating_sub(1),
            TokenKind::Symbol(',') if parens == 0 && braces == 0 && brackets == 0 => {
                if argument_start < index {
                    result.push((argument_start, index));
                }
                argument_start = index + 1;
            }
            _ => {}
        }
    }
    if argument_start < end {
        result.push((argument_start, end));
    }
    result
}

fn matching(tokens: &[Token], open: usize, left: char, right: char) -> Option<usize> {
    if !symbol_at(tokens, open, left) {
        return None;
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            TokenKind::Symbol(character) if character == left => depth += 1,
            TokenKind::Symbol(character) if character == right => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn ident(token: &Token, expected: &str) -> bool {
    matches!(&token.kind, TokenKind::Identifier(name) if name == expected)
}

fn ident_at(tokens: &[Token], index: usize, expected: &str) -> bool {
    tokens
        .get(index)
        .is_some_and(|token| ident(token, expected))
}

fn token_identifier(tokens: &[Token], index: usize) -> Option<&str> {
    match &tokens.get(index)?.kind {
        TokenKind::Identifier(name) => Some(name),
        _ => None,
    }
}

fn is_identifier(token: &Token) -> bool {
    matches!(token.kind, TokenKind::Identifier(_))
}

fn symbol_at(tokens: &[Token], index: usize, symbol: char) -> bool {
    tokens
        .get(index)
        .is_some_and(|token| matches!(token.kind, TokenKind::Symbol(value) if value == symbol))
}

fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut index = 0usize;
    let mut line = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            if byte == b'\n' {
                line += 1;
            }
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            let mut nesting = 1usize;
            while index < bytes.len() && nesting > 0 {
                if bytes.get(index..index + 2) == Some(b"/*") {
                    nesting += 1;
                    index += 2;
                } else if bytes.get(index..index + 2) == Some(b"*/") {
                    nesting -= 1;
                    index += 2;
                } else {
                    if bytes[index] == b'\n' {
                        line += 1;
                    }
                    index += 1;
                }
            }
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            let start = index;
            let token_line = line;
            let quote = byte;
            let triple = bytes.get(index..index + 3) == Some(&[quote, quote, quote]);
            let delimiter = if triple { 3 } else { 1 };
            index += delimiter;
            let content_start = index;
            let mut escaped = false;
            let mut dynamic = false;
            while index < bytes.len() {
                if !escaped && triple && bytes.get(index..index + 3) == Some(&[quote, quote, quote])
                {
                    break;
                }
                if !escaped && !triple && bytes[index] == quote {
                    break;
                }
                if bytes[index] == b'\n' {
                    line += 1;
                }
                if bytes[index] == b'$' {
                    dynamic = true;
                }
                if bytes[index] == b'\\' {
                    escaped = !escaped;
                } else {
                    escaped = false;
                }
                index += 1;
            }
            let content_end = index;
            if index < bytes.len() {
                index += delimiter;
            }
            let raw = source.get(content_start..content_end).unwrap_or_default();
            let value = (!dynamic && !raw.contains('\\')).then(|| raw.to_owned());
            result.push(Token {
                kind: TokenKind::String(value),
                start,
                end: index,
                line: token_line,
            });
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || bytes[index] == b'_'
                    || bytes[index] == b'$')
            {
                index += 1;
            }
            result.push(Token {
                kind: TokenKind::Identifier(source[start..index].to_owned()),
                start,
                end: index,
                line,
            });
            continue;
        }
        if byte.is_ascii_punctuation() {
            result.push(Token {
                kind: TokenKind::Symbol(byte as char),
                start: index,
                end: index + 1,
                line,
            });
        }
        index += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        DependencyExposure, DependencyScope, parse_includes, parse_project_dependency_details,
        parse_project_directories, parse_source_edges,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn includes_ignore_comments_strings_dynamic_paths_and_nested_calls() {
        let source = r#"
            // include(":comment")
            val text = "include(\":string\") and https://example.test/a//b"
            include(":app", ":lib")
            if (providers.gradleProperty("extra").isPresent) {
                include(":conditional")
            }
            if (false)
                include(":unbraced-conditional")
            configureSomething {
                include(":foreign-block")
            }
            include(":$dynamic")
        "#;
        assert_eq!(
            parse_includes(source),
            BTreeSet::from([":app".into(), ":lib".into()])
        );
    }

    #[test]
    fn dependencies_keep_configuration_context_and_ignore_fake_project_calls() {
        let source = r#"
            val example = "implementation(project(\":fake\"))"
            /* dependencies { implementation(project(":comment")) } */
            dependencies {
                implementation(project(":lib"))
                testImplementation(project(":test-support"))
                androidTestImplementation(project(":android-test-support"))
                commonTestApi(project(":common-test-support"))
                contestImplementation(project(":contest-support"))
                debugApi(project(":debug-lib"))
            api(project(":public-api"))
            compileOnly(project(":compile-only"))
            runtimeOnly(project(":runtime-only"))
                implementation(project(path = ":named-arg"))
                implementation(project(projectPath))
                // implementation(project(":comment"))
                nested { implementation(project(":nested")) }
            }
        if (false) {
            dependencies { implementation(project(":conditional")) }
        }
        if (false)
            dependencies { implementation(project(":unbraced-conditional")) }
        dependencies {
            if (false)
                implementation(project(":unbraced-configuration"))
        }
        "#;
        let parsed = parse_project_dependency_details(source);
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":lib".into(),
            configuration: "implementation".into(),
            scope: DependencyScope::Production,
            exposure: DependencyExposure::Implementation,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":test-support".into(),
            configuration: "testImplementation".into(),
            scope: DependencyScope::Test("test".into()),
            exposure: DependencyExposure::Implementation,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":android-test-support".into(),
            configuration: "androidTestImplementation".into(),
            scope: DependencyScope::Test("androidTest".into()),
            exposure: DependencyExposure::Implementation,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":common-test-support".into(),
            configuration: "commonTestApi".into(),
            scope: DependencyScope::Test("commonTest".into()),
            exposure: DependencyExposure::Api,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":contest-support".into(),
            configuration: "contestImplementation".into(),
            scope: DependencyScope::Variant("contest".into()),
            exposure: DependencyExposure::Implementation,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":debug-lib".into(),
            configuration: "debugApi".into(),
            scope: DependencyScope::Variant("debug".into()),
            exposure: DependencyExposure::Api,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":public-api".into(),
            configuration: "api".into(),
            scope: DependencyScope::Production,
            exposure: DependencyExposure::Api,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":compile-only".into(),
            configuration: "compileOnly".into(),
            scope: DependencyScope::Production,
            exposure: DependencyExposure::CompileOnly,
        }));
        assert!(parsed.contains(&super::ProjectDependency {
            path: ":runtime-only".into(),
            configuration: "runtimeOnly".into(),
            scope: DependencyScope::Production,
            exposure: DependencyExposure::Runtime,
        }));
        assert!(!parsed.iter().any(|dependency| {
            dependency.path.contains("fake")
                || dependency.path.contains("comment")
                || dependency.path.contains("nested")
                || dependency.path.contains("conditional")
        }));
        assert_eq!(
            parsed
                .iter()
                .map(|dependency| dependency.path.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                ":lib".into(),
                ":test-support".into(),
                ":android-test-support".into(),
                ":common-test-support".into(),
                ":contest-support".into(),
                ":debug-lib".into(),
                ":public-api".into(),
                ":compile-only".into(),
                ":runtime-only".into(),
            ])
        );
    }

    #[test]
    fn source_set_dependency_blocks_keep_static_scope_and_reject_unknown_ancestors() {
        let source = r#"
            kotlin {
                sourceSets {
                    val commonMain by getting {
                        dependencies {
                            implementation(project(":common-implementation"))
                            api(project(":common-api"))
                        }
                    }
                    val androidMain by creating {
                        dependencies { implementation(project(":android-only")) }
                    }
                    commonMain.dependencies {
                        implementation(project(":common-dsl"))
                    }
                    val unrelatedMain by creating {
                        unrelatedConfiguration {
                            dependencies { implementation(project(":uncertain")) }
                        }
                        dependencies { implementation(project(":unrelated-set")) }
                    }
                }
            }
            if (false) {
                dependencies { implementation(project(":conditional")) }
            }
        "#;

        let parsed = parse_project_dependency_details(source);
        let scopes = parsed
            .iter()
            .map(|dependency| (dependency.path.as_str(), &dependency.scope))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            scopes[":common-implementation"],
            &DependencyScope::Variant("commonMain".into())
        );
        assert_eq!(
            scopes[":common-api"],
            &DependencyScope::Variant("commonMain".into())
        );
        assert_eq!(
            scopes[":common-dsl"],
            &DependencyScope::Variant("commonMain".into())
        );
        assert_eq!(
            scopes[":android-only"],
            &DependencyScope::Variant("androidMain".into())
        );
        assert_eq!(
            scopes[":unrelated-set"],
            &DependencyScope::Variant("unrelatedMain".into())
        );
        assert!(!scopes.contains_key(":uncertain"));
        assert!(!scopes.contains_key(":conditional"));
    }

    #[test]
    fn groovy_literals_and_comments_are_scanned_without_confusing_urls() {
        let settings = r#"
            // include ':comment'
            rootProject.name = 'example'
            include ':app', ':lib'
            def message = 'https://example.test/a//b include(\':fake\')'
        "#;
        assert_eq!(
            parse_includes(settings),
            BTreeSet::from([":app".into(), ":lib".into()])
        );
        let build = r#"
            dependencies {
                implementation project(':lib')
                testImplementation(project(':test-support'))
                // implementation project(':comment')
            }
        "#;
        assert_eq!(
            parse_project_dependency_details(build)
                .into_iter()
                .map(|dependency| (dependency.path, dependency.exposure))
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                (":lib".into(), DependencyExposure::Implementation),
                (":test-support".into(), DependencyExposure::Implementation),
            ])
        );
    }

    #[test]
    fn source_set_edges_are_scoped_to_their_declaration_blocks() {
        let source = r#"
            kotlin {
                sourceSets {
                    val sharedMain by creating
                    val androidMain by getting {
                        dependsOn(sharedMain)
                        configureSomething { dependsOn(fakeMain) }
                    }
                    getByName("iosMain") {
                        dependsOn("sharedMain")
                    }
                    named("desktopMain") { dependsOn(sharedMain) }
                    androidMain.dependsOn(sharedMain)
                    unrelated { dependsOn(fakeMain) }
                    val fakeMain by creating {
                        // dependsOn(sharedMain)
                        val text = "dependsOn(fakeMain)"
                    }
                    val guardedMain by creating {
                        if (false)
                            dependsOn(sharedMain)
                    }
                    if (false)
                        falseChild.dependsOn(sharedMain)
                }
            }
        "#;
        let mut edges = BTreeMap::new();
        parse_source_edges(source, &mut edges);
        assert_eq!(edges["androidMain"], BTreeSet::from(["sharedMain".into()]));
        assert_eq!(edges["iosMain"], BTreeSet::from(["sharedMain".into()]));
        assert_eq!(edges["desktopMain"], BTreeSet::from(["sharedMain".into()]));
        assert!(!edges.contains_key("fakeMain"));
        assert!(!edges.contains_key("guardedMain"));
        assert!(!edges.contains_key("falseChild"));
    }

    #[test]
    fn source_set_edges_reject_conditionals_and_unrelated_dsl_blocks() {
        let source = r#"
            if (false) {
                kotlin {
                    sourceSets {
                        val conditionalBase by creating
                        val conditionalChild by creating { dependsOn(conditionalBase) }
                    }
                }
            }
            configureSomething {
                sourceSets {
                    val foreignBase by creating
                    val foreignChild by creating { dependsOn(foreignBase) }
                }
            }
            kotlin {
                sourceSets {
                    val realBase by creating
                    val realChild by creating { dependsOn(realBase) }
                }
            }
        "#;
        let mut edges = BTreeMap::new();
        parse_source_edges(source, &mut edges);

        assert_eq!(edges["realChild"], BTreeSet::from(["realBase".into()]));
        assert!(!edges.contains_key("conditionalChild"));
        assert!(!edges.contains_key("foreignChild"));
    }

    #[test]
    fn project_directory_requires_a_static_relative_file_literal() {
        let settings = r#"
            include(":lib", ":other")
            project(":lib").projectDir = file("modules/library")
            project(":other").projectDir = file(modulePath)
            project(":dynamic").projectDir = file("$modulePath")
            if (false) {
                project(":conditional").projectDir = file("modules/conditional")
            }
            if (false)
                project(":unbraced-conditional").projectDir = file("modules/unbraced")
            configureSomething {
                project(":foreign").projectDir = file("modules/foreign")
            }
        "#;
        assert_eq!(
            parse_project_directories(settings),
            BTreeMap::from([(":lib".into(), "modules/library".into())])
        );
    }
}
