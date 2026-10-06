use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn repo(root: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "repo"])
        .arg(root)
        .output()
        .expect("run entanglement");
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse report")
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("missing file {suffix}"))
}

fn default_target<'a>(file: &'a Value, local: &str, suffix: &str) -> &'a Value {
    let item = file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["path"]
                .as_array()
                .is_some_and(|path| path.first().is_some_and(|part| part == local))
                && item["resolution"]["status"] == "exact"
        })
        .unwrap_or_else(|| panic!("missing default target for {local}"));
    let target = &item["resolution"]["symbols"];
    assert_eq!(target["name"], "default");
    assert!(target["file"].as_str().unwrap().ends_with(suffix));
    target
}

fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

#[test]
fn anonymous_default_callables_resolve_outer_values_and_cycles() {
    let temp = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "src/arrow.ts",
            r#"import { run } from "./consumer";
export default (flag: boolean) => {
  function helper() { return 1; }
  return flag ? run(false) : helper();
};
"#,
        ),
        (
            "src/consumer.ts",
            r#"import arrowDefault from "./arrow";
import parenthesized from "./parenthesized";
import asDefault from "./as";
import satisfiesDefault from "./satisfies";
import functionDefault from "./function";
import generatorDefault from "./generator";
import jsDefault from "./default_js";
import tsxDefault from "./default_tsx";
import wrappedDefault from "./wrapped";
export function run(flag: boolean) { return flag ? arrowDefault(false) : 0; }
export function useAll(flag: boolean) {
  parenthesized(flag); asDefault(flag); satisfiesDefault(flag);
  functionDefault(flag); generatorDefault(flag); jsDefault(flag); tsxDefault(flag);
}
export function useWrapped() { return wrappedDefault(); }
"#,
        ),
        (
            "src/parenthesized.ts",
            "export default (((flag: boolean) => flag));",
        ),
        (
            "src/as.ts",
            "export default ((flag: boolean) => flag) as (flag: boolean) => boolean;",
        ),
        (
            "src/satisfies.ts",
            "export default ((flag: boolean) => flag) satisfies (flag: boolean) => boolean;",
        ),
        (
            "src/function.ts",
            "export default function (flag: boolean) { return flag; }",
        ),
        (
            "src/generator.ts",
            "export default function* (flag: boolean) { if (flag) yield 1; }",
        ),
        (
            "src/default_js.js",
            "export default (flag) => flag ? 1 : 0;",
        ),
        (
            "src/default_tsx.tsx",
            "export default ((flag: boolean) => flag ? <span /> : null);",
        ),
        (
            "src/wrapped.ts",
            r#"function wrap(value: unknown) { return value; }
export default wrap(() => { function helper() { return 1; } });
"#,
        ),
    ] {
        write(temp.path(), path, source);
    }

    let report = repo(temp.path());
    let consumer = file(&report, "/consumer.ts");
    let arrow_default = default_target(consumer, "arrowDefault", "/arrow.ts");
    for (local, path) in [
        ("parenthesized", "/parenthesized.ts"),
        ("asDefault", "/as.ts"),
        ("satisfiesDefault", "/satisfies.ts"),
        ("functionDefault", "/function.ts"),
        ("generatorDefault", "/generator.ts"),
        ("jsDefault", "/default_js.js"),
        ("tsxDefault", "/default_tsx.tsx"),
    ] {
        default_target(consumer, local, path);
    }
    assert!(
        consumer["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["path"][0] == "wrappedDefault" && item["resolution"]["status"] == "unresolved"
            })
    );

    let arrow = file(&report, "/arrow.ts");
    let outer = arrow["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["start_byte"] == arrow_default["start_byte"])
        .expect("default symbol identifies the outer arrow");
    assert!(recursive(outer));
    assert!(recursive(function(consumer, "run")));
    assert!(!recursive(function(arrow, "helper")));
}

fn injection_count(file: &Value, language: &str, analyzed: bool) -> usize {
    file["injections"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["language"] == language && item["analyzed"] == analyzed)
        .count()
}

#[test]
fn html_selector_case_normalization_keeps_unknown_injections_visible() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        "page.html",
        r#"<script LaNg=TyPeScRiPt>
function langTs(flag: boolean): boolean { return flag ? langTs(false) : false; }
</script>
<script TYPE='ApPlIcAtIoN/TyPeScRiPt'>
function applicationTs(flag: boolean): boolean { return flag ? applicationTs(false) : false; }
</script>
<script type=TEXT/TYPESCRIPT>
function textTs(flag: boolean): boolean { return flag ? textTs(false) : false; }
</script>
<script TYPE="MODULE">function moduleJs(flag) { return flag && moduleJs(false); }</script>
<script LANG='JaVaScRiPt'>function langJs(flag) { return flag ? langJs(false) : false; }</script>
<script TYPE='application/x-custom'>function unknownScript() { return true; }</script>
<style LaNg='TeXt/CsS'>.button { color: red; }</style>
"#,
    );

    let report = repo(temp.path());
    let html = file(&report, "/page.html");
    assert_eq!(html["language"], "html");
    assert_eq!(injection_count(html, "typescript", true), 3);
    assert_eq!(injection_count(html, "javascript", true), 2);
    assert_eq!(injection_count(html, "css", true), 1);
    assert_eq!(injection_count(html, "application/x-custom", false), 1);
    for name in ["langTs", "applicationTs", "textTs", "moduleJs", "langJs"] {
        assert_eq!(function(html, name)["cyclomatic_complexity"], 2);
    }
}
