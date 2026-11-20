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
