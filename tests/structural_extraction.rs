#[path = "structural_extraction/go_embedding.rs"]
mod go_embedding;
#[path = "structural_extraction/go_interfaces.rs"]
mod go_interfaces;
#[path = "structural_extraction/php.rs"]
mod php;
#[path = "structural_extraction/python.rs"]
mod python;
#[path = "structural_extraction/rust.rs"]
mod rust;
#[path = "structural_extraction/web.rs"]
mod web;

mod support;
use serde_json::Value;
use support::{report, run, write};

fn declaration<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["unit"]
                .as_str()
                .unwrap()
                .ends_with(&format!(":{name}"))
        })
        .unwrap()
}

#[test]
fn all_registered_language_groups_emit_coverage() {
    let root = tempfile::tempdir().unwrap();
    let fixture_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/structure_fixtures");
    for fixture in std::fs::read_dir(fixture_root).unwrap() {
        let fixture = fixture.unwrap();
        write(
            &root.path().join(fixture.file_name()),
            std::fs::read(fixture.path()).unwrap(),
        );
    }
    let result = report(&run("repo", root.path(), None, Some("structure")));
    let coverage = result["structure"]["coverage"].as_array().unwrap();
    for suffix in [
        "rs", "php", "py", "go", "kt", "js", "jsx", "ts", "tsx", "astro", "svelte", "html", "css",
    ] {
        assert!(coverage.iter().any(|item| {
            item["path"]
                .as_str()
                .unwrap()
                .ends_with(&format!(".{suffix}"))
        }));
    }
    for language in [
        "source.rust",
        "source.php",
        "source.python",
        "source.go",
        "source.kotlin",
        "source.js",
        "source.ts",
        "source.tsx",
        "source.astro",
        "source.svelte",
        "source.angular",
        "source.html",
        "source.css",
    ] {
        assert!(
            coverage.iter().any(|item| item["language"] == language),
            "missing language {language}: {coverage:#?}"
        );
    }
    for host in ["model.astro", "model.svelte"] {
        let host_coverage = coverage
            .iter()
            .filter(|item| item["path"].as_str().unwrap().ends_with(host))
            .collect::<Vec<_>>();
        assert!(
            host_coverage
                .iter()
                .any(|item| item["language"] == "source.js")
        );
        assert!(
            host_coverage
                .iter()
                .any(|item| item["language"] == "source.css")
        );
        assert!(
            host_coverage
                .iter()
                .any(|item| item["effective"] == "inapplicable")
        );
    }
    for item in coverage {
        assert!(!item["limitations"].as_array().unwrap().is_empty());
        assert_ne!(item["status"], "complete");
    }
    let relationships = result["structure"]["relationships"].as_array().unwrap();
    for name in [
        "model.rs",
        "model.php",
        "model.py",
        "types.go",
        "model.kt",
        "model.js",
        "model.jsx",
        "model.ts",
        "model.tsx",
        "model.astro",
        "model.svelte",
        "model.html",
        "model.css",
    ] {
        assert!(
            relationships.iter().any(|edge| edge["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .any(|evidence| evidence["path"].as_str().unwrap().ends_with(name))),
            "missing dependencies for {name}"
        );
    }
}
