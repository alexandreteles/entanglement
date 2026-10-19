use crate::metrics::selection::{Metric, Selection};
use serde_json::Value;

/// Remove unselected fields only from report objects, leaving metric payloads intact.
pub(super) fn project(result: &mut Value, selection: Selection) {
    match result {
        Value::Array(items) => items.iter_mut().for_each(|item| project(item, selection)),
        Value::Object(object) => {
            object.retain(|field, _| {
                let metric = match field.as_str() {
                    "halstead" => Metric::Halstead,
                    "maintainability_index" | "maintainability_index_bands" => Metric::Mi,
                    "cyclomatic_density" => Metric::Density,
                    "cyclomatic_complexity"
                    | "contributions"
                    | "added_contributions"
                    | "removed_contributions" => Metric::Cc,
                    "cognitive_complexity"
                    | "cognitive_contributions"
                    | "added_cognitive_contributions"
                    | "removed_cognitive_contributions" => Metric::Cogc,
                    _ => return true,
                };
                selection.includes(metric)
            });
            for field in ["files", "functions", "before", "after", "patch"] {
                if let Some(value) = object.get_mut(field) {
                    project(value, selection);
                }
            }
        }
        _ => {}
    }
}
