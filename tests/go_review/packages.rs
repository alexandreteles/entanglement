use super::{file, report, resolutions, run, status, write};

#[test]
fn production_imports_exclude_test_only_members_not_package_name_suffixes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(&root.join("go.mod"), "module example.com/app\n");
    write(
        &root.join("dep/dep.go"),
        "package dep\nfunc Prod() {}\nfunc Use() { TestOnly() }\n",
    );
    write(
        &root.join("dep/dep_test.go"),
        "package dep\nfunc TestOnly() {}\nfunc TestUse() { Prod(); TestOnly() }\n",
    );
    write(
        &root.join("normal/normal.go"),
        "package normal_test\nfunc F() {}\n",
    );
    write(
        &root.join("main.go"),
        "package app\nimport (\"example.com/app/dep\"; \"example.com/app/normal\")\nfunc Use() { dep.Prod(); dep.TestOnly(); normal_test.F() }\n",
    );
    let output = report(&run(&["repo"], root, None));
    let main = resolutions(file(&output, "main.go"));
    assert_eq!(status(&main, "dep.Prod"), ["exact dep.go:Prod"]);
    assert_eq!(status(&main, "dep.TestOnly"), ["unresolved"]);
    assert_eq!(status(&main, "normal_test.F"), ["exact normal.go:F"]);
    assert_eq!(
        status(&resolutions(file(&output, "dep/dep.go")), "TestOnly"),
        ["unresolved"]
    );
    assert_eq!(
        status(&resolutions(file(&output, "dep_test.go")), "TestOnly"),
        ["exact dep_test.go:TestOnly"]
    );
}

#[test]
fn conditional_inventory_never_claims_a_unique_build_dependent_target() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("main.go"),
        "package app\nfunc Use() { Platform(); Tagged(); Duplicate() }\n",
    );
    write(
        &root.join("platform_linux.go"),
        "package app\nfunc Platform() {}\nfunc Duplicate() {}\n",
    );
    write(
        &root.join("platform_windows.go"),
        "package app\nfunc Duplicate() {}\n",
    );
    write(
        &root.join("tagged.go"),
        "//go:build custom\n\npackage app\nfunc Tagged() {}\n",
    );
    let output = report(&run(&["repo"], root, None));
    let refs = resolutions(file(&output, "main.go"));
    assert_eq!(status(&refs, "Platform"), ["unresolved"]);
    assert_eq!(status(&refs, "Tagged"), ["unresolved"]);
    assert_eq!(status(&refs, "Duplicate"), ["ambiguous"]);
    assert_eq!(output["files"].as_array().unwrap().len(), 4);
}

#[test]
fn nested_and_malformed_modules_do_not_inherit_parent_identity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(&root.join("go.mod"), "module example.com/app\n");
    write(&root.join("dep/dep.go"), "package dep\nfunc F() {}\n");
    write(&root.join("sub/go.mod"), "module example.com/other\n");
    write(&root.join("sub/sub.go"), "package sub\nfunc F() {}\n");
    write(&root.join("broken/go.mod"), "go 1.22\n");
    write(
        &root.join("broken/broken.go"),
        "package broken\nimport \"example.com/app/dep\"\nfunc F() { dep.F() }\n",
    );
    write(
        &root.join("main.go"),
        "package app\nimport \"example.com/app/sub\"\nfunc Use() { sub.F() }\n",
    );
    let output = report(&run(&["repo"], root, None));
    assert_eq!(
        status(&resolutions(file(&output, "main.go")), "sub.F"),
        ["unresolved"]
    );
    assert_eq!(
        status(&resolutions(file(&output, "broken.go")), "dep.F"),
        ["unresolved"]
    );
}

#[test]
fn escaped_import_paths_and_quoted_module_paths_resolve_by_value() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (module, import) in [
        (r#""example.com/app""#, r#""example.com/\x61pp/dep""#),
        (r#""example.com/\x61pp""#, r#""example.com/app/dep""#),
    ] {
        write(
            &root.join("go.mod"),
            &format!("module {module} // decoded\n"),
        );
        write(&root.join("dep/dep.go"), "package dep\nfunc F() {}\n");
        write(
            &root.join("main.go"),
            &format!("package app\nimport {import}\nfunc Use() {{ dep.F() }}\n"),
        );
        let output = report(&run(&["repo"], root, None));
        assert_eq!(
            status(&resolutions(file(&output, "main.go")), "dep.F"),
            ["exact dep.go:F"]
        );
    }
}

#[test]
fn unrelated_embedded_go_programs_do_not_share_package_members_or_imports() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("programs.html");
    write(
        &path,
        r#"<script lang="go">
package first
import "fmt"
func A() { fmt.Println("a") }
</script>
<script lang="go">
package second
func B() { A(); fmt.Println("b") }
</script>
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let refs = resolutions(&output["files"][0]);
    assert_eq!(status(&refs, "A"), ["unresolved"]);
    let fmt = status(&refs, "fmt.Println");
    assert!(
        fmt.contains(&"external") && fmt.contains(&"unresolved"),
        "{fmt:?}"
    );
}

#[cfg(unix)]
#[test]
fn divergent_logical_package_contexts_cannot_collapse_to_one_exact_target() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("a/shared.go"),
        "package shared\nfunc Use() { Helper() }\n",
    );
    write(
        &root.join("a/helper.go"),
        "package shared\nfunc Helper() {}\n",
    );
    std::fs::create_dir_all(root.join("b")).unwrap();
    std::os::unix::fs::symlink(root.join("a/shared.go"), root.join("b/shared.go")).unwrap();
    let output = report(&run(&["repo"], root, None));
    let refs = resolutions(file(&output, "shared.go"));
    assert_eq!(status(&refs, "Helper"), ["unresolved"]);
}
