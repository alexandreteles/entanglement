use super::*;

#[test]
fn interface_virtual_self_calls_do_not_form_direct_recursion_but_super_is_exact() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package dispatch\n",
        "interface Port {\n",
        "  fun ping(): Int = this.ping()\n",
        "  val state: Int get() = this.state\n",
        "  val first: Int get() = this.second\n",
        "  val second: Int get() = this.first\n",
        "}\n",
        "interface CompactInterface{fun host(){fun local(){local()};local()}}\n",
        "open class Base { open fun compute(): Int = 1 }\n",
        "class Derived : Base() { override fun compute(): Int = super.compute() }\n",
        "fun inspect(port: Port) { port.ping(); val current = port.state }\n",
    );
    write(&root.join("src/main/kotlin/Dispatch.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Dispatch.kt");
    let ping = function(file, "ping");
    assert!(
        !ping["cognitive_contributions"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion")),
        "an interface default method dispatches virtually to `this`, so self-calls are not direct recursion"
    );
    exact_target(ref_at(file, source, "ping()", 1), "/Dispatch.kt", "ping");
    let getters: Vec<_> = file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["name"] == "get")
        .collect();
    assert_eq!(getters.len(), 3);
    assert!(
        getters.iter().all(|getter| {
            !getter["cognitive_contributions"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
        }),
        "interface property reads dispatch through virtual getters, not a direct getter SCC"
    );
    let local = function(file, "local");
    assert!(
        local["cognitive_contributions"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion")),
        "a local executable inside a compact interface body still dispatches directly to itself: {local:#}"
    );
    exact_target(ref_at(file, source, "local()}", 0), "/Dispatch.kt", "local");
    exact_target(ref_at(file, source, "second", 0), "/Dispatch.kt", "second");
    exact_target(ref_at(file, source, "state", 1), "/Dispatch.kt", "state");
    exact_target(ref_at(file, source, "first", 1), "/Dispatch.kt", "first");
    let super_call = ref_at(file, source, "compute()", 2);
    exact_target(super_call, "/Dispatch.kt", "compute");
    assert_eq!(
        super_call["resolution"]["symbols"]["start_byte"],
        source.find("open fun compute").unwrap(),
        "super dispatch selects the base implementation"
    );
}

#[test]
fn nested_class_supertypes_do_not_leak_to_the_enclosing_class() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package nesting\n",
        "open class Base { fun work() = 1 }\n",
        "class Outer { class Inner : Base() }\n",
        "fun Outer.work() = 2\n",
        "fun use(value: Outer) = value.work()\n",
    );
    write(&root.join("src/main/kotlin/Nested.kt"), source);

    let project = report(&run(&root, "repo", None));
    let reference = ref_at(file(&project, "/Nested.kt"), source, "work()", 2);
    exact_target(reference, "/Nested.kt", "work");
    assert_eq!(
        reference["resolution"]["symbols"]["start_byte"],
        source.find("fun Outer.work").unwrap(),
        "Outer must not inherit Base through its nested Inner declaration"
    );
}
