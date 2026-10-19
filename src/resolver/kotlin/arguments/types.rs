use std::collections::BTreeMap;

pub(in crate::resolver::kotlin) fn function_type_receiver(function_type: &str) -> Option<&str> {
    let (signature, _) = function_type.split_once("->")?;
    let open = signature.find(".(")?;
    let receiver = signature[..open].trim_end_matches('.');
    (!receiver.is_empty()).then_some(receiver)
}

pub(in crate::resolver::kotlin) fn function_type_parameters(
    function_type: &str,
) -> Option<Vec<String>> {
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
    let expected_types = parameter_type_names(parameter_text);
    expected_types.len() == actual.len()
        && expected_types.iter().zip(actual).all(|(expected, actual)| {
            actual
                .as_deref()
                .is_none_or(|actual| types_match(expected, actual))
        })
}

pub(in crate::resolver::kotlin) fn substitute_type_name(
    type_name: &str,
    substitutions: &BTreeMap<String, String>,
) -> String {
    let mut result = String::with_capacity(type_name.len());
    for part in type_name.split_inclusive(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        let identifier = part.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
        result.push_str(
            substitutions
                .get(identifier)
                .filter(|_| !identifier.is_empty())
                .map_or(identifier, String::as_str),
        );
        result.push_str(&part[identifier.len()..]);
    }
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

pub(in crate::resolver::kotlin) fn is_type_compatible(expected: &str, actual: &str) -> bool {
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

pub(in crate::resolver::kotlin) fn is_builtin_type(type_name: &str) -> bool {
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

pub(in crate::resolver::kotlin) fn builtin_bound_result(actual: &str, bound: &str) -> Option<bool> {
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

pub(in crate::resolver::kotlin) fn normalize_type(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('?')
        .split('<')
        .next()
        .unwrap_or(value)
        .replace("kotlin.", "")
}

pub(in crate::resolver::kotlin) fn types_match(left: &str, right: &str) -> bool {
    let left = normalize_type(left);
    let right = normalize_type(right);
    left == right
}
