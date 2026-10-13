use super::matching::parameter_index;
use super::types::{builtin_bound_result, normalize_type};
use crate::graph::{CallArgumentFact, CallableSignature};
use std::collections::{BTreeMap, HashSet};

pub(in crate::resolver::kotlin) fn generic_type_substitutions(
    signature: &CallableSignature,
    arguments: &[CallArgumentFact],
) -> BTreeMap<String, String> {
    if signature.type_parameters.is_empty() {
        return BTreeMap::new();
    }
    let type_parameters = signature
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<HashSet<_>>();
    let mut substitutions = BTreeMap::<String, String>::new();
    let mut conflicting = HashSet::<String>::new();
    let mut bound = HashSet::<usize>::new();
    let mut positional = 0usize;
    for argument in arguments {
        let Some(index) = parameter_index(signature, argument, &bound, &mut positional) else {
            continue;
        };
        bind_generic_argument(
            signature,
            argument,
            index,
            &type_parameters,
            &mut substitutions,
            &mut conflicting,
        );
        if !signature.parameters[index].is_vararg {
            bound.insert(index);
            while positional < signature.parameters.len() && bound.contains(&positional) {
                positional += 1;
            }
        }
    }
    substitutions
}

pub(in crate::resolver::kotlin) fn bind_generic_argument(
    signature: &CallableSignature,
    argument: &CallArgumentFact,
    index: usize,
    type_parameters: &HashSet<&str>,
    substitutions: &mut BTreeMap<String, String>,
    conflicting: &mut HashSet<String>,
) {
    let (Some(actual), Some(expected)) = (
        argument.type_name.as_deref(),
        signature.parameters[index].type_name.as_deref(),
    ) else {
        return;
    };
    let expected = expected.trim_end_matches('?');
    if !type_parameters.contains(expected) {
        return;
    }
    let name = expected.to_owned();
    let actual = normalize_type(actual);
    match substitutions.get(&name) {
        Some(previous) if normalize_type(previous) != actual => {
            conflicting.insert(name.clone());
            substitutions.remove(&name);
        }
        _ if !conflicting.contains(&name) => {
            substitutions.insert(name, actual);
        }
        _ => {}
    }
}

pub(in crate::resolver::kotlin) fn generic_bounds_match(
    signature: &CallableSignature,
    substitutions: &BTreeMap<String, String>,
) -> bool {
    signature.type_parameters.iter().all(|parameter| {
        let Some(actual) = substitutions.get(&parameter.name) else {
            return true;
        };
        parameter
            .bounds
            .iter()
            .all(|bound| builtin_bound_result(actual, bound) != Some(false))
    })
}
