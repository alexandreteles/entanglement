use support::write;
mod support;
use support::{file, function, recursive};

use std::path::Path;

use serde_json::Value;

fn resolution_status<'a>(file: &'a Value, name: &str) -> &'a str {
    file["resolution"]
        .as_array()
        .expect("reference resolutions")
        .iter()
        .find(|item| {
            item["path"]
                .as_array()
                .is_some_and(|path| path.iter().any(|part| part == name))
        })
        .unwrap_or_else(|| panic!("missing reference {name}"))["resolution"]["status"]
        .as_str()
        .expect("resolution status")
}

fn package(root: &Path) {
    write(&root.join("package.json"), r#"{"name":"review-fixture"}"#);
}

#[test]
fn cyclic_star_branch_keeps_leaf_resolution_and_recursion_edges() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    package(&root);
    write(
        &root.join("src/main.ts"),
        "import { leaf } from './barrel';\nexport function entry(flag: boolean) { if (flag) leaf(false); }\n",
    );
    write(
        &root.join("src/barrel.ts"),
        "export * from './cycle-a';\nexport * from './leaf';\n",
    );
    write(&root.join("src/cycle-a.ts"), "export * from './cycle-b';\n");
    write(&root.join("src/cycle-b.ts"), "export * from './cycle-a';\n");
    write(
        &root.join("src/leaf.ts"),
        "import { entry } from './main';\nexport function leaf(flag: boolean) { if (flag) entry(false); }\n",
    );

    let report = support::report(&support::run("repo", &root, None, Some("cogc")));
    let main = file(&report, "/main.ts");
    let leaf = file(&report, "/leaf.ts");
    assert_eq!(resolution_status(main, "leaf"), "exact");
    let leaf_reference = main["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["path"]
                .as_array()
                .is_some_and(|path| path.iter().any(|part| part == "leaf"))
        })
        .expect("leaf reference");
    assert_eq!(leaf_reference["resolution"]["symbols"]["name"], "leaf");
    assert!(
        leaf_reference["resolution"]["symbols"]["file"]
            .as_str()
            .is_some_and(|path| path.ends_with("/src/leaf.ts"))
    );
    assert_eq!(resolution_status(leaf, "entry"), "exact");
    assert!(recursive(function(main, "entry")));
    assert!(recursive(function(leaf, "leaf")));
}

#[test]
fn pure_star_cycles_stay_unresolved_and_conflicting_stars_stay_ambiguous() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    package(&root);
    write(
        &root.join("src/main.ts"),
        "import { absent } from './cycle-a';\nimport { duplicate } from './conflict';\nfunction cycleCaller() { absent(); }\nfunction conflictCaller() { duplicate(); }\n",
    );
    write(&root.join("src/cycle-a.ts"), "export * from './cycle-b';\n");
    write(&root.join("src/cycle-b.ts"), "export * from './cycle-a';\n");
    write(
        &root.join("src/first.ts"),
        "export function duplicate() {}\n",
    );
    write(
        &root.join("src/second.ts"),
        "export function duplicate() {}\n",
    );
    write(
        &root.join("src/conflict.ts"),
        "export * from './cycle-a';\nexport * from './first';\nexport * from './second';\n",
    );

    let report = support::report(&support::run("repo", &root, None, Some("cogc")));
    let main = file(&report, "/main.ts");
    assert_eq!(resolution_status(main, "absent"), "unresolved");
    assert_eq!(resolution_status(main, "duplicate"), "ambiguous");
}

#[test]
fn missing_and_external_star_branches_remain_conservative() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    package(&root);
    write(
        &root.join("src/main.ts"),
        "import { leaf as missingBranch } from './missing-branch';\nimport { leaf as externalBranch } from './external-branch';\nfunction missingCaller() { missingBranch(); }\nfunction externalCaller() { externalBranch(); }\n",
    );
    write(&root.join("src/leaf.ts"), "export function leaf() {}\n");
    write(
        &root.join("src/missing-branch.ts"),
        "export * from './leaf';\nexport * from './not-present';\n",
    );
    write(
        &root.join("src/external-branch.ts"),
        "export * from './leaf';\nexport * from 'external-package';\n",
    );

    let report = support::report(&support::run("repo", &root, None, Some("cogc")));
    let main = file(&report, "/main.ts");
    assert_eq!(resolution_status(main, "missingBranch"), "unresolved");
    assert_eq!(resolution_status(main, "externalBranch"), "unresolved");
}
