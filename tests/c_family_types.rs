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

#[test]
fn diamond_inheritance_distinguishes_shared_and_repeated_bases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("diamond.hpp");
    write(
        &path,
        "struct Top { int get() { return 1; } }; struct L : Top {}; struct R : Top {}; struct D : L, R {}; struct VL : virtual Top {}; struct VR : virtual Top {}; struct Shared : VL, VR {}; class Hidden : Top { protected: int value; };\n",
    );
    let result = report(&run("file", &path, None, Some("structure")));
    let records = result["structure"]["effective"].as_array().unwrap();
    let get = |name: &str| {
        records
            .iter()
            .find(|item| item["unit"].as_str().unwrap().ends_with(name))
            .unwrap()
    };
    assert!(get(":D")["conflicts"].to_string().contains("get()"));
    assert_eq!(get(":Shared")["conflicts"].as_array().unwrap().len(), 0);
    assert!(
        get(":Hidden")["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "value" && item["visibility"] == "protected")
    );
    assert!(
        result["structure"]["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["source"].as_str().unwrap().ends_with(":Hidden")
                && edge["kind"] == "inherit"
                && edge["contract"] == false)
    );
}
