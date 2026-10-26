use super::*;

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
