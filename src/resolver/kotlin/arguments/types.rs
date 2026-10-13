use std::collections::BTreeMap;

pub(in crate::resolver::kotlin) fn substitute_type_name(
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

pub(in crate::resolver::kotlin) fn is_type_compatible(expected: &str, actual: &str) -> bool {
    if types_match(expected, actual) {
        return true;
    }
    let expected = normalize_type(expected);
    let actual = normalize_type(actual);
    match expected.as_str() {
        "Any" | "Object" => true,
        "Number" => {
            matches!(
                actual.as_str(),
                "Byte" | "Short" | "Int" | "Long" | "Float" | "Double"
            ) || !is_builtin_type(&actual)
        }
        "CharSequence" => actual == "String" || !is_builtin_type(&actual),
        _ if is_builtin_type(&expected) && is_builtin_type(&actual) => false,
        // User-defined source types may be related through inheritance or an
        // alias. If that relation is not proven here, keep the candidate
        // uncertain instead of discarding it and selecting a broader overload.
        _ => true,
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
    if types_match(&actual, &bound) || bound == "Any" || bound == "Object" {
        return Some(true);
    }
    match bound.as_str() {
        "Number" => {
            if matches!(
                actual.as_str(),
                "Byte" | "Short" | "Int" | "Long" | "Float" | "Double"
            ) {
                Some(true)
            } else if is_builtin_type(&actual) {
                Some(false)
            } else {
                None
            }
        }
        "CharSequence" => {
            if actual == "String" {
                Some(true)
            } else if is_builtin_type(&actual) {
                Some(false)
            } else {
                None
            }
        }
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
