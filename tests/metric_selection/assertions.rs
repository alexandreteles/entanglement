use serde_json::{Map, Value};

pub(super) fn project_metric(value: &Value, selected: &str) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| keep_field(key, selected))
                .map(|(key, value)| {
                    let value = if key == "maintainability_index" && selected == "mi" {
                        value.clone()
                    } else {
                        project_metric(value, selected)
                    };
                    (key.clone(), value)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| project_metric(value, selected))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn keep_field(key: &str, selected: &str) -> bool {
    match key {
        "nloc" => true,
        "cyclomatic_complexity"
        | "contributions"
        | "added_contributions"
        | "removed_contributions" => selected == "cc",
        "cyclomatic_density" => selected == "density",
        "cognitive_complexity"
        | "cognitive_contributions"
        | "added_cognitive_contributions"
        | "removed_cognitive_contributions" => selected == "cogc",
        "halstead" => selected == "halstead",
        "maintainability_index" | "maintainability_index_bands" => selected == "mi",
        _ => true,
    }
}

pub(super) fn function<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["files"]
        .as_array()
        .expect("file reports")
        .iter()
        .flat_map(|file| file["functions"].as_array().into_iter().flatten())
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

pub(super) fn has_recursion(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

pub(super) fn assert_json_selection(all: &Value, selected: &Value, metric: &str) {
    assert_eq!(project_metric(all, metric), *selected, "selection {metric}");
    let file = &selected["files"][0];
    let function = function(selected, "left");
    assert!(function["nloc"].is_number());
    for (field, owner) in [
        ("cyclomatic_complexity", "cc"),
        ("contributions", "cc"),
        ("cyclomatic_density", "density"),
        ("cognitive_complexity", "cogc"),
        ("cognitive_contributions", "cogc"),
        ("halstead", "halstead"),
        ("maintainability_index", "mi"),
    ] {
        assert_eq!(
            function.get(field).is_some(),
            metric == owner,
            "field {field} for selection {metric}"
        );
    }
    if metric == "mi" {
        assert!(file["maintainability_index"]["volume"].is_number());
        assert!(file["maintainability_index"]["cyclomatic_complexity"].is_number());
        assert!(function["maintainability_index"]["nloc"].is_number());
    }
}

fn has_metric_section(output: &str, metric: &str) -> bool {
    match metric {
        "nloc" => output.contains("NLOC"),
        "cc" => output.lines().any(|line| {
            let lower = line.to_ascii_lowercase();
            line.split_whitespace().any(|word| word == "CC")
                && !lower.contains("density")
                && !lower.contains("maintainability")
                && !lower.contains("inputs:")
                && !lower.contains("effects:")
        }),
        "density" => output.contains("density"),
        "cogc" => output.contains("CogC") || output.contains("Cognitive complexity"),
        "halstead" => output.contains("Halstead"),
        "mi" => output
            .to_ascii_lowercase()
            .contains("maintainability index"),
        _ => false,
    }
}

pub(super) fn assert_human_selection(output: &str, selected: &str) {
    assert!(has_metric_section(output, "nloc"), "NLOC is always shown");
    for metric in ["cc", "density", "cogc", "halstead", "mi"] {
        assert_eq!(
            has_metric_section(output, metric),
            selected == metric,
            "human section {metric} for selection {selected}:\n{output}"
        );
    }
}
