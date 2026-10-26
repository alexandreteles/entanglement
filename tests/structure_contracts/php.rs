use super::*;

#[test]
fn php_trait_visibility_adaptations_and_inherited_private_are_explicit() {
    let result = analyze(
        "model.php",
        "<?php trait Source { public function run() {} } class Base { private function hidden() {} } class Item extends Base { use Source { run as private privateRun; run as protected protectedRun; } }",
    );
    let item = declaration(&result, "Item");
    assert_eq!(member(item, "privateRun")["visibility"], "private");
    assert_eq!(member(item, "protectedRun")["visibility"], "protected");
    assert_eq!(member(item, "hidden")["visibility"], "private");
}

#[test]
fn php_version_dependent_property_and_constant_origins_remain_candidates() {
    let result = analyze(
        "model.php",
        "<?php trait Supplied { public int $value = 2; public const LIMIT = 2; public function run() {} } class ParentType { public int $value = 1; public const LIMIT = 1; public function run() {} } class Child extends ParentType { use Supplied; } class Grandchild extends Child {}",
    );
    for name in ["Child", "Grandchild"] {
        let item = declaration(&result, name);
        for member_name in ["value", "LIMIT"] {
            let origins = item["members"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|member| member["name"] == member_name)
                .collect::<Vec<_>>();
            assert_eq!(origins.len(), 2, "{item:#}");
            assert!(
                origins
                    .iter()
                    .all(|member| member["body"].is_null() && member["selection"] == "candidate"),
                "{item:#}"
            );
            assert!(
                item["obligations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|obligation| obligation == &format!("php_version:{member_name}")),
                "{item:#}"
            );
        }
        assert!(
            member(item, "run")["body"]
                .as_str()
                .unwrap()
                .contains("Supplied::run")
        );
    }
}

#[test]
fn php_noncolliding_property_origins_remain_known() {
    let result = analyze(
        "model.php",
        "<?php trait Supplied { public int $other = 2; public int $hidden = 2; public const EXPOSED = 2; public function run() {} } class ParentType { public int $value = 1; private int $hidden = 1; public const LIMIT = 1; private const EXPOSED = 1; public function run() {} } class Child extends ParentType { use Supplied; }",
    );
    let item = declaration(&result, "Child");
    for member_name in ["value", "LIMIT"] {
        let inherited = member(item, member_name);
        assert!(
            inherited["origin"]
                .as_str()
                .unwrap()
                .contains("ParentType::"),
            "{item:#}"
        );
        assert!(inherited["selection"].is_null());
    }
    for member_name in ["hidden", "EXPOSED"] {
        assert!(
            member(item, member_name)["origin"]
                .as_str()
                .unwrap()
                .contains("Supplied::"),
            "{item:#}"
        );
        assert!(member(item, member_name)["selection"].is_null());
    }
    assert!(
        member(item, "other")["origin"]
            .as_str()
            .unwrap()
            .contains("Supplied::other")
    );
    assert!(
        member(item, "run")["body"]
            .as_str()
            .unwrap()
            .contains("Supplied::run")
    );
    assert!(
        !item["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|obligation| obligation.as_str().unwrap().starts_with("php_version:"))
    );
}
