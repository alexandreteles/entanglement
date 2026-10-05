use std::process::Command;

use serde_json::Value;

fn analyze(source: &str) -> Value {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("scope.rs");
    std::fs::write(&path, source).expect("write Rust source");
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "file"])
        .arg(path)
        .output()
        .expect("run entanglement");
    assert!(
        output.status.success(),
        "analysis failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

#[test]
fn file_index_uses_aggregate_volume_nloc_and_cyclomatic_complexity() {
    let report = analyze(
        "fn branch(flag: bool) { if flag { let first = 1; } else {} }\nfn linear() { let second = 2; }\n",
    );
    let file = &report["files"][0];
    let functions = file["functions"].as_array().expect("function reports");
    let index = &file["maintainability_index"];
    let cyclomatic_total = functions
        .iter()
        .map(|function| function["cyclomatic_complexity"].as_u64().unwrap())
        .sum::<u64>();

    assert_eq!(index["volume"], file["halstead"]["volume"]);
    assert_eq!(index["nloc"], file["nloc"]);
    assert_eq!(index["cyclomatic_complexity"], cyclomatic_total);
    for function in functions {
        let index = &function["maintainability_index"];
        assert_eq!(index["volume"], function["halstead"]["volume"]);
        assert_eq!(index["nloc"], function["nloc"]);
        assert_eq!(
            index["cyclomatic_complexity"],
            function["cyclomatic_complexity"]
        );
    }

    let volume = index["volume"].as_f64().unwrap();
    let nloc = index["nloc"].as_u64().unwrap() as f64;
    let cc = index["cyclomatic_complexity"].as_u64().unwrap() as f64;
    let expected = ((171.0 - 5.2 * volume.max(1.0).ln() - 0.23 * cc - 16.2 * nloc.max(1.0).ln())
        * 100.0
        / 171.0)
        .clamp(0.0, 100.0);
    assert!((index["score"].as_f64().unwrap() - expected).abs() < 1e-10);
}

#[test]
fn existing_empty_file_has_a_measured_good_score() {
    let report = analyze("");
    let index = &report["files"][0]["maintainability_index"];

    assert_eq!(index["volume"], 0.0);
    assert_eq!(index["nloc"], 0);
    assert_eq!(index["cyclomatic_complexity"], 0);
    assert_eq!(index["score"], 100.0);
    assert_eq!(index["rating"], "green_good");
}
