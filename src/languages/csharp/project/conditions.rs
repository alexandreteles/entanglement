//! Evaluate the MSBuild property and condition forms used for source membership.
use std::collections::BTreeMap;
use std::path::Path;

pub(super) type Properties = BTreeMap<String, String>;

pub(super) fn expand(input: &str, properties: &Properties) -> Option<String> {
    let mut output = String::new();
    let mut rest = input;
    while let Some((prefix, expression)) = rest.split_once("$(") {
        output.push_str(prefix);
        let close = expression
            .char_indices()
            .scan(0usize, |depth, (i, ch)| {
                let end = ch == ')' && *depth == 0;
                match ch {
                    '(' => *depth += 1,
                    ')' => *depth = depth.saturating_sub(1),
                    _ => {}
                }
                Some((i, end))
            })
            .find(|(_, end)| *end)?
            .0;
        let key = &expression[..close];
        if let Some((property, method)) = key.split_once('.') {
            let (method, argument) = method.split_once('(')?;
            let argument = argument.strip_suffix(')')?.trim_matches(['\'', '"']);
            let value = properties
                .get(&property.to_ascii_lowercase())
                .map(String::as_str)
                .unwrap_or("");
            let matches = match method.to_ascii_lowercase().as_str() {
                "contains" => value.contains(argument),
                "startswith" => value.starts_with(argument),
                "endswith" => value.ends_with(argument),
                _ => return None,
            };
            output.push_str(if matches { "true" } else { "false" });
        } else {
            output.push_str(
                properties
                    .get(&key.to_ascii_lowercase())
                    .map(String::as_str)
                    .unwrap_or(""),
            );
        }
        rest = &expression[close + 1..];
    }
    output.push_str(rest);
    Some(output)
}

pub(super) fn condition(
    input: Option<&str>,
    properties: &Properties,
    directory: &Path,
    snapshot: &crate::snapshot::Snapshot,
) -> Option<bool> {
    let Some(input) = input else {
        return Some(true);
    };
    let expanded = expand(input, properties)?;
    evaluate(expanded.trim(), directory, snapshot)
}

fn evaluate(input: &str, directory: &Path, snapshot: &crate::snapshot::Snapshot) -> Option<bool> {
    let lower = input.to_ascii_lowercase();
    for (operator, disjunction) in [(" or ", true), (" and ", false)] {
        if let Some(index) = split_operator(&lower, operator) {
            let left = evaluate(input[..index].trim(), directory, snapshot);
            let right = evaluate(input[index + operator.len()..].trim(), directory, snapshot);
            if left == Some(disjunction) || right == Some(disjunction) {
                return Some(disjunction);
            }
            return left.and(right);
        }
    }
    if input.starts_with('(') && input.ends_with(')') {
        return evaluate(input[1..input.len() - 1].trim(), directory, snapshot);
    }
    if let Some(inner) = input.strip_prefix('!') {
        return evaluate(inner.trim(), directory, snapshot).map(|value| !value);
    }
    for (operator, equal) in [("==", true), ("!=", false)] {
        if let Some(i) = split_operator(input, operator) {
            let unquote = |s: &str| s.trim().trim_matches(['\'', '"']).to_ascii_lowercase();
            return Some((unquote(&input[..i]) == unquote(&input[i + 2..])) == equal);
        }
    }
    if lower.starts_with("exists(") && input.ends_with(')') {
        let path = directory.join(
            input[7..input.len() - 1]
                .trim()
                .trim_matches(['\'', '"'])
                .replace('\\', "/"),
        );
        return Some(path.is_dir() || snapshot.read(&path).ok()?.is_some());
    }
    match lower.trim_matches(['\'', '"']) {
        "true" => Some(true),
        "false" | "" => Some(false),
        _ => None,
    }
}

fn split_operator(input: &str, operator: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    for (index, ch) in input.char_indices() {
        if let Some(delimiter) = quote {
            if delimiter == ch {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && input[index..].starts_with(operator) {
            return Some(index);
        }
    }
    None
}
