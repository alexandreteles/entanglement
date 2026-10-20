mod support;
use support::report;

use serde_json::Value;

fn function<'a>(functions: &'a Value, name: &str) -> &'a Value {
    functions
        .as_array()
        .expect("function reports")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn range(function: &Value) -> Value {
    serde_json::json!({
        "start_byte": function["start_byte"],
        "end_byte": function["end_byte"],
    })
}

#[test]
fn function_deltas_include_matched_before_and_after_ranges() {
    let before = "pub fn alpha() {}\npub fn beta() {}\npub fn gone() {}\n";
    let (_temp, root, _) = support::rust_project(before);
    let diff = root.join("change.diff");
    std::fs::write(
        &diff,
        concat!(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n",
            "+pub fn added() {}\n",
            " pub fn alpha() {}\n",
            " pub fn beta() {}\n",
            "-pub fn gone() {}\n",
        ),
    )
    .expect("write function patch");

    let result = report(&support::run("candidate", &root, Some(&diff), None));
    let file_delta = &result["patch"]["files"][0];
    let old_functions = &file_delta["before"]["functions"];
    let new_functions = &file_delta["after"]["functions"];
    let deltas = &file_delta["functions"];

    for name in ["alpha", "beta"] {
        let delta = function(deltas, name);
        assert_eq!(delta["before_range"], range(function(old_functions, name)));
        assert_eq!(delta["after_range"], range(function(new_functions, name)));
    }

    let added = function(deltas, "added");
    assert!(added.get("before_range").is_some());
    assert!(added["before_range"].is_null());
    assert_eq!(
        added["after_range"],
        range(function(new_functions, "added"))
    );

    let gone = function(deltas, "gone");
    assert_eq!(gone["before_range"], range(function(old_functions, "gone")));
    assert!(gone.get("after_range").is_some());
    assert!(gone["after_range"].is_null());

    let filtered = report(&support::run("candidate", &root, Some(&diff), Some("cc")));
    let filtered_delta = function(&filtered["patch"]["files"][0]["functions"], "beta");
    assert_eq!(
        filtered_delta["before_range"],
        function(deltas, "beta")["before_range"]
    );
    assert_eq!(
        filtered_delta["after_range"],
        function(deltas, "beta")["after_range"]
    );
}
