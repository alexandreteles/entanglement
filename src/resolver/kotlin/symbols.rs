use super::*;

pub(super) fn same_logical_declaration(left: &Candidate, right: &Candidate) -> bool {
    if left.name != right.name || left.package != right.package || left.owner != right.owner {
        return false;
    }
    let left_decl = left.declaration;
    let right_decl = right.declaration;
    match (left_decl, right_decl) {
        (Some(left), Some(right)) => {
            let both_type = matches!(
                left.kind,
                DeclarationKind::Class | DeclarationKind::TypeAlias
            ) && matches!(
                right.kind,
                DeclarationKind::Class | DeclarationKind::TypeAlias
            );
            both_type
                || match (&left.signature, &right.signature) {
                    (Some(left), Some(right)) => {
                        left.receiver_type == right.receiver_type
                            && left.parameters.len() == right.parameters.len()
                            && left
                                .parameters
                                .iter()
                                .zip(&right.parameters)
                                .all(|(left, right)| left.type_name == right.type_name)
                    }
                    (None, None) => left.kind == right.kind,
                    _ => false,
                }
        }
        _ => false,
    }
}

pub(super) fn candidate_type_segments<'a>(candidate: &Candidate<'a>) -> Vec<&'a str> {
    candidate
        .package
        .iter()
        .chain(candidate.owner)
        .map(String::as_str)
        .chain(std::iter::once(candidate.name))
        .collect()
}

pub(super) fn candidate_type_name(candidate: &Candidate) -> String {
    candidate_type_segments(candidate).join(".")
}

pub(super) fn is_extension_candidate(candidate: &Candidate) -> bool {
    candidate
        .signature()
        .is_some_and(|signature| signature.receiver_type.is_some())
}

pub(super) fn same_callable_signature(
    left: Option<&DeclarationFacts>,
    right: Option<&DeclarationFacts>,
) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    if left.kind != right.kind {
        return false;
    }
    let (Some(left), Some(right)) = (left.signature.as_ref(), right.signature.as_ref()) else {
        return false;
    };
    left.receiver_type.as_deref().map(normalize_type)
        == right.receiver_type.as_deref().map(normalize_type)
        && left.parameters.len() == right.parameters.len()
        && left
            .parameters
            .iter()
            .zip(&right.parameters)
            .all(|(left, right)| {
                left.is_vararg == right.is_vararg
                    && left.type_name.as_deref().map(normalize_type)
                        == right.type_name.as_deref().map(normalize_type)
            })
}

pub(super) fn is_type_declaration(candidate: &Candidate) -> bool {
    candidate.declaration.is_some_and(|declaration| {
        matches!(
            declaration.kind,
            DeclarationKind::Class
                | DeclarationKind::Object
                | DeclarationKind::CompanionObject
                | DeclarationKind::TypeAlias
        )
    }) || matches!(
        candidate.kind,
        DefinitionKind::Struct | DefinitionKind::TypeAlias
    )
}

pub(super) fn is_kotlin_builtin(name: &str, reference: &Reference) -> bool {
    let builtins = [
        "Any",
        "Nothing",
        "Unit",
        "Boolean",
        "Byte",
        "Short",
        "Int",
        "Long",
        "Float",
        "Double",
        "Char",
        "String",
        "Array",
        "List",
        "MutableList",
        "Set",
        "Map",
        "Pair",
        "Triple",
        "println",
        "print",
        "TODO",
        "require",
        "check",
        "error",
        "listOf",
        "mutableListOf",
    ];
    reference.path.len() == 1 && builtins.contains(&name)
}

pub(super) fn analysis(
    reference: &Reference,
    resolution: Resolution,
    dispatch: Option<CallDispatch>,
) -> ReferenceAnalysis {
    ReferenceAnalysis {
        path: reference.path.clone(),
        start_byte: reference.start_byte,
        end_byte: reference.end_byte,
        resolution,
        dispatch,
        execution_targets: Vec::new(),
    }
}

pub(super) fn unique_candidates(mut candidates: Vec<usize>) -> Vec<usize> {
    candidates.sort_unstable();
    candidates.dedup();
    candidates
}

pub(super) fn owner_paths(classes: &[&Candidate]) -> BTreeSet<(Vec<String>, Vec<String>)> {
    classes
        .iter()
        .map(|class| {
            (
                class.package.to_vec(),
                extended_owner(class.owner, class.name),
            )
        })
        .collect()
}

pub(super) fn extended_owner(owner: &[String], name: &str) -> Vec<String> {
    owner
        .iter()
        .cloned()
        .chain(std::iter::once(name.to_owned()))
        .collect()
}

pub(super) fn exact_argument_call<'a>(
    calls: &'a [CallFacts],
    argument: &CallArgumentFact,
) -> Option<&'a CallFacts> {
    calls
        .iter()
        .filter(|call| {
            call.start_byte == argument.start_byte
                && call.end_byte == argument.end_byte
                && call.target.is_some()
        })
        .min_by_key(|call| call.convention.is_none())
}

pub(super) fn call_target_reference<'a>(
    references: impl IntoIterator<Item = &'a Reference>,
    context_id: usize,
    call: &CallFacts,
) -> Option<&'a Reference> {
    let target = call.target.as_ref()?;
    references.into_iter().find(|reference| {
        reference.context_id == context_id
            && reference.start_byte == target.start_byte
            && reference.end_byte == target.end_byte
            && matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
    })
}

pub(super) fn is_explicit_receiver(name: &str) -> bool {
    name == "this" || name == "super" || name.starts_with("this@")
}

pub(super) fn matches_convention(
    candidate: &Candidate,
    convention: Option<CallConvention>,
) -> bool {
    let Some(declaration) = candidate.declaration else {
        return convention.is_none();
    };
    match convention {
        Some(CallConvention::Operator) => declaration.modifiers.is_operator,
        Some(CallConvention::Infix) => declaration.modifiers.is_infix,
        None => true,
    }
}

pub(super) fn candidate_visibility(candidate: &Candidate) -> SemanticVisibility {
    candidate.declaration.map_or_else(
        || {
            if candidate.public {
                SemanticVisibility::Public
            } else {
                SemanticVisibility::Private
            }
        },
        |declaration| declaration.visibility,
    )
}

pub(super) fn imported_name(import: &Import) -> Option<&str> {
    import
        .alias
        .as_deref()
        .or_else(|| import.path.last().map(String::as_str))
}

pub(super) fn call_dispatch(candidate: &Candidate, reference: &Reference) -> CallDispatch {
    let Some(declaration) = candidate.declaration else {
        return CallDispatch::Unknown;
    };
    if reference.path.iter().any(|segment| segment == "super") {
        return CallDispatch::Direct;
    }
    if is_type_declaration(candidate) {
        // A selected constructor is statically dispatched even when instances
        // of its class can later participate in virtual method dispatch.
        return CallDispatch::Direct;
    }
    if declaration.is_interface_member {
        return CallDispatch::Virtual;
    }
    if declaration.modifiers.is_expect
        || declaration.modifiers.is_abstract
        || declaration.modifiers.is_open
    {
        return CallDispatch::Virtual;
    }
    if declaration.modifiers.is_final || !declaration.modifiers.is_override {
        CallDispatch::Direct
    } else {
        CallDispatch::Virtual
    }
}
