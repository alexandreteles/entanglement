use super::*;

struct ManifestCase {
    before: Option<&'static str>,
    after: Option<&'static str>,
    files: &'static [(&'static str, &'static str)],
    namespace: &'static str,
    before_exact: bool,
    after_target: Option<&'static str>,
}

#[test]
fn composer_candidate_changes_use_snapshot_routes_and_dependencies() {
    const OTHER: &str = "{\"autoload\":{\"psr-4\":{\"Api\\\\\":\"other/\"}}}\n";
    const ROUTED: &str = "{\"autoload\":{\"psr-4\":{\"Api\\\\\":\"src/\"}}}\n";
    for case in [
        ManifestCase {
            before: Some(OTHER),
            after: Some(ROUTED),
            namespace: "Api",
            before_exact: false,
            after_target: Some("/src/Record.php"),
            files: &[(
                "src/Record.php",
                "<?php namespace Api; final class Record {}\n",
            )],
        },
        ManifestCase {
            before: None,
            after: Some(OTHER),
            namespace: "Api",
            before_exact: true,
            after_target: None,
            files: &[("src/Record.php", "<?php namespace Api; class Record {}\n")],
        },
        ManifestCase {
            before: Some(OTHER),
            after: None,
            namespace: "Api",
            before_exact: false,
            after_target: Some("/src/Record.php"),
            files: &[("src/Record.php", "<?php namespace Api; class Record {}\n")],
        },
        ManifestCase {
            before: Some("{\"name\":\"app/root\"}\n"),
            after: Some("{\"name\":\"app/root\",\"require\":{\"lib/record\":\"*\"}}\n"),
            namespace: "Lib",
            before_exact: false,
            after_target: Some("/library/src/Record.php"),
            files: &[
                (
                    "library/composer.json",
                    r#"{"name":"lib/record","autoload":{"psr-4":{"Lib\\":"src/"}}}"#,
                ),
                (
                    "library/src/Record.php",
                    "<?php namespace Lib; class Record {}\n",
                ),
            ],
        },
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let manifest = root.join("composer.json");
        for (path, source) in case.files {
            write(&root.join(path), source);
        }
        if let Some(source) = case.before {
            write(&manifest, source);
        }
        let needle = format!("\\{}\\Record()", case.namespace);
        let source = format!("<?php function make() {{ return new {needle}; }}\n");
        write(&root.join("client.php"), &source);
        let initial = support::report(&support::run("repo", root, None, Some("cogc")));
        assert_eq!(
            reference(file(&initial, "/client.php"), &source, &needle)["status"] == "exact",
            case.before_exact
        );
        let diff = root.join("change.diff");
        write(
            &diff,
            support::patch("composer.json", case.before, case.after),
        );
        let candidate =
            support::report(&support::run("candidate", root, Some(&diff), Some("cogc")));
        let client = file(&candidate, "/client.php");
        if let Some(target) = case.after_target {
            exact(client, &source, &needle, target, "Record");
        } else {
            assert_ne!(reference(client, &source, &needle)["status"], "exact");
        }
        assert_eq!(manifest.exists(), case.before.is_some());
        assert_eq!(
            std::fs::read_to_string(&manifest).ok().as_deref(),
            case.before
        );
        match case.after {
            Some(source) => write(&manifest, source),
            None => std::fs::remove_file(&manifest).unwrap(),
        }
        assert_eq!(
            candidate["files"],
            support::report(&support::run("repo", root, None, Some("cogc")))["files"]
        );
    }
}

#[test]
fn php_composer_routes_keep_classmap_exclusions_separate() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"Api\\":"src/"},"psr-0":{"Old_Ns\\":"psr0/"},"files":["src/helpers.php"],"classmap":["legacy/"],"exclude-from-classmap":["/src/","/legacy/Excluded.php"]}}"#,
    );
    write(
        &root.join("src/Allowed.php"),
        "<?php namespace Api; class Allowed {}\n",
    );
    write(
        &root.join("src/helpers.php"),
        "<?php function helper() {}\n",
    );
    write(&root.join("legacy/Allowed.php"), "<?php class Legacy {}\n");
    write(
        &root.join("legacy/Excluded.php"),
        "<?php class Excluded {}\n",
    );
    write(
        &root.join("psr0/Old_Ns/Thing/Name.php"),
        "<?php namespace Old_Ns; class Thing_Name {}\n",
    );
    let source = "<?php function exercise() { new \\Api\\Allowed(); helper(); new Legacy(); new Excluded(); new \\Old_Ns\\Thing_Name(); }\n";
    write(&root.join("client.php"), source);
    let report = support::report(&support::run("repo", root, None, Some("cogc")));
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
fn php_composer_development_requirements_apply_only_to_the_starting_project() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("composer.json"),
        r#"{"require":{"lib/a":"*"},"require-dev":{"lib/own-dev":"*"}}"#,
    );
    write(
        &root.join("a/composer.json"),
        r#"{"name":"lib/a","require":{"lib/prod":"*"},"require-dev":{"lib/test":"*"},"autoload":{"psr-4":{"A\\":"src/"}}}"#,
    );
    write(
        &root.join("a/src/Entry.php"),
        "<?php namespace A; class Entry { public function exercise() { new \\TestOnly\\Record(); } }\n",
    );
    for (directory, package, namespace) in [
        ("prod", "lib/prod", "Production"),
        ("test", "lib/test", "TestOnly"),
        ("dev", "lib/own-dev", "OwnDev"),
    ] {
        write(&root.join(format!("{directory}/composer.json")), serde_json::json!({"name":package,"autoload":{"psr-4":{format!("{namespace}\\"):"src/"}}}).to_string());
        write(
            &root.join(format!("{directory}/src/Record.php")),
            format!("<?php namespace {namespace}; class Record {{}}\n"),
        );
    }
    let source = "<?php function exercise() { new \\A\\Entry(); new \\Production\\Record(); new \\OwnDev\\Record(); new \\TestOnly\\Record(); }\n";
    write(&root.join("client.php"), source);
    let report = support::report(&support::run("repo", root, None, Some("cogc")));
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
            &root.join(format!("{app}/composer.json")),
            r#"{"require":{"lib/shared":"*"}}"#,
        );
        write(&root.join(format!("{app}/composer.lock")), serde_json::json!({"packages":[{"name":"lib/shared","require":if requires_extra { serde_json::json!({"lib/extra":"*"}) } else { serde_json::json!({}) }}]}).to_string());
        write(&root.join(format!("{app}/client.php")), source);
    }
    write(
        &root.join("shared/composer.json"),
        r#"{"name":"lib/shared","require":{"lib/extra":"*"},"autoload":{"psr-4":{"Shared\\":"src/"}}}"#,
    );
    write(
        &root.join("shared/src/Record.php"),
        "<?php namespace Shared; class Record {}\n",
    );
    write(
        &root.join("extra/composer.json"),
        r#"{"name":"lib/extra","autoload":{"psr-4":{"Extra\\":"src/"}}}"#,
    );
    write(
        &root.join("extra/src/Record.php"),
        "<?php namespace Extra; class Record {}\n",
    );
    let report = support::report(&support::run("repo", root, None, Some("cogc")));
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
        &root.join("composer.json"),
        r#"{"require":{"lib/record":"*"},"repositories":[{"type":"path","url":"chosen"}]}"#,
    );
    for directory in ["chosen", "other"] {
        write(
            &root.join(format!("{directory}/composer.json")),
            r#"{"name":"lib/record","autoload":{"psr-4":{"Api\\":"src/"}}}"#,
        );
        write(
            &root.join(format!("{directory}/src/Record.php")),
            "<?php namespace Api; class Record {}\n",
        );
    }
    let source = "<?php function make() { return new \\Api\\Record(); }\n";
    write(&root.join("client.php"), source);
    let report = support::report(&support::run("repo", root, None, Some("cogc")));
    exact(
        file(&report, "/client.php"),
        source,
        "\\Api\\Record()",
        "/chosen/src/Record.php",
        "Record",
    );
    write(
        &root.join("composer.json"),
        r#"{"require":{"lib/record":"*"}}"#,
    );
    let ambiguous = support::report(&support::run("repo", root, None, Some("cogc")));
    assert_ne!(
        reference(file(&ambiguous, "/client.php"), source, "\\Api\\Record()")["status"],
        "exact"
    );
}

#[test]
fn php_literal_include_crosses_a_composer_boundary_but_conditional_include_does_not() {
    for conditional in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(&root.join("composer.json"), r#"{"name":"app/main"}"#);
        write(
            &root.join("plugin/composer.json"),
            r#"{"name":"lib/plugin","autoload":{"files":["helpers.php"]}}"#,
        );
        write(
            &root.join("plugin/helpers.php"),
            "<?php function helper() {}\n",
        );
        let include = "require __DIR__ . '/plugin/helpers.php';";
        let source = format!(
            "<?php\n{}\nfunction entry() {{ helper(); }}\n",
            if conditional {
                format!("if ($flag) {{ {include} }}")
            } else {
                include.to_owned()
            }
        );
        write(&root.join("main.php"), &source);
        let report = support::report(&support::run("repo", root, None, Some("cogc")));
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
