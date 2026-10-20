use support::write;
mod support;
use support::{file, function, recursive};

use serde_json::Value;

fn refs(file: &Value) -> &[Value] {
    file["resolution"].as_array().map_or(&[], Vec::as_slice)
}

fn reference<'a>(file: &'a Value, path: &[&str]) -> &'a Value {
    refs(file)
        .iter()
        .find(|item| {
            item["path"].as_array().is_some_and(|parts| {
                parts.len() == path.len()
                    && path
                        .iter()
                        .zip(parts)
                        .all(|(expected, actual)| actual.as_str() == Some(*expected))
            })
        })
        .unwrap_or_else(|| panic!("missing reference path {path:?}"))
}

fn exact_target(file: &Value, path: &[&str], suffix: &str, symbol: &str) {
    let resolution = &reference(file, path)["resolution"];
    assert_eq!(resolution["status"], "exact", "reference {path:?}");
    assert_eq!(resolution["symbols"]["name"], symbol);
    assert!(
        resolution["symbols"]["file"]
            .as_str()
            .is_some_and(|target| target.ends_with(suffix))
    );
}

#[path = "python_resolution/modules.rs"]
mod modules;
#[path = "python_resolution/scopes.rs"]
mod scopes;
