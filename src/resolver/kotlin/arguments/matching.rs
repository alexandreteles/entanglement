use super::function_types::lambda_parameters_match;
use super::generic::{generic_bounds_match, generic_type_substitutions};
use super::types::{is_type_compatible, substitute_type_name};
use crate::graph::{CallArgumentFact, CallableParameter, CallableSignature};
use std::collections::HashSet;

pub(in crate::resolver::kotlin) fn arguments_match(
    signature: &CallableSignature,
    arguments: &[CallArgumentFact],
) -> bool {
    let substitutions = generic_type_substitutions(signature, arguments);
    if !generic_bounds_match(signature, &substitutions) {
        return false;
    }
    let Some(bound) = bind_arguments(signature, arguments, &substitutions) else {
        return false;
    };
    signature
        .parameters
        .iter()
        .enumerate()
        .all(|(index, parameter)| {
            bound.contains(&index) || parameter.has_default || parameter.is_vararg
        })
}

fn bind_arguments(
    signature: &CallableSignature,
    arguments: &[CallArgumentFact],
    substitutions: &std::collections::BTreeMap<String, String>,
) -> Option<HashSet<usize>> {
    let mut bound = HashSet::<usize>::new();
    let mut positional = 0usize;
    for argument in arguments {
        let index = parameter_index(signature, argument, &bound, &mut positional)?;
        let parameter = signature.parameters.get(index)?;
        if !parameter.is_vararg && !bound.insert(index) {
            return None;
        }
        if !argument_matches_parameter(argument, parameter, substitutions) {
            return None;
        }
        while positional < signature.parameters.len() && bound.contains(&positional) {
            positional += 1;
        }
    }
    Some(bound)
}

pub(in crate::resolver::kotlin) fn lambda_parameter_for_argument<'a>(
    signature: &'a CallableSignature,
    arguments: &[CallArgumentFact],
    wanted: &CallArgumentFact,
) -> Option<&'a CallableParameter> {
    let mut bound = HashSet::<usize>::new();
    let mut positional = 0usize;
    for argument in arguments {
        let parameter_index = parameter_index(signature, argument, &bound, &mut positional)?;
        let parameter = signature.parameters.get(parameter_index)?;
        if !parameter.is_vararg {
            bound.insert(parameter_index);
        }
        if argument.start_byte == wanted.start_byte && argument.end_byte == wanted.end_byte {
            return Some(parameter);
        }
    }
    None
}

pub(in crate::resolver::kotlin) fn parameter_index(
    signature: &CallableSignature,
    argument: &CallArgumentFact,
    bound: &HashSet<usize>,
    positional: &mut usize,
) -> Option<usize> {
    if let Some(name) = argument.name.as_deref() {
        return signature
            .parameters
            .iter()
            .position(|parameter| parameter.name.as_deref() == Some(name));
    }
    if argument.is_trailing_lambda
        && let Some((index, _)) =
            signature
                .parameters
                .iter()
                .enumerate()
                .rev()
                .find(|(index, parameter)| {
                    !bound.contains(index)
                        && parameter
                            .type_name
                            .as_deref()
                            .is_some_and(|ty| ty.contains("->"))
                })
    {
        return Some(index);
    }
    while *positional < signature.parameters.len() && bound.contains(positional) {
        *positional += 1;
    }
    if *positional < signature.parameters.len() {
        let index = *positional;
        if !signature.parameters[index].is_vararg {
            *positional += 1;
        }
        Some(index)
    } else if signature
        .parameters
        .last()
        .is_some_and(|parameter| parameter.is_vararg)
    {
        Some(signature.parameters.len() - 1)
    } else {
        None
    }
}

fn argument_matches_parameter(
    argument: &CallArgumentFact,
    parameter: &CallableParameter,
    substitutions: &std::collections::BTreeMap<String, String>,
) -> bool {
    if let (Some(actual), Some(expected)) = (
        argument.type_name.as_deref(),
        parameter.type_name.as_deref(),
    ) {
        let expected = substitute_type_name(expected, substitutions);
        if !is_type_compatible(&expected, actual) {
            return false;
        }
    }
    if argument.is_lambda
        && let Some(expected) = parameter.type_name.as_deref()
    {
        let expected = substitute_type_name(expected, substitutions);
        return lambda_parameters_match(&expected, &argument.lambda_parameter_types);
    }
    true
}
