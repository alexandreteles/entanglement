use std::collections::{BTreeMap, BTreeSet};

use super::lexer::{matching, statement_start, symbol_at};
use super::model::GradleBlock;
use super::script::StaticScript;
use super::source_set_edges::{collect_depends_calls, first_source_set_argument};
use super::source_set_items::{SourceSetItem, source_set_item_at};

/// Extract explicit `dependsOn` facts only from recognized source-set blocks.
pub(in crate::resolver::kotlin) fn parse_source_edges(
    source: &str,
    edges: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let script = StaticScript::parse(source);
    let tokens = script.tokens();
    let mut index = 0;
    while index < tokens.len() {
        let Some((open, close)) = source_sets_block_at(&script, index) else {
            index += 1;
            continue;
        };
        collect_source_set_block(&script, open, close, edges);
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
