use super::*;

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
