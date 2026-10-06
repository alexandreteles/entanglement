use crate::metrics::SyntaxRole;

pub(super) fn capture_role(name: &str) -> Option<SyntaxRole> {
    STANDARD_ROLES
        .iter()
        .find_map(|(capture, role)| (*capture == name).then_some(*role))
        .or_else(|| {
            name.starts_with("definition.")
                .then_some(SyntaxRole::Definition)
        })
        .or_else(|| {
            name.starts_with("reference.")
                .then_some(SyntaxRole::Reference)
        })
}

const STANDARD_ROLES: &[(&str, SyntaxRole)] = &[
    ("syntax.node", SyntaxRole::Node),
    ("comment", SyntaxRole::Comment),
    ("metric.function", SyntaxRole::Function),
    ("metric.condition", SyntaxRole::Condition),
    ("metric.logical_condition", SyntaxRole::LogicalCondition),
    ("metric.cognitive.if", SyntaxRole::CognitiveIf),
    (
        "metric.cognitive.condition_boundary",
        SyntaxRole::CognitiveConditionBoundary,
    ),
    ("metric.cognitive.else", SyntaxRole::CognitiveElse),
    ("metric.cognitive.else_if", SyntaxRole::CognitiveElseIf),
    ("metric.cognitive.loop", SyntaxRole::CognitiveLoop),
    ("metric.cognitive.let_else", SyntaxRole::CognitiveLetElse),
    ("metric.cognitive.multiway", SyntaxRole::CognitiveMultiway),
    (
        "metric.cognitive.logical_expression",
        SyntaxRole::CognitiveLogicalExpression,
    ),
    (
        "metric.cognitive.logical_and",
        SyntaxRole::CognitiveLogicalAnd,
    ),
    (
        "metric.cognitive.logical_or",
        SyntaxRole::CognitiveLogicalOr,
    ),
    (
        "metric.cognitive.parentheses",
        SyntaxRole::CognitiveParentheses,
    ),
    ("metric.cognitive.closure", SyntaxRole::CognitiveClosure),
    (
        "metric.cognitive.labeled_jump",
        SyntaxRole::CognitiveLabeledJump,
    ),
    ("metric.multiway", SyntaxRole::Multiway),
    ("metric.case", SyntaxRole::Case),
    ("import", SyntaxRole::Import),
    ("injection.content", SyntaxRole::InjectionContent),
];
