use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::lexer::{matching, split_arguments, statement_end, symbol_at};
use super::script::StaticScript;

/// Extract only top-level, literal module inclusions from settings scripts.
pub(in crate::resolver::kotlin) fn parse_includes(source: &str) -> BTreeSet<String> {
    let script = StaticScript::parse(source);
    let tokens = script.tokens();
    let mut modules = BTreeSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if script.is_root_statement(index, "include") {
            index = collect_include(&script, index, &mut modules);
        } else {
            index += 1;
        }
    }
    modules
}

fn collect_include(script: &StaticScript, index: usize, modules: &mut BTreeSet<String>) -> usize {
    let tokens = script.tokens();
    if symbol_at(tokens, index + 1, '(') {
        let Some(close) = matching(tokens, index + 1, '(', ')') else {
            return index + 1;
        };
        for (start, end) in split_arguments(tokens, index + 2, close) {
            if let Some(path) = script.literal_path(start, end) {
                modules.insert(path);
            }
        }
        return close + 1;
    }

    // Groovy also uses `include ':app', ':lib'`.
    let end = statement_end(tokens, index + 1);
    for (start, end) in split_arguments(tokens, index + 1, end) {
        if let Some(path) = script.literal_path(start, end) {
            modules.insert(path);
        }
    }
    end.max(index + 1)
}

/// Resolve only direct settings-script mappings to relative file literals.
pub(in crate::resolver::kotlin) fn parse_project_directories(
    source: &str,
) -> BTreeMap<String, PathBuf> {
    let script = StaticScript::parse(source);
    let tokens = script.tokens();
    let mut directories = BTreeMap::new();
    for index in 0..tokens.len() {
        if !script.is_root_statement(index, "project")
            || !symbol_at(tokens, index + 1, '(')
            || symbol_at(tokens, index.saturating_sub(1), '.')
        {
            continue;
        }
        if let Some((module_path, directory)) = script.project_directory(index) {
            directories.insert(module_path, directory);
        }
    }
    directories
}
