mod support;
use support::{report, run, write};

#[test]
fn cross_file_impl_uses_resolved_target_ownership() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("Cargo.toml"),
        "[package]\nname = 'fixture'\nversion = '0.1.0'\nedition = '2024'\n",
    );
    write(
        &root.path().join("src/lib.rs"),
        "pub mod model;\npub mod implementation;\n",
    );
    write(
        &root.path().join("src/model.rs"),
        "pub trait Required { fn run(&self); }\npub struct Item;\n",
    );
    write(
        &root.path().join("src/implementation.rs"),
        "use crate::model::{Item, Required};\nimpl Required for Item { fn run(&self) {} }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let derived = result["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|declaration| declaration["unit"].as_str().unwrap().ends_with(":Item"))
        .unwrap();
    let member = derived["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == "run")
        .unwrap();
    assert!(!member["required"].as_bool().unwrap(), "{derived:#}");
    assert!(
        member["body"]
            .as_str()
            .unwrap()
            .contains("implementation.rs"),
        "{derived:#}"
    );
    assert!(
        !derived["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["name"] == "@impl")
    );
    assert!(
        result["structure"]["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "implement"
                && edge["source"].as_str().unwrap().ends_with(":Item")
                && edge["target"].as_str().unwrap().ends_with(":Required"))
    );
}
