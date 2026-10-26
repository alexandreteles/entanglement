#[path = "structure_contracts/go.rs"]
mod go;
#[path = "structure_contracts/javascript.rs"]
mod javascript;
#[path = "structure_contracts/kotlin.rs"]
mod kotlin;
#[path = "structure_contracts/php.rs"]
mod php;
#[path = "structure_contracts/python.rs"]
mod python;
#[path = "structure_contracts/rust.rs"]
mod rust;
#[path = "structure_contracts/typescript.rs"]
mod typescript;

mod support;
use serde_json::Value;
use support::{patch, report, run, write};

fn analyze(name: &str, source: &str) -> Value {
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(name), source);
    report(&run("repo", root.path(), None, Some("structure")))
}

fn declaration<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|declaration| {
            declaration["unit"]
                .as_str()
                .unwrap()
                .ends_with(&format!(":{name}"))
        })
        .unwrap()
}

fn member<'a>(declaration: &'a Value, name: &str) -> &'a Value {
    declaration["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == name)
        .unwrap_or_else(|| panic!("missing member {name}: {declaration:#}"))
}
