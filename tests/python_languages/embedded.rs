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
