use super::*;

#[test]
fn patch_and_candidate_reports_match_fresh_python_analysis() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    write(
        &root.join("pyproject.toml"),
        "[project]\nname = \"fixture\"\n",
    );
    let target = root.join("src/main.py");
    let before = "def choose(flag):\n    if flag:\n        return 1\n    return 0\n";
    let after = "def choose(flag):\n    if flag and not flag:\n        return 1\n    return 0\n";
    write(&target, before);
    write(&root.join("src/helper.py"), "def helper(): return 1\n");

    let file_report = report(&run("file", &target, None, Some("cc,cogc")));
    let repo_report = report(&run("repo", &root, None, Some("cc,cogc")));
    assert_eq!(
        file(&file_report, "/main.py")["functions"],
        file(&repo_report, "/main.py")["functions"]
    );
    assert!(file(&repo_report, "/main.py")["halstead"].is_null());

    let diff = temp.path().join("change.diff");
    write(
        &diff,
        "--- a/src/main.py\n+++ b/src/main.py\n@@ -1,4 +1,4 @@\n def choose(flag):\n-    if flag:\n+    if flag and not flag:\n         return 1\n     return 0\n",
    );
    let patched = report(&run("patch", &target, Some(&diff), Some("cc,cogc")));
    let candidate = report(&run("candidate", &target, Some(&diff), Some("cc,cogc")));
    write(&target, after);
    let fresh = report(&run("repo", &root, None, Some("cc,cogc")));
    assert_eq!(file(&candidate, "/main.py"), file(&fresh, "/main.py"));
    assert_eq!(
        function(&patched["patch"]["files"][0]["after"], "choose")["cyclomatic_complexity"],
        function(file(&fresh, "/main.py"), "choose")["cyclomatic_complexity"]
    );
    assert!(candidate["files"][0].get("maintainability_index").is_none());
}

#[test]
fn malformed_python_recovers_and_invalid_utf8_is_reported_cleanly() {
    let temp = tempfile::tempdir().unwrap();
    let malformed = temp.path().join("malformed.py");
    write(
        &malformed,
        "def broken(:\n    pass\n\ndef intact():\n    return 1\n",
    );
    let parsed = report(&run("file", &malformed, None, Some("cc")));
    function(&parsed["files"][0], "intact");

    let invalid = temp.path().join("invalid.py");
    write(&invalid, b"def broken():\n    return \xff\n");
    let recovered = report(&run("file", &invalid, None, Some("cc")));
    assert_eq!(recovered["files"][0]["language"], "python");
}
