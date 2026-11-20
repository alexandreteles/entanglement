mod support;
use support::{function, only_file, report, run, write};

#[test]
fn constructors_operators_and_associated_namespaces_resolve_to_the_written_body() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("example.cpp");
    write(
        &path,
        "namespace tools { struct Item { explicit Item(int) {} int operator()() const { return 1; } Item operator+(const Item&) const { return *this; } }; int read(Item) { return 2; } }\nnamespace t = tools;\nint use(t::Item obj) { tools::Item other(1); auto result = obj + other; return read(obj) + obj(); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    for name in ["Item", "operator+", "read", "operator()"] {
        assert!(
            source["resolution"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["resolution"]["status"] == "exact"
                    && r["resolution"]["symbols"]["name"] == name),
            "{name}: {}",
            source["resolution"]
        );
    }
    assert_eq!(function(source, "use(t::Item)")["cognitive_complexity"], 0);
}

#[test]
fn a_dependent_call_and_a_function_address_add_no_recursion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generic.cpp");
    write(
        &path,
        "int check(int n) { auto address = &check; return n; }\ntemplate <class T> int check(T t) { return t.check(); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    for name in ["check(int)", "check(T)"] {
        assert_eq!(function(source, name)["cognitive_complexity"], 0);
    }
    assert!(
        source["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["path"][0] == "check" && r["resolution"]["status"] == "exact")
    );
}
