use super::*;

/// Extract literal project dependencies while retaining configuration and
/// source-set context for the resolver's visibility rules.
pub(super) fn project_dependencies(script: &StaticScript) -> BTreeSet<ProjectDependency> {
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
