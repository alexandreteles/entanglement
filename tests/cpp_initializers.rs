mod support;
use support::{only_file, references_to, report, run, write};

#[test]
fn constructor_initializers_name_members_and_bases() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("scope.cpp");
    write(
        &path,
        "struct Base { int id; };\nstruct Scope : Base {\n  Scope() : Base(), size_(0) {}\n  Scope(int size);\n  int size_;\n};\nScope::Scope(int size) : Base(), size_(size) {}\n",
    );
    let report = report(&run("file", &path, None, None));
    let file = only_file(&report);
    for (name, count, kind) in [("Base", 3, "struct"), ("size_", 2, "other")] {
        let references = references_to(file, name);
        assert_eq!(references.len(), count, "{name}");
        assert!(
            references
                .iter()
                .all(|item| item["resolution"]["status"] == "exact"
                    && item["resolution"]["symbols"]["kind"] == kind)
        );
    }
}

#[test]
fn constructor_parameters_shadow_globals_in_initializer_lists() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("scope.cpp");
    write(
        &path,
        "int size = 4;\nint other = 5;\nstruct Scope {\n Scope(int size) : size_(size), other_(other) {}\n Scope(int size, int extra);\n int size_; int other_;\n};\nScope::Scope(int size, int extra) : size_(size), other_(other) {}\n",
    );
    let report = report(&run("file", &path, None, None));
    let file = only_file(&report);
    assert!(references_to(file, "size").is_empty());
    let globals = references_to(file, "other");
    assert_eq!(globals.len(), 2);
    assert!(
        globals
            .iter()
            .all(|item| item["resolution"]["symbols"]["kind"] == "static")
    );
}
