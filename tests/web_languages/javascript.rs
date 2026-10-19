use super::*;

#[test]
fn recovered_jsx_prose_keeps_real_expressions_and_attribute_operands() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"
function fn() { return true; }
export function Panel(flag) {
  return <Fade in={flag && fn()}>Shipping ? function-like Word() & Fake Other
    {flag ? fn() : (() => flag && fn())()}
  </Fade>;
}
"#;
    let clean = source
        .replace("in=", "ok=")
        .replace("Shipping ? function-like Word() & Fake Other", "Shipping");
    for extension in ["jsx", "tsx"] {
        let recovered = analyze_source(temp.path(), &format!("Recovered.{extension}"), source);
        let clean = analyze_source(temp.path(), &format!("Clean.{extension}"), &clean);
        let recovered = &recovered["files"][0];
        let clean = &clean["files"][0];
        assert_eq!(recovered["nloc"], clean["nloc"]);
        for name in ["Fake", "Other", "Word"] {
            assert!(
                !recovered["resolution"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["path"] == serde_json::json!([name]))
            );
        }
        assert_eq!(
            recovered["functions"].as_array().unwrap().len(),
            clean["functions"].as_array().unwrap().len()
        );
        for name in ["fn", "Panel"] {
            for metric in ["nloc", "cyclomatic_complexity", "cognitive_complexity"] {
                assert_eq!(
                    function(recovered, name)[metric],
                    function(clean, name)[metric],
                    "{extension}: {name}: {metric}"
                );
            }
        }
        for metric in [
            "distinct_operators",
            "distinct_operands",
            "total_operators",
            "total_operands",
            "volume",
        ] {
            assert_eq!(
                recovered["halstead"][metric], clean["halstead"][metric],
                "{extension}: {metric}"
            );
        }
        assert_eq!(
            recovered["resolution"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["path"] == serde_json::json!(["fn"]))
                .count(),
            3
        );
    }
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
    assert_eq!(function(file, "onClick")["cognitive_complexity"], 1);
    let references = file["resolution"].as_array().unwrap();
    assert!(!references.iter().any(|item| {
        ["commentOnlyName", "jsxTextOnlyName", "jsxCommentOnlyName"]
            .iter()
            .any(|name| item["path"].to_string().contains(name))
    }));
}

#[test]
fn tagged_javascript_templates_can_use_the_registered_python_analyzer() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("embedded.js");
    write(
        &path,
        r#"const guest = python`def from_template(flag):
    if flag:
        return 1
`;
function host(flag) { return flag ? 1 : 0; }
"#,
    );
    let result = report(&run("file", &path, None, Some("cc,cogc")));
    let file = &result["files"][0];
    assert_eq!(file["language"], "javascript");
    assert!(
        file["injections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["language"] == "python" && item["analyzed"] == true })
    );
    assert_eq!(function(file, "from_template")["cyclomatic_complexity"], 2);
    assert_eq!(function(file, "host")["cyclomatic_complexity"], 2);
}

#[test]
fn jsx_tags_resolve_components_without_calls_or_text_operands() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"
import * as UI from "./widgets.js";
import { Button } from "./widgets.js";
export function Panel(flag) {
  return <div><Button>{flag ? "yes" : "no"}</Button><UI.Button />
    <UI.Nested.Button /><Panel /><my-widget /><svg:path />proseOnly &amp;</div>;
}
"#;
    for extension in ["jsx", "tsx"] {
        let root = temp.path().join(extension);
        write(&root.join("package.json"), "{}");
        write(
            &root.join("widgets.js"),
            "export function Button() { return null; }",
        );
        let path = root.join(format!("Panel.{extension}"));
        write(&path, source);
        let report = report(&run("repo", &root, None, Some("cc,cogc,halstead")));
        let panel = file(&report, &format!("Panel.{extension}"));
        let references = panel["resolution"].as_array().unwrap();
        for name in ["div", "my-widget", "svg", "path", "proseOnly"] {
            assert!(
                !references
                    .iter()
                    .any(|item| item["path"] == serde_json::json!([name]))
            );
        }
        for parts in [
            vec!["Button"],
            vec!["UI", "Button"],
            vec!["UI", "Nested", "Button"],
            vec!["Panel"],
        ] {
            assert_eq!(
                references
                    .iter()
                    .filter(|item| item["path"] == serde_json::json!(parts))
                    .count(),
                1
            );
        }
        assert!(has_resolution(panel, "Button", "exact"));
        assert!(!recursive(function(panel, "Panel")));
        assert_eq!(function(panel, "Panel")["cyclomatic_complexity"], 2);
        let sparse = analyze_source(
            &root,
            &format!("Sparse.{extension}"),
            &source.replace("proseOnly &amp;", ""),
        );
        let sparse = &sparse["files"][0];
        for metric in [
            "distinct_operators",
            "distinct_operands",
            "total_operators",
            "total_operands",
            "volume",
        ] {
            assert_eq!(
                panel["halstead"][metric], sparse["halstead"][metric],
                "{extension}: {metric}"
            );
        }
    }
}
