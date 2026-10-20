use support::write;
mod support;
use support::{file, function, recursive};

use serde_json::Value;

fn reference<'a>(file: &'a Value, source: &str, needle: &str) -> &'a Value {
    let start = source
        .rfind(needle)
        .unwrap_or_else(|| panic!("Missing source {needle}"));
    &file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["start_byte"] == start)
        .unwrap_or_else(|| {
            panic!(
                "Missing reference {needle} at {start}: {}",
                file["resolution"]
            )
        })["resolution"]
}

fn exact(file: &Value, source: &str, needle: &str, suffix: &str, name: &str) {
    let target = reference(file, source, needle);
    assert_eq!(target["status"], "exact", "{needle}: {target}");
    assert_eq!(target["symbols"]["name"], name, "{needle}: {target}");
    assert!(
        target["symbols"]["file"]
            .as_str()
            .is_some_and(|path| path.ends_with(suffix)),
        "{needle}: {target}"
    );
}

#[path = "php_resolution/autoload.rs"]
mod autoload;
#[path = "php_resolution/dispatch.rs"]
mod dispatch;
#[path = "php_resolution/imports.rs"]
mod imports;
