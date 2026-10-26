use super::*;

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
