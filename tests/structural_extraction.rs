mod support;
use serde_json::Value;
use support::{report, run, write};

fn declaration<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["unit"]
                .as_str()
                .unwrap()
                .ends_with(&format!(":{name}"))
        })
        .unwrap()
}

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
fn php_trait_alias_and_precedence_use_the_existing_resolver() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.php"),
        "<?php trait First { public function run() {} } trait Second { public function run() {} } class Consumer { use First, Second { First::run insteadof Second; First::run as aliasRun; } }",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let members = declaration(&result, "Consumer")["members"]
        .as_array()
        .unwrap();
    for name in ["run", "aliasRun"] {
        let member = members
            .iter()
            .find(|member| member["name"] == name)
            .unwrap();
        assert!(member["body"].as_str().unwrap().ends_with(":First::run"));
    }
}

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
fn all_registered_language_groups_emit_coverage() {
    let root = tempfile::tempdir().unwrap();
    let fixture_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/structure_fixtures");
    for fixture in std::fs::read_dir(fixture_root).unwrap() {
        let fixture = fixture.unwrap();
        write(
            &root.path().join(fixture.file_name()),
            std::fs::read(fixture.path()).unwrap(),
        );
    }
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let coverage = result["structure"]["coverage"].as_array().unwrap();
    for suffix in [
        "rs", "php", "py", "go", "kt", "js", "jsx", "ts", "tsx", "astro", "svelte", "html", "css",
    ] {
        assert!(coverage.iter().any(|item| {
            item["path"]
                .as_str()
                .unwrap()
                .ends_with(&format!(".{suffix}"))
        }));
    }
    for language in [
        "source.rust",
        "source.php",
        "source.python",
        "source.go",
        "source.kotlin",
        "source.js",
        "source.ts",
        "source.tsx",
        "source.astro",
        "source.svelte",
        "source.angular",
        "source.html",
        "source.css",
    ] {
        assert!(
            coverage.iter().any(|item| item["language"] == language),
            "missing language {language}: {coverage:#?}"
        );
    }
    for host in ["model.astro", "model.svelte"] {
        let host_coverage = coverage
            .iter()
            .filter(|item| item["path"].as_str().unwrap().ends_with(host))
            .collect::<Vec<_>>();
        assert!(
            host_coverage
                .iter()
                .any(|item| item["language"] == "source.js")
        );
        assert!(
            host_coverage
                .iter()
                .any(|item| item["language"] == "source.css")
        );
        assert!(
            host_coverage
                .iter()
                .any(|item| item["effective"] == "inapplicable")
        );
    }
    for item in coverage {
        assert!(!item["limitations"].as_array().unwrap().is_empty());
        assert_ne!(item["status"], "complete");
    }
    let relationships = result["structure"]["relationships"].as_array().unwrap();
    for name in [
        "model.rs",
        "model.php",
        "model.py",
        "types.go",
        "model.kt",
        "model.js",
        "model.jsx",
        "model.ts",
        "model.tsx",
        "model.astro",
        "model.svelte",
        "model.html",
        "model.css",
    ] {
        assert!(
            relationships.iter().any(|edge| edge["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .any(|evidence| evidence["path"].as_str().unwrap().ends_with(name))),
            "missing dependencies for {name}"
        );
    }
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
fn python_class_annotations_keep_declared_property_types() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.py"),
        "class Item:\n    pass\nclass Base:\n    value: Item\nclass Child(Base):\n    pass\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let child = declaration(&result, "Child");
    assert!(
        child["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["name"] == "value"),
        "{child:#}"
    );
    assert!(
        child["types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|structure| structure["written"]["text"] == "Item"),
        "{child:#}"
    );
}

#[test]
fn php_each_declared_property_has_a_separate_origin() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.php"),
        "<?php class Item {} class Base { public Item $first, $second; } class Child extends Base {}\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let child = declaration(&result, "Child");
    for name in ["first", "second"] {
        assert!(
            child["members"]
                .as_array()
                .unwrap()
                .iter()
                .any(|member| member["name"] == name),
            "{child:#}"
        );
    }
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
fn php_private_property_types_stay_in_the_full_projection() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.php"),
        "<?php class Database {} class Facade { private Database $database; }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let edges = result["structure"]["relationships"].as_array().unwrap();
    let dependencies = edges
        .iter()
        .filter(|edge| {
            edge["source"].as_str().unwrap().ends_with(":Facade")
                && edge["target"].as_str().unwrap().ends_with(":Database")
        })
        .collect::<Vec<_>>();
    assert!(!dependencies.is_empty());
    assert!(
        dependencies.iter().all(|edge| edge["contract"] == false),
        "{dependencies:#?}"
    );
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

#[test]
fn local_types_keep_their_callable_ownership() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("types.py"),
        "def first():\n    class Item:\n        pass\n    return Item\ndef second():\n    class Item:\n        pass\n    return Item\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let units = result["structure"]["units"].as_array().unwrap();
    assert!(units.iter().any(|unit| unit["name"] == "first::Item"));
    assert!(units.iter().any(|unit| unit["name"] == "second::Item"));
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
