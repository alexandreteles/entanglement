mod function_types;
mod generic;
mod matching;
mod types;

pub(super) use function_types::{function_type_parameters, function_type_receiver};
pub(super) use generic::generic_type_substitutions;
pub(super) use matching::{arguments_match, lambda_parameter_for_argument};
pub(super) use types::{normalize_type, substitute_type_name};
