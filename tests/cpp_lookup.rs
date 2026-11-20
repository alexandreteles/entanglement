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

#[test]
fn c_linkage_can_cross_a_namespace_and_keeps_a_cpp_overload_separate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("api.c"), "int run(void) { return 1; }\n");
    write(&root.join("other.cpp"), "int run() { return 2; }\n");
    write(
        &root.join("bridge.cpp"),
        "namespace api { extern \"C\" int run(); }\nint bridge() { return api::run(); }\n",
    );
    let result = report(&run("repo", root, None, None));
    let file = support::file(&result, "bridge.cpp");
    let reference = file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"].as_array().unwrap().last().unwrap() == "run")
        .unwrap();
    assert!(
        reference["resolution"]["symbols"]["file"]
            .as_str()
            .unwrap()
            .ends_with("api.c"),
        "{reference}"
    );
}

#[test]
fn a_const_receiver_selects_the_const_overload_and_dependent_calls_stay_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proof.cpp");
    write(
        &path,
        "struct Item { int get() { return get(); } int get() const { return get(); } };\ntemplate<class T> int probe(T x) { return probe(x); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    for name in ["Item::get()", "Item::get() const"] {
        assert_eq!(function(source, name)["cognitive_complexity"], 1, "{name}");
    }
    assert_eq!(function(source, "probe(T)")["cognitive_complexity"], 0);
}

#[test]
fn an_inherited_method_and_a_locally_bound_lambda_have_exact_targets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owners.cpp");
    write(
        &path,
        "struct Base { int value() { return 4; } }; struct Derived : Base {};\nint use(Derived d) { auto thunk = [&]() { return d.value(); }; return thunk(); }\n",
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    for name in ["value", "thunk"] {
        assert!(
            source["resolution"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["resolution"]["status"] == "exact"
                    && item["resolution"]["symbols"]["name"] == name),
            "{name}: {}",
            source["resolution"]
        );
    }
}
