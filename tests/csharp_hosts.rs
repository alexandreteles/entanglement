mod support;
use support::{file, function, patch, references_to, report, run, write};

#[test]
fn scripts_follow_relative_loads_and_keep_directive_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let script = "#r \"System.Xml\"\n#load \"sub/Helpers.csx\"\nint Run() => Twice(2);\n";
    write(&root.join("Entry.csx"), script);
    write(
        &root.join("sub/Helpers.csx"),
        "#load \"../Entry.csx\"\nint Twice(int n) => n+n;\n",
    );
    write(&root.join("Other.csx"), "int Twice(int n) => 0;\n");
    let data = report(&run("repo", root, None, Some("cc,structure")));
    let main = file(&data, "Entry.csx");
    assert_eq!(main["nloc"], 3);
    assert_eq!(
        function(main, "Run")["start_byte"],
        script.find("int Run").unwrap()
    );
    assert_eq!(
        references_to(main, "Twice")[0]["resolution"]["status"],
        "exact"
    );
    let relations = data["structure"]["relationships"].as_array().unwrap();
    assert!(
        relations
            .iter()
            .any(|edge| edge["kind"] == "import" && edge["resolution"] == "external")
    );
    let diff = root.join("load.diff");
    write(
        &diff,
        patch(
            "Entry.csx",
            Some(script),
            Some(&script.replace("sub/Helpers.csx", "Missing.csx")),
        ),
    );
    let after = report(&run("candidate", root, Some(&diff), Some("cc,structure")));
    assert_ne!(
        references_to(file(&after, "Entry.csx"), "Twice")[0]["resolution"]["status"],
        "exact"
    );
}
