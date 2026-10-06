use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(mode: &str, path: &Path, diff: Option<&Path>, metrics: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json"]);
    if let Some(metrics) = metrics {
        command.args(["--metrics", metrics]);
    }
    command.arg(mode).arg(path);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    command.output().expect("run entanglement")
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn analyze_source(root: &Path, name: &str, source: &str) -> Value {
    let path = root.join(name);
    write(&path, source);
    report(&run("file", &path, None, Some("cc,cogc,halstead")))
}

fn write(path: &Path, contents: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .expect("file reports")
        .iter()
        .find(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("missing file ending in {suffix}"))
}

fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .expect("function reports")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn change<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["patch"]["files"]
        .as_array()
        .expect("patch file reports")
        .iter()
        .find(|change| {
            change["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("missing patch for {suffix}"))
}

fn recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

fn has_resolution(file: &Value, name: &str, status: &str) -> bool {
    file["resolution"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["path"]
                .as_array()
                .is_some_and(|path| path.iter().any(|part| part == name))
                && item["resolution"]["status"] == status
        })
    })
}

#[test]
fn file_mode_selects_each_typescript_and_javascript_family_extension() {
    let temp = tempfile::tempdir().unwrap();
    for (extension, language) in [
        ("ts", "typescript"),
        ("tsx", "tsx"),
        ("js", "javascript"),
        ("jsx", "javascript"),
        ("mts", "typescript"),
        ("cts", "typescript"),
        ("mjs", "javascript"),
        ("cjs", "javascript"),
    ] {
        let path = temp.path().join(format!("source.{extension}"));
        let source = if matches!(extension, "jsx" | "tsx") {
            "export function visit(flag) { return flag ? <span>{visit(false)}</span> : 0; }\n"
        } else {
            "export function visit(flag) { return flag ? visit(false) : 0; }\n"
        };
        write(&path, source);
        let output = run("file", &path, None, Some("cc"));
        let result = report(&output);
        let file = &result["files"][0];
        assert_eq!(file["language"], language, "{extension}");
        assert!(!file["functions"].as_array().unwrap().is_empty());
    }
}

#[test]
fn tsx_react_and_solid_shapes_capture_runtime_functions_and_ignore_jsx_text() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"
import { For, Show } from "solid-js";
interface Item { active: boolean; label: string }
type PanelProps = { items: Item[] };
// commentOnlyName must not become a reference
function* values(count: number) { if (count > 0) yield count; }
class Registry {
  @observe
  method(item: Item) { if (item.active && item.label) return item.label; return ""; }
}
export function Panel(items: Item[]) {
  const onClick = (item: Item) => item.active && activate(item.label);
  return <Show when={items.length > 0} fallback={<p>jsxTextOnlyName</p>}>
    {/* jsxCommentOnlyName must not become a reference */}
    <For each={items}>{item => <button onClick={() => onClick(item)}>{item.label}</button>}</For>
  </Show>;
}
export function ReactPanel({ items }: PanelProps) {
  return <ul>{items.map(item => <li key={item.label} onClick={() => item.active && activate(item.label)}>{item.label}</li>)}</ul>;
}
function activate(label: string) { return label; }
"#;
    let path = temp.path().join("Panel.tsx");
    write(&path, source);
    let output = run("file", &path, None, Some("cc,cogc,halstead"));
    let report = report(&output);
    let file = &report["files"][0];
    let names: Vec<_> = file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert!(names.contains(&"Panel"));
    assert!(names.contains(&"ReactPanel"));
    assert!(names.contains(&"onClick"));
    assert!(names.contains(&"values"));
    assert!(names.contains(&"method"));
    assert!(
        names.len() >= 7,
        "nested JSX callbacks should have function scopes"
    );
    assert!(
        !names
            .iter()
            .any(|name| matches!(*name, "Item" | "PanelProps"))
    );
    assert_eq!(function(file, "values")["cyclomatic_complexity"], 2);
    assert_eq!(function(file, "method")["cyclomatic_complexity"], 3);
    assert_eq!(function(file, "onClick")["cyclomatic_complexity"], 2);
    let references = file["resolution"].as_array().unwrap();
    assert!(!references.iter().any(|item| {
        ["commentOnlyName", "jsxTextOnlyName", "jsxCommentOnlyName"]
            .iter()
            .any(|name| item["path"].to_string().contains(name))
    }));
}

#[test]
fn html_scripts_styles_and_tagged_templates_keep_host_expressions() {
    let temp = tempfile::tempdir().unwrap();
    let html = temp.path().join("page.html");
    write(
        &html,
        r#"<script lang="ts">
function htmlLoop(flag: boolean): boolean { return flag ? htmlLoop(false) : false; }
</script>
<script type="module">function jsLoop(flag) { return flag && jsLoop(false); }</script>
<script type="text/typescript">function mimeLoop(flag: boolean): boolean { return flag ? mimeLoop(false) : false; }</script>
<style>.card { color: red; }</style>
"#,
    );
    let html_report = report(&run("file", &html, None, Some("cc,cogc")));
    let html_file = &html_report["files"][0];
    assert_eq!(html_file["language"], "html");
    assert!(recursive(function(html_file, "htmlLoop")));
    assert!(recursive(function(html_file, "jsLoop")));
    assert!(recursive(function(html_file, "mimeLoop")));
    assert_eq!(function(html_file, "htmlLoop")["cyclomatic_complexity"], 2);
    assert_eq!(function(html_file, "jsLoop")["cyclomatic_complexity"], 2);
    assert_eq!(function(html_file, "mimeLoop")["cyclomatic_complexity"], 2);
    for language in ["typescript", "javascript", "css"] {
        assert!(
            html_file["injections"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| {
                    item["analyzed"] == true
                        && item["language"].as_str().unwrap().contains(language)
                }),
            "missing analyzed HTML {language} block"
        );
    }
    let css = temp.path().join("theme.css");
    write(&css, ".card { color: red; }\n");
    let css_report = report(&run("file", &css, None, Some("halstead")));
    assert_eq!(css_report["files"][0]["language"], "css");

    let source = r#"
function render(flag: boolean, choose: () => string) {
  const markup = html`<p>café ${flag && choose()} ${flag ? choose() : "none"}</p>`;
  const styles = css`:root { color: ${flag ? "red" : "blue"}; }`;
  const staticMarkup = html`<span>static</span>`;
  const staticStyles = css`.card { display: block; }`;
  const extra = custom`${flag}`;
  return markup + styles + staticMarkup + staticStyles + extra;
}
function embeddedRecursion(flag: boolean) {
  return html`<p>${flag && embeddedRecursion(false)}</p>`;
}
function memberTag() {
  return Lit.html`<strong>member template</strong>`;
}
"#;
    let ts = temp.path().join("template.ts");
    write(&ts, source);
    let template_report = report(&run("file", &ts, None, Some("cc,cogc,halstead")));
    let template_file = &template_report["files"][0];
    let render = function(template_file, "render");
    assert_eq!(render["cyclomatic_complexity"], 4);
    for operand in ["flag", "choose", "red", "blue"] {
        assert!(
            render["halstead"]["operands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|token| token["text"] == operand),
            "host token {operand} disappeared from its function"
        );
    }
    assert!(recursive(function(template_file, "embeddedRecursion")));
    let injections = template_file["injections"].as_array().unwrap();
    for (language, minimum_count) in [("html", 3), ("css", 2)] {
        assert!(
            injections
                .iter()
                .filter(|item| {
                    item["analyzed"] == true
                        && item["language"].as_str().unwrap().contains(language)
                })
                .count()
                >= minimum_count,
            "missing static or interpolated {language} injection"
        );
    }
    assert!(
        injections
            .iter()
            .any(|item| { item["language"] == "custom" && item["analyzed"] == false })
    );
    for injection in injections.iter().filter(|item| item["analyzed"] == true) {
        let start = injection["start_byte"].as_u64().unwrap() as usize;
        let end = injection["end_byte"].as_u64().unwrap() as usize;
        assert!(source.is_char_boundary(start) && source.is_char_boundary(end));
    }

    let root = temp.path().join("template-project");
    write(
        &root.join("package.json"),
        "{\"name\":\"template-fixture\"}\n",
    );
    let template = root.join("render.ts");
    let before = concat!(
        "function render(flag: boolean, choose: () => string) {\n",
        "  const markup = html`<p>café ${flag && choose()} ${flag ? choose() : \"none\"}</p>`;\n",
        "  const styles = css`:root { color: ${flag ? \"red\" : \"blue\"}; }`;\n",
        "  const staticMarkup = html`<span>static</span>`;\n",
        "  const staticStyles = css`.card { display: block; }`;\n",
        "  const extra = custom`${flag}`;\n",
        "  return markup + styles + staticMarkup + staticStyles + extra;\n}\n",
    );
    let after = before
        .replace(
            "<p>café ${flag && choose()}",
            "<section>café ${flag || choose()}",
        )
        .replace("</p>`;", "</section>`;");
    write(&template, before);
    let diff = temp.path().join("template.diff");
    write(
        &diff,
        concat!(
            "--- a/render.ts\n+++ b/render.ts\n@@ -1,8 +1,8 @@\n",
            " function render(flag: boolean, choose: () => string) {\n",
            "-  const markup = html`<p>café ${flag && choose()} ${flag ? choose() : \"none\"}</p>`;\n",
            "+  const markup = html`<section>café ${flag || choose()} ${flag ? choose() : \"none\"}</section>`;\n",
            "   const styles = css`:root { color: ${flag ? \"red\" : \"blue\"}; }`;\n",
            "   const staticMarkup = html`<span>static</span>`;\n",
            "   const staticStyles = css`.card { display: block; }`;\n",
            "   const extra = custom`${flag}`;\n",
            "   return markup + styles + staticMarkup + staticStyles + extra;\n",
            " }\n",
        ),
    );
    let patched = report(&run("candidate", &template, Some(&diff), None));
    assert_eq!(std::fs::read(&template).unwrap(), before.as_bytes());
    write(&template, after.as_bytes());
    let fresh = report(&run("repo", &root, None, None));
    assert_eq!(file(&patched, "/render.ts"), file(&fresh, "/render.ts"));
}

#[test]
fn svelte_scripts_styles_and_template_expressions_use_registered_analyzers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    write(
        &root.join("package.json"),
        r##"{
  "imports": {
    "#lib": "./src/lib/index.js",
    "#lib/*": "./src/lib/*",
    "#config": { "types": "./src/config.ts", "default": "./src/config.js" },
    "#vendor": "external-package"
  }
}
"##,
    );
    write(
        &root.join("src/lib/index.ts"),
        "export function share(value: string) { return value; }\n",
    );
    write(
        &root.join("src/config.ts"),
        "export function setting(value: string) { return value; }\n",
    );
    write(
        &root.join("src/lib/format.ts"),
        "export function format(value: string) { return value.trim(); }\n",
    );
    write(
        &root.join("src/lib/player.svelte.ts"),
        "export function play(id: string) { return id; }\n",
    );
    write(
        &root.join("src/lib/Row.svelte"),
        r#"<script lang="ts">
  import { format } from './format';
  import { play } from './player.svelte';
  import { request } from '$lib/api';
  import { format as kitFormat } from '#lib/format.js';
  import { share } from '#lib';
  import { setting } from '#config';
  import { vendor } from '#vendor';
  import { missing } from '#missing';
  let { items, open }: { items: string[]; open: boolean } = $props();
  function pick(value: string): string {
    return open && value ? pick(format(value)) : play(request(value));
  }
  function kit(value: string) {
    return kitFormat(share(setting(vendor(missing(value)))));
  }
</script>

{#if open && items.length}
  {#each items as item (item)}
    <button onclick={() => (item ? pick(item) : null)} {@attach focus}>{item}</button>
  {/each}
{:else}
  <p>none</p>
{/if}

<style>
  p { color: red; }
</style>
"#,
    );
    write(
        &root.join("src/lib/Plain.svelte"),
        "<script>\n  let count = 0;\n</script>\n<button onclick={() => count && count++}>{count}</button>\n",
    );
    let output = report(&run("repo", &root, None, Some("cc,cogc")));

    let row = file(&output, "Row.svelte");
    assert_eq!(row["language"], "svelte");
    let pick = function(row, "pick");
    assert_eq!(pick["cyclomatic_complexity"], 3);
    assert!(recursive(pick));
    let handlers = row["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["name"].as_str().unwrap().starts_with("anonymous"))
        .collect::<Vec<_>>();
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0]["cyclomatic_complexity"], 2);
    let injections = row["injections"].as_array().unwrap();
    assert!(injections.iter().all(|item| item["analyzed"] == true));
    assert!(injections.iter().any(|item| item["language"] == "css"));
    assert!(has_resolution(row, "format", "exact"));
    assert!(has_resolution(row, "play", "exact"));
    assert!(has_resolution(row, "request", "unresolved"));
    assert!(!has_resolution(row, "request", "external"));
    assert!(has_resolution(row, "kitFormat", "exact"));
    assert!(has_resolution(row, "share", "exact"));
    assert!(has_resolution(row, "setting", "exact"));
    assert!(has_resolution(row, "vendor", "external"));
    assert!(has_resolution(row, "missing", "unresolved"));

    let plain = file(&output, "Plain.svelte");
    let languages = plain["injections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["language"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(languages.iter().all(|language| *language == "javascript"));
    assert_eq!(function(plain, "anonymous@4")["cyclomatic_complexity"], 2);
}

#[test]
fn multiline_template_holes_keep_host_tokens_and_count_guest_text_after_holes() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"
function hostCall(value: boolean) { return value; }
function multiline(flag: boolean, ready: boolean) {
  return html`<p>
    ${flag
      ? hostCall(false)
      : ''}
    <b>afterFirst</b>
    ${ready
      ? hostCall(true)
      : ''}
    <i>afterSecond</i>
  </p>`;
}
"#;
    let without_text = source
        .replace("    <b>afterFirst</b>\n", "")
        .replace("    <i>afterSecond</i>\n", "");
    let report = analyze_source(temp.path(), "rich.ts", source);
    let sparse = analyze_source(temp.path(), "sparse.ts", &without_text);
    let file = &report["files"][0];
    assert_eq!(
        file["nloc"].as_u64().unwrap(),
        sparse["files"][0]["nloc"].as_u64().unwrap() + 2
    );
    let function = function(file, "multiline");
    assert_eq!(function["cyclomatic_complexity"], 3);
    assert_eq!(
        function["halstead"]["operands"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["text"] == "hostCall")
            .count(),
        2
    );
    let host_tokens = file["halstead"]["operators"]
        .as_array()
        .unwrap()
        .iter()
        .chain(file["halstead"]["operands"].as_array().unwrap())
        .filter(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains("hostCall"))
        });
    assert_eq!(host_tokens.count(), 3);
    let calls: Vec<_> = file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["path"]
                .as_array()
                .is_some_and(|path| path.iter().any(|part| part == "hostCall"))
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|item| item["resolution"]["status"] == "exact")
    );
}

#[test]
fn patch_mode_analyzes_typescript_in_memory() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.ts");
    let before = "export function first(flag: boolean) { if (flag) return 1; return 0; }\n";
    write(&path, before);
    let diff = temp.path().join("rename.diff");
    write(
        &diff,
        "--- a/source.ts\n+++ b/source.ts\n@@ -1 +1 @@\n-export function first(flag: boolean) { if (flag) return 1; return 0; }\n+export function second(flag: boolean) { if (flag) return 2; return 0; }\n",
    );
    let patched = report(&run("patch", &path, Some(&diff), Some("cc")));
    let change = &patched["patch"]["files"][0];
    assert!(
        change["before"]["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "first")
    );
    assert!(
        change["after"]["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "second")
    );
    assert_eq!(std::fs::read(&path).unwrap(), before.as_bytes());
}

#[test]
fn candidate_file_uses_typescript_project_manifests_and_incremental_matches_fresh() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("web-project");
    write(&root.join("tsconfig.json"), "{\"compilerOptions\":{}}\n");
    let left = root.join("src/left.ts");
    let right = root.join("src/right.ts");
    let original = "import { right } from './right';\nexport function left(flag: boolean) { if (flag) right(false); }\n";
    write(&left, original);
    write(
        &right,
        "import { left } from './left';\nexport function right(flag: boolean) { if (flag) left(false); }\n",
    );
    let before_report = report(&run("repo", &root, None, Some("cogc")));
    assert!(recursive(function(
        file(&before_report, "/left.ts"),
        "left"
    )));
    assert!(recursive(function(
        file(&before_report, "/right.ts"),
        "right"
    )));
    let diff = temp.path().join("break-cycle.diff");
    write(
        &diff,
        "--- a/src/left.ts\n+++ b/src/left.ts\n@@ -1,2 +1,2 @@\n import { right } from './right';\n-export function left(flag: boolean) { if (flag) right(false); }\n+export function left(flag: boolean) { if (flag) return; }\n",
    );
    let candidate = report(&run("candidate", &left, Some(&diff), Some("cogc")));
    assert!(candidate["files"].as_array().unwrap().len() >= 2);
    assert!(!recursive(function(file(&candidate, "/left.ts"), "left")));
    assert!(!recursive(function(file(&candidate, "/right.ts"), "right")));
    assert!(candidate["files"][0].get("halstead").is_none());
    assert!(candidate["files"][0].get("maintainability_index").is_none());
    let alias = report(&run(
        "candidate",
        &left,
        Some(&diff),
        Some("cognitive-complexity"),
    ));
    assert_eq!(alias["files"], candidate["files"]);

    let changed = "import { right } from './right';\nexport function left(flag: boolean) { if (flag) return; }\n";
    write(&left, changed);
    let fresh = report(&run("repo", &root, None, Some("cogc")));
    assert_eq!(
        candidate["files"].as_array().unwrap().len(),
        fresh["files"].as_array().unwrap().len()
    );
    assert_eq!(
        function(file(&candidate, "/left.ts"), "left"),
        function(file(&fresh, "/left.ts"), "left")
    );
    assert_eq!(
        function(file(&candidate, "/right.ts"), "right"),
        function(file(&fresh, "/right.ts"), "right")
    );
}

#[test]
fn candidate_tracks_created_deleted_and_renamed_typescript_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    write(&root.join("src/old.ts"), "export function oldName() {}\n");
    write(
        &root.join("src/from.ts"),
        "export function movedName() {}\n",
    );
    let diff = temp.path().join("paths.diff");
    write(
        &diff,
        concat!(
            "--- /dev/null\n+++ b/src/new.ts\n@@ -0,0 +1 @@\n+export function added() {}\n",
            "--- a/src/old.ts\n+++ /dev/null\n@@ -1 +0,0 @@\n-export function oldName() {}\n",
            "--- a/src/from.ts\n+++ b/src/to.ts\n@@ -1 +1 @@\n-export function movedName() {}\n+export function renamed() {}\n",
        ),
    );
    let candidate = report(&run("candidate", &root, Some(&diff), Some("mi,cogc")));
    let files = candidate["patch"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 3);
    assert!(change(&candidate, "/new.ts")["before"].is_null());
    assert!(change(&candidate, "/new.ts")["after"].is_object());
    assert!(change(&candidate, "/old.ts")["before"].is_object());
    assert!(change(&candidate, "/old.ts")["after"].is_null());
    assert!(change(&candidate, "/to.ts")["before"].is_object());
    assert!(change(&candidate, "/to.ts")["after"].is_object());
    assert!(
        file(&candidate, "/new.ts")["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "added")
    );
    assert!(
        file(&candidate, "/to.ts")["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "renamed")
    );
    assert_eq!(
        std::fs::read(root.join("src/old.ts")).unwrap(),
        b"export function oldName() {}\n"
    );
    assert_eq!(
        std::fs::read(root.join("src/from.ts")).unwrap(),
        b"export function movedName() {}\n"
    );
}

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
