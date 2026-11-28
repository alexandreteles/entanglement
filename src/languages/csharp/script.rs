use super::conditional::{Line, blank};
use crate::languages::CapturedTree;
use crate::metrics::{
    SyntaxEvent, SyntaxRole,
    halstead::{HalsteadToken, HalsteadTokenKind},
};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct Directive {
    pub path: String,
    pub load: bool,
    pub range: Range<usize>,
}

pub(crate) fn projected(lines: &[Line], source: &[u8]) -> Option<Vec<u8>> {
    let mut result = None;
    for line in lines
        .iter()
        .filter(|l| matches!(l.command.as_str(), "load" | "r"))
    {
        blank(&mut result.get_or_insert_with(|| source.to_vec())[line.range.clone()]);
    }
    result
}

pub(crate) fn capture(lines: &[Line], captured: &mut CapturedTree, include_tokens: bool) {
    let Some(data) = captured.facts.semantic.csharp.as_deref_mut() else {
        return;
    };
    for line in lines
        .iter()
        .filter(|l| matches!(l.command.as_str(), "load" | "r"))
    {
        let quoted = line
            .argument
            .strip_prefix('"')
            .and_then(|s| s.rsplit_once('"'))
            .map(|(s, _)| s.to_owned());
        if let Some(path) = quoted {
            data.script.push(Directive {
                path,
                load: line.command == "load",
                range: line.range.clone(),
            });
        }
        let id = usize::MAX - line.row;
        captured.events.push(SyntaxEvent {
            role: SyntaxRole::Node,
            start_byte: line.range.start,
            end_byte: line.range.end,
            start_row: line.row,
            end_row: line.row,
            end_column: 1,
            node_id: id,
            parent_id: None,
            terminal: true,
        });
        if include_tokens {
            let end = line.range.start + 1 + line.command.len();
            captured.tokens.push(HalsteadToken {
                kind: HalsteadTokenKind::Operator,
                token: format!("#{}", line.command),
                start_byte: line.range.start,
                end_byte: end,
                line: line.row + 1,
            });
            captured.tokens.push(HalsteadToken {
                kind: HalsteadTokenKind::Operand,
                token: line.argument.clone(),
                start_byte: line.range.end - line.argument.len(),
                end_byte: line.range.end,
                line: line.row + 1,
            });
        }
    }
}
