mod support;
use support::function;

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn write(path: &Path, source: &str) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "invalid PHP fixture: {}",
        tree.root_node().to_sexp()
    );
    std::fs::write(path, source).unwrap();
}

fn report(mode: &str, path: &Path, diff: Option<&Path>, metrics: Option<&str>) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json", mode]).arg(path);
    if let Some(metrics) = metrics {
        command.args(["--metrics", metrics]);
    }
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    let output = command.output().expect("run entanglement");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("read report")
}

#[test]
fn native_metrics_keep_nested_callable_decisions_separate() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("flow.php");
    write(
        &path,
        r#"<?php
// commentOnlySentinel
function flow($a, $b) {
    if ($a && $b) { return 1; }
    elseif ($a || $b) { return 2; }
    else { return 0; }
}
function outer($flag) {
    $callback = fn($value) => $value ? 1 : 0;
    return $callback($flag);
}
class Worker {
    public function iterate($values) {
        foreach ($values as $value) { if ($value) { continue; } }
    }
}
"#,
    );
    let result = report("file", &path, None, None);
    let file = &result["files"][0];
    assert_eq!(file["language"], "php");
    assert_eq!(function(file, "flow")["cyclomatic_complexity"], 5);
    assert_eq!(function(file, "flow")["cognitive_complexity"], 5);
    assert_eq!(function(file, "outer")["cyclomatic_complexity"], 1);
    assert_eq!(function(file, "$callback")["cyclomatic_complexity"], 2);
    assert_eq!(function(file, "iterate")["cyclomatic_complexity"], 3);
    assert_eq!(function(file, "iterate")["cognitive_complexity"], 3);
    assert!(!file["halstead"].to_string().contains("commentOnlySentinel"));
}

#[test]
fn templates_use_html_javascript_and_css_analyzers() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("page.phtml");
    write(
        &path,
        r#"<main>
<script>function client(flag) { if (flag) return 1; return 0; }</script>
<style>.card { color: red; }</style>
<?php function render($flag) { if ($flag) { ?>
<p>visible markup</p>
<?php } } ?>
</main>"#,
    );
    let result = report("file", &path, None, None);
    let file = &result["files"][0];
    assert_eq!(file["language"], "php");
    assert_eq!(function(file, "render")["cyclomatic_complexity"], 2);
    assert_eq!(function(file, "client")["cyclomatic_complexity"], 2);
    assert!(
        !function(file, "render")["halstead"]
            .to_string()
            .contains("visible markup")
    );
    let injections = file["injections"].as_array().unwrap();
    for language in ["html", "javascript", "css"] {
        assert!(injections.iter().any(|injection| injection["language"] == language && injection["analyzed"] == true), "missing {language} injection: {injections:?}");
    }
}

#[test]
fn repository_patch_and_candidate_commands_select_php_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("changed.php");
    let diff = temp.path().join("change.diff");
    write(
        &path,
        "<?php\nfunction changed($value) { return $value; }\n",
    );
    std::fs::write(&diff, "--- a/changed.php\n+++ b/changed.php\n@@ -1,2 +1,2 @@\n <?php\n-function changed($value) { return $value; }\n+function changed($value) { if ($value) { return 1; } return 0; }\n").unwrap();
    let repo = report("repo", temp.path(), None, None);
    assert_eq!(repo["files"][0]["language"], "php");
    let patch = report("patch", &path, Some(&diff), None);
    assert_eq!(
        function(&patch["files"][0], "changed")["cyclomatic_complexity"],
        2
    );
    let candidate = report("candidate", temp.path(), Some(&diff), None);
    assert_eq!(
        function(&candidate["files"][0], "changed")["cyclomatic_complexity"],
        2
    );
}

#[test]
fn property_hooks_have_independent_metric_scopes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hooks.php");
    write(
        &path,
        r#"<?php
final class Reading {
    private int $raw = 0;
    public int $value {
        get { if ($this->raw > 0) { return $this->raw; } return 0; }
        set(int $value) { if ($value >= 0) { $this->raw = $value; } }
    }
    public function __construct(public final string $label) {}
}

function discard($value) { (void) $value; }
"#,
    );
    let result = report("file", &path, None, None);
    let file = &result["files"][0];
    for hook in ["get", "set"] {
        assert_eq!(function(file, hook)["cyclomatic_complexity"], 2);
        assert_eq!(function(file, hook)["cognitive_complexity"], 1);
    }
    assert_eq!(function(file, "__construct")["cyclomatic_complexity"], 1);
    assert_eq!(function(file, "discard")["cyclomatic_complexity"], 1);
}

#[test]
fn pure_markup_php_templates_keep_their_script_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plain.phtml");
    write(
        &path,
        "<script>function plain(value) { return value ? 1 : 0; }</script>",
    );
    let result = report("file", &path, None, None);
    assert_eq!(
        function(&result["files"][0], "plain")["cyclomatic_complexity"],
        2
    );
}

#[test]
fn selected_metrics_keep_exact_php_line_and_token_counts() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("leaf.php");
    write(
        &path,
        "<?php\n// no code on this line\nfunction leaf($value) { return $value; }\n",
    );
    for (metric, field) in [
        ("nloc", "nloc"),
        ("cc", "cyclomatic_complexity"),
        ("density", "cyclomatic_density"),
        ("cogc", "cognitive_complexity"),
        ("halstead", "halstead"),
        ("mi", "maintainability_index"),
    ] {
        let result = report("file", &path, None, Some(metric));
        let file = &result["files"][0];
        let leaf = function(file, "leaf");
        assert_eq!(file["nloc"], 2);
        assert_eq!(leaf["nloc"], 1);
        assert!(leaf.get(field).is_some(), "missing {field} for {metric}");
        if metric == "halstead" {
            assert_eq!(leaf["halstead"]["distinct_operators"], 8);
            assert_eq!(leaf["halstead"]["total_operators"], 9);
            assert_eq!(leaf["halstead"]["distinct_operands"], 2);
            assert_eq!(leaf["halstead"]["total_operands"], 3);
        }
        if matches!(metric, "nloc" | "cc" | "density" | "cogc") {
            assert!(leaf.get("halstead").is_none());
        }
    }
}

#[test]
fn first_class_method_bindings_keep_their_static_target() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("callbacks.php");
    write(
        &path,
        r#"<?php
final class Callback {
    public static function repeat($value) {
        $next = self::repeat(...);
        return $next($value);
    }
    public function invoke($value) {
        $next = $this->invoke(...);
        return $next($value);
    }
}

"#,
    );
    let result = report("file", &path, None, None);
    for name in ["repeat", "invoke"] {
        assert!(
            function(&result["files"][0], name)["cognitive_contributions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "recursion"),
            "missing callable recursion for {name}"
        );
    }
}

#[test]
fn mutations_prevent_an_exact_callable_binding() {
    let temp = tempfile::tempdir().unwrap();
    for (index, mutation) in [
        "foreach (['other'] as $callback) {}",
        "try { throw new Exception(); } catch (Exception $callback) {}",
        "[$callback] = ['other'];",
        "list($callback) = ['other'];",
        "$callback[0] = 'x';",
        "static $callback = 'other';",
        "extract(['callback' => 'other']);",
    ]
    .iter()
    .enumerate()
    {
        let path = temp.path().join(format!("mutation{index}.php"));
        write(
            &path,
            &format!(
                "<?php\nfunction target() {{}}\nfunction run() {{ $callback = 'target'; {mutation} $callback(); }}\n"
            ),
        );
        let result = report("file", &path, None, None);
        let references = result["files"][0]["resolution"].as_array().unwrap();
        let callback = references
            .iter()
            .find(|reference| reference["path"] == serde_json::json!(["$callback"]))
            .unwrap();
        assert_eq!(
            callback["resolution"]["status"], "unresolved",
            "mutation {mutation}"
        );
    }
}

#[test]
fn declared_receiver_chains_keep_proven_recursion() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("chains.php");
    write(
        &path,
        r#"<?php
final class Chain {
    public Chain $next;
    public static function make(): Chain { return new Chain(); }
    public function recur(): void {
        self::make()->recur();
        $this->next->recur();
    }
}
"#,
    );
    let result = report("file", &path, None, None);
    let recur = function(&result["files"][0], "recur");
    assert!(
        recur["cognitive_contributions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "recursion")
    );
    let references = result["files"][0]["resolution"].as_array().unwrap();
    assert_eq!(
        references
            .iter()
            .filter(|reference| reference["path"]
                .as_array()
                .is_some_and(|path| path.last().is_some_and(|name| name == "recur"))
                && reference["resolution"]["status"] == "exact")
            .count(),
        2
    );
}

#[test]
fn immediate_closure_calls_keep_their_outer_caller() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("immediate.php");
    write(
        &path,
        r#"<?php
function arrow_cycle() { return (fn() => arrow_cycle())(); }
function closure_cycle() { return (function() { return closure_cycle(); })(); }
final class Result {
    public function chain() { return (fn(): Result => new Result())()->chain(); }
}
"#,
    );
    let result = report("file", &path, None, None);
    for name in ["arrow_cycle", "closure_cycle", "chain"] {
        assert!(
            function(&result["files"][0], name)["cognitive_contributions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "recursion"),
            "missing recursion for {name}"
        );
    }
}

#[test]
fn static_methods_do_not_bind_this() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("static_this.php");
    write(
        &path,
        r#"<?php
final class StaticContext {
    public static function invalid() {
        $this->invalid();
        $callback = $this->invalid(...);
        $callback();
    }
}
"#,
    );
    let result = report("file", &path, None, None);
    let file = &result["files"][0];
    assert!(
        !function(file, "invalid")["cognitive_contributions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "recursion")
    );
    assert!(
        file["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .all(|reference| reference["resolution"]["status"] == "unresolved")
    );
}
