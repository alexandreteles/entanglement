mod support;

use support::{patch, report, run, write};

#[test]
fn structural_selection_reports_scope_and_keeps_metric_selection() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lib.rs");
    write(&path, "pub struct Item;\n");
    let local = report(&run("file", &path, None, Some("structure")));
    assert_eq!(local["structure"]["scope"], "local");
    assert!(local["files"][0].get("maintainability_index").is_none());
    let repo = report(&run("repo", root.path(), None, Some("structure")));
    assert_eq!(repo["structure"]["scope"], "repo");
    assert_eq!(repo["structure"]["graphs"].as_array().unwrap().len(), 4);
    let metrics = report(&run("repo", root.path(), None, Some("nloc,cc")));
    assert!(metrics.get("structure").is_none());
}

#[test]
fn candidate_reports_complete_graphs_and_a_structural_delta() {
    let root = tempfile::tempdir().unwrap();
    let before = "pub struct Item;\n";
    let after = "pub struct Item;\npub struct Other;\n";
    write(&root.path().join("lib.rs"), before);
    let diff = root.path().join("change.diff");
    write(&diff, patch("lib.rs", Some(before), Some(after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    assert_eq!(result["structure"]["scope"], "repo");
    assert!(result["structure"]["delta"]["events"].is_array());
    assert!(
        result["structure"]["delta"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "effective_added")
    );
    assert!(result["structure"]["before"].is_object());
    let local = report(&run(
        "patch",
        &root.path().join("lib.rs"),
        Some(&diff),
        Some("structure"),
    ));
    assert_eq!(local["structure"]["scope"], "local");
}

#[test]
fn structural_capture_preserves_existing_metrics_and_resolution() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("lib.rs"),
        "pub trait Task { fn run(&self); }\npub struct Worker;\nimpl Task for Worker { fn run(&self) { first(); } }\nfn first() { second(); }\nfn second() { first(); }\n",
    );
    let metrics = report(&run("repo", root.path(), None, Some("nloc,cc,cogc,mi")));
    let mut combined = report(&run(
        "repo",
        root.path(),
        None,
        Some("nloc,cc,cogc,mi,structure"),
    ));
    combined.as_object_mut().unwrap().remove("structure");
    assert_eq!(combined, metrics);
}

#[test]
fn graph_reports_match_the_deterministic_chain_fixture() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structural_chain");
    let result = report(&run("repo", &root, None, Some("structure")));
    let graphs: Vec<_> = result["structure"]["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|graph| {
            serde_json::json!({
                "granularity": graph["granularity"], "projection": graph["projection"],
                "units": graph["units"].as_array().unwrap().len(),
                "edges": graph["edges"].as_array().unwrap().len(),
                "cycles": graph["cycles"].as_array().unwrap().len(), "pc": graph["pc"],
            })
        })
        .collect();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/structural_chain/graphs.json")).unwrap();
    assert_eq!(serde_json::json!(graphs), expected);
    let full = &result["structure"]["graphs"][1];
    assert_eq!(
        full["impacts"],
        serde_json::json!([
            {"unit":0,"dependencies":[1,2],"dependents":[],"predecessors":[[0,0],[1,0],[2,1]]},
            {"unit":1,"dependencies":[2],"dependents":[0],"predecessors":[[1,1],[2,1]]},
            {"unit":2,"dependencies":[],"dependents":[0,1],"predecessors":[[2,2]]}
        ])
    );
    let reached: Vec<_> = full["impacts"][0]["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|index| {
            full["units"][index.as_u64().unwrap() as usize]
                .as_str()
                .unwrap()
        })
        .collect();
    assert!(reached[0].ends_with("b.js") && reached[1].ends_with("c.js"));
}

#[test]
fn human_reports_effective_member_origins() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("lib.rs"),
        "pub trait Base { fn execute(&self); }\npub trait Child: Base {}\n",
    );
    support::assert_human(
        "repo",
        root.path(),
        None,
        &[
            "Structure: repo",
            "PC",
            "Effective declarations:",
            "execute req <-",
        ],
    );
}
