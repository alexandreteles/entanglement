use super::*;

fn patch(path: &str, before: &str, after: &str) -> String {
    diffy::create_patch(before, after)
        .to_string()
        .replacen("--- original", &format!("--- a/{path}"), 1)
        .replacen("+++ modified", &format!("+++ b/{path}"), 1)
}

fn reference<'a>(file: &'a Value, source: &str, needle: &str) -> &'a Value {
    let start = source
        .match_indices(needle)
        .next()
        .unwrap_or_else(|| panic!("missing {needle:?}"))
        .0;
    file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["start_byte"] == start)
        .unwrap_or_else(|| panic!("missing reference at {start}"))
}

#[test]
fn candidate_matches_same_line_lambdas_after_a_line_shift_and_incremental_edit() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source_path = root.join("src/main/kotlin/Callbacks.kt");
    let before = concat!(
        "package sample\n",
        "fun visit(values: List<Int>) { values.forEach { if (it > 0) println(it) }; ",
        "values.forEach { if (it < 0) println(it) } }\n",
    );
    let after = concat!(
        "package sample\n",
        "// Shift both lambda scopes without changing their relative positions.\n",
        "fun visit(values: List<Int>) { values.forEach { if (it > 0 && it != 2) println(it) }; ",
        "values.forEach { if (it < 0) println(it) } }\n",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\n",
    );
    write(&source_path, before);
    let diff = temp.path().join("change.diff");
    write(
        &diff,
        concat!(
            "--- a/src/main/kotlin/Callbacks.kt\n",
            "+++ b/src/main/kotlin/Callbacks.kt\n",
            "@@ -1,2 +1,3 @@\n",
            " package sample\n",
            "+// Shift both lambda scopes without changing their relative positions.\n",
            "-fun visit(values: List<Int>) { values.forEach { if (it > 0) println(it) }; values.forEach { if (it < 0) println(it) } }\n",
            "+fun visit(values: List<Int>) { values.forEach { if (it > 0 && it != 2) println(it) }; values.forEach { if (it < 0) println(it) } }\n",
        ),
    );

    let candidate = report(&run("candidate", &root, Some(&diff), Some("all")));
    assert_eq!(std::fs::read_to_string(&source_path).unwrap(), before);
    let candidate_file = file(&candidate, "Callbacks.kt");
    let after_functions = candidate_file["functions"].as_array().unwrap();
    let anonymous: Vec<_> = after_functions
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous"))
        })
        .collect();
    assert_eq!(anonymous.len(), 2, "both same-line lambdas need scopes");

    let deltas = candidate["patch"]["files"][0]["functions"]
        .as_array()
        .expect("function deltas");
    let matched_anonymous: Vec<_> = deltas
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous"))
        })
        .collect();
    assert_eq!(matched_anonymous.len(), 2, "each lambda has one delta");
    assert!(
        matched_anonymous
            .iter()
            .all(|item| item["before_range"].is_object() && item["after_range"].is_object())
    );
    let changed = matched_anonymous
        .iter()
        .filter(|item| item["cyclomatic_complexity"]["delta"] == 1)
        .count();
    assert_eq!(changed, 1, "only the first lambda adds a decision");

    write(&source_path, after);
    let fresh = report(&run("repo", &root, None, Some("all")));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn candidate_recomputes_overloads_import_aliases_and_extension_receiver_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let api_path = root.join("src/main/kotlin/api/Api.kt");
    let client_path = root.join("src/main/kotlin/client/Client.kt");
    let api_before = concat!(
        "package api\n",
        "fun choose(value: Int) = value\n",
        "fun choose(value: String) = value.length\n",
        "fun String.decorate() = this\n",
    );
    let api_after = api_before.replace(
        "fun String.decorate() = this",
        "fun CharSequence.decorate() = toString()",
    );
    let client_before = concat!(
        "package client\n",
        "import api.choose\n",
        "import api.decorate\n",
        "fun use(value: Any, s: String) { choose(value); s.decorate() }\n",
    );
    let client_after = concat!(
        "package client\n",
        "import api.choose as select\n",
        "import api.decorate\n",
        "fun use(value: Any, s: String) { select(value); s.decorate() }\n",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\n",
    );
    write(&api_path, api_before);
    write(&client_path, client_before);
    let before = report(&run("repo", &root, None, Some("all")));
    let client_before_file = file(&before, "/client/Client.kt");
    assert_ne!(
        reference(client_before_file, client_before, "choose(value)")["resolution"]["status"],
        "exact",
        "unknown overload context must not choose an overload"
    );

    let diff = temp.path().join("change.diff");
    write(
        &diff,
        format!(
            "{}{}",
            patch("src/main/kotlin/api/Api.kt", api_before, &api_after),
            patch(
                "src/main/kotlin/client/Client.kt",
                client_before,
                client_after
            )
        ),
    );
    let candidate = report(&run("candidate", &root, Some(&diff), Some("all")));
    let client_after_file = file(&candidate, "/client/Client.kt");
    assert_ne!(
        reference(client_after_file, client_after, "select(value)")["resolution"]["status"],
        "exact"
    );
    write(&api_path, api_after.as_bytes());
    write(&client_path, client_after.as_bytes());
    let fresh = report(&run("repo", &root, None, Some("all")));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn multiline_lambda_edit_keeps_unchanged_sibling_matched_after_a_line_shift() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source_path = root.join("src/main/kotlin/Callbacks.kt");
    let before = concat!(
        "package sample\n",
        "fun visit(values: List<Int>) {\n",
        "    values.forEach {\n",
        "        if (it > 0) println(it)\n",
        "    }\n",
        "    values.forEach {\n",
        "        if (it < 0) println(it)\n",
        "    }\n",
        "}\n",
    );
    let after = concat!(
        "package sample\n",
        "// Keep this callback on its original owner after a line shift.\n",
        "fun visit(values: List<Int>) {\n",
        "    values.forEach {\n",
        "        if (it > 0 && it != 2) println(it)\n",
        "    }\n",
        "    values.forEach {\n",
        "        if (it < 0) println(it)\n",
        "    }\n",
        "}\n",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\n",
    );
    write(&source_path, before);
    let diff = temp.path().join("change.diff");
    write(&diff, patch("src/main/kotlin/Callbacks.kt", before, after));

    let candidate = report(&run("candidate", &root, Some(&diff), Some("all")));
    let deltas = candidate["patch"]["files"][0]["functions"]
        .as_array()
        .unwrap();
    let lambdas: Vec<_> = deltas
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    assert_eq!(lambdas.len(), 2);
    assert!(
        lambdas
            .iter()
            .all(|item| item["before_range"].is_object() && item["after_range"].is_object())
    );
    assert_eq!(
        lambdas
            .iter()
            .filter(|item| item["cyclomatic_complexity"]["delta"] == 1)
            .count(),
        1
    );

    write(&source_path, after);
    let fresh = report(&run("repo", &root, None, Some("all")));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn default_argument_lambdas_stay_with_their_overloaded_owner() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source_path = root.join("src/main/kotlin/Overloads.kt");
    let before = concat!(
        "package sample\n",
        "fun render(callback: () -> Unit = {\n",
        "    if (firstReady && enabled) first()\n",
        "}, mode: String) { callback() }\n",
        "\n",
        "fun render(callback: () -> Unit = {\n",
        "    if (secondReady) second()\n",
        "}, mode: Int) { callback() }\n",
    );
    let after = concat!(
        "package sample\n",
        "fun render(mode: String) { println(mode) }\n",
        "\n",
        "fun render(callback: () -> Unit = {\n",
        "    if (secondReady) second()\n",
        "}, mode: Int) { callback() }\n",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\n",
    );
    write(&source_path, before);
    let diff = temp.path().join("change.diff");
    write(&diff, patch("src/main/kotlin/Overloads.kt", before, after));

    let candidate = report(&run("candidate", &root, Some(&diff), Some("all")));
    let deltas = candidate["patch"]["files"][0]["functions"]
        .as_array()
        .unwrap();
    let lambdas: Vec<_> = deltas
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    assert_eq!(lambdas.len(), 2);
    let removed: Vec<_> = lambdas
        .iter()
        .filter(|item| item["after_range"].is_null())
        .collect();
    let retained: Vec<_> = lambdas
        .iter()
        .filter(|item| item["before_range"].is_object() && item["after_range"].is_object())
        .collect();
    assert_eq!(removed.len(), 1);
    assert_eq!(retained.len(), 1);
    assert_eq!(removed[0]["cyclomatic_complexity"]["before"], 3);
    assert_eq!(retained[0]["cyclomatic_complexity"]["before"], 2);
    assert_eq!(retained[0]["cyclomatic_complexity"]["after"], 2);

    write(&source_path, after);
    let fresh = report(&run("repo", &root, None, Some("all")));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn candidate_and_patch_keep_context_receiver_edits_stable_with_crlf_and_fake_context_text() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source_path = root.join("src/main/kotlin/Contexts.kt");
    let before = concat!(
        "package sample\r\n",
        "fun addContext(flag: Boolean) = if (flag) 1 else 0\r\n",
        "context(String)\r\n",
        "fun removeContext(flag: Boolean) = if (flag) length else 0\r\n",
        "fun invocation(value: Boolean) {\r\n",
        "    // context(String) in a comment is not a declaration prefix\r\n",
        "    val fake = \"context(value: String) { braces }\"\r\n",
        "    context(value) { if (it) println(fake) }\r\n",
        "}\r\n",
    );
    let after = concat!(
        "package sample\r\n",
        "context(String)\r\n",
        "fun addContext(flag: Boolean) = if (flag) length else 0\r\n",
        "fun removeContext(flag: Boolean) = if (flag) 1 else 0\r\n",
        "fun invocation(value: Boolean) {\r\n",
        "    // context(String) in a comment is not a declaration prefix\r\n",
        "    val fake = \"context(value: String) { braces }\"\r\n",
        "    context(value) { if (it) println(fake) }\r\n",
        "}\r\n",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\r\n",
    );
    write(&source_path, before);
    let before_report = report(&run("repo", &root, None, Some("all")));
    let before_file = file(&before_report, "Contexts.kt");
    assert!(has_operand(
        &function(before_file, "removeContext")["halstead"],
        "String"
    ));
    assert_eq!(
        function(before_file, "invocation")["cyclomatic_complexity"],
        1
    );
    let before_callbacks: Vec<_> = before_file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    assert_eq!(
        before_callbacks.len(),
        1,
        "the ordinary context(value) call must remain a call with a lambda"
    );
    assert_eq!(before_callbacks[0]["cyclomatic_complexity"], 2);

    let diff = temp.path().join("contexts.diff");
    write(&diff, patch("src/main/kotlin/Contexts.kt", before, after));
    let candidate = report(&run("candidate", &root, Some(&diff), Some("all")));
    assert_eq!(std::fs::read(&source_path).unwrap(), before.as_bytes());
    let candidate_file = file(&candidate, "Contexts.kt");
    let add_context = function(candidate_file, "addContext");
    let remove_context = function(candidate_file, "removeContext");
    assert!(has_operand(&add_context["halstead"], "String"));
    assert!(!has_operand(&remove_context["halstead"], "String"));
    let context_type_start = after.find("context(String)").unwrap() + "context(".len();
    assert!(has_operand_at(
        &add_context["halstead"],
        "String",
        context_type_start
    ));
    assert_eq!(
        function(candidate_file, "invocation")["cyclomatic_complexity"],
        1
    );
    let candidate_callbacks: Vec<_> = candidate_file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    assert_eq!(candidate_callbacks.len(), 1);
    assert_eq!(candidate_callbacks[0]["cyclomatic_complexity"], 2);
    let callback_deltas: Vec<_> = candidate["patch"]["files"][0]["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    assert_eq!(callback_deltas.len(), 1);
    assert!(callback_deltas[0]["before_range"].is_object());
    assert!(callback_deltas[0]["after_range"].is_object());
    assert_eq!(callback_deltas[0]["cyclomatic_complexity"]["delta"], 0);

    for (name, before_cc, after_cc) in [("addContext", 2, 2), ("removeContext", 2, 2)] {
        let deltas = candidate["patch"]["files"][0]["functions"]
            .as_array()
            .expect("candidate scope deltas");
        let delta = deltas
            .iter()
            .find(|item| item["name"] == name)
            .unwrap_or_else(|| panic!("missing stable patch delta for {name}: {deltas:#?}"));
        assert!(delta["before_range"].is_object() && delta["after_range"].is_object());
        assert_eq!(delta["cyclomatic_complexity"]["before"], before_cc);
        assert_eq!(delta["cyclomatic_complexity"]["after"], after_cc);
    }

    let patch_report = report(&run("patch", &source_path, Some(&diff), Some("all")));
    let patched_file = &patch_report["patch"]["files"][0]["after"];
    assert_eq!(
        function(patched_file, "addContext")["cyclomatic_complexity"],
        2
    );
    assert_eq!(
        function(patched_file, "removeContext")["cyclomatic_complexity"],
        2
    );
    assert_eq!(
        function(patched_file, "invocation")["cyclomatic_complexity"],
        1
    );

    write(&source_path, after);
    let fresh_after = report(&run("repo", &root, None, Some("all")));
    assert_eq!(candidate["files"], fresh_after["files"]);

    let reverse_diff = temp.path().join("contexts-reverse.diff");
    write(
        &reverse_diff,
        patch("src/main/kotlin/Contexts.kt", after, before),
    );
    let removed = report(&run("candidate", &root, Some(&reverse_diff), Some("all")));
    let removed_file = file(&removed, "Contexts.kt");
    assert!(!has_operand(
        &function(removed_file, "addContext")["halstead"],
        "String"
    ));
    assert!(has_operand(
        &function(removed_file, "removeContext")["halstead"],
        "String"
    ));
    write(&source_path, before);
    let fresh_before = report(&run("repo", &root, None, Some("all")));
    assert_eq!(removed["files"], fresh_before["files"]);
}
