mod support;
use support::{file, function, report, run, write};

use serde_json::Value;

fn has_operand(halstead: &Value, spelling: &str) -> bool {
    halstead["operands"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(spelling))
        })
    })
}

fn has_operand_at(halstead: &Value, spelling: &str, start_byte: usize) -> bool {
    halstead["operands"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"] == spelling && item["start_byte"].as_u64() == Some(start_byte as u64)
        })
    })
}

fn contribution_count(function: &Value, kind: &str) -> usize {
    function["contributions"]
        .as_array()
        .expect("complexity contributions")
        .iter()
        .filter(|item| item["kind"] == kind)
        .count()
}

fn cognitive_contribution_count(function: &Value, kind: &str) -> usize {
    function["cognitive_contributions"]
        .as_array()
        .expect("cognitive complexity contributions")
        .iter()
        .filter(|item| item["kind"] == kind)
        .count()
}

#[path = "kotlin_languages/candidate.rs"]
mod candidate;
#[path = "kotlin_languages/commands.rs"]
mod commands;
#[path = "kotlin_languages/native.rs"]
mod native;
