use std::{path::Path, process::Command};

use serde_json::Value;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn analyze(root: &Path, diff: Option<&Path>) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json", "--metrics", "cogc"]);
    command
        .arg(if diff.is_some() { "candidate" } else { "repo" })
        .arg(root);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("Missing file {suffix}"))
}

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

fn recursive(file: &Value, name: &str) -> bool {
    let function = file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("Missing function {name}: {}", file["functions"]));
    function["cognitive_contributions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|contribution| contribution["kind"] == "recursion")
}

#[test]
fn php_import_kinds_group_aliases_case_and_global_fallback_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "composer.json",
        r#"{"autoload":{"psr-4":{"Api\\":"api/"},"files":["api/functions.php","global.php"]}}"#,
    );
    write(
        root,
        "api/Record.php",
        "<?php namespace Api; class Record {}\n",
    );
    write(
        root,
        "api/functions.php",
        "<?php namespace Api; function transform() {} const LIMIT = 3;\n",
    );
    write(
        root,
        "global.php",
        "<?php function fallback() {} const GLOBAL_LIMIT = 5;\n",
    );
    let source = r#"<?php
namespace Client;
use Api\{Record as Item, function transform as Item, const LIMIT as Item};
use Api as Prefix;
function exercise() {
    $record = new iTeM();
    ITEM();
    echo Item;
    Prefix\transform();
    fallback();
    echo GLOBAL_LIMIT;
    echo item;
}
"#;
    write(root, "client.php", source);
    let report = analyze(root, None);
    let client = file(&report, "/client.php");
    exact(client, source, "iTeM()", "/api/Record.php", "Record");
    exact(client, source, "ITEM()", "/api/functions.php", "transform");
    exact(client, source, "Item;", "/api/functions.php", "LIMIT");
    exact(
        client,
        source,
        "Prefix\\transform()",
        "/api/functions.php",
        "transform",
    );
    exact(client, source, "fallback()", "/global.php", "fallback");
    exact(
        client,
        source,
        "GLOBAL_LIMIT;",
        "/global.php",
        "GLOBAL_LIMIT",
    );
    assert_ne!(reference(client, source, "item;")["status"], "exact");
}

#[test]
fn php_namespaced_and_fully_qualified_calls_form_cross_file_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "composer.json",
        r#"{"autoload":{"files":["a.php","b.php"]}}"#,
    );
    let first = "<?php namespace Cycle; function first() { namespace\\second(); }\nfunction direct() { DIRECT(); }\n";
    let second = "<?php namespace Cycle; function second() { \\Cycle\\first(); }\n";
    write(root, "a.php", first);
    write(root, "b.php", second);
    let report = analyze(root, None);
    exact(
        file(&report, "/a.php"),
        first,
        "namespace\\second()",
        "/b.php",
        "second",
    );
    exact(
        file(&report, "/b.php"),
        second,
        "\\Cycle\\first()",
        "/a.php",
        "first",
    );
    assert!(recursive(file(&report, "/a.php"), "first"));
    assert!(recursive(file(&report, "/a.php"), "direct"));
    assert!(recursive(file(&report, "/b.php"), "second"));
}

#[test]
fn php_dispatch_keeps_virtual_and_late_bound_calls_out_of_exact_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
class Open {
    public function virtualCall() { $this->virtualCall(); }
    public static function lateCall() { static::lateCall(); }
    private function privateCall() { $this->privateCall(); }
    final public function finalCall() { $this->finalCall(); }
    public static function staticCall() { self::staticCall(); }
}
final class Closed { public function closedCall() { $this->closedCall(); } }
"#;
    write(temp.path(), "dispatch.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/dispatch.php");
    for name in ["privateCall", "finalCall", "staticCall", "closedCall"] {
        assert!(recursive(target, name), "Missing exact cycle for {name}");
    }
    for name in ["virtualCall", "lateCall"] {
        assert!(
            !recursive(target, name),
            "Unproved dispatch created a cycle for {name}"
        );
    }
}

#[test]
fn php_inherited_constructor_execution_joins_the_call_cycle() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        "composer.json",
        r#"{"autoload":{"psr-4":{"App\\":"src/"},"files":["factory.php"]}}"#,
    );
    write(
        temp.path(),
        "src/ParentRecord.php",
        "<?php namespace App; class ParentRecord { public function __construct() { \\build(); } }\n",
    );
    write(
        temp.path(),
        "src/ChildRecord.php",
        "<?php namespace App; final class ChildRecord extends ParentRecord {}\n",
    );
    write(
        temp.path(),
        "factory.php",
        "<?php function build() { return new \\App\\ChildRecord(); }\n",
    );
    let report = analyze(temp.path(), None);
    assert!(recursive(file(&report, "/factory.php"), "build"));
    assert!(recursive(file(&report, "/ParentRecord.php"), "__construct"));
}

#[test]
fn php_traits_select_precedence_aliases_and_visibility() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
trait First { public function step() { $this->again(); } }
trait Second { public function step() {} }
final class Record {
    use First, Second { First::step insteadof Second; First::step as again; Second::step as private hidden; }
}
function exercise() {
    $record = new Record();
    $record->step();
    $record->again();
    $record->hidden();
}
"#;
    write(temp.path(), "traits.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/traits.php");
    exact(target, source, "step();", "/traits.php", "step");
    exact(target, source, "again();", "/traits.php", "step");
    assert_ne!(reference(target, source, "hidden();")["status"], "exact");
    assert!(recursive(target, "step"));
}

#[test]
fn php_callable_aliases_execute_only_after_invocation() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
function called() { $first = called(...); $second = $first; $second(); }
function escaped() { return escaped(...); }
function closureCalled() { $callback = function () { closureCalled(); }; $callback(); }
function closureEscaped() { return function () { closureEscaped(); }; }
"#;
    write(temp.path(), "callables.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/callables.php");
    assert!(recursive(target, "called"));
    assert!(recursive(target, "closureCalled"));
    assert!(!recursive(target, "escaped"));
    assert!(!recursive(target, "closureEscaped"));
}

#[test]
fn php_property_hooks_create_read_and_write_execution_edges() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
final class Record {
    public int $value { get { return readValue(); } set { writeValue($value); } }
    public int $backed { get => $this->backed; set { $this->backed = $value; } }
}
function readValue() { return new Record()->value; }
function writeValue(int $value) { $record = new Record(); $record->value = $value; }
"#;
    write(temp.path(), "hooks.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/hooks.php");
    assert!(recursive(target, "readValue"));
    assert!(recursive(target, "writeValue"));
    let hooks = target["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|function| matches!(function["name"].as_str(), Some("get" | "set")))
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 4);
    assert_eq!(
        hooks
            .iter()
            .filter(|function| function["cognitive_contributions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|contribution| contribution["kind"] == "recursion"))
            .count(),
        2
    );
}

#[test]
fn php_candidate_recomputes_composer_autoload_without_source_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let before = "{\"autoload\":{\"psr-4\":{\"Api\\\\\":\"other/\"}}}\n";
    let after = "{\"autoload\":{\"psr-4\":{\"Api\\\\\":\"src/\"}}}\n";
    write(root, "composer.json", before);
    write(
        root,
        "src/Record.php",
        "<?php namespace Api; final class Record {}\n",
    );
    let source = "<?php function make() { return new \\Api\\Record(); }\n";
    write(root, "client.php", source);
    let initial = analyze(root, None);
    assert_ne!(
        reference(file(&initial, "/client.php"), source, "\\Api\\Record()")["status"],
        "exact"
    );
    let diff = diffy::create_patch(before, after)
        .to_string()
        .replacen("--- original", "--- a/composer.json", 1)
        .replacen("+++ modified", "+++ b/composer.json", 1);
    let patch = root.join("change.diff");
    std::fs::write(&patch, diff).unwrap();
    let candidate = analyze(root, Some(&patch));
    exact(
        file(&candidate, "/client.php"),
        source,
        "\\Api\\Record()",
        "/src/Record.php",
        "Record",
    );
    assert_eq!(
        std::fs::read_to_string(root.join("composer.json")).unwrap(),
        before
    );
    write(root, "composer.json", after);
    assert_eq!(candidate["files"], analyze(root, None)["files"]);
}

#[test]
fn php_namespace_regions_select_classes_and_static_properties() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
namespace First { class Record { public static int $value = 1; } }
namespace Second {
    class Record { public static int $value = 2; }
    function exercise() { $record = new Record(); return Record::$value; }
}
"#;
    write(temp.path(), "regions.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/regions.php");
    exact(target, source, "Record();", "/regions.php", "Record");
    assert_eq!(
        reference(target, source, "Record();")["symbols"]["module"],
        serde_json::json!(["Second"])
    );
    exact(target, source, "$value;", "/regions.php", "value");
}

#[test]
fn php_pipe_invokes_callable_but_string_callable_uses_global_name() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
namespace { function leaf() {} }
namespace App {
    function leaf() {}
    function pipeline() { return 1 |> pipeline(...); }
    function strings() { $callback = 'leaf'; $callback(); }
}
"#;
    write(temp.path(), "pipe.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/pipe.php");
    assert!(recursive(target, "pipeline"));
    exact(target, source, "$callback();", "/pipe.php", "leaf");
    assert_eq!(
        reference(target, source, "$callback();")["symbols"]["module"],
        serde_json::json!([])
    );
}

#[test]
fn php_nested_named_functions_do_not_inherit_receiver_bindings() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
final class Record { public function leaf() {} }
function outer(Record $record) {
    $record->leaf();
    function inner() { $record->leaf(); }
}
"#;
    write(temp.path(), "bindings.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/bindings.php");
    assert_ne!(reference(target, source, "leaf();")["status"], "exact");
}

#[test]
fn php_inherited_private_constructor_uses_the_declaring_access_scope() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
class ParentRecord {
    private function __construct() { self::make(); denied(); }
    public static function make() { return new ChildRecord(); }
}
final class ChildRecord extends ParentRecord {}
function denied() { return new ChildRecord(); }
class ProtectedParent {
    protected function __construct() { ProtectedChild::buildProtected(); }
}
final class ProtectedChild extends ProtectedParent {
    public static function buildProtected() { return new ProtectedChild(); }
}
"#;
    write(temp.path(), "constructors.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/constructors.php");
    assert!(recursive(target, "__construct"));
    assert!(recursive(target, "make"));
    assert!(!recursive(target, "denied"));
    assert!(recursive(target, "buildProtected"));
}

#[test]
fn php_private_and_final_trait_methods_use_the_consuming_class() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
trait Calls {
    private function hidden() { $this->run(); }
    public function run() { $this->hidden(); }
}
trait FinalCalls { public function fixed() { $this->fixed(); } }
final class Closed { use Calls; }
class Open { use FinalCalls { fixed as final; } }
"#;
    write(temp.path(), "trait_calls.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/trait_calls.php");
    assert!(recursive(target, "hidden"));
    assert!(recursive(target, "run"));
    assert!(recursive(target, "fixed"));
}

#[test]
fn php_conditional_classes_and_invalid_static_calls_do_not_form_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
if ($flag) { class Conditional { public static function uncertain() { self::uncertain(); } } }
class Record { public function instance() { factory(); } }
function factory() { Record::instance(); }
"#;
    write(temp.path(), "uncertain.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/uncertain.php");
    assert!(!recursive(target, "uncertain"));
    assert!(!recursive(target, "instance"));
    assert!(!recursive(target, "factory"));
}

#[test]
fn php_composer_routes_keep_classmap_exclusions_separate() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "composer.json",
        r#"{"autoload":{"psr-4":{"Api\\":"src/"},"psr-0":{"Old_Ns\\":"psr0/"},"files":["src/helpers.php"],"classmap":["legacy/"],"exclude-from-classmap":["/src/","/legacy/Excluded.php"]}}"#,
    );
    write(
        root,
        "src/Allowed.php",
        "<?php namespace Api; class Allowed {}\n",
    );
    write(root, "src/helpers.php", "<?php function helper() {}\n");
    write(root, "legacy/Allowed.php", "<?php class Legacy {}\n");
    write(root, "legacy/Excluded.php", "<?php class Excluded {}\n");
    write(
        root,
        "psr0/Old_Ns/Thing/Name.php",
        "<?php namespace Old_Ns; class Thing_Name {}\n",
    );
    let source = "<?php function exercise() { new \\Api\\Allowed(); helper(); new Legacy(); new Excluded(); new \\Old_Ns\\Thing_Name(); }\n";
    write(root, "client.php", source);
    let report = analyze(root, None);
    let target = file(&report, "/client.php");
    exact(
        target,
        source,
        "\\Api\\Allowed()",
        "/src/Allowed.php",
        "Allowed",
    );
    exact(target, source, "helper()", "/src/helpers.php", "helper");
    exact(target, source, "Legacy()", "/legacy/Allowed.php", "Legacy");
    exact(
        target,
        source,
        "\\Old_Ns\\Thing_Name()",
        "/psr0/Old_Ns/Thing/Name.php",
        "Thing_Name",
    );
    assert_ne!(reference(target, source, "Excluded()")["status"], "exact");
}

#[test]
fn php_candidate_manifest_addition_and_deletion_use_the_snapshot() {
    for addition in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let manifest = "{\"autoload\":{\"psr-4\":{\"Api\\\\\":\"other/\"}}}\n";
        let (before, after) = if addition {
            ("", manifest)
        } else {
            (manifest, "")
        };
        if !addition {
            write(root, "composer.json", before);
        }
        write(
            root,
            "src/Record.php",
            "<?php namespace Api; class Record {}\n",
        );
        let source = "<?php function make() { return new \\Api\\Record(); }\n";
        write(root, "client.php", source);
        let initial = analyze(root, None);
        assert_eq!(
            reference(file(&initial, "/client.php"), source, "\\Api\\Record()")["status"]
                == "exact",
            addition
        );
        let patch = diffy::create_patch(before, after)
            .to_string()
            .replacen(
                "--- original",
                if addition {
                    "--- /dev/null"
                } else {
                    "--- a/composer.json"
                },
                1,
            )
            .replacen(
                "+++ modified",
                if addition {
                    "+++ b/composer.json"
                } else {
                    "+++ /dev/null"
                },
                1,
            );
        let diff = root.join("change.diff");
        std::fs::write(&diff, patch).unwrap();
        let candidate = analyze(root, Some(&diff));
        assert_eq!(
            reference(file(&candidate, "/client.php"), source, "\\Api\\Record()")["status"]
                == "exact",
            !addition
        );
        assert_eq!(root.join("composer.json").exists(), !addition);
        if addition {
            write(root, "composer.json", after);
        } else {
            std::fs::remove_file(root.join("composer.json")).unwrap();
        }
        assert_eq!(candidate["files"], analyze(root, None)["files"]);
    }
}

#[test]
fn php_nested_composer_projects_need_a_declared_dependency() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let before = "{\"name\":\"app/root\"}\n";
    let after = "{\"name\":\"app/root\",\"require\":{\"lib/record\":\"*\"}}\n";
    write(root, "composer.json", before);
    write(
        root,
        "library/composer.json",
        r#"{"name":"lib/record","autoload":{"psr-4":{"Lib\\":"src/"}}}"#,
    );
    write(
        root,
        "library/src/Record.php",
        "<?php namespace Lib; class Record {}\n",
    );
    let source = "<?php function make() { return new \\Lib\\Record(); }\n";
    write(root, "client.php", source);
    let initial = analyze(root, None);
    assert_ne!(
        reference(file(&initial, "/client.php"), source, "\\Lib\\Record()")["status"],
        "exact"
    );
    let patch = diffy::create_patch(before, after)
        .to_string()
        .replacen("--- original", "--- a/composer.json", 1)
        .replacen("+++ modified", "+++ b/composer.json", 1);
    let diff = root.join("change.diff");
    std::fs::write(&diff, patch).unwrap();
    let candidate = analyze(root, Some(&diff));
    exact(
        file(&candidate, "/client.php"),
        source,
        "\\Lib\\Record()",
        "/library/src/Record.php",
        "Record",
    );
    assert_eq!(
        std::fs::read_to_string(root.join("composer.json")).unwrap(),
        before
    );
}

#[test]
fn php_composer_development_requirements_apply_only_to_the_starting_project() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "composer.json",
        r#"{"require":{"lib/a":"*"},"require-dev":{"lib/own-dev":"*"}}"#,
    );
    write(
        root,
        "a/composer.json",
        r#"{"name":"lib/a","require":{"lib/prod":"*"},"require-dev":{"lib/test":"*"},"autoload":{"psr-4":{"A\\":"src/"}}}"#,
    );
    write(
        root,
        "a/src/Entry.php",
        "<?php namespace A; class Entry { public function exercise() { new \\TestOnly\\Record(); } }\n",
    );
    for (directory, package, namespace) in [
        ("prod", "lib/prod", "Production"),
        ("test", "lib/test", "TestOnly"),
        ("dev", "lib/own-dev", "OwnDev"),
    ] {
        write(root, &format!("{directory}/composer.json"), &serde_json::json!({"name":package,"autoload":{"psr-4":{format!("{namespace}\\"):"src/"}}}).to_string());
        write(
            root,
            &format!("{directory}/src/Record.php"),
            &format!("<?php namespace {namespace}; class Record {{}}\n"),
        );
    }
    let source = "<?php function exercise() { new \\A\\Entry(); new \\Production\\Record(); new \\OwnDev\\Record(); new \\TestOnly\\Record(); }\n";
    write(root, "client.php", source);
    let report = analyze(root, None);
    let target = file(&report, "/client.php");
    exact(target, source, "\\A\\Entry()", "/a/src/Entry.php", "Entry");
    exact(
        target,
        source,
        "\\Production\\Record()",
        "/prod/src/Record.php",
        "Record",
    );
    exact(
        target,
        source,
        "\\OwnDev\\Record()",
        "/dev/src/Record.php",
        "Record",
    );
    assert_ne!(
        reference(target, source, "\\TestOnly\\Record()")["status"],
        "exact"
    );
    let library = "<?php namespace A; class Entry { public function exercise() { new \\TestOnly\\Record(); } }\n";
    exact(
        file(&report, "/a/src/Entry.php"),
        library,
        "\\TestOnly\\Record()",
        "/test/src/Record.php",
        "Record",
    );
}

#[test]
fn php_composer_lock_graphs_do_not_merge_across_projects() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = "<?php function exercise() { new \\Extra\\Record(); }\n";
    for (app, requires_extra) in [("first", true), ("second", false)] {
        write(
            root,
            &format!("{app}/composer.json"),
            r#"{"require":{"lib/shared":"*"}}"#,
        );
        write(root, &format!("{app}/composer.lock"), &serde_json::json!({"packages":[{"name":"lib/shared","require":if requires_extra { serde_json::json!({"lib/extra":"*"}) } else { serde_json::json!({}) }}]}).to_string());
        write(root, &format!("{app}/client.php"), source);
    }
    write(
        root,
        "shared/composer.json",
        r#"{"name":"lib/shared","require":{"lib/extra":"*"},"autoload":{"psr-4":{"Shared\\":"src/"}}}"#,
    );
    write(
        root,
        "shared/src/Record.php",
        "<?php namespace Shared; class Record {}\n",
    );
    write(
        root,
        "extra/composer.json",
        r#"{"name":"lib/extra","autoload":{"psr-4":{"Extra\\":"src/"}}}"#,
    );
    write(
        root,
        "extra/src/Record.php",
        "<?php namespace Extra; class Record {}\n",
    );
    let report = analyze(root, None);
    exact(
        file(&report, "/first/client.php"),
        source,
        "\\Extra\\Record()",
        "/extra/src/Record.php",
        "Record",
    );
    assert_ne!(
        reference(
            file(&report, "/second/client.php"),
            source,
            "\\Extra\\Record()"
        )["status"],
        "exact"
    );
}

#[test]
fn php_composer_path_repositories_select_duplicate_local_package_names() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "composer.json",
        r#"{"require":{"lib/record":"*"},"repositories":[{"type":"path","url":"chosen"}]}"#,
    );
    for directory in ["chosen", "other"] {
        write(
            root,
            &format!("{directory}/composer.json"),
            r#"{"name":"lib/record","autoload":{"psr-4":{"Api\\":"src/"}}}"#,
        );
        write(
            root,
            &format!("{directory}/src/Record.php"),
            "<?php namespace Api; class Record {}\n",
        );
    }
    let source = "<?php function make() { return new \\Api\\Record(); }\n";
    write(root, "client.php", source);
    let report = analyze(root, None);
    exact(
        file(&report, "/client.php"),
        source,
        "\\Api\\Record()",
        "/chosen/src/Record.php",
        "Record",
    );
    write(root, "composer.json", r#"{"require":{"lib/record":"*"}}"#);
    let ambiguous = analyze(root, None);
    assert_ne!(
        reference(file(&ambiguous, "/client.php"), source, "\\Api\\Record()")["status"],
        "exact"
    );
}

#[test]
fn php_trait_self_construction_uses_the_consuming_class() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
trait Factory { public static function make() { return new self(); } }
final class Record {
    use Factory;
    public function __construct() { self::make(); }
}
"#;
    write(temp.path(), "trait_factory.php", source);
    let report = analyze(temp.path(), None);
    let target = file(&report, "/trait_factory.php");
    assert!(recursive(target, "make"));
    assert!(recursive(target, "__construct"));
}

#[test]
fn php_trait_precedence_keeps_later_visibility_and_final_adaptations() {
    let template = r#"<?php
trait First { public function step() { $this->step(); } }
trait Second { public function step() {} }
class Record {
    use First, Second { First::step insteadof Second; step as MODIFIER; }
}
function exercise() { $record = new Record(); $record->step(); }
"#;
    for modifier in ["private", "final"] {
        let temp = tempfile::tempdir().unwrap();
        let source = template.replace("MODIFIER", modifier);
        write(temp.path(), "trait_adaptations.php", &source);
        let report = analyze(temp.path(), None);
        let target = file(&report, "/trait_adaptations.php");
        assert!(recursive(target, "step"), "Missing {modifier} adaptation");
        assert_eq!(
            reference(target, &source, "step();")["status"] == "exact",
            modifier == "final"
        );
    }
}

#[test]
fn php_shared_trait_body_does_not_collapse_open_and_final_receiver_contexts() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<?php
trait Calls { public function step() { $this->step(); } }
final class Closed { use Calls; }
class Open { use Calls; }
"#;
    write(temp.path(), "shared_trait.php", source);
    let report = analyze(temp.path(), None);
    assert!(!recursive(file(&report, "/shared_trait.php"), "step"));
}

#[test]
fn php_literal_include_crosses_a_composer_boundary_but_conditional_include_does_not() {
    for conditional in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(root, "composer.json", r#"{"name":"app/main"}"#);
        write(
            root,
            "plugin/composer.json",
            r#"{"name":"lib/plugin","autoload":{"files":["helpers.php"]}}"#,
        );
        write(root, "plugin/helpers.php", "<?php function helper() {}\n");
        let include = "require __DIR__ . '/plugin/helpers.php';";
        let source = format!(
            "<?php\n{}\nfunction entry() {{ helper(); }}\n",
            if conditional {
                format!("if ($flag) {{ {include} }}")
            } else {
                include.to_owned()
            }
        );
        write(root, "main.php", &source);
        let report = analyze(root, None);
        let target = file(&report, "/main.php");
        if conditional {
            assert_ne!(reference(target, &source, "helper();")["status"], "exact");
        } else {
            exact(
                target,
                &source,
                "helper();",
                "/plugin/helpers.php",
                "helper",
            );
        }
    }
}
