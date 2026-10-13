use super::types::types_match;

pub(in crate::resolver::kotlin) fn function_type_receiver(function_type: &str) -> Option<&str> {
    let arrow = function_type.find("->")?;
    let signature = &function_type[..arrow];
    let open = signature.find(".(")?;
    let receiver = signature[..open].trim_end_matches('.');
    (!receiver.is_empty()).then_some(receiver)
}

pub(in crate::resolver::kotlin) fn function_type_parameters(
    function_type: &str,
) -> Option<Vec<String>> {
    let arrow = function_type.find("->")?;
    let signature = function_type[..arrow].trim();
    if signature.contains(".(") {
        return None;
    }
    let open = signature.find('(')?;
    let close = signature.rfind(')')?;
    if close < open {
        return None;
    }
    let parameters = signature[open + 1..close].trim();
    if parameters.is_empty() {
        return Some(Vec::new());
    }
    Some(
        parameters
            .split(',')
            .map(|parameter| {
                parameter
                    .rsplit_once(':')
                    .map_or(parameter, |(_, ty)| ty)
                    .trim()
                    .to_owned()
            })
            .collect(),
    )
}

pub(in crate::resolver::kotlin) fn lambda_parameters_match(
    expected: &str,
    actual: &[Option<String>],
) -> bool {
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
    let expected_types = if parameter_text.is_empty() {
        Vec::new()
    } else {
        parameter_text
            .split(',')
            .map(|part| {
                part.rsplit_once(':')
                    .map_or(part, |(_, ty)| ty)
                    .trim()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    expected_types.len() == actual.len()
        && expected_types.iter().zip(actual).all(|(expected, actual)| {
            actual
                .as_deref()
                .is_none_or(|actual| types_match(expected, actual))
        })
}
