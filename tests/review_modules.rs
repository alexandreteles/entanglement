use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn write(root: &Path, name: &str, source: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
    std::fs::write(path, source).unwrap();
}

fn analyze(root: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "--metrics", "cogc", "repo"])
        .arg(root)
        .output()
        .expect("run entanglement");
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .expect("file reports")
        .iter()
        .find(|item| {
            item["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("missing file ending in {suffix}"))
}

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

fn is_recursive(file: &Value, name: &str) -> bool {
    file["functions"]
        .as_array()
        .expect("function reports")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

fn package(root: &Path) {
    write(root, "package.json", r#"{"name":"review-fixture"}"#);
}

#[test]
fn cyclic_star_branch_keeps_leaf_resolution_and_recursion_edges() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    package(&root);
    write(
        &root,
        "src/main.ts",
        "import { leaf } from './barrel';\nexport function entry(flag: boolean) { if (flag) leaf(false); }\n",
    );
    write(
        &root,
        "src/barrel.ts",
        "export * from './cycle-a';\nexport * from './leaf';\n",
    );
    write(&root, "src/cycle-a.ts", "export * from './cycle-b';\n");
    write(&root, "src/cycle-b.ts", "export * from './cycle-a';\n");
    write(
        &root,
        "src/leaf.ts",
        "import { entry } from './main';\nexport function leaf(flag: boolean) { if (flag) entry(false); }\n",
    );

    let report = analyze(&root);
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
    assert!(is_recursive(main, "entry"));
    assert!(is_recursive(leaf, "leaf"));
}

#[test]
fn pure_star_cycles_stay_unresolved_and_conflicting_stars_stay_ambiguous() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    package(&root);
    write(
        &root,
        "src/main.ts",
        "import { absent } from './cycle-a';\nimport { duplicate } from './conflict';\nfunction cycleCaller() { absent(); }\nfunction conflictCaller() { duplicate(); }\n",
    );
    write(&root, "src/cycle-a.ts", "export * from './cycle-b';\n");
    write(&root, "src/cycle-b.ts", "export * from './cycle-a';\n");
    write(&root, "src/first.ts", "export function duplicate() {}\n");
    write(&root, "src/second.ts", "export function duplicate() {}\n");
    write(
        &root,
        "src/conflict.ts",
        "export * from './cycle-a';\nexport * from './first';\nexport * from './second';\n",
    );

    let report = analyze(&root);
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
        &root,
        "src/main.ts",
        "import { leaf as missingBranch } from './missing-branch';\nimport { leaf as externalBranch } from './external-branch';\nfunction missingCaller() { missingBranch(); }\nfunction externalCaller() { externalBranch(); }\n",
    );
    write(&root, "src/leaf.ts", "export function leaf() {}\n");
    write(
        &root,
        "src/missing-branch.ts",
        "export * from './leaf';\nexport * from './not-present';\n",
    );
    write(
        &root,
        "src/external-branch.ts",
        "export * from './leaf';\nexport * from 'external-package';\n",
    );

    let report = analyze(&root);
    let main = file(&report, "/main.ts");
    assert_eq!(resolution_status(main, "missingBranch"), "unresolved");
    assert_eq!(resolution_status(main, "externalBranch"), "unresolved");
}
