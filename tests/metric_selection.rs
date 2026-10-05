#[path = "metric_selection/assertions.rs"]
mod assertions;
#[path = "metric_selection/fixture.rs"]
mod fixture;

use assertions::*;
use fixture::*;

const METRICS: &[&str] = &["nloc", "cc", "density", "cogc", "halstead", "mi"];

#[test]
fn every_metric_projects_consistently_in_json_across_all_commands() {
    let project = project();
    for mode in modes(&project) {
        let all = json(&run(&mode, "json", &["all"], true));
        for metric in METRICS {
            let selected = json(&run(&mode, "json", &[*metric], false));
            assert_json_selection(&all, &selected, metric);
        }
    }
}

#[test]
fn human_reports_show_nloc_and_only_the_selected_metric_for_each_command() {
    let project = project();
    for mode in modes(&project) {
        for metric in METRICS {
            let output = run(&mode, "human", &[*metric], true);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).expect("human report is UTF-8");
            assert_human_selection(&output, metric);
        }
    }
}

#[test]
fn defaults_lists_repeats_global_positions_and_aliases_are_compatible() {
    let project = project();
    let repo = modes(&project)[1].path;
    let mode = Mode {
        command: "repo",
        path: repo,
        diff: None,
    };
    let default = run(&mode, "json", &[], true);
    let all = run(&mode, "json", &["all"], false);
    assert_eq!(default.stdout, all.stdout);

    let listed = json(&run(&mode, "json", &["cc,density", "mi,cogc"], true));
    let repeated = json(&run(&mode, "json", &["cc", "density", "mi", "cogc"], false));
    assert_eq!(listed, repeated);
    let before = json(&run(&mode, "json", &["cc"], true));
    let after = json(&run(&mode, "json", &["cc"], false));
    assert_eq!(before, after);

    for (alias, canonical) in [
        ("cyclomatic", "cc"),
        ("cyclomatic-complexity", "cc"),
        ("cyclomatic-density", "density"),
        ("cognitive", "cogc"),
        ("cognitive-complexity", "cogc"),
        ("maintainability", "mi"),
        ("maintainability-index", "mi"),
    ] {
        assert_eq!(
            json(&run(&mode, "json", &[alias], true)),
            json(&run(&mode, "json", &[canonical], true)),
            "alias {alias}"
        );
    }
    for invalid in ["unknown", "", "cc,,mi"] {
        let output = run(&mode, "json", &[invalid], true);
        assert!(
            !output.status.success(),
            "accepted invalid selection {invalid:?}"
        );
    }
}

#[test]
fn cogc_only_candidate_recomputes_recursion_for_an_unchanged_peer() {
    let project = project();
    let modes = modes(&project);
    let before = json(&run(&modes[1], "json", &["all"], true));
    assert!(has_recursion(function(&before, "right")));
    let all = json(&run(&modes[3], "json", &["all"], true));
    let selected = json(&run(&modes[3], "json", &["cogc"], false));
    assert_eq!(project_metric(&all, "cogc"), selected);
    assert!(
        !has_recursion(function(&selected, "right")),
        "breaking the cycle must remove recursion from the unchanged peer"
    );
}

#[test]
fn mi_selection_preserves_missing_file_and_function_sides() {
    let project = project();
    let diff = project.root.join("add-delete.diff");
    std::fs::write(
        &diff,
        "--- /dev/null\n+++ b/src/fresh.rs\n@@ -0,0 +1,3 @@\n+pub fn fresh(flag: bool) -> bool {\n+    if flag { true } else { false }\n+}\n--- a/src/old.rs\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-pub fn removed(flag: bool) -> bool { if flag { true } else { false } }\n",
    )
    .expect("write add/delete patch");
    let mode = Mode {
        command: "candidate",
        path: &project.root,
        diff: Some(&diff),
    };
    let report = json(&run(&mode, "json", &["mi"], false));
    for (name, function_name, added) in [("fresh.rs", "fresh", true), ("old.rs", "removed", false)]
    {
        let file = report["patch"]["files"]
            .as_array()
            .expect("patch files")
            .iter()
            .find(|file| {
                file["path"]
                    .as_str()
                    .is_some_and(|path| path.contains(name))
            })
            .unwrap_or_else(|| panic!("missing patch file {name}"));
        assert_eq!(file["before"].is_null(), added);
        assert_eq!(file["after"].is_null(), !added);
        assert!(file.get("halstead").is_none());
        assert_eq!(file["maintainability_index"]["before"].is_null(), added);
        assert_eq!(file["maintainability_index"]["after"].is_null(), !added);
        assert!(file["maintainability_index"]["score"].is_null());
        let function = file["functions"]
            .as_array()
            .expect("function deltas")
            .iter()
            .find(|function| function["name"] == function_name)
            .unwrap_or_else(|| panic!("missing function delta {function_name}"));
        assert!(function.get("cyclomatic_complexity").is_none());
        assert!(function.get("halstead").is_none());
        assert_eq!(function["maintainability_index"]["before"].is_null(), added);
        assert_eq!(function["maintainability_index"]["after"].is_null(), !added);
        assert!(function["maintainability_index"]["score"].is_null());
    }
}
