mod support;
use support::{report, run, write};

#[test]
fn include_cycles_and_module_reexports_have_source_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("a.h"), "#pragma once\n#include \"b.h\"\n");
    write(&root.join("b.h"), "#pragma once\n#include \"a.h\"\n");
    write(&root.join("main.c"), "#include \"a.h\"\n");
    write(
        &root.join("base.cppm"),
        "export module base;\nexport int value() { return 1; }\n",
    );
    write(
        &root.join("api.ixx"),
        "export module api;\nexport import base;\n",
    );
    let result = report(&run("repo", root, None, Some("structure")));
    let edges = result["structure"]["relationships"].as_array().unwrap();
    for (from, to, kind) in [
        ("a.h", "b.h", "import"),
        ("b.h", "a.h", "import"),
        ("api.ixx", "base.cppm", "reexport"),
    ] {
        assert!(
            edges
                .iter()
                .any(|edge| edge["source"].as_str().unwrap().ends_with(from)
                    && edge["target"].as_str().unwrap().ends_with(to)
                    && edge["kind"] == kind
                    && edge["resolution"] == "resolved"),
            "{edges:#?}"
        );
    }
}

#[test]
fn module_imports_select_the_interface_and_calls_select_the_implementation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("api.cppm"),
        "export module api;\nexport int answer();\n",
    );
    write(
        &root.join("body.cpp"),
        "module api;\nint answer() { return 42; }\n",
    );
    write(
        &root.join("main.cpp"),
        "import api;\nint main() { return answer(); }\n",
    );
    let result = report(&run("repo", root, None, None));
    let edges = result["structure"]["relationships"].as_array().unwrap();
    for (kind, suffix) in [("import", "api.cppm"), ("call", "body.cpp")] {
        assert!(
            edges.iter().any(
                |edge| edge["source"].as_str().unwrap().ends_with("main.cpp")
                    && edge["target"].as_str().unwrap().ends_with(suffix)
                    && edge["kind"] == kind
                    && edge["resolution"] == "resolved"
            ),
            "{kind}: {edges:#?}"
        );
    }
}

#[test]
fn incompatible_include_paths_stay_on_the_graph_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("a.h"),
        "#pragma once\n#if MODE\n#include \"b.h\"\n#endif\n",
    );
    write(
        &root.join("b.h"),
        "#pragma once\n#if !MODE\n#include \"a.h\"\n#endif\n",
    );
    write(
        &root.join("client.c"),
        "#include \"a.h\"\n#include \"b.h\"\n",
    );
    let commands:Vec<_>=[0,1].iter().map(|mode|serde_json::json!({"file":"client.c","directory":root,"arguments":["cc",format!("-DMODE={mode}"),"client.c"]})).collect();
    write(
        &root.join("compile_commands.json"),
        serde_json::json!(commands).to_string(),
    );
    let result = report(&run("repo", root, None, Some("structure")));
    assert!(
        !result["structure"]["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "import"
                && edge["source"].as_str().unwrap().ends_with(".h")
                && edge["resolution"] == "resolved")
    );
}
