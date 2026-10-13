use super::*;

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
                class.package.clone(),
                extended_owner(&class.owner, &class.name),
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
    calls: &'a [crate::graph::CallFacts],
    argument: &crate::graph::CallArgumentFact,
) -> Option<&'a crate::graph::CallFacts> {
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
    references: &'a [Reference],
    context_id: usize,
    call: &crate::graph::CallFacts,
) -> Option<&'a Reference> {
    let target = call.target.as_ref()?;
    references.iter().find(|reference| {
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
    let Some(declaration) = candidate.declaration.as_ref() else {
        return convention.is_none();
    };
    match convention {
        Some(CallConvention::Operator) => declaration.modifiers.is_operator,
        Some(CallConvention::Infix) => declaration.modifiers.is_infix,
        None => true,
    }
}

pub(super) fn candidate_visibility(candidate: &Candidate) -> SemanticVisibility {
    candidate.declaration.as_ref().map_or_else(
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
    let Some(declaration) = candidate.declaration.as_ref() else {
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
