use super::*;

#[test]
fn formatting_only_candidate_does_not_change_effective_members() {
    let root = tempfile::tempdir().unwrap();
    let before = "class Base:\n    def method(self): pass\nclass Item(Base): pass\n";
    let after = format!("\n\n{before}");
    write(&root.path().join("model.py"), before);
    let diff = root.path().join("change.diff");
    write(&diff, patch("model.py", Some(before), Some(&after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    assert!(
        !result["structure"]["delta"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "effective"),
        "{}",
        result["structure"]["delta"]
    );
}
