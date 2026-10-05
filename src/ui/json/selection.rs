use crate::metrics::selection::{Metric, Selection};
use serde_json::Value;

pub(super) fn project(result: &mut Value, selection: Selection) {
    let Some(object) = result.as_object_mut() else {
        return;
    };
    if !selection.includes(Metric::Mi) {
        object.remove("maintainability_index_bands");
    }
    if let Some(files) = object.get_mut("files").and_then(Value::as_array_mut) {
        for file in files {
            project_file(file, selection);
        }
    }
    if let Some(patch) = object.get_mut("patch") {
        project_patch(patch, selection);
    }
}

fn project_patch(patch: &mut Value, selection: Selection) {
    let Some(object) = patch.as_object_mut() else {
        return;
    };
    let Some(files) = object.get_mut("files").and_then(Value::as_array_mut) else {
        return;
    };
    for file in files {
        let Some(object) = file.as_object_mut() else {
            continue;
        };
        for side in ["before", "after"] {
            if let Some(value) = object.get_mut(side) {
                project_file(value, selection);
            }
        }
        remove_unselected(object, "halstead", selection.includes(Metric::Halstead));
        remove_unselected(
            object,
            "maintainability_index",
            selection.includes(Metric::Mi),
        );
        if let Some(functions) = object.get_mut("functions").and_then(Value::as_array_mut) {
            for function in functions {
                project_function(function, selection);
            }
        }
    }
}

fn project_file(file: &mut Value, selection: Selection) {
    let Some(object) = file.as_object_mut() else {
        return;
    };
    remove_unselected(object, "halstead", selection.includes(Metric::Halstead));
    remove_unselected(
        object,
        "maintainability_index",
        selection.includes(Metric::Mi),
    );
    if let Some(functions) = object.get_mut("functions").and_then(Value::as_array_mut) {
        for function in functions {
            project_function(function, selection);
        }
    }
}

fn project_function(function: &mut Value, selection: Selection) {
    let Some(object) = function.as_object_mut() else {
        return;
    };
    remove_unselected(object, "halstead", selection.includes(Metric::Halstead));
    remove_unselected(
        object,
        "maintainability_index",
        selection.includes(Metric::Mi),
    );
    remove_unselected(
        object,
        "cyclomatic_complexity",
        selection.includes(Metric::Cc),
    );
    remove_unselected(
        object,
        "cyclomatic_density",
        selection.includes(Metric::Density),
    );
    let include_cc = selection.includes(Metric::Cc);
    remove_unselected(object, "contributions", include_cc);
    remove_unselected(object, "added_contributions", include_cc);
    remove_unselected(object, "removed_contributions", include_cc);
    remove_unselected(
        object,
        "cognitive_complexity",
        selection.includes(Metric::Cogc),
    );
    let include_cognitive = selection.includes(Metric::Cogc);
    remove_unselected(object, "cognitive_contributions", include_cognitive);
    remove_unselected(object, "added_cognitive_contributions", include_cognitive);
    remove_unselected(object, "removed_cognitive_contributions", include_cognitive);
}

fn remove_unselected(object: &mut serde_json::Map<String, Value>, field: &str, selected: bool) {
    if !selected {
        object.remove(field);
    }
}
