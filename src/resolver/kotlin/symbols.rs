use super::*;

pub(super) fn same_logical_declaration(left: &Candidate, right: &Candidate) -> bool {
    if left.name != right.name || left.package != right.package || left.owner != right.owner {
        return false;
    }
    let left_decl = left.declaration.as_ref();
    let right_decl = right.declaration.as_ref();
    match (left_decl, right_decl) {
        (Some(left), Some(right)) => {
            let both_type = matches!(
                left.kind,
                crate::graph::DeclarationKind::Class | crate::graph::DeclarationKind::TypeAlias
            ) && matches!(
                right.kind,
                crate::graph::DeclarationKind::Class | crate::graph::DeclarationKind::TypeAlias
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

pub(super) fn candidate_type_segments(candidate: &Candidate) -> Vec<String> {
    candidate
        .package
        .iter()
        .chain(&candidate.owner)
        .chain(std::iter::once(&candidate.name))
        .cloned()
        .collect()
}

pub(super) fn candidate_type_name(candidate: &Candidate) -> String {
    candidate_type_segments(candidate).join(".")
}

pub(super) fn is_extension_candidate(candidate: &Candidate) -> bool {
    candidate
        .declaration
        .as_ref()
        .and_then(|declaration| declaration.signature.as_ref())
        .is_some_and(|signature| signature.receiver_type.is_some())
}

pub(super) fn same_callable_signature(
    left: Option<&crate::graph::DeclarationFacts>,
    right: Option<&crate::graph::DeclarationFacts>,
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
    candidate.declaration.as_ref().is_some_and(|declaration| {
        matches!(
            declaration.kind,
            crate::graph::DeclarationKind::Class
                | crate::graph::DeclarationKind::Object
                | crate::graph::DeclarationKind::CompanionObject
                | crate::graph::DeclarationKind::TypeAlias
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
