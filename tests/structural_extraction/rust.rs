use super::*;

#[test]
fn rust_trait_requirements_and_defaults_keep_body_origins() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("lib.rs"),
        "pub trait Base { fn required(&self); fn provided(&self) {} }\npub trait Child: Base {}\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let members = declaration(&result, "Child")["members"].as_array().unwrap();
    assert!(members.iter().any(|member| member["name"] == "required"
        && member["required"] == true
        && member["body"].is_null()));
    assert!(members.iter().any(|member| member["name"] == "provided"
        && member["required"] == false
        && member["body"].is_string()));
}

#[test]
fn rust_associated_requirements_and_bounds_remain_symbolic() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("lib.rs"),
        "pub trait Bound {}\npub trait Requirement<T: Bound> { type Item; const LIMIT: usize; fn execute(&self) -> Self::Item; }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let requirement = declaration(&result, "Requirement");
    let members = requirement["members"].as_array().unwrap();
    for name in ["Item", "LIMIT", "execute"] {
        assert!(
            members
                .iter()
                .any(|member| member["name"] == name && member["required"] == true),
            "{requirement:#}"
        );
    }
    assert!(
        !result["structure"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| unit["name"] == "Requirement::Item")
    );
    assert!(
        requirement["types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|structure| structure["written"]["text"]
                .as_str()
                .unwrap()
                .contains("T: Bound"))
    );
}

#[test]
fn rust_public_positional_payload_types_remain_contract_dependencies() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("lib.rs"),
        "pub struct Payload; pub struct Tuple(pub Payload, Payload); pub enum Choice { First(Payload), Second { value: Payload } }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let edges = result["structure"]["relationships"].as_array().unwrap();
    for name in ["Tuple", "Choice"] {
        assert!(
            edges.iter().any(|edge| edge["source"]
                .as_str()
                .unwrap()
                .ends_with(&format!(":{name}"))
                && edge["target"].as_str().unwrap().ends_with(":Payload")
                && edge["contract"] == true),
            "{edges:#?}"
        );
        assert!(
            declaration(&result, name)["types"]
                .as_array()
                .unwrap()
                .iter()
                .any(|structure| structure["written"]["text"] == "Payload")
        );
    }
    assert!(
        result["structure"]["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .any(|coverage| coverage["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|limit| limit == "positional_members"))
    );
}
