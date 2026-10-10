use super::*;

#[test]
fn html_scripts_styles_and_tagged_templates_keep_host_expressions() {
    let temp = tempfile::tempdir().unwrap();
    let html = temp.path().join("page.html");
    write(
        &html,
        r#"<script lang="ts">
function htmlLoop(flag: boolean): boolean { return flag ? htmlLoop(false) : false; }
</script>
<script type="module">function jsLoop(flag) { return flag && jsLoop(false); }</script>
<script type="text/typescript">function mimeLoop(flag: boolean): boolean { return flag ? mimeLoop(false) : false; }</script>
<style>.card { color: red; }</style>
"#,
    );
    let html_report = report(&run("file", &html, None, Some("cc,cogc")));
    let html_file = &html_report["files"][0];
    assert_eq!(html_file["language"], "html");
    assert!(recursive(function(html_file, "htmlLoop")));
    assert!(recursive(function(html_file, "jsLoop")));
    assert!(recursive(function(html_file, "mimeLoop")));
    assert_eq!(function(html_file, "htmlLoop")["cyclomatic_complexity"], 2);
    assert_eq!(function(html_file, "jsLoop")["cyclomatic_complexity"], 2);
    assert_eq!(function(html_file, "mimeLoop")["cyclomatic_complexity"], 2);
    for language in ["typescript", "javascript", "css"] {
        assert!(
            html_file["injections"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| {
                    item["analyzed"] == true
                        && item["language"].as_str().unwrap().contains(language)
                }),
            "missing analyzed HTML {language} block"
        );
    }
    let css = temp.path().join("theme.css");
    write(&css, ".card { color: red; }\n");
    let css_report = report(&run("file", &css, None, Some("halstead")));
    assert_eq!(css_report["files"][0]["language"], "css");

    let source = r#"
function render(flag: boolean, choose: () => string) {
  const markup = html`<p>café ${flag && choose()} ${flag ? choose() : "none"}</p>`;
  const styles = css`:root { color: ${flag ? "red" : "blue"}; }`;
  const staticMarkup = html`<span>static</span>`;
  const staticStyles = css`.card { display: block; }`;
  const extra = custom`${flag}`;
  return markup + styles + staticMarkup + staticStyles + extra;
}
function embeddedRecursion(flag: boolean) {
  return html`<p>${flag && embeddedRecursion(false)}</p>`;
}
function memberTag() {
  return Lit.html`<strong>member template</strong>`;
}
"#;
    let ts = temp.path().join("template.ts");
    write(&ts, source);
    let template_report = report(&run("file", &ts, None, Some("cc,cogc,halstead")));
    let template_file = &template_report["files"][0];
    let render = function(template_file, "render");
    assert_eq!(render["cyclomatic_complexity"], 4);
    for operand in ["flag", "choose", "red", "blue"] {
        assert!(
            render["halstead"]["operands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|token| token["text"] == operand),
            "host token {operand} disappeared from its function"
        );
    }
    assert!(recursive(function(template_file, "embeddedRecursion")));
    let injections = template_file["injections"].as_array().unwrap();
    for (language, minimum_count) in [("html", 3), ("css", 2)] {
        assert!(
            injections
                .iter()
                .filter(|item| {
                    item["analyzed"] == true
                        && item["language"].as_str().unwrap().contains(language)
                })
                .count()
                >= minimum_count,
            "missing static or interpolated {language} injection"
        );
    }
    assert!(
        injections
            .iter()
            .any(|item| { item["language"] == "custom" && item["analyzed"] == false })
    );
    for injection in injections.iter().filter(|item| item["analyzed"] == true) {
        let start = injection["start_byte"].as_u64().unwrap() as usize;
        let end = injection["end_byte"].as_u64().unwrap() as usize;
        assert!(source.is_char_boundary(start) && source.is_char_boundary(end));
    }

    let root = temp.path().join("template-project");
    write(
        &root.join("package.json"),
        "{\"name\":\"template-fixture\"}\n",
    );
    let template = root.join("render.ts");
    let before = concat!(
        "function render(flag: boolean, choose: () => string) {\n",
        "  const markup = html`<p>café ${flag && choose()} ${flag ? choose() : \"none\"}</p>`;\n",
        "  const styles = css`:root { color: ${flag ? \"red\" : \"blue\"}; }`;\n",
        "  const staticMarkup = html`<span>static</span>`;\n",
        "  const staticStyles = css`.card { display: block; }`;\n",
        "  const extra = custom`${flag}`;\n",
        "  return markup + styles + staticMarkup + staticStyles + extra;\n}\n",
    );
    let after = before
        .replace(
            "<p>café ${flag && choose()}",
            "<section>café ${flag || choose()}",
        )
        .replace("</p>`;", "</section>`;");
    write(&template, before);
    let diff = temp.path().join("template.diff");
    write(
        &diff,
        concat!(
            "--- a/render.ts\n+++ b/render.ts\n@@ -1,8 +1,8 @@\n",
            " function render(flag: boolean, choose: () => string) {\n",
            "-  const markup = html`<p>café ${flag && choose()} ${flag ? choose() : \"none\"}</p>`;\n",
            "+  const markup = html`<section>café ${flag || choose()} ${flag ? choose() : \"none\"}</section>`;\n",
            "   const styles = css`:root { color: ${flag ? \"red\" : \"blue\"}; }`;\n",
            "   const staticMarkup = html`<span>static</span>`;\n",
            "   const staticStyles = css`.card { display: block; }`;\n",
            "   const extra = custom`${flag}`;\n",
            "   return markup + styles + staticMarkup + staticStyles + extra;\n",
            " }\n",
        ),
    );
    let patched = report(&run("candidate", &template, Some(&diff), None));
    assert_eq!(std::fs::read(&template).unwrap(), before.as_bytes());
    write(&template, after.as_bytes());
    let fresh = report(&run("repo", &root, None, None));
    assert_eq!(file(&patched, "/render.ts"), file(&fresh, "/render.ts"));
}
