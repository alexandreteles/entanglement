use super::*;

/// Extract explicit `dependsOn` facts only from recognized source-set blocks.
pub(super) fn source_edges(script: &StaticScript, edges: &mut BTreeMap<String, BTreeSet<String>>) {
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
