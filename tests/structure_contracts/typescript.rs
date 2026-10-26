use super::*;

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
