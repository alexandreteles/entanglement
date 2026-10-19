mod support;
use support::{file, function, report, run, write};

use serde_json::Value;

fn recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

fn injection_count(file: &Value, language: &str, analyzed: bool) -> usize {
    file["injections"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["language"] == language && item["analyzed"] == analyzed)
        .count()
}

fn named_operand(halstead: &Value, spelling: &str) -> bool {
    halstead["operands"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(spelling))
        })
    })
}

fn operand_count(halstead: &Value, spelling: &str) -> usize {
    halstead["operands"].as_array().map_or(0, |items| {
        items.iter().filter(|item| item["text"] == spelling).count()
    })
}

#[path = "python_languages/commands.rs"]
mod commands;
#[path = "python_languages/embedded.rs"]
mod embedded;
#[path = "python_languages/native.rs"]
mod native;
