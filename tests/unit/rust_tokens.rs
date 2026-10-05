use super::capture;
use crate::metrics::halstead::HalsteadTokenKind;

fn capture_source(source: &str) -> Vec<super::HalsteadToken> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    capture(tree.root_node(), source.as_bytes())
}

#[test]
fn comments_and_literals_are_captured_atomically() {
    let tokens = capture_source(
        "fn f() { // fake + operator\n let s = r#\"// string + ( )\"#; let c = b'x'; let yes = true; let r#type = -2; }",
    );
    let spellings = tokens
        .iter()
        .map(|token| token.token.as_str())
        .collect::<Vec<_>>();

    assert!(!spellings.iter().any(|token| token.contains("fake")));
    assert_eq!(
        tokens
            .iter()
            .filter(|token| token.kind == HalsteadTokenKind::Operand
                && token.token == "r#\"// string + ( )\"#")
            .count(),
        1
    );
    assert!(tokens.iter().any(|token| token.token == "b'x'"));
    assert!(tokens.iter().any(|token| token.token == "true"));
    assert!(tokens.iter().any(|token| token.token == "r#type"));
    assert!(
        tokens
            .iter()
            .any(|token| { token.kind == HalsteadTokenKind::Operator && token.token == "-" })
    );
    assert!(
        tokens
            .iter()
            .any(|token| { token.kind == HalsteadTokenKind::Operand && token.token == "2" })
    );
    assert_eq!(
        tokens
            .iter()
            .filter(|token| token.kind == HalsteadTokenKind::Operator && token.token == "+")
            .count(),
        0
    );
}

#[test]
fn self_paths_lifetimes_and_macro_metavariables_are_operands() {
    let tokens = capture_source(
        "macro_rules! m { ($item:expr) => { $item }; } impl S { fn f<'a>(&'a self) { self.run(); Self::new(); let _ = crate::root; let _ = super::root; } }",
    );
    for spelling in ["'a", "self", "Self", "crate", "super", "$item"] {
        assert!(
            tokens.iter().any(|token| {
                token.kind == HalsteadTokenKind::Operand && token.token == spelling
            }),
            "missing operand {spelling}"
        );
    }
}
