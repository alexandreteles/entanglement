use super::*;

#[test]
fn php_import_kinds_group_aliases_case_and_global_fallback_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"Api\\":"api/"},"files":["api/functions.php","global.php"]}}"#,
    );
    write(
        &root.join("api/Record.php"),
        "<?php namespace Api; class Record {}\n",
    );
    write(
        &root.join("api/functions.php"),
        "<?php namespace Api; function transform() {} const LIMIT = 3;\n",
    );
    write(
        &root.join("global.php"),
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
    write(&root.join("client.php"), source);
    let report = support::report(&support::run("repo", root, None, Some("cogc")));
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
        &root.join("composer.json"),
        r#"{"autoload":{"files":["a.php","b.php"]}}"#,
    );
    let first = "<?php namespace Cycle; function first() { namespace\\second(); }\nfunction direct() { DIRECT(); }\n";
    let second = "<?php namespace Cycle; function second() { \\Cycle\\first(); }\n";
    write(&root.join("a.php"), first);
    write(&root.join("b.php"), second);
    let report = support::report(&support::run("repo", root, None, Some("cogc")));
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
    assert!(recursive(function(file(&report, "/a.php"), "first")));
    assert!(recursive(function(file(&report, "/a.php"), "direct")));
    assert!(recursive(function(file(&report, "/b.php"), "second")));
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
    write(&temp.path().join("regions.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
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
    write(&temp.path().join("pipe.php"), source);
    let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
    let target = file(&report, "/pipe.php");
    assert!(recursive(function(target, "pipeline")));
    exact(target, source, "$callback();", "/pipe.php", "leaf");
    assert_eq!(
        reference(target, source, "$callback();")["symbols"]["module"],
        serde_json::json!([])
    );
}

#[test]
fn php_conditional_declarations_and_invalid_static_calls_do_not_form_cycles() {
    for (start, end) in [
        ("if ($flag) {", "}"),
        ("try { mayThrow();", "} catch (Throwable $error) {}"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = format!(
            r#"<?php
{start}
class Conditional {{ public static function uncertain() {{ self::uncertain(); }} }}
function conditionalFunction() {{ conditionalFunction(); }}
{end}
class Record {{ public function instance() {{ factory(); }} }}
function factory() {{ Record::instance(); }}
conditionalFunction();
Conditional::uncertain();
new Conditional();
"#
        );
        write(&temp.path().join("uncertain.php"), &source);
        let report = support::report(&support::run("repo", temp.path(), None, Some("cogc")));
        let target = file(&report, "/uncertain.php");
        for name in ["uncertain", "conditionalFunction", "instance", "factory"] {
            assert!(!recursive(function(target, name)), "{start}: {name}");
        }
        for needle in ["conditionalFunction();", "uncertain();", "Conditional();"] {
            assert_eq!(
                reference(target, &source, needle)["status"],
                "unresolved",
                "{start}: {needle}"
            );
        }
    }
}
