mod support;
use support::{file, function, recursive, references_to, report, run, write};

#[test]
fn fields_attributes_indexers_and_accessor_execution_keep_source_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Page.cs");
    write(
        &path,
        "class FlagAttribute : System.Attribute {} [Flag] class Page { private int size; int Shared => size; int this[int index] => Shared; int Count { get => Count; } int Read(Page other) => other[0]; }",
    );
    let output = report(&run("file", &path, None, Some("cogc,structure")));
    let code = file(&output, "Page.cs");
    for name in ["Flag", "size", "Shared", "this", "Count"] {
        assert_eq!(
            references_to(code, name)[0]["resolution"]["status"],
            "exact",
            "{name}"
        );
    }
    assert!(recursive(function(code, "get Count")));
}

#[test]
fn dynamic_calls_and_hidden_static_members_do_not_add_recursion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Dispatch.cs");
    write(
        &path,
        "class A { private int Hidden() => 1; public virtual int Virtual() => 1; public int Run(dynamic target) => target.Run(target); } class B : A { public override int Virtual() => base.Virtual(); int Read(A other) => other.Hidden(); }",
    );
    let output = report(&run("file", &path, None, Some("cogc")));
    let code = file(&output, "Dispatch.cs");
    assert_ne!(
        references_to(code, "Run")[0]["resolution"]["status"],
        "exact"
    );
    assert_ne!(
        references_to(code, "Hidden")[0]["resolution"]["status"],
        "exact"
    );
    assert_eq!(
        references_to(code, "Virtual")[0]["resolution"]["status"],
        "exact"
    );
    assert!(!recursive(function(code, "Run")));
}

#[test]
fn records_primary_constructors_and_extension_blocks_share_the_receiver_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Record.cs");
    write(
        &path,
        "public record Row(int Size); public static class Extensions { extension(Row row) { public int Twice() => row.Size * 2; } } class Example { int Score() { Row row = new(1); return row.Twice() + row.Size; } }",
    );
    let output = report(&run("file", &path, None, Some("cogc,structure")));
    let code = file(&output, "Record.cs");
    for name in ["Twice", "Size"] {
        let uses = references_to(code, name);
        assert!(!uses.is_empty());
        assert!(
            uses.iter().all(|r| r["resolution"]["status"] == "exact"),
            "{name}: {uses:?}"
        );
    }
    let record = output["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["unit"].as_str().unwrap().ends_with(":Row"))
        .unwrap();
    assert!(
        record["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "Size" && m["visibility"] == "public")
    );
}
