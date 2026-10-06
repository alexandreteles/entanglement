use super::*;

#[test]
fn python_function_and_comprehension_bindings_stay_scoped() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("scope-project");
    write(
        &root,
        "pyproject.toml",
        "[project]\nname = 'scope-fixture'\n",
    );
    write(&root, "src/pkg/__init__.py", "");
    write(&root, "src/pkg/mod.py", "def leaf():\n    return 1\n");
    let source = r#"from pkg.mod import leaf

def assigned():
    leaf()
    leaf = lambda: 0
    leaf()

def comprehension(items):
    [leaf() for leaf in items]
    leaf()

class Calls:
    def leaf(self):
        return 1
    def invoke(self):
        self.leaf()
    def bare(self):
        leaf()
"#;
    write(&root, "src/scopes.py", source);
    write(
        &root,
        "src/star.py",
        "from pkg.mod import *\n\ndef use_star_name():\n    leaf()\n",
    );
    write(
        &root,
        "src/module_write.py",
        r#"from pkg.mod import leaf
def earlier_body():
    return leaf()

leaf = lambda: None

def later_body():
    return leaf()
"#,
    );
    write(
        &root,
        "src/global_write.py",
        r#"from pkg.mod import leaf
def replace_global():
    global leaf
    leaf = lambda: None
    leaf()
"#,
    );
    write(
        &root,
        "src/nonlocal_write.py",
        r#"from pkg.mod import leaf
def outer():
    leaf = lambda: None
    def inner():
        nonlocal leaf
        leaf()
"#,
    );

    let project = analyze(&root);
    let scopes = file(&project, "/scopes.py");
    let mut calls: Vec<_> = refs(scopes)
        .iter()
        .filter(|item| item["path"] == serde_json::json!(["leaf"]))
        .collect();
    calls.sort_by_key(|item| item["start_byte"].as_u64().unwrap());
    assert_eq!(
        calls.len(),
        5,
        "expected two assigned and three comp/method references"
    );
    assert!(
        calls[..3]
            .iter()
            .all(|item| item["resolution"]["status"] != "exact")
    );
    assert!(
        calls[3..]
            .iter()
            .all(|item| item["resolution"]["status"] == "exact"),
        "post-comprehension and bare class references: {:#?}",
        &calls[3..]
    );
    assert!(
        calls[4]["resolution"]["symbols"]["file"]
            .as_str()
            .unwrap()
            .ends_with("/pkg/mod.py")
    );
    assert_eq!(
        reference(scopes, &["self", "leaf"])["resolution"]["status"],
        "unresolved",
        "instance method dispatch needs type information"
    );

    let star = file(&project, "/star.py");
    assert_eq!(
        reference(star, &["leaf"])["resolution"]["status"],
        "unresolved"
    );
    let module_write = file(&project, "/module_write.py");
    assert_eq!(
        reference(module_write, &["leaf"])["resolution"]["status"],
        "unresolved",
        "module assignment after a deferred function blocks exact assumptions"
    );
    assert_eq!(
        refs(module_write)
            .iter()
            .filter(|item| item["path"] == serde_json::json!(["leaf"]))
            .count(),
        2
    );
    assert!(
        refs(module_write)
            .iter()
            .filter(|item| item["path"] == serde_json::json!(["leaf"]))
            .all(|item| item["resolution"]["status"] == "unresolved")
    );
    for suffix in ["/global_write.py", "/nonlocal_write.py"] {
        assert_eq!(
            reference(file(&project, suffix), &["leaf"])["resolution"]["status"],
            "unresolved",
            "rebinding effects are conservative in {suffix}"
        );
    }
}

#[test]
fn python_pattern_and_delete_bindings_do_not_create_false_recursion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("deceptive-project");
    let source = r#"
def before_assignment():
    before_assignment()
    before_assignment = 1

def exception_alias():
    exception_alias()
    try: pass
    except Exception as exception_alias: pass
    exception_alias()

def match_binding(value):
    match_binding(value)
    match value:
        case match_binding: pass
    match_binding(value)

def match_dict_binding(value):
    match_dict_binding(value)
    match value:
        case {"name": match_dict_binding}: pass
    match_dict_binding(value)

def deleting():
    deleting()
    del deleting

def attr():
    factory().attr()

def global_directive():
    global global_directive
    global_directive()

def class_global(): pass
class C:
    class_global = 1
    def method(self):
        class_global()
"#;
    write(&root, "deceptive.py", source);
    let report = analyze(&root);
    let deceptive = file(&report, "/deceptive.py");
    for name in [
        "before_assignment",
        "exception_alias",
        "match_binding",
        "match_dict_binding",
        "deleting",
        "attr",
        "global_directive",
    ] {
        assert!(!recursive(deceptive, name), "false recursion for {name}");
        assert!(
            refs(deceptive)
                .iter()
                .filter(|item| item["path"][0] == name)
                .all(|item| item["resolution"]["status"] != "exact")
        );
    }
    exact_target(
        deceptive,
        &["class_global"],
        "/deceptive.py",
        "class_global",
    );
    assert!(!recursive(deceptive, "method"));
}
