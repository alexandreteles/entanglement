use super::*;

#[test]
fn fstrings_keep_guest_ranges_and_host_metrics_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"
def render(flag, choose, value, width):
    markup = html(f"""<p>café
        {flag and choose(False)}
        {flag if value else choose(True)}
    </p>""")
    styles = tools.css(r".card { color: red; }")
    plain = html("<b>plain guest</b>")
    binary = html(b"<i>bytes guest</i>")
    formatted = html(f"<p>{value:>{width}}</p>")
    escaped = html(f"<p>{{literal}}</p>")
    printed = print("<script>not a template</script>")
    unknown = custom_html("<i>not injected</i>")
    return markup, styles, plain, binary, formatted, escaped, printed, unknown

def recursive(flag):
    return html(f"<p>{flag and recursive(False)}</p>")
"#;
    let path = temp.path().join("templates.py");
    write(&path, source);
    let report = report(&run("file", &path, None, Some("cc,cogc,halstead")));
    let file = &report["files"][0];
    assert_eq!(injection_count(file, "html", true), 3);
    assert_eq!(injection_count(file, "css", true), 1);
    assert_eq!(injection_count(file, "html", false), 3);
    assert_eq!(injection_count(file, "javascript", true), 0);
    assert_eq!(file["injections"].as_array().unwrap().len(), 7);
    let render = function(file, "render");
    assert!(render["cyclomatic_complexity"].as_u64().unwrap() >= 3);
    assert!(named_operand(&render["halstead"], "choose"));
    assert_eq!(
        operand_count(&render["halstead"], "choose"),
        3,
        "the function parameter and two host hole references each count once"
    );
    assert!(recursive(function(file, "recursive")));
    for injection in file["injections"].as_array().unwrap() {
        let start = injection["start_byte"].as_u64().unwrap() as usize;
        let end = injection["end_byte"].as_u64().unwrap() as usize;
        assert!(source.is_char_boundary(start) && source.is_char_boundary(end));
    }
}

#[test]
fn html_python_script_blocks_use_the_registered_language_analyzer() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("page.html");
    write(
        &path,
        r#"<script lang=python>
def lang_loop(flag):
    if flag:
        return lang_loop(False)
</script>
<script type="python">
def type_loop(flag):
    if flag:
        return type_loop(False)
</script>
"#,
    );
    let report = report(&run("file", &path, None, Some("cc,cogc")));
    let html = &report["files"][0];
    assert_eq!(injection_count(html, "python", true), 2);
    for name in ["lang_loop", "type_loop"] {
        let target = function(html, name);
        assert!(recursive(target));
        assert_eq!(target["cyclomatic_complexity"], 2);
    }
}

#[test]
fn python_can_inject_kotlin_through_its_registered_language_name() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("embedded.py");
    write(
        &path,
        r#"
def host():
    return kotlin("""fun embedded(flag: Boolean) {
    if (flag) println("from Kotlin")
}""")
"#,
    );

    let result = report(&run("file", &path, None, Some("cc,cogc,halstead")));
    let file = &result["files"][0];
    assert_eq!(file["language"], "python");
    assert_eq!(injection_count(file, "kotlin", true), 1);
    assert_eq!(function(file, "embedded")["cyclomatic_complexity"], 2);
    assert_eq!(function(file, "host")["cyclomatic_complexity"], 1);
}

#[test]
fn kotlin_guest_declarations_are_isolated_by_host_file_and_injection_context() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let api = concat!(
        "def api():\n",
        "    return kotlin(\"\"\"fun isolated() = 1\"\"\")\n",
    );
    let use_source = concat!(
        "def first_context():\n",
        "    return kotlin(\"\"\"fun localContextOnly() = 1\"\"\")\n",
        "def second_context():\n",
        "    return kotlin(\"\"\"fun useGuest() { isolated(); localContextOnly() }\"\"\")\n",
    );
    write(&root.join("Api.py"), api);
    write(&root.join("Use.py"), use_source);

    let report = report(&run("repo", &root, None, Some("cogc")));
    let use_file = file(&report, "/Use.py");
    let resolution_at = |needle: &str, occurrence: usize| {
        let start = use_source
            .match_indices(needle)
            .nth(occurrence)
            .unwrap_or_else(|| panic!("missing {occurrence} occurrence of {needle:?}"))
            .0;
        use_file["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["start_byte"] == start)
            .unwrap_or_else(|| panic!("missing reference at byte {start} for {needle:?}"))
    };

    assert_ne!(
        resolution_at("isolated()", 0)["resolution"]["status"],
        "exact",
        "Kotlin guest declarations from another Python host file stay isolated"
    );
    assert_ne!(
        resolution_at("localContextOnly()", 1)["resolution"]["status"],
        "exact",
        "separate Kotlin injections in one host file are separate source contexts"
    );
}

#[test]
fn adjacent_kotlin_injections_keep_context_prefixes_mapped_through_candidate_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let path = root.join("embedded.py");
    let before = concat!(
        "def host():\r\n",
        "    first = kotlin(\"\"\"fun first(flag: Boolean) = if (flag) 1 else 0\r\n",
        "\"\"\")\r\n",
        "    second = kotlin(\"\"\"fun second(flag: Boolean) = if (flag) 1 else 0\r\n",
        "fun ordinaryContext(value: Boolean) {\r\n",
        "    // context(String) is only a comment\r\n",
        "    val fake = \"context(value: String) { braces }\"\r\n",
        "    context(value) { if (it) println(fake) }\r\n",
        "}\r\n",
        "\"\"\")\r\n",
    );
    let after = concat!(
        "def host():\r\n",
        "    first = kotlin(\"\"\"fun first(flag: Boolean) = if (flag) 1 else 0\r\n",
        "\"\"\")\r\n",
        "    second = kotlin(\"\"\"context(String)\r\n",
        "fun second(flag: Boolean) = if (flag) length else 0\r\n",
        "fun ordinaryContext(value: Boolean) {\r\n",
        "    // context(String) is only a comment\r\n",
        "    val fake = \"context(value: String) { braces }\"\r\n",
        "    context(value) { if (it) println(fake) }\r\n",
        "}\r\n",
        "\"\"\")\r\n",
    );
    write(&path, before);
    let before_report = report(&run("file", &path, None, Some("all")));
    let before_file = &before_report["files"][0];
    assert_eq!(injection_count(before_file, "kotlin", true), 2);
    assert_eq!(function(before_file, "second")["cyclomatic_complexity"], 2);
    let before_context = function(before_file, "ordinaryContext");
    assert_eq!(before_context["cyclomatic_complexity"], 1);
    assert!(
        before_file["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("anonymous@"))
                    && item["cyclomatic_complexity"] == 2
            })
    );

    let diff = temp.path().join("embedded.diff");
    let patch = diffy::create_patch(before, after)
        .to_string()
        .replacen("--- original", "--- a/embedded.py", 1)
        .replacen("+++ modified", "+++ b/embedded.py", 1);
    write(&diff, patch);
    let candidate = report(&run("candidate", &path, Some(&diff), Some("all")));
    assert_eq!(std::fs::read(&path).unwrap(), before.as_bytes());
    let candidate_file = file(&candidate, "embedded.py");
    assert_eq!(injection_count(candidate_file, "kotlin", true), 2);
    let second = function(candidate_file, "second");
    assert_eq!(second["cyclomatic_complexity"], 2);
    let added_header_start = after.find("context(String)").unwrap() + "context(".len();
    assert!(
        candidate_file["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["name"] == "second"
                    && item["halstead"]["operands"]
                        .as_array()
                        .is_some_and(|operands| {
                            operands.iter().any(|token| {
                                token["text"] == "String"
                                    && token["start_byte"].as_u64()
                                        == Some(added_header_start as u64)
                            })
                        })
            })
    );
    assert_eq!(
        function(candidate_file, "ordinaryContext")["cyclomatic_complexity"],
        1
    );
    assert!(
        candidate_file["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("anonymous@"))
                    && item["cyclomatic_complexity"] == 2
            })
    );
    let deltas = candidate["patch"]["files"][0]["functions"]
        .as_array()
        .expect("candidate deltas");
    let second_delta = deltas
        .iter()
        .find(|item| item["name"] == "second")
        .expect("second keeps a before and after scope");
    assert!(second_delta["before_range"].is_object() && second_delta["after_range"].is_object());
    let callback_deltas: Vec<_> = deltas
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

    write(&path, after);
    let fresh_after = report(&run("repo", &root, None, Some("all")));
    assert_eq!(candidate["files"], fresh_after["files"]);
    let reverse_diff = temp.path().join("embedded-reverse.diff");
    let reverse_patch = diffy::create_patch(after, before)
        .to_string()
        .replacen("--- original", "--- a/embedded.py", 1)
        .replacen("+++ modified", "+++ b/embedded.py", 1);
    write(&reverse_diff, reverse_patch);
    let removed = report(&run("candidate", &path, Some(&reverse_diff), Some("all")));
    assert_eq!(
        function(file(&removed, "embedded.py"), "second")["cyclomatic_complexity"],
        2
    );
    write(&path, before);
    let fresh_before = report(&run("repo", &root, None, Some("all")));
    assert_eq!(removed["files"], fresh_before["files"]);
}
