use super::*;

/// Extract only top-level, literal module inclusions from settings scripts.
pub(super) fn includes(script: &StaticScript) -> BTreeSet<String> {
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
pub(super) fn project_directories(script: &StaticScript) -> BTreeMap<String, PathBuf> {
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
