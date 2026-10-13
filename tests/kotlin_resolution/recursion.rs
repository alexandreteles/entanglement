use super::*;

fn recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

fn named_functions<'a>(file: &'a Value, name: &str) -> Vec<&'a Value> {
    file["functions"]
        .as_array()
        .expect("function reports")
        .iter()
        .filter(|item| item["name"] == name)
        .collect()
}

#[test]
fn calls_in_default_arguments_do_not_make_the_callable_body_recursive() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = "package cycle\nfun defaulted(value: Int = defaulted(0)): Int = value\n";
    write(&root.join("src/main/kotlin/cycle/Defaults.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Defaults.kt");
    let defaulted = function(file, "defaulted");
    assert!(
        !recursive(defaulted),
        "a reference evaluated as an argument default is outside the callable body"
    );
    exact_target(
        ref_at(file, source, "defaulted(0)", 0),
        "/Defaults.kt",
        "defaulted",
    );
}

#[test]
fn immutable_literal_lambda_invocation_can_form_a_cycle_on_its_reported_scope() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = "package cycle\nval callback: () -> Unit = { callback() }\n";
    write(&root.join("src/main/kotlin/cycle/Lambda.kt"), source);

    let project = report(&run(&root, "repo", None));
    let callback = function(file(&project, "/Lambda.kt"), "callback");
    assert!(
        recursive(callback),
        "a direct immutable lambda initializer is an executable recursion target"
    );
    let invocation = ref_at(file(&project, "/Lambda.kt"), source, "callback()", 0);
    exact_target(invocation, "/Lambda.kt", "callback");
}

#[test]
fn property_getter_reads_form_mutual_recursion_on_the_getter_scopes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package cycle\n",
        "val first: Int get() = second\n",
        "val second: Int get() = first\n",
    );
    write(&root.join("src/main/kotlin/cycle/Properties.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Properties.kt");
    let getters = named_functions(file, "get");
    assert_eq!(getters.len(), 2, "one reported getter scope per property");
    assert!(
        getters.iter().all(|getter| recursive(getter)),
        "each getter participates in the property read cycle: {getters:#?}"
    );
    exact_target(
        ref_at(file, source, "second\n", 0),
        "/Properties.kt",
        "second",
    );
    exact_target(
        ref_at(file, source, "first\n", 0),
        "/Properties.kt",
        "first",
    );
}

#[test]
fn constructor_and_init_cycles_use_only_reported_executable_ranges() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package cycle\n",
        "class InitCycle() {\n",
        "    init { InitCycle() }\n",
        "}\n",
        "class ImplicitInitCycle {\n",
        "    init { ImplicitInitCycle() }\n",
        "}\n",
        "open class OpenInitCycle() {\n",
        "    init { OpenInitCycle() }\n",
        "}\n",
        "class SecondaryCycle {\n",
        "    constructor() { SecondaryCycle(true) }\n",
        "    constructor(flag: Boolean) { if (flag) SecondaryCycle() }\n",
        "}\n",
    );
    write(&root.join("src/main/kotlin/cycle/Constructors.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Constructors.kt");
    let init_call = ref_at(file, source, "InitCycle()", 1);
    assert_eq!(init_call["resolution"]["status"], "exact", "{init_call:#}");
    let implicit_init_call = ref_at(file, source, "ImplicitInitCycle()", 0);
    assert!(
        implicit_init_call["resolution"]["status"] == "exact",
        "{implicit_init_call:#}"
    );
    let open_init_call = ref_at(file, source, "OpenInitCycle()", 1);
    exact_target(open_init_call, "/Constructors.kt", "OpenInitCycle");
    let init_scopes = named_functions(file, "init");
    assert_eq!(init_scopes.len(), 3);
    assert!(
        init_scopes.iter().all(|init| recursive(init)),
        "explicit-primary, implicit-primary, and open-class construction execute their recursive init blocks: {init_scopes:#?}"
    );
    let primary = function(file, "InitCycle");
    assert!(
        recursive(primary),
        "constructing InitCycle executes the recursive init block"
    );
    let open_primary = function(file, "OpenInitCycle");
    assert!(
        recursive(open_primary),
        "an open class constructor must retain its direct initializer edge"
    );

    let constructors = named_functions(file, "SecondaryCycle.constructor");
    assert_eq!(
        constructors.len(),
        2,
        "both secondary constructors need scopes"
    );
    assert!(
        constructors
            .iter()
            .all(|constructor| recursive(constructor)),
        "the opposite constructor calls form a two-scope cycle: {constructors:#?}"
    );
    let no_arg = ref_at(file, source, "SecondaryCycle()", 0);
    let bool_arg = ref_at(file, source, "SecondaryCycle(true)", 0);
    assert_eq!(no_arg["resolution"]["status"], "exact", "{no_arg:#}");
    assert_eq!(bool_arg["resolution"]["status"], "exact", "{bool_arg:#}");
}

#[test]
fn operator_and_infix_convention_calls_can_form_direct_recursion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package cycle\n",
        "class Convention {\n",
        "    operator fun invoke() { this() }\n",
        "    operator fun get(index: Int): Int = this[index]\n",
        "    operator fun plus(other: Convention): Convention = this + other\n",
        "    infix fun combine(other: Convention): Convention = this combine other\n",
        "}\n",
    );
    write(&root.join("src/main/kotlin/cycle/Convention.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Convention.kt");
    for name in ["invoke", "get", "plus", "combine"] {
        assert!(
            recursive(function(file, name)),
            "the convention call in {name} directly targets its own executable scope"
        );
    }
}

#[test]
fn callable_references_are_nonrecursive_until_the_reference_is_invoked() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package cycle\n",
        "fun stored() { val reference = ::stored }\n",
        "fun invoked() { val reference = ::invoked; reference() }\n",
    );
    write(&root.join("src/main/kotlin/cycle/References.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/References.kt");
    exact_target(
        ref_at(file, source, "reference()", 0),
        "/References.kt",
        "invoked",
    );
    assert!(
        !recursive(function(file, "stored")),
        "creating a callable reference alone does not execute its target"
    );
    assert!(
        recursive(function(file, "invoked")),
        "invoking an immutable local reference back to the owner forms recursion"
    );
}

#[test]
fn getter_references_and_property_writes_do_not_create_getter_edges() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package cycle\n",
        "fun holdTarget(): Int { val getter = ::observed; return 1 }\n",
        "val observed: Int get() = holdTarget()\n",
        "var writable: Int = 0\n",
        "    get() = 1\n",
        "    set(value) { writable /* assign through the setter */ = value }\n",
        "fun assign() { writable = 2 }\n",
    );
    write(&root.join("src/main/kotlin/cycle/Accessors.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Accessors.kt");
    let getters = named_functions(file, "get");
    let setters = named_functions(file, "set");
    assert_eq!(getters.len(), 2);
    assert_eq!(setters.len(), 1);
    exact_target(
        ref_at(
            file,
            source,
            "writable /* assign through the setter */ = value",
            0,
        ),
        "/Accessors.kt",
        "writable",
    );
    assert!(
        getters.iter().all(|getter| !recursive(getter)),
        "a callable reference and a property write must not create getter recursion: {getters:#?}"
    );
    assert!(
        recursive(setters[0]),
        "assignment to a property from its setter invokes the setter, not its getter"
    );
    assert!(
        !recursive(function(file, "holdTarget")),
        "storing the callable reference does not invoke the getter"
    );
    assert!(!recursive(function(file, "assign")));
}

#[test]
fn qualified_property_navigation_keeps_read_write_and_readwrite_references() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let read_source = concat!(
        "package access\n",
        "var sink = 0\n",
        "class ReadBox {\n",
        "    var value: Int = 0\n",
        "        get() = readBridge(this)\n",
        "        set(input) {}\n",
        "}\n",
        "fun readBridge(box: ReadBox): Int { sink = box.value; return 0 }\n",
        "fun readOnly(box: ReadBox) { sink = box.value }\n",
    );
    let write_source = concat!(
        "package access\n",
        "class WriteBox {\n",
        "    var value: Int = 0\n",
        "        get() = writeBridge(this, 0)\n",
        "        set(input) { writeBridge(this, input) }\n",
        "}\n",
        "fun writeBridge(box: WriteBox, input: Int): Int { box.value = input; return 0 }\n",
        "fun writeOnly(box: WriteBox, input: Int) { box.value = input }\n",
    );
    let update_source = concat!(
        "package access\n",
        "class UpdateBox {\n",
        "    var value: Int = 0\n",
        "        get() = updateBridge(this)\n",
        "        set(input) { updateBridge(this) }\n",
        "}\n",
        "fun updateBridge(box: UpdateBox): Int { box.value++; box.value--; return 0 }\n",
        "fun updateOnly(box: UpdateBox) { box.value++ }\n",
    );
    write(&root.join("src/main/kotlin/access/Read.kt"), read_source);
    write(&root.join("src/main/kotlin/access/Write.kt"), write_source);
    write(
        &root.join("src/main/kotlin/access/Update.kt"),
        update_source,
    );

    let project = report(&run(&root, "repo", None));
    let read_file = file(&project, "/Read.kt");
    for occurrence in [1, 2] {
        let reference = ref_at(read_file, read_source, "value", occurrence);
        exact_target(reference, "/Read.kt", "value");
        assert_eq!(
            reference["path"],
            serde_json::json!(["box", "value"]),
            "a qualified RHS getter read must retain its receiver: {reference:#}"
        );
    }
    assert!(recursive(function(read_file, "get")));
    assert!(recursive(function(read_file, "readBridge")));
    assert!(!recursive(function(read_file, "set")));
    assert!(!recursive(function(read_file, "readOnly")));

    let write_file = file(&project, "/Write.kt");
    for occurrence in [1, 2] {
        let reference = ref_at(write_file, write_source, "value", occurrence);
        exact_target(reference, "/Write.kt", "value");
        assert_eq!(
            reference["path"],
            serde_json::json!(["box", "value"]),
            "a qualified assignment target must retain its receiver: {reference:#}"
        );
    }
    assert!(recursive(function(write_file, "set")));
    assert!(recursive(function(write_file, "writeBridge")));
    assert!(
        !recursive(function(write_file, "get")),
        "a property write must invoke its setter without creating a getter edge"
    );
    assert!(!recursive(function(write_file, "writeOnly")));

    let update_file = file(&project, "/Update.kt");
    for occurrence in [1, 2, 3] {
        let reference = ref_at(update_file, update_source, "value", occurrence);
        exact_target(reference, "/Update.kt", "value");
        assert_eq!(
            reference["path"],
            serde_json::json!(["box", "value"]),
            "a qualified increment/decrement target must retain its receiver: {reference:#}"
        );
    }
    assert!(
        recursive(function(update_file, "get"))
            && recursive(function(update_file, "set"))
            && recursive(function(update_file, "updateBridge")),
        "postfix read-write access must reach both getter and setter execution scopes"
    );
    assert!(!recursive(function(update_file, "updateOnly")));
}
