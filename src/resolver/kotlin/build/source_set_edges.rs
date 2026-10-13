use std::collections::{BTreeMap, BTreeSet};

use super::blocks::valid_source_set_name;
use super::lexer::{matching, symbol_at, token_identifier};
use super::script::StaticScript;

pub(super) fn collect_depends_calls(
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

pub(super) fn first_source_set_argument(
    script: &StaticScript,
    start: usize,
    end: usize,
) -> Option<String> {
    let name = token_identifier(script.tokens(), start);
    if let Some(name) = name.filter(|_| script.is_static_token(start)) {
        return valid_source_set_name(name).then(|| name.to_owned());
    }
    let name = script.literal_string(start, end)?;
    valid_source_set_name(&name).then_some(name)
}
