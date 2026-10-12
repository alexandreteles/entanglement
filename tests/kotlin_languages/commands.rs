use super::*;
use serde_json::{Map, Value};

const METRICS: &[&str] = &["nloc", "cc", "density", "cogc", "halstead", "mi"];

fn projection(value: &Value, metric: &str) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| keep_field(key, metric))
                .map(|(key, value)| {
                    let value = if key == "maintainability_index" && metric == "mi" {
                        value.clone()
                    } else {
                        projection(value, metric)
                    };
                    (key.clone(), value)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| projection(item, metric)).collect())
        }
        _ => value.clone(),
    }
}

fn keep_field(key: &str, metric: &str) -> bool {
    match key {
        "nloc" => true,
        "cyclomatic_complexity"
        | "contributions"
        | "added_contributions"
        | "removed_contributions" => metric == "cc",
        "cyclomatic_density" => metric == "density",
        "cognitive_complexity"
        | "cognitive_contributions"
        | "added_cognitive_contributions"
        | "removed_cognitive_contributions" => metric == "cogc",
        "halstead" => metric == "halstead",
        "maintainability_index" | "maintainability_index_bands" => metric == "mi",
        _ => true,
    }
}

#[test]
fn kotlin_metrics_project_consistently_for_file_repo_patch_and_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source_path = root.join("src/main/kotlin/App.kt");
    let before = "package app\nfun calculate(flag: Boolean) = if (flag) 1 else 0\n";
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\n",
    );
    write(
        &root.join("build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
    );
    write(&source_path, before);
    let diff = temp.path().join("change.diff");
    write(
        &diff,
        "--- a/src/main/kotlin/App.kt\n+++ b/src/main/kotlin/App.kt\n@@ -1,2 +1,2 @@\n package app\n-fun calculate(flag: Boolean) = if (flag) 1 else 0\n+fun calculate(flag: Boolean) = if (flag && !flag) 1 else 0\n",
    );

    let modes = [
        ("file", source_path.as_path()),
        ("repo", root.as_path()),
        ("patch", source_path.as_path()),
        ("candidate", root.as_path()),
    ];
    for (mode, path) in modes {
        let diff_path = matches!(mode, "patch" | "candidate").then_some(diff.as_path());
        let all = report(&run(mode, path, diff_path, Some("all")));
        for metric in METRICS {
            let selected = report(&run(mode, path, diff_path, Some(metric)));
            assert_eq!(
                projection(&all, metric),
                selected,
                "{metric} selection in Kotlin {mode} mode"
            );
        }
        assert!(
            file(&all, "App.kt")["functions"][0]["cyclomatic_complexity"].is_number(),
            "all metrics must include function CC in {mode} mode"
        );
        assert!(file(&all, "App.kt")["functions"][0]["halstead"].is_object());
        assert!(file(&all, "App.kt")["functions"][0]["maintainability_index"].is_object());
    }
}

#[test]
fn kotlin_file_selection_includes_gradle_kotlin_scripts() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.kt");
    write(
        &source,
        "fun selectedFromKotlin(flag: Boolean) = if (flag) \"yes\" else \"no\"\n",
    );
    let selected = report(&run("file", &source, None, Some("cc")));
    assert_eq!(selected["files"][0]["language"], "kotlin");
    assert!(
        selected["files"][0]["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "selectedFromKotlin")
    );

    let script = temp.path().join("build.gradle.kts");
    write(
        &script,
        concat!(
            "val selected = true\n",
            "if (selected && args.isNotEmpty()) println(\"Gradle script\")\n",
            "tasks.register(\"verify\") { doLast { if (selected) println(\"verified\") } }\n",
        ),
    );
    let result = report(&run("file", &script, None, Some("cc")));
    assert_eq!(result["files"][0]["language"], "kotlin");
    let functions = result["files"][0]["functions"].as_array().unwrap();
    let script_scope = functions
        .iter()
        .find(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("script@"))
        })
        .expect("script body scope");
    assert!(script_scope["cyclomatic_complexity"].as_u64().unwrap() >= 3);
    assert!(functions.iter().any(|item| {
        item["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("anonymous@"))
    }));
}
