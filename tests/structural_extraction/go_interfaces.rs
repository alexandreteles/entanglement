use super::*;

#[test]
fn go_interface_proofs_check_full_signatures_and_receiver_sets() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.go"),
        "package fixture\ntype Contract interface { Execute(int) string }\ntype Good struct {}\nfunc (value Good) Execute(input int) string { return \"ok\" }\ntype Wrong struct {}\nfunc (value Wrong) Execute(input string) string { return input }\ntype PointerOnly struct {}\nfunc (value *PointerOnly) Execute(input int) string { return \"ok\" }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let proof = |name: &str, receiver: &str| {
        declaration(&result, name)["satisfies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|proof| {
                proof["contract"].as_str().unwrap().ends_with(":Contract")
                    && proof["receiver"] == receiver
                    && proof["status"] == "complete"
            })
    };
    assert!(proof("Good", "value"));
    assert!(
        declaration(&result, "Wrong")["satisfies"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(proof("PointerOnly", "pointer"));
    assert!(!proof("PointerOnly", "value"));
}

#[test]
fn go_interface_method_sets_do_not_require_executable_bodies() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.go"),
        "package fixture\ntype Base interface { Execute(int) string }\ntype Left interface { Base }\ntype Right interface { Base }\ntype Diamond interface { Left; Right }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let diamond = declaration(&result, "Diamond");
    assert!(
        diamond["satisfies"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |proof| proof["contract"].as_str().unwrap().ends_with(":Base")
                    && proof["receiver"] == "value"
            ),
        "{diamond:#}"
    );
    assert!(diamond["conflicts"].as_array().unwrap().is_empty());
    assert!(
        !diamond["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|obligation| obligation
                .as_str()
                .unwrap()
                .starts_with("promotion_ambiguous"))
    );
}

#[test]
fn go_named_types_and_shadowed_predeclared_types_keep_their_identity() {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join("go.mod"), "module fixture\n\ngo 1.25\n");
    write(
        &root.path().join("contracts/types.go"),
        "package contracts\ntype Contract interface { Execute(string) }\n",
    );
    write(
        &root.path().join("implementation/types.go"),
        "package implementation\ntype string struct {}\ntype Candidate struct {}\nfunc (value Candidate) Execute(input string) {}\n",
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
fn go_transitive_type_set_constraints_do_not_become_method_only_proofs() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.go"),
        "package fixture\ntype Restricted interface { ~int }\ntype Child interface { Restricted }\ntype Candidate struct {}\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let candidate = declaration(&result, "Candidate");
    assert!(
        !candidate["satisfies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|proof| proof["contract"].as_str().unwrap().ends_with(":Child")),
        "{candidate:#}"
    );
}
