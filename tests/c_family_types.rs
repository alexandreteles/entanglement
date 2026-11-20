mod support;
use support::{report, run, write};

#[test]
fn cpp_types_own_fields_inheritance_and_overloaded_members() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("types.hpp"),
        "struct Value {};\nstruct Base { virtual int get(int) = 0; };\nclass Child : public Base { public: Value field; int get(int) override { return 1; } int get(double) { return 2; } };\n",
    );
    let result = report(&run("repo", root, None, Some("structure")));
    let edges = result["structure"]["relationships"].as_array().unwrap();
    for (to, kind) in [(":Base", "inherit"), (":Value", "field")] {
        assert!(
            edges
                .iter()
                .any(|edge| edge["source"].as_str().unwrap().ends_with(":Child")
                    && edge["target"].as_str().unwrap().ends_with(to)
                    && edge["kind"] == kind
                    && edge["contract"] == true),
            "{edges:#?}"
        );
    }
    let child = result["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["unit"].as_str().unwrap().ends_with(":Child"))
        .unwrap();
    assert!(child.to_string().contains("get(double)"), "{child:#?}");
}
