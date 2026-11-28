mod support;

use support::{file, function, only_file, patch, report, run, write};

#[test]
fn native_scopes_count_decisions_and_skip_documentation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Rates.cs");
    let source = "/// <summary>Rates</summary>\ninterface IRate { int Read(); }\nclass Rates {\n  int Price => 1;\n  int Read(int n) {\n    int Local(int value) => value > 0 ? value : 0;\n    if (n > 2 && n < 5) return Local(n);\n    return n switch { 1 => 2, _ => 0 };\n  }\n}\n";
    write(&path, source);
    let output = report(&run("file", &path, None, Some("all")));
    let measured = only_file(&output);
    assert_eq!(measured["language"], "c-sharp");
    assert_eq!(measured["nloc"], 9);
    assert_eq!(measured["functions"].as_array().unwrap().len(), 3);
    assert_eq!(function(measured, "Read")["cyclomatic_complexity"], 4);
    assert_eq!(function(measured, "Local")["cyclomatic_complexity"], 2);
    assert_eq!(function(measured, "Price")["cyclomatic_complexity"], 1);
    assert!(measured["maintainability_index"]["score"].is_number());
}

#[test]
fn candidate_and_patch_match_a_fresh_source_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Top.cs");
    let before = "int Score(int n) => n;\nConsole.WriteLine(Score(1));\n";
    let after = before.replace("=> n;", "=> n > 0 ? n : 0;");
    let diff = dir.path().join("edit.diff");
    write(&path, before);
    write(&diff, patch("Top.cs", Some(before), Some(&after)));
    let local = report(&run("patch", &path, Some(&diff), Some("cc,mi")));
    let candidate = report(&run("candidate", dir.path(), Some(&diff), Some("cc,mi")));
    assert_eq!(file(&local, "Top.cs"), file(&candidate, "Top.cs"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    write(&path, after);
    let fresh = report(&run("repo", dir.path(), None, Some("cc,mi")));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn constant_fields_do_not_create_executable_functions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Fields.cs");
    write(
        &path,
        "class Fields { public const int Capacity = 12; int count = Start(); static int Start() => 0; }",
    );
    let output = report(&run("file", &path, None, Some("cc")));
    let code = only_file(&output);
    assert_eq!(code["functions"].as_array().unwrap().len(), 2);
    assert_eq!(
        function(code, "count initializer")["cyclomatic_complexity"],
        1
    );
}
