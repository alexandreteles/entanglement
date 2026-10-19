//! Conservative Kotlin call argument binding and type compatibility.

use crate::graph::{CallArgumentFact, CallableParameter, CallableSignature};
use std::collections::{BTreeMap, HashMap, HashSet};

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

struct ArgumentBinding {
    parameter: Option<usize>,
    first_binding: bool,
}

/// Structural binding is independent of inferred argument types. Both raw
/// lambda lookup and typed matching borrow the same plan for one signature.
struct BindingPlan {
    bindings: Vec<ArgumentBinding>,
    required_parameters_bound: bool,
    raw_types: ArgumentTypes,
    inferred_types: ArgumentTypes,
}

impl BindingPlan {
    fn new(signature: &CallableSignature, arguments: &[CallArgumentFact]) -> Self {
        let mut bound = HashSet::new();
        let mut positional = 0;
        let bindings: Vec<_> = arguments
            .iter()
            .map(|argument| {
                let parameter = parameter_index(signature, argument, &bound, &mut positional);
                let first_binding = parameter.is_some_and(|index| {
                    signature.parameters[index].is_vararg || bound.insert(index)
                });
                ArgumentBinding {
                    parameter,
                    first_binding,
                }
            })
            .collect();
        let required_parameters_bound =
            signature
                .parameters
                .iter()
                .enumerate()
                .all(|(index, parameter)| {
                    bound.contains(&index) || parameter.has_default || parameter.is_vararg
                });
        Self {
            bindings,
            required_parameters_bound,
            raw_types: ArgumentTypes::default(),
            inferred_types: ArgumentTypes::default(),
        }
    }
}

#[derive(Default)]
struct ArgumentTypes {
    substitutions: OnceCell<BTreeMap<String, String>>,
    matches: OnceCell<bool>,
}

/// Derived evidence lives only for one resolver invocation. No candidate
/// selection is cached: lexical receiver and lookup semantics remain local.
#[derive(Default)]
pub(super) struct CallArgumentEvidence {
    pub(super) inferred: OnceCell<Rc<Vec<CallArgumentFact>>>,
    bindings: RefCell<HashMap<usize, Rc<BindingPlan>>>,
}

pub(super) struct CallArguments<'a> {
    pub(super) arguments: &'a [CallArgumentFact],
    pub(super) evidence: Rc<CallArgumentEvidence>,
    inferred: bool,
}

impl<'a> CallArguments<'a> {
    pub(super) fn new(
        arguments: &'a [CallArgumentFact],
        evidence: Rc<CallArgumentEvidence>,
    ) -> Self {
        Self {
            arguments,
            evidence,
            inferred: false,
        }
    }

    pub(super) fn with_inferred<'b>(&self, arguments: &'b [CallArgumentFact]) -> CallArguments<'b> {
        debug_assert_eq!(arguments.len(), self.arguments.len());
        CallArguments {
            arguments,
            evidence: Rc::clone(&self.evidence),
            inferred: true,
        }
    }

    pub(super) fn bind<'s>(&self, signature: &'s CallableSignature) -> BoundArguments<'s, 'a> {
        // Signatures are canonical immutable graph facts for this invocation.
        let key = signature as *const CallableSignature as usize;
        let plan = self
            .evidence
            .bindings
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| Rc::new(BindingPlan::new(signature, self.arguments)))
            .clone();
        BoundArguments {
            signature,
            arguments: self.arguments,
            plan,
            inferred: self.inferred,
        }
    }
}

pub(super) struct BoundArguments<'s, 'a> {
    signature: &'s CallableSignature,
    arguments: &'a [CallArgumentFact],
    plan: Rc<BindingPlan>,
    inferred: bool,
}

impl<'s, 'a> BoundArguments<'s, 'a> {
    fn types(&self) -> &ArgumentTypes {
        if self.inferred {
            &self.plan.inferred_types
        } else {
            &self.plan.raw_types
        }
    }

    pub(super) fn substitutions(&self) -> &BTreeMap<String, String> {
        self.types().substitutions.get_or_init(|| {
            substitutions_for_bindings(self.signature, self.arguments, &self.plan.bindings)
        })
    }

    pub(super) fn matches(&self) -> bool {
        *self.types().matches.get_or_init(|| self.compute_match())
    }

    fn compute_match(&self) -> bool {
        let signature = self.signature;
        let substitutions = self.substitutions();
        if !generic_bounds_match(signature, substitutions)
            || !self
                .plan
                .bindings
                .iter()
                .zip(self.arguments)
                .all(|(binding, argument)| {
                    binding.first_binding
                        && binding.parameter.is_some_and(|index| {
                            argument_matches_parameter(
                                argument,
                                &signature.parameters[index],
                                substitutions,
                            )
                        })
                })
        {
            return false;
        }
        self.plan.required_parameters_bound
    }

    pub(super) fn lambda_parameter(
        &self,
        wanted: &CallArgumentFact,
    ) -> Option<&'s CallableParameter> {
        for (binding, argument) in self.plan.bindings.iter().zip(self.arguments) {
            // Invalid earlier arguments intentionally stop lambda lookup,
            // while generic substitutions still inspect every valid binding.
            let index = binding.parameter?;
            if argument.start_byte == wanted.start_byte && argument.end_byte == wanted.end_byte {
                return self.signature.parameters.get(index);
            }
        }
        None
    }
}

fn substitutions_for_bindings(
    signature: &CallableSignature,
    arguments: &[CallArgumentFact],
    bindings: &[ArgumentBinding],
) -> BTreeMap<String, String> {
    if signature.type_parameters.is_empty() {
        return BTreeMap::new();
    }
    let type_parameters = signature
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<HashSet<_>>();
    let mut substitutions = BTreeMap::new();
    let mut conflicting = HashSet::new();
    for (binding, argument) in bindings.iter().zip(arguments) {
        if let Some(index) = binding.parameter {
            bind_generic_argument(
                signature,
                argument,
                index,
                &type_parameters,
                &mut substitutions,
                &mut conflicting,
            );
        }
    }
    substitutions
}

fn parameter_index(
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
    substitutions: &BTreeMap<String, String>,
) -> bool {
    if argument.type_name.is_none() && !argument.is_lambda {
        return true;
    }
    let Some(expected) = parameter.type_name.as_deref() else {
        return true;
    };
    let expected = substitute_type_name(expected, substitutions);
    argument
        .type_name
        .as_deref()
        .is_none_or(|actual| is_type_compatible(&expected, actual))
        && (!argument.is_lambda
            || lambda_parameters_match(&expected, &argument.lambda_parameter_types))
}

fn bind_generic_argument(
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

fn generic_bounds_match(
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

pub(super) fn function_type_receiver(function_type: &str) -> Option<&str> {
    let (signature, _) = function_type.split_once("->")?;
    let open = signature.find(".(")?;
    let receiver = signature[..open].trim_end_matches('.');
    (!receiver.is_empty()).then_some(receiver)
}

pub(super) fn function_type_parameters(function_type: &str) -> Option<Vec<String>> {
    let (signature, _) = function_type.split_once("->")?;
    let signature = signature.trim();
    if signature.contains(".(") {
        return None;
    }
    let open = signature.find('(')?;
    let close = signature.rfind(')')?;
    if close < open {
        return None;
    }
    let parameters = signature[open + 1..close].trim();
    Some(parameter_type_names(parameters))
}

fn parameter_type_names(parameters: &str) -> Vec<String> {
    if parameters.is_empty() {
        return Vec::new();
    }
    parameters
        .split(',')
        .map(|parameter| {
            parameter
                .rsplit_once(':')
                .map_or(parameter, |(_, ty)| ty)
                .trim()
                .to_owned()
        })
        .collect()
}

pub(super) fn lambda_parameters_match(expected: &str, actual: &[Option<String>]) -> bool {
    if actual.iter().all(Option::is_none) {
        return true;
    }
    let Some((parameters, _return_type)) = expected.split_once("->") else {
        return true;
    };
    let parameter_text = parameters
        .rsplit_once('.')
        .map_or(parameters, |(_, function_parameters)| function_parameters);
    let parameter_text = parameter_text
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')');
    let expected_types = parameter_type_names(parameter_text);
    expected_types.len() == actual.len()
        && expected_types.iter().zip(actual).all(|(expected, actual)| {
            actual
                .as_deref()
                .is_none_or(|actual| types_match(expected, actual))
        })
}

pub(super) fn substitute_type_name(
    type_name: &str,
    substitutions: &BTreeMap<String, String>,
) -> String {
    let mut result = String::with_capacity(type_name.len());
    let mut identifier = String::new();
    let flush_identifier = |identifier: &mut String, result: &mut String| {
        if identifier.is_empty() {
            return;
        }
        if let Some(replacement) = substitutions.get(identifier) {
            result.push_str(replacement);
        } else {
            result.push_str(identifier);
        }
        identifier.clear();
    };
    for character in type_name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            identifier.push(character);
        } else {
            flush_identifier(&mut identifier, &mut result);
            result.push(character);
        }
    }
    flush_identifier(&mut identifier, &mut result);
    result
}

fn builtin_supertype_match(expected: &str, actual: &str) -> Option<bool> {
    match expected {
        "Any" | "Object" => Some(true),
        "Number" => Some(matches!(
            actual,
            "Byte" | "Short" | "Int" | "Long" | "Float" | "Double"
        )),
        "CharSequence" => Some(actual == "String"),
        _ => None,
    }
}

pub(super) fn is_type_compatible(expected: &str, actual: &str) -> bool {
    if types_match(expected, actual) {
        return true;
    }
    let expected = normalize_type(expected);
    let actual = normalize_type(actual);
    match builtin_supertype_match(&expected, &actual) {
        Some(matches) => matches || !is_builtin_type(&actual),
        None if is_builtin_type(&expected) && is_builtin_type(&actual) => false,
        // Keep unproven source-type relations uncertain instead of selecting
        // a broader overload by discarding their candidate.
        None => true,
    }
}

pub(super) fn is_builtin_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "Any"
            | "Object"
            | "Nothing"
            | "Unit"
            | "Boolean"
            | "Byte"
            | "Short"
            | "Int"
            | "Long"
            | "UByte"
            | "UShort"
            | "UInt"
            | "ULong"
            | "Float"
            | "Double"
            | "Char"
            | "String"
            | "Number"
            | "CharSequence"
    )
}

pub(super) fn builtin_bound_result(actual: &str, bound: &str) -> Option<bool> {
    let actual = normalize_type(actual);
    let bound = normalize_type(bound);
    if types_match(&actual, &bound) {
        return Some(true);
    }
    if let Some(matches) = builtin_supertype_match(&bound, &actual) {
        return (matches || is_builtin_type(&actual)).then_some(matches);
    }
    match bound.as_str() {
        "String" | "Int" | "Long" | "Double" | "Float" | "Short" | "Byte" | "UByte" | "UShort"
        | "UInt" | "ULong" | "Boolean" | "Char" => Some(false),
        _ => None,
    }
}

pub(super) fn normalize_type(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('?')
        .split('<')
        .next()
        .unwrap_or(value)
        .replace("kotlin.", "")
}

pub(super) fn types_match(left: &str, right: &str) -> bool {
    let left = normalize_type(left);
    let right = normalize_type(right);
    left == right
}

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_arguments.rs"]
mod tests;
