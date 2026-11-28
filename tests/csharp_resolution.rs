mod support;

use support::{file, function, patch, recursive, references_to, report, run, write};

#[test]
fn resolves_extensions_overloads_and_virtual_dispatch_with_project_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let library = "<Project Sdk=\"Microsoft.NET.Sdk\"/>";
    write(&root.join("Lib/Lib.csproj"), library);
    write(&root.join("Other/Other.csproj"), library);
    let implementation = "namespace Lib; public class Row { } public static class Rows { public static int Count(this Row row, bool visible = true) => 1; public static int Count(this Row row, string text) => 2; }";
    write(&root.join("Lib/Rows.cs"), implementation);
    write(&root.join("Other/Rows.cs"), implementation);
    let project = "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><ProjectReference Include=\"../Lib/Lib.csproj\"/></ItemGroup></Project>";
    write(&root.join("App/App.csproj"), project);
    write(
        &root.join("App/Global.cs"),
        "global using R = Lib.Row; global using Lib;",
    );
    write(
        &root.join("App/App.cs"),
        "interface I { int Run(); } class App { R NewRow() => new R(); int Read(I item) { var row = NewRow(); return row.Count(visible: true) + item.Run(); } int Direct() => Direct(); }",
    );
    let data = report(&run("repo", root, None, Some("cogc")));
    let app = file(&data, "App.cs");
    let calls = references_to(app, "Count");
    assert_eq!(calls[0]["resolution"]["status"], "exact");
    assert!(
        calls[0]["resolution"]["symbols"]["file"]
            .as_str()
            .unwrap()
            .ends_with("Lib/Rows.cs")
    );
    assert_eq!(
        references_to(app, "Run")[0]["resolution"]["status"],
        "exact"
    );
    assert!(recursive(function(app, "Direct")));
    assert!(!recursive(function(app, "Read")));
    let diff = root.join("ref.diff");
    write(&diff, patch("App/App.csproj", Some(project), Some(library)));
    let candidate = report(&run("candidate", root, Some(&diff), Some("cogc")));
    assert_ne!(
        references_to(file(&candidate, "App.cs"), "Count")[0]["resolution"]["status"],
        "exact"
    );
    write(&root.join("App/App.csproj"), library);
    assert_eq!(
        candidate["files"],
        report(&run("repo", root, None, Some("cogc")))["files"]
    );
}

#[test]
fn conditional_targets_do_not_create_recursion_across_configurations() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("App.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFrameworks>netstandard2.0;netstandard2.1</TargetFrameworks></PropertyGroup></Project>",
    );
    write(
        &root.join("Branches.cs"),
        "class Branches {\n#if NETSTANDARD2_0\n int A() => B();\n int B() => 0;\n#else\n int A() => 0;\n int B() => A();\n#endif\n}\n",
    );
    let result = report(&run("repo", root, None, Some("cogc,structure")));
    let branches = file(&result, "Branches.cs");
    assert_eq!(branches["functions"].as_array().unwrap().len(), 4);
    assert!(
        !branches["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(recursive)
    );
    for name in ["A", "B"] {
        assert_eq!(
            references_to(branches, name)[0]["resolution"]["status"],
            "exact"
        );
    }
}

#[test]
fn opposite_conditional_calls_in_shared_functions_are_not_a_cycle() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("App.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"/>",
    );
    write(
        &dir.path().join("Branches.cs"),
        "class C { void A() {\n#if DEBUG\n B();\n#endif\n } void B() {\n#if !DEBUG\n A();\n#endif\n } }",
    );
    let output = report(&run("repo", dir.path(), None, Some("cogc")));
    let target = file(&output, "Branches.cs");
    for name in ["A", "B"] {
        assert_eq!(
            references_to(target, name)[0]["resolution"]["status"],
            "exact"
        );
        assert!(!recursive(function(target, name)));
    }
}

#[test]
fn a_project_reference_selects_the_nearest_compatible_framework() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("App/App.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup><ItemGroup><ProjectReference Include=\"../Lib/Lib.csproj\"/></ItemGroup></Project>",
    );
    write(
        &root.join("Lib/Lib.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFrameworks>netstandard2.0;netstandard2.1</TargetFrameworks></PropertyGroup></Project>",
    );
    let library = "public class Api {\n#if NETSTANDARD2_1\n public static int Get() => 21;\n#else\n public static int Get() => 20;\n#endif\n}";
    write(&root.join("Lib/Api.cs"), library);
    write(
        &root.join("App/Use.cs"),
        "class Use { int F() => Api.Get(); }",
    );
    let output = report(&run("repo", root, None, Some("cogc")));
    let call = &references_to(file(&output, "Use.cs"), "Get")[0]["resolution"];
    assert_eq!(call["status"], "exact");
    assert_eq!(
        call["symbols"]["start_byte"],
        library.find("public static").unwrap()
    );
}
