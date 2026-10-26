use super::*;

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
