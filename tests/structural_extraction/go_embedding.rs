use super::*;

#[test]
fn go_pointer_receiver_and_embedding_remain_distinct() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.go"),
        "package fixture\ntype Base struct {}\nfunc (base Base) Value() {}\nfunc (base *Base) Pointer() {}\ntype Outer struct { Base }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let members = declaration(&result, "Outer")["members"].as_array().unwrap();
    assert!(members.iter().any(|member| member["name"] == "Value"));
    assert!(
        members
            .iter()
            .any(|member| member["name"] == "Pointer" && member["receiver"] == "pointer")
    );
}

#[test]
fn go_embedding_conflicts_have_no_selected_body() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.go"),
        "package fixture\ntype Left struct {}\nfunc (left Left) Shared() {}\ntype Right struct {}\nfunc (right Right) Shared() {}\ntype Outer struct { Left; Right }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let outer = declaration(&result, "Outer");
    assert!(
        !outer["conflicts"].as_array().unwrap().is_empty(),
        "{outer:#}"
    );
    assert!(
        outer["members"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|member| member["name"] == "Shared")
            .all(|member| member["body"].is_null())
    );
}

#[test]
fn go_each_declared_field_can_hide_a_promoted_method() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.go"),
        "package fixture\ntype Contract interface { Execute() }\ntype Base struct {}\nfunc (value Base) Execute() {}\ntype Candidate struct { Base; First, Execute int }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let candidate = declaration(&result, "Candidate");
    assert!(
        !candidate["satisfies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|proof| proof["contract"].as_str().unwrap().ends_with(":Contract")),
        "{candidate:#}"
    );
}

#[test]
fn go_pointer_embedding_promotes_pointer_methods_to_value_set() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.go"),
        "package fixture\ntype Base struct {}\nfunc (base *Base) Pointer() {}\ntype Outer struct { *Base }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let outer = declaration(&result, "Outer");
    assert!(
        outer["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["name"] == "Pointer" && member["receiver"] == "value"),
        "{outer:#}"
    );
}
