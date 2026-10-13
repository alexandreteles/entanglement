//! Compiler-free extraction of static Gradle project and source-set facts.
//! Kotlin source analysis does not require these optional build files.

mod lexer;
mod syntax;

use lexer::{
    ident, matching, split_arguments, statement_end, statement_start, symbol_at, token_identifier,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use syntax::{GradleBlock, StaticScript, dependency_scope, valid_source_set_name};

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_build.rs"]
mod tests;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::resolver::kotlin) enum DependencyScope {
    /// Standard production configurations such as `implementation` and `api`.
    Production,
    /// Test configurations retain the exact source-set prefix.
    Test(String),
    /// A named variant or source set such as `debug` or `commonMain`.
    Variant(String),
    /// A known but unsupported configuration that cannot provide visibility.
    Other,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::resolver::kotlin) struct ProjectDependency {
    pub path: String,
    pub configuration: String,
    pub scope: DependencyScope,
    pub exposure: DependencyExposure,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::resolver::kotlin) enum DependencyExposure {
    /// Compile-visible and re-exported to consumers.
    Api,
    /// Compile-visible within this module only.
    Implementation,
    /// Compile-only visibility.
    CompileOnly,
    /// Runtime-only and therefore not compile-visible.
    Runtime,
    /// Unsupported configuration.
    Other,
}

pub(super) struct Settings {
    pub modules: BTreeSet<String>,
    pub directories: BTreeMap<String, PathBuf>,
}

impl Settings {
    pub(super) fn parse(source: &str) -> Self {
        let script = StaticScript::parse(source);
        Self {
            modules: includes(&script),
            directories: project_directories(&script),
        }
    }
}

#[derive(Default)]
pub(super) struct BuildFacts {
    pub dependencies: BTreeSet<ProjectDependency>,
    pub source_edges: BTreeMap<String, BTreeSet<String>>,
}

impl BuildFacts {
    pub(super) fn parse(source: &str) -> Self {
        let script = StaticScript::parse(source);
        let mut edges = BTreeMap::new();
        source_edges(&script, &mut edges);
        Self {
            dependencies: project_dependencies(&script),
            source_edges: edges,
        }
    }
}

#[cfg(test)]
fn parse_includes(source: &str) -> BTreeSet<String> {
    Settings::parse(source).modules
}

#[cfg(test)]
fn parse_project_directories(source: &str) -> BTreeMap<String, PathBuf> {
    Settings::parse(source).directories
}

#[cfg(test)]
fn parse_project_dependency_details(source: &str) -> BTreeSet<ProjectDependency> {
    BuildFacts::parse(source).dependencies
}

#[cfg(test)]
fn parse_source_edges(source: &str, edges: &mut BTreeMap<String, BTreeSet<String>>) {
    for (child, parents) in BuildFacts::parse(source).source_edges {
        edges.entry(child).or_default().extend(parents);
    }
}

pub(super) fn classify_configuration(configuration: &str) -> (DependencyScope, DependencyExposure) {
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
    let prefix = &configuration[..configuration.len() - suffix.len()];
    let scope = if prefix.is_empty() || prefix.eq_ignore_ascii_case("main") {
        DependencyScope::Production
    } else if prefix == "test" || prefix.ends_with("Test") {
        DependencyScope::Test(prefix.to_owned())
    } else {
        DependencyScope::Variant(prefix.to_owned())
    };
    (scope, exposure.clone())
}

/// Extract only top-level, literal module inclusions from settings scripts.
fn includes(script: &StaticScript) -> BTreeSet<String> {
    let tokens = script.tokens();
    let mut modules = BTreeSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if script.is_root_statement(index, "include") {
            index = collect_include(script, index, &mut modules);
        } else {
            index += 1;
        }
    }
    modules
}

fn collect_include(script: &StaticScript, index: usize, modules: &mut BTreeSet<String>) -> usize {
    let tokens = script.tokens();
    let (start, end, next) = if symbol_at(tokens, index + 1, '(') {
        let Some(close) = matching(tokens, index + 1, '(', ')') else {
            return index + 1;
        };
        (index + 2, close, close + 1)
    } else {
        // Groovy also uses `include ':app', ':lib'`.
        let end = statement_end(tokens, index + 1);
        (index + 1, end, end.max(index + 1))
    };
    modules.extend(
        split_arguments(tokens, start, end)
            .into_iter()
            .filter_map(|(start, end)| script.literal_path(start, end)),
    );
    next
}

/// Resolve only direct settings-script mappings to relative file literals.
fn project_directories(script: &StaticScript) -> BTreeMap<String, PathBuf> {
    let tokens = script.tokens();
    (0..tokens.len())
        .filter(|index| {
            script.is_root_statement(*index, "project")
                && symbol_at(tokens, index + 1, '(')
                && !symbol_at(tokens, index.saturating_sub(1), '.')
        })
        .filter_map(|index| script.project_directory(index))
        .collect()
}

/// Extract literal project dependencies while retaining configuration and
/// source-set context for the resolver's visibility rules.
fn project_dependencies(script: &StaticScript) -> BTreeSet<ProjectDependency> {
    let tokens = script.tokens();
    let mut dependencies = BTreeSet::new();
    let mut index = 0;
    while index < tokens.len() {
        let Some(context) = dependency_block_at(script, index) else {
            index += 1;
            continue;
        };
        let next = context.close + 1;
        collect_dependency_block(script, context, &mut dependencies);
        index = next;
    }
    dependencies
}

struct DependencyBlock {
    open: usize,
    close: usize,
    scope: DependencyScope,
}

fn dependency_block_at(script: &StaticScript, index: usize) -> Option<DependencyBlock> {
    let tokens = script.tokens();
    if !script.is_static_identifier(index, "dependencies") {
        return None;
    }
    let scope = dependency_scope(tokens, script.syntax(), script.scopes(), index)?;
    let dotted = symbol_at(tokens, index.saturating_sub(1), '.');
    if !dotted && !lexer::statement_start(tokens, index) {
        return None;
    }
    if dotted && scope == DependencyScope::Production {
        return None;
    }

    let (open, close) = script.braced_call_at(index)?;
    Some(DependencyBlock { open, close, scope })
}

fn collect_dependency_block(
    script: &StaticScript,
    block: DependencyBlock,
    result: &mut BTreeSet<ProjectDependency>,
) {
    let direct_depth = script.brace_depth(block.open).unwrap_or_default() + 1;
    let mut index = block.open + 1;
    while index < block.close {
        let Some(configuration) = direct_configuration(script, index, direct_depth) else {
            index += 1;
            continue;
        };
        let (declared_scope, exposure) = classify_configuration(configuration);
        let scope = if block.scope == DependencyScope::Production {
            declared_scope
        } else {
            block.scope.clone()
        };
        let Some((paths, next)) = dependency_paths_after(script, index + 1) else {
            index += 1;
            continue;
        };
        for path in paths {
            result.insert(ProjectDependency {
                path,
                configuration: configuration.to_owned(),
                scope: scope.clone(),
                exposure: exposure.clone(),
            });
        }
        index = next.max(index + 1);
    }
}

fn direct_configuration(script: &StaticScript, index: usize, direct_depth: usize) -> Option<&str> {
    (script.brace_depth(index) == Some(direct_depth)
        && statement_start(script.tokens(), index)
        && script.is_static_token(index))
    .then_some(())?;
    token_identifier(script.tokens(), index)
}

fn dependency_paths_after(
    script: &StaticScript,
    argument_start: usize,
) -> Option<(Vec<String>, usize)> {
    let tokens = script.tokens();
    if symbol_at(tokens, argument_start, '(') {
        let close = matching(tokens, argument_start, '(', ')')?;
        let paths = split_arguments(tokens, argument_start + 1, close)
            .into_iter()
            .filter_map(|(start, end)| project_dependency_argument(script, start, end))
            .collect();
        return Some((paths, close + 1));
    }

    // Groovy's `implementation project(':lib')` form.
    let end = statement_end(tokens, argument_start);
    let path = project_dependency_argument(script, argument_start, end);
    Some((path.into_iter().collect(), end))
}

fn project_dependency_argument(script: &StaticScript, start: usize, end: usize) -> Option<String> {
    let tokens = script.tokens();
    if !ident(tokens.get(start)?, "project") || !symbol_at(tokens, start + 1, '(') {
        return None;
    }
    let close = matching(tokens, start + 1, '(', ')')?;
    (close + 1 == end)
        .then(|| script.literal_path(start + 2, close))
        .flatten()
}

/// Extract explicit `dependsOn` facts only from recognized source-set blocks.
fn source_edges(script: &StaticScript, edges: &mut BTreeMap<String, BTreeSet<String>>) {
    let tokens = script.tokens();
    let mut index = 0;
    while index < tokens.len() {
        let Some((open, close)) = source_sets_block_at(script, index) else {
            index += 1;
            continue;
        };
        collect_source_set_block(script, open, close, edges);
        index = close + 1;
    }
}

fn source_sets_block_at(script: &StaticScript, index: usize) -> Option<(usize, usize)> {
    let tokens = script.tokens();
    let recognized_parent = matches!(
        script.ancestors(index),
        [GradleBlock::Kotlin] | [GradleBlock::Android]
    );
    if !script.is_static_identifier(index, "sourceSets")
        || !statement_start(tokens, index)
        || symbol_at(tokens, index.saturating_sub(1), '.')
        || !recognized_parent
    {
        return None;
    }

    script.braced_call_at(index)
}

fn collect_source_set_block(
    script: &StaticScript,
    open: usize,
    close: usize,
    edges: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let mut index = open + 1;
    while index < close {
        if symbol_at(script.tokens(), index, '{') {
            index = matching(script.tokens(), index, '{', '}').map_or(index + 1, |end| end + 1);
            continue;
        }
        if !statement_start(script.tokens(), index) {
            index += 1;
            continue;
        }

        match source_set_item_at(script, index, close) {
            Some(SourceSetItem::Declared(declaration)) => {
                if let Some((body_open, body_close)) = declaration.body {
                    collect_depends_calls(script, &declaration.name, body_open, body_close, edges);
                    index = body_close + 1;
                } else {
                    index = declaration.next;
                }
            }
            Some(SourceSetItem::Named { name, open, close }) => {
                collect_depends_calls(script, &name, open, close, edges);
                index = close + 1;
            }
            Some(SourceSetItem::Dotted { child, start, end }) => {
                if let Some(parent) = first_source_set_argument(script, start, end) {
                    edges.entry(child).or_default().insert(parent);
                }
                index = end + 1;
            }
            None => index += 1,
        }
    }
}

pub(super) enum SourceSetItem {
    Declared(DeclaredSourceSet),
    Named {
        name: String,
        open: usize,
        close: usize,
    },
    Dotted {
        child: String,
        start: usize,
        end: usize,
    },
}

fn source_set_item_at(
    script: &StaticScript,
    index: usize,
    block_close: usize,
) -> Option<SourceSetItem> {
    if !script.outside_control_flow(index) {
        return None;
    }
    if script.is_identifier(index, "val") {
        return declared_source_set(script, index, block_close).map(SourceSetItem::Declared);
    }
    if let Some((name, open, close)) = named_source_set(script, index, block_close) {
        return Some(SourceSetItem::Named { name, open, close });
    }
    dotted_depends(script, index, block_close).map(|(child, start, end)| SourceSetItem::Dotted {
        child,
        start,
        end,
    })
}

pub(super) struct DeclaredSourceSet {
    pub(super) name: String,
    pub(super) body: Option<(usize, usize)>,
    pub(super) next: usize,
}

fn declared_source_set(
    script: &StaticScript,
    index: usize,
    block_close: usize,
) -> Option<DeclaredSourceSet> {
    let tokens = script.tokens();
    let name_index = index + 1;
    let name = token_identifier(tokens, name_index)?.to_owned();
    if !script.is_static_token(name_index) || !script.is_identifier(index + 2, "by") {
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
    script: &StaticScript,
    index: usize,
    block_close: usize,
) -> Option<(String, usize, usize)> {
    if !(script.is_identifier(index, "getByName") || script.is_identifier(index, "named"))
        || !script.is_static_token(index)
        || !symbol_at(script.tokens(), index + 1, '(')
    {
        return None;
    }
    let tokens = script.tokens();
    let arguments_close = matching(tokens, index + 1, '(', ')')?;
    let name = script.literal_string(index + 2, arguments_close)?;
    if !valid_source_set_name(&name) {
        return None;
    }
    let open = arguments_close + 1;
    if !symbol_at(tokens, open, '{') {
        return None;
    }
    let close = matching(tokens, open, '{', '}')?;
    (close < block_close).then_some((name, open, close))
}

fn dotted_depends(
    script: &StaticScript,
    index: usize,
    block_close: usize,
) -> Option<(String, usize, usize)> {
    let tokens = script.tokens();
    let child = token_identifier(tokens, index)?.to_owned();
    if !script.is_static_token(index)
        || !symbol_at(tokens, index + 1, '.')
        || !script.is_static_identifier(index + 2, "dependsOn")
        || !symbol_at(tokens, index + 3, '(')
    {
        return None;
    }
    let end = matching(tokens, index + 3, '(', ')')?;
    (end < block_close).then_some((child, index + 4, end))
}

fn collect_depends_calls(
    script: &StaticScript,
    child: &str,
    body_open: usize,
    body_close: usize,
    edges: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let tokens = script.tokens();
    let mut index = body_open + 1;
    while index < body_close {
        if symbol_at(tokens, index, '{') {
            index = matching(tokens, index, '{', '}').map_or(index + 1, |end| end + 1);
            continue;
        }
        if script.is_static_identifier(index, "dependsOn")
            && symbol_at(tokens, index + 1, '(')
            && let Some(close) = matching(tokens, index + 1, '(', ')')
        {
            if let Some(parent) = first_source_set_argument(script, index + 2, close) {
                edges.entry(child.to_owned()).or_default().insert(parent);
            }
            index = close + 1;
        } else {
            index += 1;
        }
    }
}

fn first_source_set_argument(script: &StaticScript, start: usize, end: usize) -> Option<String> {
    let name = token_identifier(script.tokens(), start);
    if let Some(name) = name.filter(|_| script.is_static_token(start)) {
        return valid_source_set_name(name).then(|| name.to_owned());
    }
    let name = script.literal_string(start, end)?;
    valid_source_set_name(&name).then_some(name)
}
