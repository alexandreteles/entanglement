//! Conservative Kotlin call argument binding and type compatibility.

use crate::graph::{CallArgumentFact, CallableParameter, CallableSignature};
use std::collections::{BTreeMap, HashMap, HashSet};

mod types;
pub(super) use types::*;

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

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_arguments.rs"]
mod tests;
