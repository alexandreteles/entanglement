mod support;

use support::{report, run, write};

#[test]
fn static_resources_use_repository_paths_and_preserve_cycles() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("index.html"),
        "<link rel=\"stylesheet\" href=\"./a.css\"><script src=\"./app.js\"></script>",
    );
    write(&root.path().join("a.css"), "@import './b.css';\n");
    write(&root.path().join("b.css"), "@import url('./a.css');\n");
    write(&root.path().join("app.js"), "export const value = 1;\n");
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let relationships = result["structure"]["relationships"].as_array().unwrap();
    assert_eq!(
        relationships
            .iter()
            .filter(|edge| edge["kind"] == "resource" && edge["resolution"] == "resolved")
            .count(),
        4
    );
    let full = result["structure"]["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|graph| graph["granularity"] == "module" && graph["projection"] == "full")
        .unwrap();
    assert_eq!(full["cycles"].as_array().unwrap().len(), 1);
}

#[test]
fn injected_styles_are_captured_once_and_dynamic_urls_stay_unresolved() {
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("View.svelte"),
        "<script>let dynamic = './missing.css';</script>\n<style>@import './base.css';</style>\n<img src={dynamic}>",
    );
    write(&root.path().join("base.css"), "body { color: black; }\n");
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let relationships = result["structure"]["relationships"].as_array().unwrap();
    let resources: Vec<_> = relationships
        .iter()
        .filter(|edge| edge["kind"] == "resource")
        .collect();
    assert_eq!(resources.len(), 2);
    assert!(
        resources
            .iter()
            .any(|edge| edge["resolution"] == "unresolved")
    );
    let resolved = resources
        .iter()
        .find(|edge| edge["resolution"] == "resolved")
        .unwrap();
    assert_eq!(resolved["evidence"].as_array().unwrap().len(), 1);
}
