use super::*;

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
