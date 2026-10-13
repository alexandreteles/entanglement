use super::*;

#[test]
fn declaration_only_primary_constructors_have_no_executable_metric_scope() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("declarations.kt");
    write(
        &path,
        concat!(
            "expect class ExpectedApi()\n",
            "external class ExternalApi()\n",
            "abstract class AbstractBase()\n",
        ),
    );

    let output = report(&run("file", &path, None, Some("cc")));
    let file = &output["files"][0];
    let functions = file["functions"].as_array().unwrap();
    assert!(!functions.iter().any(|item| item["name"] == "ExpectedApi"));
    assert!(!functions.iter().any(|item| item["name"] == "ExternalApi"));
    assert!(
        functions.iter().any(|item| item["name"] == "AbstractBase"),
        "an abstract class still has an executable primary constructor scope"
    );
}
