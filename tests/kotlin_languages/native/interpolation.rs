use super::*;

#[test]
fn kotlin_multidollar_raw_string_interpolation_keeps_only_interpolated_names_as_operands() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("interpolation.kt");
    let source_text = r#"fun render(name: String) = $$"""price: $$name; literal: $name; value: $$${name.length}"""
"#;
    write(&path, source_text);

    let output = report(&run("file", &path, None, Some("halstead")));
    let file = &output["files"][0];
    assert_eq!(file["language"], "kotlin");
    let render = function(file, "render");
    let direct_interpolation = source_text.find("$$name").unwrap();
    let literal_dollar_name = source_text.find("literal: $name").unwrap() + "literal: ".len();
    let expression_interpolation = source_text.rfind("name.length").unwrap();

    assert!(has_operand_at(
        &render["halstead"],
        "name",
        direct_interpolation + 2
    ));
    assert!(has_operand_at(
        &render["halstead"],
        "name",
        expression_interpolation
    ));
    assert!(
        !has_operand_at(&render["halstead"], "name", literal_dollar_name + 1),
        "single-dollar text is literal with a two-dollar interpolation prefix"
    );
}
