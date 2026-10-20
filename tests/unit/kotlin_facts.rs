use super::{
    all_nodes, collect_calls, collect_references, integer_literal_type, real_literal_type,
};
use crate::model::BindingEvidence;
use crate::model::{ReferenceAccess, ReferenceKind};

#[test]
fn numeric_literal_types_respect_suffixes_and_ranges() {
    assert_eq!(integer_literal_type("42"), Some("Int"));
    assert_eq!(integer_literal_type("2_147_483_648"), Some("Long"));
    assert_eq!(integer_literal_type("42L"), Some("Long"));
    assert_eq!(integer_literal_type("42u"), Some("UInt"));
    assert_eq!(integer_literal_type("4_294_967_296u"), Some("ULong"));
    assert_eq!(integer_literal_type("0xFFFF_FFFFuL"), Some("ULong"));
    assert_eq!(integer_literal_type("9_223_372_036_854_775_808L"), None);
    assert_eq!(integer_literal_type("18_446_744_073_709_551_616u"), None);
    assert_eq!(real_literal_type("1.25"), "Double");
    assert_eq!(real_literal_type("1.25F"), "Float");
}

#[test]
fn call_argument_types_come_from_numeric_literal_cst_wrappers() {
    let source = "fun sample() { consume(1L); consume(1u); consume(0xFF); consume(0b1); consume(1f); consume(1.0F); consume(1.0) }";
    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
    parser.set_language(&language).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let calls = collect_calls(&all_nodes(tree.root_node()), source.as_bytes(), &[]);
    let argument_types = calls
        .into_iter()
        .flat_map(|call| {
            call.arguments
                .into_iter()
                .map(|argument| argument.type_name)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        argument_types,
        [
            Some("Long".to_owned()),
            Some("UInt".to_owned()),
            Some("Int".to_owned()),
            Some("Int".to_owned()),
            Some("Float".to_owned()),
            Some("Float".to_owned()),
            Some("Double".to_owned()),
        ]
    );
}

#[test]
fn binding_argument_types_keep_containing_argument_precedence() {
    let source = "fun sample(value: Box) { consume(value); consume(inner(value)); consume(1L) }";
    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
    parser.set_language(&language).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let name_start = source.find("value:").unwrap();
    let bindings = [crate::model::LocalBinding {
        evidence: BindingEvidence::Both,
        context_id: 0,
        name: "value".to_owned(),
        start_byte: name_start,
        end_byte: name_start + "value".len(),
        scope_start: 0,
        scope_end: source.len(),
        type_name: Some("Box".to_owned()),
        alias_target: None,
        callable_reference: false,
        immutable: true,
    }];
    let types = collect_calls(&all_nodes(tree.root_node()), source.as_bytes(), &bindings)
        .into_iter()
        .flat_map(|call| {
            call.arguments
                .into_iter()
                .map(|argument| argument.type_name)
        })
        .collect::<Vec<_>>();
    // An enclosing argument containing a nested call remains conservative;
    // merging enrichment into capture must not infer a new overload target.
    assert_eq!(
        types,
        [Some("Box".to_owned()), None, None, Some("Long".to_owned())]
    );
}

#[test]
fn qualified_member_reads_writes_and_postfix_updates_are_referenced() {
    let source = "class Box(var value: Int) { fun update(input: Int, other: Box) { val before = other.value; other.value = input; other.value++ } }";
    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
    parser.set_language(&language).unwrap();
    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();
    let scope_index = super::super::scopes::ScopeIndex::new(root, source.as_bytes());
    let references = collect_references(&scope_index, scope_index.nodes(), &[]);
    let access_for = |member_start: usize| {
        references
            .iter()
            .find(|reference| {
                reference.start_byte == member_start
                    && reference.kind == ReferenceKind::Qualified
                    && reference.path == ["other", "value"]
            })
            .and_then(|reference| reference.access)
    };
    let member_starts = source
        .match_indices(".value")
        .map(|(dot, _)| dot + 1)
        .collect::<Vec<_>>();

    assert_eq!(member_starts.len(), 3);
    assert_eq!(access_for(member_starts[0]), Some(ReferenceAccess::Read));
    assert_eq!(access_for(member_starts[1]), Some(ReferenceAccess::Write));
    assert_eq!(
        access_for(member_starts[2]),
        Some(ReferenceAccess::ReadWrite)
    );
}
