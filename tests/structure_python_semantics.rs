mod support;
use support::{patch, report, run, write};

fn declaration<'a>(report: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
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

#[test]
fn python_diamond_selects_c3_origin_from_source_base_order() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("model.py"),
        "class A:\n    def method(self): pass\nclass B(A):\n    def method(self): pass\nclass C(A):\n    def method(self): pass\nclass D(B, C): pass\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let derived = declaration(&result, "D");
    let method = derived["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == "method")
        .unwrap();
    assert!(
        method["origin"].as_str().unwrap().contains("B::method"),
        "{derived:#}"
    );
    assert_eq!(method["provenance"].as_array().unwrap().len(), 2);
}

#[test]
fn inconsistent_python_mro_has_no_invented_inherited_body() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("model.py"),
        "class X: pass\nclass Y: pass\nclass A(X, Y):\n    def method(self): pass\nclass B(Y, X): pass\nclass C(A, B): pass\n",
    );
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let derived = declaration(&result, "C");
    assert!(
        derived["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|conflict| conflict == "inconsistent_mro"),
        "{derived:#}"
    );
    assert!(derived["members"].as_array().unwrap().is_empty());
}

#[test]
fn candidate_base_order_change_reports_selected_body_change() {
    let root = tempfile::tempdir().unwrap();
    let before = "class A:\n    def method(self): pass\nclass B(A):\n    def method(self): pass\nclass C(A):\n    def method(self): pass\nclass D(B, C): pass\n";
    let after = before.replace("class D(B, C)", "class D(C, B)");
    write(&root.path().join("model.py"), before);
    let diff = root.path().join("change.diff");
    write(&diff, patch("model.py", Some(before), Some(&after)));
    let result = report(&run(
        "candidate",
        root.path(),
        Some(&diff),
        Some("structure"),
    ));
    let derived = declaration(&result, "D");
    let method = derived["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == "method")
        .unwrap();
    assert!(
        method["origin"].as_str().unwrap().contains("C::method"),
        "{derived:#}"
    );
    assert!(
        result["structure"]["delta"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "effective"
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().ends_with(":D")))
    );
    let event = result["structure"]["delta"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| {
            event["kind"] == "effective"
                && event["units"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|unit| unit.as_str().unwrap().ends_with(":D"))
        })
        .unwrap();
    assert!(
        event["changed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["ordered"] == true),
        "{event:#}"
    );
}
