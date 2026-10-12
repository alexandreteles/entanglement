use super::*;

#[test]
fn imports_overloads_and_lexical_shadowing_resolve_conservatively() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let api = r#"package api
fun choose(value: Int) = value
fun choose(value: String) = value
fun chooseNullable(value: Int?) = value
fun chooseNullable(value: String?) = value
fun named(label: String, count: Int = 1) = count
fun named(label: String, suffix: String) = label + suffix
fun collect(vararg names: String) = names.size
fun collect(count: Int) = count
fun single() = 1
"#;
    let caller_source = r#"package client
import api.choose
import api.chooseNullable
import api.collect
import api.named
import api.single

fun importedCall() = single()
fun intOverloadCall() = choose(1)
fun stringOverloadCall() = choose("s")
fun uncertainOverloadCall() = chooseNullable(null)
fun namedDefaultCall() = named(label = "x")
fun varargCall() = collect("a", "b")
fun localTypedCall() {
    val typed: String = "x"
    choose(typed)
}
fun localShadow() {
    fun single() = 2
    single()
}
"#;
    write(&root.join("src/main/kotlin/api/Api.kt"), api);
    write(&root.join("src/main/kotlin/client/Use.kt"), caller_source);

    let project = report(&run(&root, "repo", None));
    let caller = file(&project, "/client/Use.kt");
    exact_target(
        ref_at(caller, caller_source, "single()", 0),
        "/api/Api.kt",
        "single",
    );
    let mut overload_failures = Vec::new();
    for (call, declaration) in [
        ("choose(1)", "fun choose(value: Int)"),
        ("choose(\"s\")", "fun choose(value: String)"),
        (
            "named(label = \"x\")",
            "fun named(label: String, count: Int = 1)",
        ),
        ("collect(\"a\", \"b\")", "fun collect(vararg names: String)"),
        ("choose(typed)", "fun choose(value: String)"),
    ] {
        let overload = ref_at(caller, caller_source, call, 0);
        let target_start = overload["resolution"]["symbols"]["start_byte"]
            .as_u64()
            .map(|start| start as usize);
        let expected_start = api.find(declaration).unwrap();
        if overload["resolution"]["status"] != "exact" || target_start != Some(expected_start) {
            overload_failures.push(format!(
                "{call}: expected exact {declaration} at {expected_start}, got {overload:#}"
            ));
        }
    }
    assert!(
        overload_failures.is_empty(),
        "statically typed argument forms failed:\n{}",
        overload_failures.join("\n")
    );
    let uncertain = ref_at(caller, caller_source, "chooseNullable(null)", 0);
    assert_ne!(
        uncertain["resolution"]["status"], "exact",
        "null is compatible with either nullable overload without a declared type"
    );
    exact_target(
        ref_at(caller, caller_source, "single()", 2),
        "/client/Use.kt",
        "single",
    );
}

#[test]
fn wildcard_and_unresolved_imports_do_not_invent_local_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    write(
        &root.join("src/main/kotlin/client/Star.kt"),
        "package client\nimport absent.*\nfun useStar() = hidden()\n",
    );
    let project = report(&run(&root, "repo", None));
    let star = file(&project, "/client/Star.kt");
    let reference = ref_at(
        star,
        "package client\nimport absent.*\nfun useStar() = hidden()\n",
        "hidden()",
        0,
    );
    assert_ne!(reference["resolution"]["status"], "exact");
}

#[test]
fn local_functions_do_not_leak_out_of_scope_or_type_larger_expressions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package sample\n",
        "fun container() {\n",
        "  fun println(value: String) {}\n",
        "  println(\"inside\")\n",
        "}\n",
        "fun elsewhere() { println(\"outside\") }\n",
        "fun text(): String = \"sample\"\n",
        "fun pick(value: String) = value\n",
        "fun pick(value: Int) = value\n",
        "fun use() { pick(text().length) }\n",
    );
    write(&root.join("src/main/kotlin/sample/Scopes.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Scopes.kt");
    let local_print = ref_at(file, source, "println(\"inside\")", 0);
    exact_target(local_print, "/Scopes.kt", "println");
    assert_eq!(
        local_print["resolution"]["symbols"]["start_byte"],
        source.find("fun println").unwrap()
    );
    let outside_print = ref_at(file, source, "println(\"outside\")", 0);
    assert_ne!(
        outside_print["resolution"]["status"], "exact",
        "a nested local declaration cannot replace an out-of-scope package or library call"
    );

    let nested_expression = ref_at(file, source, "pick(text().length)", 0);
    if nested_expression["resolution"]["status"] == "exact" {
        assert_eq!(
            nested_expression["resolution"]["symbols"]["start_byte"],
            source.find("fun pick(value: Int)").unwrap(),
            "the nested text() result must not be mistaken for the type of `.length`"
        );
    }
}

#[test]
fn script_files_and_gradle_scripts_are_not_one_kotlin_source_namespace() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let use_script = concat!(
        "fun localScript() = 1\n",
        "fun useLocal() = localScript()\n",
        "fun useOtherScript() = scriptOnly()\n",
    );
    let source_file = "fun useBuildScript() = gradleOnly()\n";
    write(&root.join("Api.kts"), "fun scriptOnly() = 2\n");
    write(&root.join("Use.kts"), use_script);
    write(&root.join("build.gradle.kts"), "fun gradleOnly() = 3\n");
    write(&root.join("src/main/kotlin/Source.kt"), source_file);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/Use.kts");
    exact_target(
        ref_at(use_file, use_script, "localScript()", 1),
        "/Use.kts",
        "localScript",
    );
    assert_ne!(
        ref_at(use_file, use_script, "scriptOnly()", 0)["resolution"]["status"],
        "exact",
        "separate script files are separate source units"
    );
    assert_ne!(
        ref_at(file(&project, "/Source.kt"), source_file, "gradleOnly()", 0,)["resolution"]["status"],
        "exact",
        "a Gradle build script does not contribute declarations to application source"
    );
}
