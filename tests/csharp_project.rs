mod support;
use support::{file, patch, references_to, report, run, write};

#[test]
fn sdk_framework_and_platform_symbols_select_the_compiled_branch() {
    let dir = tempfile::tempdir().unwrap();
    for (project, framework, condition) in [
        (
            "Desktop",
            "net481",
            "NETFRAMEWORK && NET472_OR_GREATER && !NET",
        ),
        (
            "Mobile",
            "net10.0-android35.0",
            "NET && NETCOREAPP3_1_OR_GREATER && ANDROID35_0",
        ),
    ] {
        let root = dir.path().join(project);
        write(
            &root.join("Example.csproj"),
            format!(
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>{framework}</TargetFramework></PropertyGroup></Project>"
            ),
        );
        let code = format!(
            "class C {{\n#if {condition}\n int Value() => 1;\n#else\n int Value() => 0;\n#endif\n int Read() => Value();\n}}"
        );
        write(&root.join("Example.cs"), &code);
        let output = report(&run("repo", &root, None, Some("cogc")));
        let call = &references_to(file(&output, "Example.cs"), "Value")[0]["resolution"];
        assert_eq!(call["status"], "exact");
        assert_eq!(
            call["symbols"]["start_byte"],
            code.find("int Value").unwrap()
        );
    }
}

#[test]
fn compile_items_imports_and_virtual_manifests_select_the_same_sources_as_a_fresh_scan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let manifest =
        "<Project Sdk=\"Microsoft.NET.Sdk\"><Import Project=\"Rules.targets\"/></Project>";
    let rules = "<Project><ItemGroup><Compile Remove=\"Excluded/**/*.cs\"/><Compile Include=\"../Shared/*.cs\" Exclude=\"../Shared/Other.cs\"/></ItemGroup></Project>";
    write(&root.join("App/App.csproj"), manifest);
    write(&root.join("App/Rules.targets"), rules);
    write(
        &root.join("App/Use.cs"),
        "class Use { int Run() => Shared.Get(); }",
    );
    for name in [
        "App/Excluded/Copy.cs",
        "Shared/Other.cs",
        "Shared/Target.cs",
    ] {
        write(
            &root.join(name),
            "public class Shared { public static int Get() => 1; }",
        );
    }
    let scan = report(&run("repo", root, None, Some("structure")));
    let call = &references_to(file(&scan, "Use.cs"), "Get")[0]["resolution"];
    assert_eq!(call["status"], "exact");
    assert!(
        call["symbols"]["file"]
            .as_str()
            .unwrap()
            .ends_with("Shared/Target.cs")
    );
    let new_rules = rules.replace("../Shared/*.cs", "../Shared/Absent.cs");
    let diff = root.join("change.diff");
    write(
        &diff,
        patch("App/Rules.targets", Some(rules), Some(&new_rules)),
    );
    let after = report(&run("candidate", root, Some(&diff), Some("structure")));
    write(&root.join("App/Rules.targets"), new_rules);
    let fresh = report(&run("repo", root, None, Some("structure")));
    assert_eq!(after["files"], fresh["files"]);
    assert_eq!(
        after["structure"]["effective"],
        fresh["structure"]["effective"]
    );
    assert_ne!(
        references_to(file(&fresh, "Use.cs"), "Get")[0]["resolution"]["status"],
        "exact"
    );
}
#[test]
fn directory_properties_choose_compile_items_and_the_sdk_sets_configuration_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("Directory.Build.props"),
        "<Project><PropertyGroup><DefaultItemExcludes>$(DefaultItemExcludes);Copies/**</DefaultItemExcludes></PropertyGroup></Project>",
    );
    write(
        &root.join("App/App.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><Choose><When Condition=\"'$(Configuration)' == 'Release'\"><ItemGroup><Compile Include=\"../Shared/Release.cs\"/></ItemGroup></When></Choose></Project>",
    );
    write(
        &root.join("App/Call.cs"),
        "class C {\n#if RELEASE && TRACE\n int F() => Answer.Get();\n#endif\n}",
    );
    for name in ["Shared/Release.cs", "App/Copies/Duplicate.cs"] {
        write(
            &root.join(name),
            "public static class Answer { public static int Get() => 42; }",
        );
    }
    let result = report(&run("repo", root, None, Some("cogc")));
    let call = &references_to(file(&result, "Call.cs"), "Get")[0]["resolution"];
    assert_eq!(call["status"], "exact");
    assert!(
        call["symbols"]["file"]
            .as_str()
            .unwrap()
            .ends_with("Shared/Release.cs")
    );
}
