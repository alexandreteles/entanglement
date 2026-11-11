mod support;
use support::{coverage, file, function, recursive, report, resolution_statuses, run, write};

#[test]
fn c_metrics_follow_shared_rules() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("scan.c");
    write(
        &path,
        r#"#include <stdio.h>

/* commentOnlySentinel */
static int scan(const int *values, int count, int limit) {
    int total = 0;
    for (int i = 0; i < count; i++) {
        if (values[i] > limit && limit > 0 || count == 1) {
            continue;
        } else if (values[i] < 0) {
            break;
        } else {
            total += values[i];
        }
    }
    while (total > 100) total /= 2;
    do { total--; } while (total > 50);
    switch (total) {
    case 1: case 2: return 1;
    default: break;
    }
    return total > 0 ? total : scan(values, count - 1, limit);
}

int *first(int *values) { return values; }
"#,
    );
    let output = report(&run("file", &path, None, None));
    let file = &output["files"][0];
    assert_eq!(file["language"], "c");
    let scan = function(file, "scan");
    assert_eq!(scan["cyclomatic_complexity"], 11);
    assert_eq!(scan["cognitive_complexity"], 12);
    assert!(recursive(scan));
    assert_eq!(scan["nloc"], 19);
    assert_eq!(function(file, "first")["cyclomatic_complexity"], 1);
}

#[test]
fn c_references_follow_linkage_and_includes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("include/util.h"),
        "#ifndef UTIL_H\n#define UTIL_H\n#define TWICE(x) ((x) * 2)\nint shared(int value);\n#endif\n",
    );
    write(
        &root.join("util.c"),
        "#include \"include/util.h\"\nstatic int helper(int value) { return value + 1; }\nint shared(int value) { return value > 0 ? helper(value - 1) : 0; }\n",
    );
    write(
        &root.join("main.c"),
        "#include <stdio.h>\n#include \"util.h\"\nstatic int helper(int value) { return shared(TWICE(value)); }\nint main(void) { int shared = 1; printf(\"%d\", helper(shared)); return 0; }\n",
    );
    let output = report(&run("repo", root, None, None));
    let main = file(&output, "main.c");
    assert_eq!(resolution_statuses(main, "helper"), ["exact main.c:helper"]);
    assert_eq!(
        resolution_statuses(main, "shared"),
        ["exact util.c:shared", "unresolved"]
    );
    assert_eq!(resolution_statuses(main, "TWICE"), ["exact util.h:TWICE"]);
    assert_eq!(resolution_statuses(main, "printf"), ["external"]);
    let util = file(&output, "util.c");
    assert_eq!(resolution_statuses(util, "helper"), ["exact util.c:helper"]);
}

#[test]
fn c_recursion_crosses_translation_units() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("parity.h"),
        "int is_even(int n);\nint is_odd(int n);\n",
    );
    write(
        &root.join("even.c"),
        "#include \"parity.h\"\nint is_even(int n) { return n == 0 ? 1 : is_odd(n - 1); }\n",
    );
    write(
        &root.join("odd.c"),
        "#include \"parity.h\"\nint is_odd(int n) { return n == 0 ? 0 : is_even(n - 1); }\nint twice(void) { return 2; }\n",
    );
    write(
        &root.join("other.c"),
        "int twice(void) { return 3; }\nint own(void) { return twice(); }\n",
    );
    write(
        &root.join("user.c"),
        "int twice(void);\nint use(void) { return twice(); }\n",
    );
    let output = report(&run("repo", root, None, None));
    assert!(recursive(function(file(&output, "even.c"), "is_even")));
    assert!(recursive(function(file(&output, "odd.c"), "is_odd")));
    assert_eq!(
        resolution_statuses(file(&output, "other.c"), "twice"),
        ["exact other.c:twice"]
    );
    assert_eq!(
        resolution_statuses(file(&output, "user.c"), "twice"),
        ["ambiguous"]
    );
}

#[test]
fn headers_with_cpp_syntax_use_the_cpp_grammar() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("plain.h"),
        "struct point { int x; };\nint add(int a, int b);\n",
    );
    write(
        &root.join("shape.h"),
        "namespace geometry {\nclass Shape { public: int area() const { return 1; } };\n}\n",
    );
    let output = report(&run("repo", root, None, None));
    assert_eq!(file(&output, "plain.h")["language"], "c");
    assert_eq!(file(&output, "shape.h")["language"], "cpp");
    assert_eq!(
        function(file(&output, "shape.h"), "area")["cyclomatic_complexity"],
        1
    );
}

#[test]
fn c_includes_become_module_imports() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(&root.join("include/util.h"), "int shared(int value);\n");
    write(
        &root.join("main.c"),
        "#include <stdio.h>\n#include \"util.h\"\nint main(void) { return shared(1); }\n",
    );
    let output = report(&run("repo", root, None, Some("structure")));
    let imports = output["structure"]["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|edge| edge["kind"] == "import")
        .map(|edge| {
            let target = edge["target"].as_str().unwrap();
            let target = target.rsplit('/').next().unwrap().to_owned();
            (target, edge["resolution"].as_str().unwrap().to_owned())
        })
        .collect::<Vec<_>>();
    assert!(
        imports.contains(&("util.h".into(), "resolved".into())),
        "{imports:?}"
    );
    assert!(
        imports.contains(&("<stdio.h>".into(), "external".into())),
        "{imports:?}"
    );
    let coverage = output["structure"]["coverage"].as_array().unwrap();
    assert!(
        coverage
            .iter()
            .all(|item| item["effective"] == "inapplicable")
    );
}

#[test]
fn linkage_guards_parse_in_both_languages() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let guarded = "#ifdef __cplusplus\nextern \"C\" {\n#endif\nint shared(int value);\n#ifdef __cplusplus\n} // extern \"C\"\n#endif\n";
    write(&root.join("api.h"), guarded);
    write(&root.join("api.hpp"), guarded);
    let output = report(&run("repo", root, None, None));
    assert_eq!(file(&output, "api.h")["language"], "c");
    for header in ["api.h", "api.hpp"] {
        assert_eq!(
            coverage(&output, header)["limitations"],
            serde_json::json!(["preprocessor_variants", "no_oo"])
        );
    }
}
