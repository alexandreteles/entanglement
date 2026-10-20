use super::*;

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
    write(&temp.path().join("dispatch.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/dispatch.php");
    for name in ["privateCall", "finalCall", "staticCall", "closedCall"] {
        assert!(
            recursive(function(target, name)),
            "Missing exact cycle for {name}"
        );
    }
    for name in ["virtualCall", "lateCall"] {
        assert!(
            !recursive(function(target, name)),
            "Unproved dispatch created a cycle for {name}"
        );
    }
}

#[test]
fn php_inherited_constructor_execution_joins_the_call_cycle() {
    let temp = tempfile::tempdir().unwrap();
    write(
        &temp.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"},"files":["factory.php"]}}"#,
    );
    write(
        &temp.path().join("src/ParentRecord.php"),
        "<?php namespace App; class ParentRecord { public function __construct() { \\build(); } }\n",
    );
    write(
        &temp.path().join("src/ChildRecord.php"),
        "<?php namespace App; final class ChildRecord extends ParentRecord {}\n",
    );
    write(
        &temp.path().join("factory.php"),
        "<?php function build() { return new \\App\\ChildRecord(); }\n",
    );
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    assert!(recursive(function(file(&report, "/factory.php"), "build")));
    assert!(recursive(function(
        file(&report, "/ParentRecord.php"),
        "__construct"
    )));
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
    write(&temp.path().join("traits.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/traits.php");
    exact(target, source, "step();", "/traits.php", "step");
    exact(target, source, "again();", "/traits.php", "step");
    assert_ne!(reference(target, source, "hidden();")["status"], "exact");
    assert!(recursive(function(target, "step")));
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
    write(&temp.path().join("callables.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/callables.php");
    assert!(recursive(function(target, "called")));
    assert!(recursive(function(target, "closureCalled")));
    assert!(!recursive(function(target, "escaped")));
    assert!(!recursive(function(target, "closureEscaped")));
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
    write(&temp.path().join("hooks.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/hooks.php");
    assert!(recursive(function(target, "readValue")));
    assert!(recursive(function(target, "writeValue")));
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
            .filter(|function| support::recursive(function))
            .count(),
        2
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
    write(&temp.path().join("bindings.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
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
    write(&temp.path().join("constructors.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/constructors.php");
    assert!(recursive(function(target, "__construct")));
    assert!(recursive(function(target, "make")));
    assert!(!recursive(function(target, "denied")));
    assert!(recursive(function(target, "buildProtected")));
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
    write(&temp.path().join("trait_calls.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/trait_calls.php");
    assert!(recursive(function(target, "hidden")));
    assert!(recursive(function(target, "run")));
    assert!(recursive(function(target, "fixed")));
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
    write(&temp.path().join("trait_factory.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/trait_factory.php");
    assert!(recursive(function(target, "make")));
    assert!(recursive(function(target, "__construct")));
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
        write(&temp.path().join("trait_adaptations.php"), &source);
        let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
        let target = file(&report, "/trait_adaptations.php");
        assert!(
            recursive(function(target, "step")),
            "Missing {modifier} adaptation"
        );
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
    write(&temp.path().join("shared_trait.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    assert!(!recursive(function(
        file(&report, "/shared_trait.php"),
        "step"
    )));
}
