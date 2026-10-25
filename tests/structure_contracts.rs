mod support;
use serde_json::Value;
use support::{patch, report, run, write};

fn analyze(name: &str, source: &str) -> Value {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(name), source);
    report(&run("repo", root.path(), None, Some("structure")))
}

fn declaration<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|declaration| {
            declaration["unit"]
                .as_str()
                .unwrap()
                .ends_with(&format!(":{name}"))
        })
        .unwrap()
}

fn member<'a>(declaration: &'a Value, name: &str) -> &'a Value {
    declaration["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == name)
        .unwrap_or_else(|| panic!("missing member {name}: {declaration:#}"))
}

#[test]
fn kotlin_requirements_and_default_bodies_have_distinct_origins() {
    let result = analyze(
        "model.kt",
        "interface Base {\n fun required(): String\n fun provided(): String = \"base\"\n}\nclass Item : Base {\n override fun required(): String = \"item\"\n}\n",
    );
    let base = declaration(&result, "Base");
    assert_eq!(member(base, "required")["required"], true);
    assert_eq!(member(base, "provided")["required"], false);
    let item = declaration(&result, "Item");
    assert!(
        member(item, "required")["body"]
            .as_str()
            .unwrap()
            .contains("Item::required"),
        "{item:#}"
    );
    assert!(
        member(item, "provided")["body"]
            .as_str()
            .unwrap()
            .contains("Base::provided"),
        "{item:#}"
    );
}

#[test]
fn kotlin_conflicting_defaults_have_no_selected_body() {
    let result = analyze(
        "model.kt",
        "interface Left { fun shared(): Int = 1 }\ninterface Right { fun shared(): Int = 2 }\nclass Conflict : Left, Right {}\n",
    );
    let conflict = declaration(&result, "Conflict");
    assert!(
        !conflict["conflicts"].as_array().unwrap().is_empty(),
        "{conflict:#}"
    );
    assert!(
        conflict["members"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|member| member["name"] == "shared")
            .all(|member| member["body"].is_null())
    );
}

#[test]
fn typescript_interface_inheritance_retains_requirements_and_nested_types() {
    let result = analyze(
        "model.ts",
        "interface Base { required(): void; }\ninterface Child extends Base { nested: Array<Array<string>>; }\n",
    );
    let child = declaration(&result, "Child");
    assert_eq!(member(child, "required")["required"], true, "{child:#}");
    assert!(
        child["types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|structure| structure["written"]["text"]
                .as_str()
                .unwrap()
                .contains("Array<Array<string>>")),
        "{child:#}"
    );
}

#[test]
fn public_member_alias_has_a_written_and_expanded_type() {
    let result = analyze(
        "model.ts",
        "type Label = Array<Array<string>>;\ninterface Item { value: Label; }\n",
    );
    let item = declaration(&result, "Item");
    assert!(
        item["types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|structure| structure["written"]["text"]
                .as_str()
                .unwrap()
                .contains("Label")
                && !structure["expanded"].is_null()),
        "{item:#}"
    );
}

#[test]
fn facade_private_signature_and_public_exposure_use_separate_projections() {
    let hidden = analyze(
        "model.ts",
        "class Database {}\nexport class Facade { private connect(): Database { return new Database(); } }\n",
    );
    let exposed = analyze(
        "model.ts",
        "class Database {}\nexport class Facade { public connect(): Database { return new Database(); } }\n",
    );
    let has_edge = |result: &Value, projection: &str| {
        result["structure"]["graphs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|graph| graph["granularity"] == "type" && graph["projection"] == projection)
            .unwrap()["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| {
                edge["source"].as_str().unwrap().ends_with(":Facade")
                    && edge["target"].as_str().unwrap().ends_with(":Database")
            })
    };
    assert!(has_edge(&hidden, "full"));
    assert!(!has_edge(&hidden, "contract"));
    assert!(has_edge(&exposed, "contract"));
}

#[test]
fn jsx_component_reference_retains_module_dependency() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("widget.jsx"),
        "export function Widget() { return <span />; }\n",
    );
    write(
        &root.path().join("app.jsx"),
        "import { Widget } from './widget';\nexport function App() { return <Widget />; }\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    assert!(
        result["structure"]["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["resolution"] == "resolved"
                && edge["source"].as_str().unwrap().contains("app.jsx")
                && edge["target"].as_str().unwrap().contains("widget.jsx")),
        "{}",
        result["structure"]
    );
}

#[test]
fn formatting_only_candidate_does_not_change_effective_members() {
    let root = tempfile::tempdir().unwrap();
    let before = "class Base:\n    def method(self): pass\nclass Item(Base): pass\n";
    let after = format!("\n\n{before}");
    write(&root.path().join("model.py"), before);
    let diff = root.path().join("change.diff");
    write(&diff, patch("model.py", Some(before), Some(&after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    assert!(
        !result["structure"]["delta"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "effective"),
        "{}",
        result["structure"]["delta"]
    );
}

#[test]
fn duplicate_declarations_have_no_certain_effective_body() {
    let result = analyze(
        "model.js",
        "class Item { first() {} }\nclass Item { second() {} }\n",
    );
    let item = declaration(&result, "Item");
    assert_eq!(item["status"], "unavailable");
    assert!(item["members"].as_array().unwrap().is_empty());
    assert!(
        item["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|conflict| conflict == "identity_ambiguous")
    );
}

#[test]
fn rust_same_named_supertrait_members_keep_distinct_contracts() {
    let result = analyze(
        "model.rs",
        "pub trait A { fn shared(&self); }\npub trait B: A { fn shared(&self) {} }\n",
    );
    let derived = declaration(&result, "B");
    let choices = derived["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|member| member["name"] == "shared")
        .collect::<Vec<_>>();
    assert_eq!(choices.len(), 2, "{derived:#}");
    assert!(choices.iter().any(|member| member["required"] == true
        && member["origin"].as_str().unwrap().contains("A::shared")));
    assert!(choices.iter().any(|member| {
        member["required"] == false
            && member["body"]
                .as_str()
                .is_some_and(|body| body.contains("B::shared"))
    }));
}

#[test]
fn rust_trait_impl_bodies_have_contract_qualified_identities() {
    let result = analyze(
        "model.rs",
        "pub trait A { fn shared(&self); }\npub trait B { fn shared(&self); }\npub struct Item;\nimpl A for Item { fn shared(&self) {} }\nimpl B for Item { fn shared(&self) {} }\n",
    );
    let item = declaration(&result, "Item");
    let bodies = item["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|member| member["body"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(bodies.iter().any(|body| body.ends_with("@A")), "{item:#}");
    assert!(bodies.iter().any(|body| body.ends_with("@B")), "{item:#}");
}

#[test]
fn script_class_methods_select_known_instance_override() {
    for name in ["model.ts", "model.js"] {
        let result = analyze(
            name,
            "class Base { method() {} }\nclass Item extends Base { method() {} }\n",
        );
        let item = declaration(&result, "Item");
        let method = member(item, "method");
        assert!(
            method["body"]
                .as_str()
                .unwrap()
                .contains("Item::method#instance"),
            "{item:#}"
        );
        assert!(
            !item["obligations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|obligation| obligation == "selection_unavailable:method"),
            "{item:#}"
        );
    }
}

#[test]
fn javascript_static_instance_and_accessors_keep_separate_origins() {
    let result = analyze(
        "model.js",
        "class Item { static shared() {} shared() {} get value() { return 1; } set value(next) {} }\n",
    );
    let item = declaration(&result, "Item");
    let members = item["members"].as_array().unwrap();
    assert_eq!(members.len(), 4, "{item:#}");
    let origins = members
        .iter()
        .filter_map(|member| member["origin"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(origins.len(), 4);
    assert!(
        members.iter().all(|member| !member["body"].is_null()),
        "{item:#}"
    );
    assert!(item["conflicts"].as_array().unwrap().is_empty());
}

#[test]
fn own_accessor_descriptor_hides_the_parent_descriptor() {
    let result = analyze(
        "model.js",
        "class Base { get value() { return 1; } set value(next) {} }\nclass Item extends Base { set value(next) {} }\n",
    );
    let item = declaration(&result, "Item");
    let values = item["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|member| member["name"] == "value")
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 1, "{item:#}");
    assert_eq!(values[0]["member_kind"], "set");
    assert!(
        values[0]["body"]
            .as_str()
            .unwrap()
            .contains("Item::value#set")
    );
}

#[test]
fn go_same_origin_diamond_has_no_selected_promoted_body() {
    let result = analyze(
        "model.go",
        "package fixture\ntype Base struct {}\nfunc (base Base) Value() {}\ntype Left struct { Base }\ntype Right struct { Base }\ntype Item struct { Left; Right }\n",
    );
    let item = declaration(&result, "Item");
    assert!(member(item, "Value")["body"].is_null(), "{item:#}");
    assert!(
        item["obligations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|obligation| obligation == "promotion_ambiguous:Value")
    );
}

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
fn alias_change_event_has_source_and_expanded_before_after_values() {
    let root = tempfile::tempdir().unwrap();
    let before = "type Label = Array<string>;\ninterface Item { value: Label; }\n";
    let after = before.replace("Array<string>", "Array<Array<string>>");
    write(&root.path().join("model.ts"), before);
    let diff = root.path().join("change.diff");
    write(&diff, patch("model.ts", Some(before), Some(&after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    let event = result["structure"]["delta"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| {
            event["kind"].as_str().unwrap().starts_with("effective")
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().ends_with(":Item"))
        })
        .unwrap();
    assert_ne!(event["before"], event["after"]);
    assert!(
        !event["after_path"]["evidence"]
            .as_array()
            .unwrap()
            .is_empty()
    );
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
