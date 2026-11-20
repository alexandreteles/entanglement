mod support;
use support::{function, only_file, report, run, write};

#[test]
fn pointer_and_value_parameters_select_different_overloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pointers.cpp");
    write(
        &path,
        "int pick(int n) { return n; }\nint pick(int *p) { return pick(p); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    assert_eq!(function(source, "pick(int*)")["cognitive_complexity"], 1);
    assert_eq!(function(source, "pick(int)")["cognitive_complexity"], 0);
}

#[test]
fn prototype_parameter_names_have_no_scope_in_the_next_function() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("public.c");
    write(
        &path,
        "void install(int (*work)(void));\nint work(void) { return work(); }\n",
    );
    let result = report(&run("file", &path, None, None));
    assert_eq!(
        function(only_file(&result), "work")["cognitive_complexity"],
        1
    );
}

#[test]
fn tag_and_typedef_spellings_resolve_in_the_correct_c_name_space() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nodes.c");
    write(
        &path,
        "typedef struct Node { int value; } Node;\nstruct Pair { struct Node *tag; Node *alias; };\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    let items = source["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["path"][0] == "Node")
        .collect::<Vec<_>>();
    assert!(
        items.iter().all(|r| r["resolution"]["status"] == "exact"),
        "{items:#?}"
    );
    assert!(
        items
            .iter()
            .any(|r| r["resolution"]["symbols"]["kind"] == "struct")
    );
    assert!(
        items
            .iter()
            .any(|r| r["resolution"]["symbols"]["kind"] == "type_alias")
    );
}
