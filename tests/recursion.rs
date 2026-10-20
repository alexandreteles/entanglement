mod support;
use support::report;

use serde_json::Value;

fn function<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| file["functions"].as_array().unwrap())
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

#[test]
fn marks_only_exact_direct_function_cycles() {
    let (_temp, root, _) = support::rust_project(
        r#"
mod child;
use child::aliased as via_alias;

pub fn direct() { direct(); }
pub fn parenthesized() { (parenthesized)(); }
pub fn nested_parenthesized() { (((nested_parenthesized)))(); }
pub fn qualified_parenthesized() { (crate::qualified_parenthesized)(); }
pub fn generic_parenthesized<T>() { ((generic_parenthesized::<u8>))(); }

pub fn qualified_left() { crate::qualified_right(); }
pub fn qualified_right() { qualified_left(); }

pub fn alias_left() { via_alias(); }

pub fn shadow_pair_left() {
    let shadow_pair_right = || {};
    (shadow_pair_right)();
}
pub fn shadow_pair_right() { shadow_pair_left(); }

fn choose(function: fn()) -> fn() { function }
pub fn value_target() { value_only(); }
pub fn value_only() {
    let _function_value = value_target;
    choose(value_target)();
}

fn consume(_: fn()) {}
pub fn argument_target() { argument_caller(); }
pub fn argument_caller() { consume((argument_target)); }

pub fn closure_target() { closure_caller(); }
pub fn closure_caller() {
    let _callback = || { closure_target(); };
}

pub async fn async_target() { async_caller().await; }
pub async fn async_caller() {
    let _future = async { async_target().await; };
}
"#,
    );
    std::fs::write(
        root.join("src/child.rs"),
        "use crate::alias_left as alias_back;\npub fn aliased() { alias_back(); }\n",
    )
    .expect("write child module");

    let report = report(&support::run("repo", &root, None, None));

    for name in [
        "direct",
        "parenthesized",
        "nested_parenthesized",
        "qualified_parenthesized",
        "generic_parenthesized",
        "qualified_left",
        "qualified_right",
        "alias_left",
        "aliased",
    ] {
        assert!(
            support::recursive(function(&report, name)),
            "{name} should be recursive"
        );
        assert_eq!(function(&report, name)["cyclomatic_complexity"], 1);
    }
    for name in [
        "shadow_pair_left",
        "shadow_pair_right",
        "choose",
        "value_target",
        "value_only",
        "consume",
        "argument_target",
        "argument_caller",
        "closure_target",
        "closure_caller",
        "async_target",
        "async_caller",
    ] {
        assert!(
            !support::recursive(function(&report, name)),
            "{name} must stay acyclic"
        );
    }
}

#[test]
fn candidate_cycle_edits_recompute_recursion_for_both_members() {
    for (before, after, recursive) in [
        (
            "pub fn left() {}\npub fn right() { left(); }\n",
            "pub fn left() { right(); }\npub fn right() { left(); }\n",
            false,
        ),
        (
            "pub fn left() { right(); }\npub fn right() { left(); }\n",
            "pub fn left() {}\npub fn right() { left(); }\n",
            true,
        ),
    ] {
        let (_temp, root, source) = support::rust_project(before);
        let previous = report(&support::run("repo", &root, None, None));
        let diff = root.join("cycle.diff");
        support::write(
            &diff,
            support::patch("src/lib.rs", Some(before), Some(after)),
        );
        let candidate = report(&support::run("candidate", &root, Some(&diff), None));
        assert_eq!(
            ["left", "right"].map(|name| support::recursive(function(&previous, name))),
            [recursive; 2]
        );
        assert_eq!(
            ["left", "right"].map(|name| support::recursive(function(&candidate, name))),
            [!recursive; 2]
        );
        assert_eq!(std::fs::read_to_string(source).unwrap(), before);
    }
}
