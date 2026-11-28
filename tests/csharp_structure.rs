mod support;
use support::{file, references_to, report, run, write};

#[test]
fn a_parse_error_keeps_its_range_and_the_other_declarations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Broken.cs");
    write(
        &path,
        "class Good { int Value() => 1; }\nclass Broken { void Missing( { }\n",
    );
    let result = report(&run("file", &path, None, Some("structure")));
    let coverage = &result["structure"]["coverage"][0];
    assert_eq!(coverage["status"], "partial");
    assert!(
        coverage["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().starts_with("parse_error:"))
    );
    assert!(
        result["structure"]["effective"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["unit"].as_str().unwrap().ends_with(":Good"))
    );
}

#[test]
fn a_broken_signature_cannot_supply_an_exact_call_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Broken.cs");
    write(
        &path,
        "class C { void Broken(int x = ) {} void Fine() {} void Run() { Fine(); Broken(1); } }",
    );
    let data = report(&run("repo", dir.path(), None, Some("structure")));
    let source = file(&data, "Broken.cs");
    assert_eq!(
        references_to(source, "Fine")[0]["resolution"]["status"],
        "exact"
    );
    assert_ne!(
        references_to(source, "Broken")[0]["resolution"]["status"],
        "exact"
    );
}

#[test]
fn partial_types_and_linked_sources_have_assembly_identity_and_source_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sdk = "<Project Sdk=\"Microsoft.NET.Sdk\"/>";
    write(&root.join("App/App.csproj"), sdk);
    write(
        &root.join("App/First.cs"),
        "namespace N; public interface I { int Get(); } public partial class Part : I { public int Get() => More(); }",
    );
    write(
        &root.join("App/Second.cs"),
        "namespace N; public partial class Part { private int More() => 2; }",
    );
    write(
        &root.join("Tests/Tests.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><Compile Include=\"../App/Second.cs\" Link=\"Shared/Second.cs\"/></ItemGroup></Project>",
    );
    let data = report(&run("repo", root, None, Some("structure")));
    assert_eq!(data["files"].as_array().unwrap().len(), 2);
    let surface = data["structure"]["effective"].as_array().unwrap();
    let parts: Vec<_> = surface
        .iter()
        .filter(|item| item["unit"].as_str().unwrap().ends_with(":N::Part"))
        .collect();
    assert_eq!(parts.len(), 2);
    let app = parts
        .iter()
        .find(|item| item["evidence"].as_array().unwrap().len() == 2)
        .unwrap();
    assert!(
        app["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "More")
    );
    let edges = data["structure"]["relationships"].as_array().unwrap();
    assert!(edges.iter().any(|edge| edge["source"] == app["unit"]
        && edge["kind"] == "implement"
        && edge["resolution"] == "resolved"));
}

#[test]
fn union_parts_share_one_type_and_keep_all_case_types() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("Union.cs"),
        "partial union Value(int, string);\npartial union Value;\n",
    );
    let data = report(&run("repo", dir.path(), None, Some("structure")));
    let effective = data["structure"]["effective"].as_array().unwrap();
    assert_eq!(effective.len(), 1);
    assert_eq!(effective[0]["evidence"].as_array().unwrap().len(), 2);
    assert!(effective[0]["conflicts"].as_array().unwrap().is_empty());
    let cases = effective[0]["types"].as_array().unwrap();
    assert!(cases.iter().any(|t| t["written"]["text"] == "int"));
    assert!(cases.iter().any(|t| t["written"]["text"] == "string"));
}

#[test]
fn explicit_interface_implementations_and_inherited_members_keep_separate_contracts() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("Family.cs"),
        "public interface I { int Count(int n); } public class Base { protected virtual int Score() => 1; private int Hidden() => 2; } public class Page : Base, I { int I.Count(int n) => n; protected override int Score() => 3; public required string Title { get; init; } }",
    );
    let result = report(&run("repo", dir.path(), None, Some("structure")));
    let page = result["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["unit"].as_str().unwrap().ends_with(":Page"))
        .unwrap();
    assert_eq!(page["obligations"].as_array().unwrap().len(), 0);
    let members = page["members"].as_array().unwrap();
    let count = members.iter().find(|m| m["name"] == "Count").unwrap();
    assert_eq!(count["contracts"].as_array().unwrap().len(), 1);
    assert!(
        members
            .iter()
            .any(|m| m["name"] == "Title" && m["required"] == true)
    );
    assert!(!members.iter().any(|m| m["name"] == "Hidden"));
}

#[test]
fn using_directives_retain_source_and_external_import_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("Lib.cs"), "namespace Lib; public class Value {}");
    write(
        &root.join("Use.cs"),
        "using System; using Row = Lib.Value; public class Use { public Row Read() => new Row(); }",
    );
    let output = report(&run("repo", root, None, Some("structure")));
    let edges = output["structure"]["relationships"].as_array().unwrap();
    assert!(edges.iter().any(|edge| edge["kind"] == "import"
        && edge["resolution"] == "external"
        && edge["target"] == "System"));
    assert!(edges.iter().any(|edge| edge["kind"] == "import"
        && edge["resolution"] == "resolved"
        && edge["target"].as_str().unwrap().ends_with("Lib.cs")));
}
