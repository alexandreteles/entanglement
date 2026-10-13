use super::super::*;

pub(super) fn recovered_modifier_node<'tree>(
    error: Node<'tree>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
) -> Option<Node<'tree>> {
    let mut before = (0..error.child_count())
        .filter_map(|index| error.child(index))
        .filter(|child| child.end_byte() <= recovered.keyword_range.start)
        .collect::<Vec<_>>();
    before.reverse();
    for child in before {
        if matches!(child.kind(), "line_comment" | "multiline_comment") {
            continue;
        }
        return (child.kind() == "modifiers").then_some(child);
    }
    None
}

pub(super) fn recovered_has_modifier(
    error: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    source: &[u8],
    name: &str,
) -> bool {
    recovered_modifier_node(error, recovered).is_some_and(|modifiers| {
        all_nodes(modifiers)
            .into_iter()
            .filter(|child| child.child_count() == 0)
            .any(|child| text(child, source) == name || child.kind() == name)
    })
}

pub(super) fn recovered_visibility(
    error: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    source: &[u8],
) -> SemanticVisibility {
    if recovered_has_modifier(error, recovered, source, "private") {
        SemanticVisibility::Private
    } else if recovered_has_modifier(error, recovered, source, "protected") {
        SemanticVisibility::Protected
    } else if recovered_has_modifier(error, recovered, source, "internal") {
        SemanticVisibility::Internal
    } else {
        SemanticVisibility::Public
    }
}

pub(super) fn recovered_modifiers(
    error: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    source: &[u8],
) -> DeclarationModifiers {
    let has = |name| recovered_has_modifier(error, recovered, source, name);
    DeclarationModifiers {
        is_expect: has("expect"),
        is_actual: has("actual"),
        is_abstract: has("abstract"),
        is_open: has("open"),
        is_final: has("final"),
        is_override: has("override"),
        is_operator: false,
        is_infix: false,
    }
}

pub(super) fn recovered_annotations(
    error: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    source: &[u8],
) -> Vec<String> {
    recovered_modifier_node(error, recovered)
        .into_iter()
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "annotation")
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "user_type")
        .map(|child| normalized_type(text(child, source)))
        .collect()
}
