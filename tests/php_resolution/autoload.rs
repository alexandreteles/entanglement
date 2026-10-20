use super::*;

#[test]
fn php_candidate_recomputes_composer_autoload_without_source_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let before = "{\"autoload\":{\"psr-4\":{\"Api\\\\\":\"other/\"}}}\n";
    let after = "{\"autoload\":{\"psr-4\":{\"Api\\\\\":\"src/\"}}}\n";
    write(&root.join("composer.json"), before);
    write(
        &root.join("src/Record.php"),
        "<?php namespace Api; final class Record {}\n",
    );
    let source = "<?php function make() { return new \\Api\\Record(); }\n";
    write(&root.join("client.php"), source);
    let initial = support::report(&support::run("repo", root, None, Some("cogc")));
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
    let candidate = support::report(&support::run("candidate", root, Some(&patch), Some("cogc")));
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
    write(&root.join("composer.json"), after);
    assert_eq!(
        candidate["files"],
        support::report(&support::run("repo", root, None, Some("cogc")))["files"]
    );
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
            write(&root.join("composer.json"), before);
        }
        write(
            &root.join("src/Record.php"),
            "<?php namespace Api; class Record {}\n",
        );
        let source = "<?php function make() { return new \\Api\\Record(); }\n";
        write(&root.join("client.php"), source);
        let initial = support::report(&support::run("repo", root, None, Some("cogc")));
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
        let candidate =
            support::report(&support::run("candidate", root, Some(&diff), Some("cogc")));
        assert_eq!(
            reference(file(&candidate, "/client.php"), source, "\\Api\\Record()")["status"]
                == "exact",
            !addition
        );
        assert_eq!(root.join("composer.json").exists(), !addition);
        if addition {
            write(&root.join("composer.json"), after);
        } else {
            std::fs::remove_file(root.join("composer.json")).unwrap();
        }
        assert_eq!(
            candidate["files"],
            support::report(&support::run("repo", root, None, Some("cogc")))["files"]
        );
    }
}

#[test]
fn php_nested_composer_projects_need_a_declared_dependency() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let before = "{\"name\":\"app/root\"}\n";
    let after = "{\"name\":\"app/root\",\"require\":{\"lib/record\":\"*\"}}\n";
    write(&root.join("composer.json"), before);
    write(
        &root.join("library/composer.json"),
        r#"{"name":"lib/record","autoload":{"psr-4":{"Lib\\":"src/"}}}"#,
    );
    write(
        &root.join("library/src/Record.php"),
        "<?php namespace Lib; class Record {}\n",
    );
    let source = "<?php function make() { return new \\Lib\\Record(); }\n";
    write(&root.join("client.php"), source);
    let initial = support::report(&support::run("repo", root, None, Some("cogc")));
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
    let candidate = support::report(&support::run("candidate", root, Some(&diff), Some("cogc")));
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
        write(&root.join(&format!("{directory}/composer.json")), &serde_json::json!({"name":package,"autoload":{"psr-4":{format!("{namespace}\\"):"src/"}}}).to_string());
        write(
            &root.join(&format!("{directory}/src/Record.php")),
            &format!("<?php namespace {namespace}; class Record {{}}\n"),
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
            &root.join(&format!("{app}/composer.json")),
            r#"{"require":{"lib/shared":"*"}}"#,
        );
        write(&root.join(&format!("{app}/composer.lock")), &serde_json::json!({"packages":[{"name":"lib/shared","require":if requires_extra { serde_json::json!({"lib/extra":"*"}) } else { serde_json::json!({}) }}]}).to_string());
        write(&root.join(&format!("{app}/client.php")), source);
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
            &root.join(&format!("{directory}/composer.json")),
            r#"{"name":"lib/record","autoload":{"psr-4":{"Api\\":"src/"}}}"#,
        );
        write(
            &root.join(&format!("{directory}/src/Record.php")),
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
