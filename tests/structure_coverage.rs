#[path = "../src/structure/delta.rs"]
mod delta;
#[path = "../src/structure/graph.rs"]
#[expect(
    dead_code,
    reason = "This test uses graph helpers without the analysis entry point."
)]
mod graph;
#[path = "../src/structure/model.rs"]
mod model;
mod support;
use support::{patch, report, run, write};

#[test]
fn candidate_parse_loss_is_not_a_cycle_resolution() {
    let root = tempfile::tempdir().unwrap();
    let before = "pub struct A { next: Box<B> }\npub struct B { next: Box<A> }\n";
    let after = "pub struct A { next: Box< }\n";
    write(&root.path().join("lib.rs"), before);
    let diff = root.path().join("change.diff");
    write(&diff, patch("lib.rs", Some(before), Some(after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    let previous = result["structure"]["before"]["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|graph| graph["granularity"] == "type" && graph["projection"] == "full")
        .unwrap();
    assert_eq!(
        previous["cycles"].as_array().unwrap().len(),
        1,
        "{previous:#}"
    );
    let events = result["structure"]["delta"]["events"].as_array().unwrap();
    assert!(events.iter().any(|event| event["kind"] == "coverage_lost"));
    assert!(
        !events.iter().any(|event| matches!(
            event["kind"].as_str(),
            Some("cycle_resolved" | "reach_removed" | "dependency_removed")
        )),
        "{events:#?}"
    );
}

#[test]
fn ignored_source_is_missing_coverage_in_snapshot_comparison() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("first.py"),
        "import second\nclass A: pass\n",
    );
    write(
        &root.path().join("second.py"),
        "import first\nclass B: pass\n",
    );
    let before = report(&run("repo", root.path(), None, Some("structure")));
    write(&root.path().join(".gitignore"), "second.py\n");
    let after = report(&run("repo", root.path(), None, Some("structure")));
    let before: model::StructureSnapshot =
        serde_json::from_value(before["structure"].clone()).unwrap();
    let after: model::StructureSnapshot =
        serde_json::from_value(after["structure"].clone()).unwrap();
    assert!(before.units.len() > after.units.len());
    let change = delta::compare(&before, &after);
    assert!(
        change
            .events
            .iter()
            .any(|event| event.kind == "coverage_lost")
    );
    assert!(!change.events.iter().any(|event| matches!(
        event.kind.as_str(),
        "cycle_resolved" | "reach_removed" | "dependency_removed"
    )));
}

#[test]
fn lost_parent_resolution_marks_untouched_child_effective_coverage() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("pyproject.toml"),
        "[project]\nname = 'fixture'\n",
    );
    let before = "class Base:\n    def method(self): pass\n";
    let after = "class Other:\n    def method(self): pass\n";
    write(&root.path().join("base.py"), before);
    write(
        &root.path().join("child.py"),
        "from base import Base\nclass Child(Base): pass\n",
    );
    let diff = root.path().join("change.diff");
    write(&diff, patch("base.py", Some(before), Some(after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    let events = result["structure"]["delta"]["events"].as_array().unwrap();
    let affects_child = |event: &&serde_json::Value| {
        event["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| unit.as_str().unwrap().ends_with(":Child"))
    };
    assert!(
        events
            .iter()
            .filter(affects_child)
            .any(|event| event["kind"] == "effective_coverage"),
        "{events:#?}"
    );
    assert!(
        !events
            .iter()
            .filter(affects_child)
            .any(|event| event["kind"] == "effective")
    );
}

#[test]
fn candidate_file_rename_uses_hashes_as_uncertain_identity_evidence() {
    let root = tempfile::tempdir().unwrap();
    let source = "pub struct Item;\n";
    write(&root.path().join("old.rs"), source);
    let diff = root.path().join("change.diff");
    write(
        &diff,
        format!(
            "{}{}",
            patch("old.rs", Some(source), None),
            patch("new.rs", None, Some(source))
        ),
    );
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    assert!(
        result["structure"]["delta"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "match_uncertain"
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().contains("old.rs"))
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().contains("new.rs")))
    );
}

#[test]
fn same_file_type_rename_is_uncertain_from_full_syntax_shape() {
    let root = tempfile::tempdir().unwrap();
    let before = "class First { method() { return 1; } }\n";
    let after = "class Second { method() { return 1; } }\n";
    write(&root.path().join("model.js"), before);
    let diff = root.path().join("change.diff");
    write(&diff, patch("model.js", Some(before), Some(after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    assert!(
        result["structure"]["delta"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "match_uncertain"
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().ends_with(":First"))
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().ends_with(":Second")))
    );
}

#[test]
fn moved_and_formatted_source_has_uncertain_structural_identity() {
    let root = tempfile::tempdir().unwrap();
    let before = "class Item { method() { return 1; } }\n";
    let after = "// Source moved.\nclass Item {\n method() {\n  return 1;\n }\n}\n";
    write(&root.path().join("old.js"), before);
    let diff = root.path().join("change.diff");
    write(
        &diff,
        format!(
            "{}{}",
            patch("old.js", Some(before), None),
            patch("new.js", None, Some(after))
        ),
    );
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    assert!(
        result["structure"]["delta"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "match_uncertain"
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().contains("old.js"))
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().contains("new.js")))
    );
}
