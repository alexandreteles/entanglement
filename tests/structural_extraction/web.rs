use super::*;

#[test]
fn javascript_static_and_accessor_members_keep_distinct_namespaces() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.js"),
        "export class Base { static run() {} run() {} get value() { return 1; } set value(input) {} }\nexport class Child extends Base { run() {} }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let child = declaration(&result, "Child");
    let members = child["members"].as_array().unwrap();
    for (name, kind, owner) in [
        ("run", "static", "Base::run"),
        ("run", "instance", "Child::run"),
        ("value", "get", "Base::value"),
        ("value", "set", "Base::value"),
    ] {
        assert!(
            members.iter().any(|member| member["name"] == name
                && member["member_kind"] == kind
                && member["body"]
                    .as_str()
                    .is_some_and(|body| body.contains(owner))),
            "{child:#}"
        );
    }
}

#[test]
fn javascript_constructors_and_computed_names_have_distinct_selection_limits() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.js"),
        "const name = globalThis.name; export class Base { constructor() {} [name]() {} get() {} } export class Child extends Base {}\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let child = declaration(&result, "Child");
    assert!(
        !child["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["member_kind"] == "constructor" && member["body"].is_string()),
        "{child:#}"
    );
    assert!(
        child["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["name"] == "get" && member["member_kind"] == "instance")
    );
    assert!(
        child["members"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|member| member["name"].as_str().unwrap().starts_with('['))
            .all(|member| member["body"].is_null())
    );
}

#[test]
fn typescript_optional_members_are_not_required_implementations() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.ts"),
        "export interface Contract { required: string; optional?: string; execute?(): void; }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let contract = declaration(&result, "Contract");
    let members = contract["members"].as_array().unwrap();
    assert!(
        members
            .iter()
            .any(|member| member["name"] == "required" && member["required"] == true)
    );
    for name in ["optional", "execute"] {
        assert!(
            members.iter().any(|member| member["name"] == name
                && member["optional"] == true
                && member["required"] == false
                && member["body"].is_null()),
            "{contract:#}"
        );
    }
}

#[test]
fn typescript_intersection_alias_collects_each_member_contract() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.ts"),
        "export interface Left { left(): void; } export interface Right { right(): void; } export type Combined = Left & Right;\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let combined = declaration(&result, "Combined");
    for name in ["left", "right"] {
        assert!(
            combined["members"]
                .as_array()
                .unwrap()
                .iter()
                .any(|member| member["name"] == name && member["required"] == true),
            "{combined:#}"
        );
    }
}

#[test]
fn generic_arguments_do_not_supply_inherited_members() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.ts"),
        "interface Base<T> { base(): void; } class Argument { unrelated() {} } interface Child extends Base<Argument> {}\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let child = declaration(&result, "Child");
    assert!(
        child["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["name"] == "base")
    );
    assert!(
        !child["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["name"] == "unrelated"),
        "{child:#}"
    );
}
