use super::support;
use super::{file, report, resolutions, status, write};

#[test]
fn module_path_edits_use_virtual_manifest_bytes_and_match_fresh_analysis() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    let before_mod = "module example.com/old\n";
    let after_mod = "module example.com/new\n";
    let before_go = "package app\nimport \"example.com/old/dep\"\nfunc Use() { dep.F() }\n";
    let after_go = before_go.replace("example.com/old", "example.com/new");
    write(&root.join("go.mod"), before_mod);
    write(&root.join("main.go"), before_go);
    write(&root.join("dep/dep.go"), "package dep\nfunc F() {}\n");
    let diff = temp.path().join("change.diff");
    write(
        &diff,
        &(support::patch("go.mod", Some(before_mod), Some(after_mod))
            + &support::patch("main.go", Some(before_go), Some(&after_go))),
    );
    let candidate = report(&support::run("candidate", &root, Some(&diff), None));
    assert_eq!(
        std::fs::read_to_string(root.join("go.mod")).unwrap(),
        before_mod
    );
    assert_eq!(
        status(&resolutions(file(&candidate, "main.go")), "dep.F"),
        ["exact dep.go:F"]
    );
    write(&root.join("go.mod"), after_mod);
    write(&root.join("main.go"), &after_go);
    let fresh = report(&support::run("repo", &root, None, None));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn nested_manifest_creation_and_deletion_use_the_candidate_snapshot() {
    const MODULE: &str = "module example.com/other\n";
    for (before, after, expected) in [
        (None, Some(MODULE), "unresolved"),
        (Some(MODULE), None, "exact sub.go:F"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        write(&root.join("go.mod"), "module example.com/app\n");
        write(&root.join("sub/sub.go"), "package sub\nfunc F() {}\n");
        write(
            &root.join("main.go"),
            "package app\nimport \"example.com/app/sub\"\nfunc Use() { sub.F() }\n",
        );
        let manifest = root.join("sub/go.mod");
        if let Some(source) = before {
            write(&manifest, source);
        }
        let diff = temp.path().join("change.diff");
        write(&diff, &support::patch("sub/go.mod", before, after));
        let candidate = report(&support::run("candidate", &root, Some(&diff), None));
        assert_eq!(manifest.exists(), before.is_some());
        assert_eq!(std::fs::read_to_string(&manifest).ok().as_deref(), before);
        assert_eq!(
            status(&resolutions(file(&candidate, "main.go")), "sub.F"),
            [expected]
        );
        match after {
            Some(source) => write(&manifest, source),
            None => std::fs::remove_file(&manifest).unwrap(),
        }
        let fresh = report(&support::run("repo", &root, None, None));
        assert_eq!(candidate["files"], fresh["files"]);
    }
}
