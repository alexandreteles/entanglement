use super::*;

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
