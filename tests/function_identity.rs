use std::process::Command;

use serde_json::Value;

fn candidate(before: &str, after: &str, metrics: &str) -> Value {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("main.js");
    std::fs::write(&file, before).unwrap();
    std::fs::write(temp.path().join("package.json"), "{}").unwrap();
    let diff = diffy::create_patch(before, after)
        .to_string()
        .replacen("--- original", "--- a/main.js", 1)
        .replacen("+++ modified", "+++ b/main.js", 1);
    let diff_path = temp.path().join("change.diff");
    std::fs::write(&diff_path, diff).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "--metrics", metrics, "candidate"])
        .arg(temp.path())
        .arg("--diff")
        .arg(diff_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn deltas(report: &Value) -> &[Value] {
    report["patch"]["files"][0]["functions"].as_array().unwrap()
}

#[test]
fn anonymous_scopes_keep_identity_when_their_display_line_changes() {
    let before = "function outer() {\n  first(() => { if (ready) work(); });\n}\n";
    let after = format!("// a comment\n{before}");
    for metrics in ["all", "cc", "nloc"] {
        let report = candidate(before, &after, metrics);
        assert_eq!(deltas(&report).len(), 2);
        for delta in deltas(&report) {
            assert!(!delta["before_range"].is_null());
            assert!(!delta["after_range"].is_null());
            assert_eq!(delta["nloc"]["delta"], 0);
            if metrics != "nloc" {
                assert_eq!(delta["cyclomatic_complexity"]["delta"], 0);
            }
        }
    }
}

#[test]
fn adding_a_same_line_lambda_preserves_the_unchanged_siblings() {
    let before = "function outer() { first(() => a()); second(() => b()); }\n";
    let after = "function outer() { added(() => c()); first(() => a()); second(() => b()); }\n";
    let report = candidate(before, after, "all");
    assert_eq!(deltas(&report).len(), 4);
    assert_eq!(
        deltas(&report)
            .iter()
            .filter(|delta| delta["before_range"].is_null())
            .count(),
        1
    );
    assert_eq!(
        deltas(&report)
            .iter()
            .filter(|delta| delta["after_range"].is_null())
            .count(),
        0
    );
}

#[test]
fn replacing_a_callback_body_uses_its_unique_call_site() {
    let before = "function outer() { first(() => a()); second(() => b()); }\n";
    let after = "function outer() { first(() => { if (ready) a(); }); second(() => b()); }\n";
    let report = candidate(before, after, "all");
    assert_eq!(deltas(&report).len(), 3);
    assert!(
        deltas(&report)
            .iter()
            .all(|delta| !delta["before_range"].is_null() && !delta["after_range"].is_null())
    );
    assert_eq!(
        deltas(&report)
            .iter()
            .filter(|delta| delta["cyclomatic_complexity"]["delta"] == 1)
            .count(),
        1
    );
}

#[test]
fn changing_one_repeated_call_site_preserves_its_proven_sibling() {
    let before = "function outer() { call(() => a()); call(() => b()); }\n";
    let after =
        "// shift\nfunction outer() { call(() => { if (ready) a(); }); call(() => b()); }\n";
    let report = candidate(before, after, "all");
    assert_eq!(deltas(&report).len(), 3);
    assert!(
        deltas(&report)
            .iter()
            .all(|delta| !delta["before_range"].is_null() && !delta["after_range"].is_null())
    );
}

#[test]
fn identical_callbacks_in_different_owners_do_not_pair_across_owners() {
    let before = "function first() { call(() => a()); }\nfunction second() { call(() => a()); }\n";
    let after = "function first() {}\nfunction second() { call(() => a()); }\n";
    let report = candidate(before, after, "nloc");
    let removed = deltas(&report)
        .iter()
        .filter(|delta| delta["after_range"].is_null())
        .collect::<Vec<_>>();
    assert_eq!(removed.len(), 1);
    assert!(
        removed[0]["name"]
            .as_str()
            .unwrap()
            .starts_with("anonymous@")
    );
}
