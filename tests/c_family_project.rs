mod support;
use serde_json::json;
use support::{file, patch, report, run, write};

#[test]
fn a_header_uses_its_translation_unit_and_a_candidate_uses_the_new_flags() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(&root.join("client.c"), "#include \"public/api.h\"\n");
    write(&root.join("public/api.h"), "int value(void);\n");
    let database = |mode| {
        json!([{"directory":root,"file":"client.c",
        "arguments":["cc","-x",mode,"-c","client.c"]}])
        .to_string()
            + "\n"
    };
    let before = database("c++");
    let after = database("c");
    write(&root.join("compile_commands.json"), &before);
    let initial = report(&run("repo", root, None, Some("nloc")));
    assert_eq!(file(&initial, "api.h")["language"], "cpp");
    assert_eq!(file(&initial, "client.c")["language"], "cpp");
    let diff = root.join("change.diff");
    write(
        &diff,
        patch("compile_commands.json", Some(&before), Some(&after)),
    );
    let changed = report(&run("candidate", root, Some(&diff), Some("nloc")));
    assert_eq!(file(&changed, "api.h")["language"], "c");
    assert_eq!(file(&changed, "client.c")["language"], "c");
    assert_eq!(
        std::fs::read_to_string(root.join("compile_commands.json")).unwrap(),
        before
    );
}

#[test]
fn created_and_shared_headers_get_one_physical_report() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(&root.join("client.c"), "#include \"api.h\"\n");
    write(&root.join("other.cpp"), "#include \"api.h\"\n");
    let diff = root.join("change.diff");
    write(
        &diff,
        patch(
            "api.h",
            None,
            Some("template<class T> struct Box { T value; };\n"),
        ),
    );
    let changed = report(&run("candidate", root, Some(&diff), Some("nloc")));
    assert_eq!(changed["files"].as_array().unwrap().len(), 3);
    assert_eq!(file(&changed, "api.h")["language"], "cpp");
    assert_eq!(file(&changed, "api.h")["nloc"], 1);
    assert!(!root.join("api.h").exists());
}
