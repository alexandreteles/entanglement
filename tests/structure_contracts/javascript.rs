use super::*;

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
