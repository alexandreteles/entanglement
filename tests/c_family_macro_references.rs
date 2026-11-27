mod support;
use support::{file, references_to, report, run, write};

#[test]
fn macro_references_follow_includes_redefinitions_and_undef() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        &root.join("util.h"),
        "#define TWICE(n) ((n) * 2)\n#define COUNT 2\n",
    );
    let source = "int TWICE;\n#include \"util.h\"\nint first(int n){return TWICE(n) + COUNT;}\nint bare(void){return TWICE;}\n#undef TWICE\n#define TWICE(n) (n)\nint second(int n){return TWICE(n);}\n#undef TWICE\nint last(void){return TWICE;}\n";
    write(&root.join("main.c"), source);
    let result = report(&run("repo", root, None, None));
    let main = file(&result, "main.c");
    let uses = references_to(main, "TWICE");
    assert_eq!(uses.len(), 4);
    let expected = [
        ("util.h", "macro", 0),
        ("main.c", "static", 0),
        ("main.c", "macro", source.find("#define").unwrap()),
        ("main.c", "static", 0),
    ];
    for (reference, (file, kind, start)) in uses.iter().zip(expected) {
        let target = &reference["resolution"]["symbols"];
        assert_eq!(target["kind"], kind);
        assert_eq!(target["start_byte"], start);
        assert!(target["file"].as_str().unwrap().ends_with(file));
    }
    assert_eq!(
        references_to(main, "COUNT")[0]["resolution"]["symbols"]["kind"],
        "macro"
    );
    let single = report(&run("file", &root.join("main.c"), None, Some("cogc")));
    let first = references_to(file(&single, "main.c"), "TWICE")[0];
    assert_eq!(first["resolution"], uses[0]["resolution"]);
}

#[test]
fn different_active_definitions_make_a_shared_macro_reference_ambiguous() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        &root.join("work.h"),
        "static inline int read(void){return VALUE;}\n",
    );
    for name in ["left", "right"] {
        write(
            &root.join(format!("{name}.c")),
            "#define VALUE 1\n#include \"work.h\"\n",
        );
    }
    let result = report(&run("repo", root, None, None));
    let matches = references_to(file(&result, "work.h"), "VALUE");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["resolution"]["status"], "ambiguous");
    assert_eq!(
        matches[0]["resolution"]["symbols"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
