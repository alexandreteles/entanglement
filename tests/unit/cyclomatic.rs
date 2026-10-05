use super::{FunctionScope, analyze};
use crate::metrics::{SyntaxEvent, SyntaxRole};
use std::collections::HashMap;

fn event(role: SyntaxRole, node_id: usize, start_byte: usize) -> SyntaxEvent {
    SyntaxEvent {
        role,
        start_byte,
        end_byte: start_byte + 1,
        start_row: 0,
        end_row: 0,
        end_column: start_byte + 1,
        node_id,
        parent_id: None,
        terminal: true,
    }
}

#[test]
fn public_calculation_starts_at_one_for_plain_functions() {
    let function = FunctionScope {
        name: "plain".into(),
        range: 0..10,
        line: 1,
    };

    let (score, contributions) = analyze(&function, &[], &HashMap::new());

    assert_eq!(score, 1);
    assert_eq!(contributions[0].kind, "baseline");
    assert_eq!(contributions[0].value, 1);
}

#[test]
fn public_calculation_counts_match_arms_from_syntax_ancestry() {
    let function = FunctionScope {
        name: "choice".into(),
        range: 0..20,
        line: 1,
    };
    let events = [
        event(SyntaxRole::Multiway, 1, 2),
        event(SyntaxRole::Case, 2, 5),
        event(SyntaxRole::Case, 3, 8),
        event(SyntaxRole::Case, 4, 11),
    ];
    let parents = HashMap::from([(2, Some(1)), (3, Some(1)), (4, Some(1))]);

    let (score, contributions) = analyze(&function, &events, &parents);

    assert_eq!(score, 3);
    assert_eq!(contributions.len(), 2);
    assert_eq!(contributions[1].kind, "multiway");
    assert_eq!(contributions[1].value, 2);
}
