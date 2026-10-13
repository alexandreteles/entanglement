use super::blocks::valid_source_set_name;
use super::lexer::{matching, symbol_at, token_identifier};
use super::script::StaticScript;

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

pub(super) fn source_set_item_at(
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
