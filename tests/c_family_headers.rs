mod support;
use serde_json::json;
use support::{file, function, patch, report, run, write};

#[test]
fn fallback_uses_cpp_syntax_in_damaged_headers() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        &root.join("queue.h"),
        "typedef TAILQ_HEAD(queue, node, ) queue_t;\n",
    );
    write(
        &root.join("view.h"),
        "struct options { int depth; };\nbool prepare(const options &selected = {});\n",
    );
    let result = report(&run("repo", root, None, Some("nloc")));
    assert_eq!(file(&result, "queue.h")["language"], "c");
    assert_eq!(file(&result, "view.h")["language"], "cpp");
}

#[test]
fn fallback_keeps_a_standalone_guarded_c_api_in_c() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let guarded = "#ifdef __cplusplus\nextern \"C\" {\n#endif\nint shared(int value);\n#ifdef __cplusplus\n} // extern \"C\"\n#endif\n";
    for name in ["api.h", "api.hpp"] {
        write(&root.join(name), guarded);
    }
    let result = report(&run("repo", root, None, Some("structure")));
    assert_eq!(file(&result, "api.h")["language"], "c");
    assert_eq!(file(&result, "api.hpp")["language"], "cpp");
    for item in result["structure"]["coverage"].as_array().unwrap() {
        assert!(
            !item["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|code| code == "parse_error")
        );
    }
}

#[test]
fn header_units_use_the_virtual_include_search_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("api.cppm"),
        "export module api;\nexport import \"value.h\";\n",
    );
    write(&root.join("compile_commands.json"),json!([{"file":"api.cppm","directory":root,"arguments":["c++","-std=c++20","-Iinclude","api.cppm"]}]).to_string());
    let diff = root.join("new.diff");
    write(
        &diff,
        patch("include/value.h", None, Some("struct Value {};\n")),
    );
    let result = report(&run("candidate", root, Some(&diff), Some("structure")));
    assert!(
        result["structure"]["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "reexport"
                && edge["resolution"] == "resolved"
                && edge["target"]
                    .as_str()
                    .unwrap()
                    .ends_with("include/value.h"))
    );
    assert!(!root.join("include/value.h").exists());
}

#[test]
fn a_forced_header_uses_the_build_directory_and_the_c_standard() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("config.h"), "#define ENABLED 1\n");
    write(
        &root.join("src/main.c"),
        "#if ENABLED && __STDC_VERSION__ >= 201112L\nstatic int loop(void) { return loop(); }\n#endif\n",
    );
    write(&root.join("compile_commands.json"),json!([{"directory":root,"file":"src/main.c","command":"cc -std=c17 -include config.h -c src/main.c"}]).to_string());
    let result = report(&run("repo", root, None, None));
    assert_eq!(
        function(file(&result, "src/main.c"), "loop")["cognitive_complexity"],
        1
    );
}

#[test]
fn standard_library_declarations_and_missing_project_headers_have_distinct_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("use.c");
    write(
        &path,
        "#include <stdio.h>\n#include <generated/api.h>\nint out(void) { return puts(\"ok\"); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = file(&result, "use.c");
    assert!(
        source["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"][0] == "puts" && item["resolution"]["status"] == "external")
    );
    let edges = result["structure"]["relationships"].as_array().unwrap();
    assert!(edges.iter().any(|edge|edge["target"]=="<generated/api.h>" && edge["resolution"]=="unresolved"));
}
