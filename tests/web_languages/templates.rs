use super::*;

#[test]
fn multiline_template_holes_keep_host_tokens_and_count_guest_text_after_holes() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"
function hostCall(value: boolean) { return value; }
function multiline(flag: boolean, ready: boolean) {
  return html`<p>
    ${flag
      ? hostCall(false)
      : ''}
    <b>afterFirst</b>
    ${ready
      ? hostCall(true)
      : ''}
    <i>afterSecond</i>
  </p>`;
}
"#;
    let without_text = source
        .replace("    <b>afterFirst</b>\n", "")
        .replace("    <i>afterSecond</i>\n", "");
    let report = analyze_source(temp.path(), "rich.ts", source);
    let sparse = analyze_source(temp.path(), "sparse.ts", &without_text);
    let file = &report["files"][0];
    assert_eq!(
        file["nloc"].as_u64().unwrap(),
        sparse["files"][0]["nloc"].as_u64().unwrap() + 2
    );
    let function = function(file, "multiline");
    assert_eq!(function["cyclomatic_complexity"], 3);
    assert_eq!(
        function["halstead"]["operands"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["text"] == "hostCall")
            .count(),
        2
    );
    let host_tokens = file["halstead"]["operators"]
        .as_array()
        .unwrap()
        .iter()
        .chain(file["halstead"]["operands"].as_array().unwrap())
        .filter(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains("hostCall"))
        });
    assert_eq!(host_tokens.count(), 3);
    let calls: Vec<_> = file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["path"]
                .as_array()
                .is_some_and(|path| path.iter().any(|part| part == "hostCall"))
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|item| item["resolution"]["status"] == "exact")
    );
}
