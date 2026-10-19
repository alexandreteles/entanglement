use super::*;
use crate::graph::CallableTypeParameter;

fn generic_signature() -> CallableSignature {
    CallableSignature {
        type_parameters: vec![CallableTypeParameter {
            name: "T".into(),
            bounds: Vec::new(),
        }],
        parameters: vec![
            CallableParameter {
                name: Some("value".into()),
                type_name: Some("T".into()),
                ..Default::default()
            },
            CallableParameter {
                name: Some("callback".into()),
                type_name: Some("(T)->Unit".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

#[test]
fn shared_bindings_keep_raw_and_inferred_types_separate() {
    let mut signature = generic_signature();
    signature.type_parameters[0].bounds.push("Number".into());
    let raw = vec![
        CallArgumentFact {
            start_byte: 1,
            end_byte: 2,
            ..Default::default()
        },
        CallArgumentFact {
            start_byte: 3,
            end_byte: 8,
            is_lambda: true,
            is_trailing_lambda: true,
            ..Default::default()
        },
    ];
    let arguments = CallArguments::new(&raw, Rc::default());
    let bound = arguments.bind(&signature);
    assert!(bound.lambda_parameter(&raw[1]).is_some());
    assert!(arguments.evidence.inferred.get().is_none());
    assert!(bound.substitutions().is_empty());
    assert!(bound.matches());
    let mut inferred = raw.clone();
    inferred[0].type_name = Some("String".into());
    let typed = arguments.with_inferred(&inferred);
    let inferred_bound = typed.bind(&signature);
    assert!(Rc::ptr_eq(&bound.plan, &inferred_bound.plan));
    assert_eq!(
        inferred_bound.substitutions().get("T").map(String::as_str),
        Some("String")
    );
    assert!(!inferred_bound.matches());
    assert!(bound.substitutions().is_empty());
    let repeated = typed.bind(&signature);
    assert!(std::ptr::eq(
        inferred_bound.substitutions(),
        repeated.substitutions()
    ));
    assert!(repeated.plan.inferred_types.matches.get().is_some());
    assert_eq!(arguments.evidence.bindings.borrow().len(), 1);
}

#[test]
fn invalid_and_duplicate_arguments_keep_partial_inference_and_lambda_barrier() {
    let signature = generic_signature();
    let arguments = vec![
        CallArgumentFact {
            name: Some("absent".into()),
            type_name: Some("String".into()),
            ..Default::default()
        },
        CallArgumentFact {
            name: Some("value".into()),
            type_name: Some("Int".into()),
            start_byte: 1,
            end_byte: 2,
            ..Default::default()
        },
        CallArgumentFact {
            is_lambda: true,
            is_trailing_lambda: true,
            start_byte: 3,
            end_byte: 8,
            ..Default::default()
        },
    ];
    let call = CallArguments::new(&arguments, Rc::default());
    let bound = call.bind(&signature);
    assert!(!bound.matches());
    assert_eq!(
        bound.substitutions().get("T").map(String::as_str),
        Some("Int")
    );
    assert!(bound.lambda_parameter(&arguments[2]).is_none());
    let mut duplicate = arguments.clone();
    duplicate.insert(
        2,
        CallArgumentFact {
            name: Some("value".into()),
            type_name: Some("String".into()),
            start_byte: 2,
            end_byte: 3,
            ..Default::default()
        },
    );
    let duplicate_call = CallArguments::new(&duplicate, Rc::default());
    let duplicate_bound = duplicate_call.bind(&signature);
    assert!(duplicate_bound.substitutions().is_empty());
    assert!(!duplicate_bound.matches());
}

#[test]
fn call_and_signature_evidence_do_not_cross_contaminate() {
    let signature = generic_signature();
    let mut bounded = signature.clone();
    bounded.type_parameters[0].bounds.push("Number".into());
    let raw = vec![
        CallArgumentFact {
            name: Some("value".into()),
            type_name: Some("String".into()),
            ..Default::default()
        },
        CallArgumentFact {
            is_lambda: true,
            is_trailing_lambda: true,
            start_byte: 3,
            end_byte: 8,
            ..Default::default()
        },
    ];
    let arguments = CallArguments::new(&raw, Rc::default());
    let first = arguments.bind(&signature);
    let second = arguments.bind(&bounded);
    assert!(!Rc::ptr_eq(&first.plan, &second.plan));
    assert!(first.substitutions().contains_key("T"));
    assert!(first.matches());
    assert!(!second.matches());
    let other_call = CallArguments::new(&raw, Rc::default());
    assert!(!Rc::ptr_eq(&first.plan, &other_call.bind(&signature).plan));
}
