use super::{all_nodes, capture};
use crate::graph::{BindingType, CallConvention, ReceiverScope};
use tree_sitter::Node;

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
    parser.set_language(&language).unwrap();
    parser.parse(source, None).unwrap()
}

fn capture_fixture(
    root: Node<'_>,
    source: &str,
    bindings: &[BindingType],
    receivers: &[ReceiverScope],
) -> super::ConventionFacts {
    let contexts = crate::languages::kotlin::syntax::context_parameters(source.as_bytes());
    let scope_index =
        crate::languages::kotlin::scopes::ScopeIndex::new(root, source.as_bytes(), &contexts);
    capture(bindings, receivers, &scope_index)
}

#[test]
fn operator_and_infix_syntax_emit_typed_member_call_facts() {
    let source = r#"
        class Box {
            operator fun plus(other: Box): Box = this + other
            infix fun work(other: Box): Box = this work other
            fun text() = "this + other and this work other"
        }
    "#;
    let tree = parse(source);
    let root = tree.root_node();
    let class_body = all_nodes(root)
        .into_iter()
        .find(|node| node.kind() == "class_body")
        .unwrap();
    let receivers = [ReceiverScope {
        start_byte: class_body.start_byte(),
        end_byte: class_body.end_byte(),
        receiver_type: "Box".into(),
        implicit: true,
        dsl_markers: Vec::new(),
        label: None,
    }];
    let facts = capture_fixture(root, source, &[], &receivers);

    assert_eq!(facts.calls.len(), 2);
    assert!(facts.calls.iter().any(|call| {
        call.convention == Some(CallConvention::Operator)
            && call.receiver_type.as_deref() == Some("Box")
    }));
    assert!(facts.calls.iter().any(|call| {
        call.convention == Some(CallConvention::Infix)
            && call.receiver_type.as_deref() == Some("Box")
    }));
    let names = facts
        .references
        .iter()
        .map(|reference| source[reference.start_byte..reference.end_byte].to_owned())
        .collect::<Vec<_>>();
    assert!(names.iter().any(|name| name == "+"));
    assert!(names.iter().any(|name| name == "work"));
    assert_eq!(
        names.len(),
        2,
        "string contents must not become conventions"
    );

    let fallback = capture_fixture(root, source, &[], &[]);
    assert!(
        fallback
            .calls
            .iter()
            .all(|call| { call.receiver_type.as_deref() == Some("Box") })
    );
}

#[test]
fn unknown_receiver_is_recorded_without_inventing_a_type() {
    let source = "fun combine(left: Any, right: Any) = left + right";
    let tree = parse(source);
    let facts = capture_fixture(tree.root_node(), source, &[], &[]);
    assert_eq!(facts.calls.len(), 1);
    assert_eq!(facts.calls[0].convention, Some(CallConvention::Operator));
    assert_eq!(facts.calls[0].receiver_type, None);
}

#[test]
fn infix_operator_is_selected_between_identifier_operands() {
    let source = "fun combine(value: Box, other: Box) = value merge other";
    let tree = parse(source);
    let value_start = source.find("value:").unwrap();
    let binding = BindingType {
        name: "value".into(),
        name_start_byte: value_start,
        name_end_byte: value_start + "value".len(),
        scope_start: 0,
        scope_end: source.len(),
        type_name: Some("Box".into()),
        alias_target: None,
        callable_reference: false,
        immutable: true,
    };
    let facts = capture_fixture(tree.root_node(), source, &[binding], &[]);

    assert_eq!(facts.references.len(), 1);
    assert_eq!(
        &source[facts.references[0].start_byte..facts.references[0].end_byte],
        "merge"
    );
    assert_eq!(facts.calls.len(), 1);
    assert_eq!(facts.calls[0].convention, Some(CallConvention::Infix));
    assert_eq!(facts.calls[0].receiver_type.as_deref(), Some("Box"));
}

#[test]
fn this_invocation_uses_its_enclosing_class_type() {
    let source = "class Action { operator fun invoke() { this() } }";
    let tree = parse(source);
    let facts = capture_fixture(tree.root_node(), source, &[], &[]);

    assert_eq!(facts.references.len(), 1);
    assert_eq!(facts.references[0].path, ["invoke"]);
    assert_eq!(facts.calls.len(), 1);
    assert_eq!(facts.calls[0].convention, Some(CallConvention::Operator));
    assert_eq!(facts.calls[0].receiver_type.as_deref(), Some("Action"));
}

#[test]
fn typed_index_and_object_call_emit_get_set_and_invoke() {
    let source = "fun use(box: Box, action: Action, value: Int) { val item = box[0]; box[1] = value; action(value) }";
    let tree = parse(source);
    let box_start = source.find("box:").unwrap();
    let action_start = source.find("action:").unwrap();
    let bindings = [
        BindingType {
            name: "box".into(),
            name_start_byte: box_start,
            name_end_byte: box_start + 3,
            scope_start: 0,
            scope_end: source.len(),
            type_name: Some("Box".into()),
            alias_target: None,
            callable_reference: false,
            immutable: true,
        },
        BindingType {
            name: "action".into(),
            name_start_byte: action_start,
            name_end_byte: action_start + 6,
            scope_start: 0,
            scope_end: source.len(),
            type_name: Some("Action".into()),
            alias_target: None,
            callable_reference: false,
            immutable: true,
        },
    ];
    let facts = capture_fixture(tree.root_node(), source, &bindings, &[]);
    for (name, receiver) in [("get", "Box"), ("set", "Box"), ("invoke", "Action")] {
        assert!(
            facts.calls.iter().any(|call| {
                call.convention == Some(CallConvention::Operator)
                    && call.receiver_type.as_deref() == Some(receiver)
                    && facts.references.iter().any(|reference| {
                        reference.path.last().is_some_and(|path| path == name)
                            && call.start_byte <= reference.start_byte
                            && reference.end_byte <= call.end_byte
                    })
            }),
            "expected a {name} call fact on {receiver}"
        );
    }
}
