use super::*;

#[test]
fn module_resolution_follows_import_forms_reexports_and_respects_shadowing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("modules");
    write(
        &root.join("package.json"),
        "{\"name\":\"module-fixture\"}\n",
    );
    let main_source = concat!(
        "import defaultCall, { lookup as aliasCall } from './barrel';\n",
        "import * as rightModule from './right';\n",
        "import { publicLeaf } from './left';\n",
        "import { duplicate } from './collision';\n",
        "import { ns } from './barrel';\n",
        "export function aliasCycle(flag: boolean) { if (flag) defaultCall(); aliasCall(); rightModule.right(); publicLeaf(); ns.duplicate(); }\n",
        "export function shadowed() { const aliasCall = () => {}; aliasCall(); }\n",
        "export function loopShadow() {\n",
        "  for (const aliasCall of [() => {}]) {\n",
        "    aliasCall();\n",
        "  }\n",
        "  aliasCall();\n",
        "}\n",
        "export function ambiguous() { duplicate(); }\n",
    );
    write(&root.join("src/main.ts"), main_source);
    write(
        &root.join("src/barrel.ts"),
        "export { left, left as entry } from './left';\nexport { right as lookup, default } from './right';\nexport * as ns from './one';\n",
    );
    write(
        &root.join("src/left.ts"),
        "import { aliasCycle } from './main';\nimport { leaf as local } from './widget.test';\nexport { local as publicLeaf };\nexport function left() { aliasCycle(true); }\n",
    );
    write(
        &root.join("src/right.ts"),
        "import { left } from './barrel';\nimport { shadowed } from './main';\nexport function right() { left(); shadowed(); }\nexport default function defaultCall() { left(); }\n",
    );
    write(&root.join("src/one.ts"), "export function duplicate() {}\n");
    write(&root.join("src/two.ts"), "export function duplicate() {}\n");
    write(
        &root.join("src/collision.ts"),
        "export * from './one';\nexport * from './two';\n",
    );
    write(
        &root.join("src/widget.test.ts"),
        "export function leaf() {}\n",
    );
    write(
        &root.join("src/dotted.ts"),
        "import { leaf } from './widget.test';\nexport function dottedCaller() { leaf(); }\n",
    );

    let project = report(&run("repo", &root, None, Some("cogc")));
    let main = file(&project, "/main.ts");
    assert!(recursive(function(main, "aliasCycle")));
    assert!(!recursive(function(main, "shadowed")));
    assert!(has_resolution(main, "aliasCall", "exact"));
    assert!(has_resolution(main, "defaultCall", "exact"));
    assert!(has_resolution(main, "right", "exact"));
    assert!(has_resolution(main, "publicLeaf", "exact"));
    assert!(main["resolution"].as_array().unwrap().iter().any(|item| {
        item["path"] == serde_json::json!(["ns", "duplicate"])
            && item["resolution"]["status"] == "exact"
            && item["resolution"]["symbols"]["file"]
                .as_str()
                .is_some_and(|path| path.ends_with("/one.ts"))
    }));

    let loop_start = main_source.find("export function loopShadow").unwrap();
    let loop_source = &main_source[loop_start..];
    let first_call = loop_source.find("aliasCall();").unwrap() + loop_start;
    let after_loop_call = loop_source[first_call - loop_start + "aliasCall();".len()..]
        .find("aliasCall();")
        .unwrap()
        + first_call
        + "aliasCall();".len();
    let resolution_at = |start_byte: usize| {
        main["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["start_byte"].as_u64() == Some(start_byte as u64))
            .unwrap()
    };
    assert_ne!(resolution_at(first_call)["resolution"]["status"], "exact");
    let after_loop = resolution_at(after_loop_call);
    assert_eq!(after_loop["resolution"]["status"], "exact");
    assert!(
        after_loop["resolution"]["symbols"]["file"]
            .as_str()
            .is_some_and(|path| path.ends_with("/right.ts"))
    );
    assert!(has_resolution(main, "duplicate", "ambiguous"));
    assert!(has_resolution(
        file(&project, "/dotted.ts"),
        "leaf",
        "exact"
    ));
}
