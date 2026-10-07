use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(args: &[&str], path: &Path, diff: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json"]).args(args).arg(path);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    command.output().expect("run entanglement")
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn write(path: &Path, source: &str) {
    if path.extension().is_some_and(|extension| extension == "go") {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_go::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "invalid Go fixture {}: {}",
            path.display(),
            tree.root_node().to_sexp()
        );
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
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

fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn kinds(function: &Value) -> Vec<(String, u64)> {
    function["cognitive_contributions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["kind"].as_str().unwrap().to_owned(),
                item["value"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// Return each reference's path and status, with the target's file suffix and name.
fn resolutions(file: &Value) -> Vec<(String, String)> {
    file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let path = item["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|part| part.as_str().unwrap())
                .collect::<Vec<_>>()
                .join(".");
            let resolution = &item["resolution"];
            let status = match &resolution["symbols"] {
                Value::Object(symbol) => format!(
                    "exact {}:{}",
                    symbol["file"].as_str().unwrap().rsplit('/').next().unwrap(),
                    symbol["name"].as_str().unwrap()
                ),
                _ => resolution["status"].as_str().unwrap().to_owned(),
            };
            (path, status)
        })
        .collect()
}

fn status<'a>(resolutions: &'a [(String, String)], path: &str) -> Vec<&'a str> {
    resolutions
        .iter()
        .filter(|(item, _)| item == path)
        .map(|(_, status)| status.as_str())
        .collect()
}

#[test]
fn go_syntax_metrics_follow_shared_rules() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("walk.go");
    write(
        &path,
        r#"package main

import "fmt"

type Tree struct{ Left, Right *Tree }

// commentOnlySentinel
func Walk(t *Tree, depth int) int {
	if t == nil {
		return 0
	} else if depth > 10 && t.Left != nil || t.Right == nil {
		return 1
	} else {
		depth++
	}
outer:
	for i := 0; i < depth; i++ {
		for {
			if i%2 == 0 {
				continue outer
			}
			break
		}
	}
	switch depth {
	case 1, 2:
		fmt.Println("small")
	case 3:
	default:
	}
	visit := func(n *Tree) int { return Walk(n, depth+1) }
	return visit(t.Left) + Walk(t.Right, depth)
}

func (t *Tree) Wait(values chan int, done chan bool, value any) {
	select {
	case <-values:
	case <-done:
	default:
	}
	switch value.(type) {
	case int:
	case string:
	}
}
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let file = &output["files"][0];
    assert_eq!(file["language"], "go");

    let walk = function(file, "Walk");
    assert_eq!(walk["cyclomatic_complexity"], 9);
    assert_eq!(walk["cognitive_complexity"], 14);
    assert_eq!(
        kinds(walk),
        [
            ("recursion", 1),
            ("if", 1),
            ("if", 1),
            ("logical_operator", 1),
            ("logical_operator", 1),
            ("else", 1),
            ("loop", 1),
            ("loop", 2),
            ("if", 3),
            ("labeled_jump", 1),
            ("match", 1),
        ]
        .map(|(kind, value)| (kind.to_owned(), value))
    );
    let visit = function(file, "visit");
    assert_eq!(visit["cyclomatic_complexity"], 1);
    assert_eq!(visit["cognitive_complexity"], 0);

    let wait = function(file, "Wait");
    assert_eq!(wait["cyclomatic_complexity"], 5);
    assert_eq!(wait["cognitive_complexity"], 2);

    let operands = file["halstead"]["operands"].as_array().unwrap();
    assert!(!operands.iter().any(|item| {
        item["text"]
            .as_str()
            .is_some_and(|text| text.contains("commentOnlySentinel"))
    }));
}

#[test]
fn go_packages_resolve_across_files_and_module_imports() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("module");
    write(
        &root.join("go.mod"),
        "module example.com/app // module path\n\ngo 1.22\n",
    );
    write(
        &root.join("internal/util/even.go"),
        "package util\n\nfunc IsEven(n int) bool {\n\tif n == 0 {\n\t\treturn true\n\t}\n\treturn isOdd(n - 1)\n}\n\nfunc helper() int { return 1 }\n",
    );
    write(
        &root.join("internal/util/odd.go"),
        "package util\n\nfunc isOdd(n int) bool {\n\tif n == 0 {\n\t\treturn false\n\t}\n\treturn IsEven(n - 1)\n}\n",
    );
    write(
        &root.join("internal/util/even_test.go"),
        "package util_test\n\nimport \"example.com/app/internal/util\"\n\nfunc TestEven() { util.IsEven(2) }\n",
    );
    write(
        &root.join("internal/util/lib.rs"),
        "mod util;\npub fn Shared() {}\n",
    );
    write(
        &root.join("cmd/main.go"),
        r#"package main

import (
	u "example.com/app/internal/util"
	"example.com/app/internal/util"
	"example.com/app/missing"
	"github.com/example/yaml/v3"
	"k8s.io/api/core/v1"
	_ "embed"
)

var Version = "1"

func main() {
	u.IsEven(3)
	util.IsEven(4)
	util.helper()
	missing.Do()
	yaml.Marshal(Version)
	v1.Pod()
	util.Shared()
	println(len(Version))
}
"#,
    );
    let output = report(&run(&["--metrics", "cogc", "repo"], &root, None));

    let main = resolutions(file(&output, "cmd/main.go"));
    assert_eq!(status(&main, "u.IsEven"), ["exact even.go:IsEven"]);
    assert_eq!(status(&main, "util.IsEven"), ["exact even.go:IsEven"]);
    assert_eq!(status(&main, "util.helper"), ["unresolved"]);
    assert_eq!(status(&main, "missing.Do"), ["unresolved"]);
    assert_eq!(status(&main, "yaml.Marshal"), ["external"]);
    assert_eq!(status(&main, "v1.Pod"), ["external"]);
    assert_eq!(status(&main, "util.Shared"), ["unresolved"]);
    assert_eq!(status(&main, "Version"), ["exact main.go:Version"; 2]);
    assert_eq!(status(&main, "println"), ["external"]);
    assert_eq!(status(&main, "len"), ["external"]);

    let tests = resolutions(file(&output, "even_test.go"));
    assert_eq!(status(&tests, "util.IsEven"), ["exact even.go:IsEven"]);
    let odd = resolutions(file(&output, "odd.go"));
    assert_eq!(status(&odd, "IsEven"), ["exact even.go:IsEven"]);
    for (suffix, name) in [("even.go", "IsEven"), ("odd.go", "isOdd")] {
        assert_eq!(
            kinds(function(file(&output, suffix), name))[0],
            ("recursion".to_owned(), 1)
        );
    }
}

#[test]
fn go_locals_shadow_package_names_from_their_declaration() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("scopes.go");
    write(
        &path,
        r#"package scopes

import "strings"

type Point struct{ X int }

func x(n int) int { return n }

func shadow(value any) {
	x := x(1)
	_ = x
	switch strings := value.(type) {
	case string:
		_ = strings
	}
	strings.ToUpper("a")
	for strings := range 3 {
		_ = strings
	}
	_ = Point{X: 1}
}
"#,
    );
    let output = report(&run(&["--metrics", "cogc", "repo"], temp.path(), None));
    let scopes = resolutions(&output["files"][0]);
    assert_eq!(status(&scopes, "x"), ["exact scopes.go:x", "unresolved"]);
    assert_eq!(status(&scopes, "strings.ToUpper"), ["external"]);
    assert_eq!(status(&scopes, "strings"), ["unresolved", "unresolved"]);
    assert_eq!(status(&scopes, "value"), ["unresolved"]);
    assert_eq!(status(&scopes, "Point"), ["exact scopes.go:Point"]);
    assert!(status(&scopes, "_").is_empty());
    assert!(status(&scopes, "X").is_empty());
}

#[test]
fn go_candidate_reports_match_fresh_analysis() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    write(&root.join("go.mod"), "module example.com/project\n");
    let target = root.join("choose.go");
    let before = "package project\n\nfunc choose(flag bool) int {\n\tif flag {\n\t\treturn 1\n\t}\n\treturn 0\n}\n";
    let after = "package project\n\nfunc choose(flag bool) int {\n\tif flag && !flag {\n\t\treturn 1\n\t}\n\treturn 0\n}\n";
    write(&target, before);
    let diff = temp.path().join("change.diff");
    write(
        &diff,
        "--- a/choose.go\n+++ b/choose.go\n@@ -1,8 +1,8 @@\n package project\n \n func choose(flag bool) int {\n-\tif flag {\n+\tif flag && !flag {\n \t\treturn 1\n \t}\n \treturn 0\n }\n",
    );
    let candidate = report(&run(&["candidate"], &target, Some(&diff)));
    let delta = &candidate["patch"]["files"][0]["functions"][0];
    assert_eq!(delta["cyclomatic_complexity"]["delta"], 1);
    assert_eq!(delta["cognitive_complexity"]["delta"], 1);
    write(&target, after);
    let fresh = report(&run(&["repo"], &root, None));
    assert_eq!(file(&candidate, "/choose.go"), file(&fresh, "/choose.go"));
}

#[path = "go_review/candidate.rs"]
mod review_candidate;
#[path = "go_review/metrics.rs"]
mod review_metrics;
#[path = "go_review/packages.rs"]
mod review_packages;
#[path = "go_review/scopes.rs"]
mod review_scopes;
