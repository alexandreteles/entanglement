mod support;
use support::{file, function, only_file, report, run, write};

#[test]
fn declaration_macros_keep_the_written_metrics_and_source_offsets() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        &root.join("config.hpp"),
        "#define BEGIN_API namespace api {\n#define END_API }\n#define LIB_API\n",
    );
    let src = "#include \"config.hpp\"\nBEGIN_API\nint LIB_API twice(int value) { return value + value; }\nEND_API\n";
    write(&root.join("lib.cpp"), src);
    let result = report(&run("repo", root, None, None));
    let source = file(&result, "lib.cpp");
    let scope = function(source, "api::twice(int)");
    assert_eq!(scope["start_byte"], src.find("int LIB_API").unwrap());
    assert_eq!(source["nloc"], 4);
    let spellings = &source["halstead"]["operands"];
    assert!(
        spellings
            .as_array()
            .unwrap()
            .iter()
            .any(|token| token["text"] == "LIB_API"),
        "{spellings}"
    );
}

#[test]
fn comments_in_macro_replacements_are_excluded_from_physical_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("macros.c");
    write(
        &path,
        "#define ADD(n) ((n) + \\\n/* first line\n   second line */ \\\n  1)\nint f(void) { return ADD(2); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    assert!(!source["halstead"].to_string().contains("first line"));
    assert_eq!(source["nloc"], 4);
}

#[test]
fn damaged_regions_retain_intact_declarations_and_report_lost_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("types.hpp");
    write(&path, "struct Intact { int value; };\nint broken = ;\n");
    let report = report(&run("file", &path, None, Some("structure")));
    let coverage = &report["structure"]["coverage"][0];
    assert_eq!(coverage["status"], "partial");
    assert!(
        coverage["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|code| code.as_str().unwrap().starts_with("parse_error:"))
    );
    assert!(
        report["structure"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| unit["name"] == "Intact")
    );
}

#[test]
fn nested_namespace_macros_keep_each_scope_in_qualified_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("api.cpp");
    write(
        &path,
        "#define BEGIN namespace api { inline namespace v1 {\n#define END } } /* close both namespaces */\nBEGIN\nint get() { return 1; }\nEND\nint use() { return api::v1::get(); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    assert!(
        source["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |item| item["path"] == serde_json::json!(["api", "v1", "get"])
                    && item["resolution"]["status"] == "exact"
            )
    );
}

#[test]
fn a_verified_function_header_survives_a_damaged_body() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.c");
    write(&path, "int compute(int n) { int x = ; return n; }\n");
    let result = report(&run("file", &path, None, Some("structure")));
    let coverage = &result["structure"]["coverage"][0];
    assert_eq!(coverage["status"], "partial");
    assert!(
        coverage["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "parse_error")
    );
}
