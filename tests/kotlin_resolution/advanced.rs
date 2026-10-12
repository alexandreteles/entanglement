use super::*;

#[test]
fn independent_gradle_roots_keep_same_package_candidates_separate() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let first = workspace.join("first");
    let second = workspace.join("second");
    let first_client = "package client\nimport shared.entry\nfun call() = entry()\n";
    let second_client = "package client\nimport shared.entry\nfun call() = entry()\n";

    write(
        &first.join("settings.gradle.kts"),
        "rootProject.name = \"first\"\n",
    );
    write(
        &second.join("settings.gradle.kts"),
        "rootProject.name = \"second\"\n",
    );
    write(
        &first.join("src/main/kotlin/shared/Entry.kt"),
        "package shared\ninternal fun entry() = 1\n",
    );
    write(
        &second.join("src/main/kotlin/shared/Entry.kt"),
        "package shared\nfun entry() = 2\n",
    );
    write(
        &first.join("src/main/kotlin/client/Client.kt"),
        first_client,
    );
    write(
        &second.join("src/main/kotlin/client/Client.kt"),
        second_client,
    );

    let project = report(&run(&workspace, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/first/src/main/kotlin/client/Client.kt"),
            first_client,
            "entry()",
            0,
        ),
        "/first/src/main/kotlin/shared/Entry.kt",
        "entry",
    );
    exact_target(
        ref_at(
            file(&project, "/second/src/main/kotlin/client/Client.kt"),
            second_client,
            "entry()",
            0,
        ),
        "/second/src/main/kotlin/shared/Entry.kt",
        "entry",
    );
}

#[test]
fn comments_and_strings_do_not_create_gradle_or_source_set_edges() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let app_build = concat!(
        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
        "// dependencies { implementation(project(\":lib\")) }\n",
        "val decoy = \"implementation(project(\\\":lib\\\"))\"\n",
    );
    let client = "package client\nimport library.sharedEntry\nfun call() = sharedEntry()\n";
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\ninclude(\":app\", \":lib\")\n",
    );
    write(&root.join("app/build.gradle.kts"), app_build);
    write(
        &root.join("lib/build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
    );
    write(
        &root.join("lib/src/main/kotlin/library/Api.kt"),
        "package library\nfun sharedEntry() = 1\n",
    );
    write(&root.join("app/src/main/kotlin/client/Client.kt"), client);

    let project = report(&run(&root, "repo", None));
    assert_ne!(
        ref_at(
            file(&project, "/app/src/main/kotlin/client/Client.kt"),
            client,
            "sharedEntry()",
            0,
        )["resolution"]["status"],
        "exact",
        "a quoted or commented dependency must not expose another Gradle module"
    );

    let kmp_root = temp.path().join("kmp");
    let build = concat!(
        "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
        "kotlin { sourceSets {\n",
        "  val commonMain by getting\n",
        "  val intermediateMain by creating\n",
        "  val androidMain by getting\n",
        "  // val androidMain by getting { dependsOn(intermediateMain) }\n",
        "  val decoy = \"dependsOn(intermediateMain)\"\n",
        "} }\n",
    );
    let intermediate = "package shared\nfun intermediateEntry() = 1\n";
    let android =
        "package client\nimport shared.intermediateEntry\nfun call() = intermediateEntry()\n";
    write(
        &kmp_root.join("settings.gradle.kts"),
        "rootProject.name = \"kmp\"\n",
    );
    write(&kmp_root.join("build.gradle.kts"), build);
    write(
        &kmp_root.join("src/intermediateMain/kotlin/shared/Intermediate.kt"),
        intermediate,
    );
    write(
        &kmp_root.join("src/androidMain/kotlin/client/Client.kt"),
        android,
    );
    let kmp = report(&run(&kmp_root, "repo", None));
    assert_ne!(
        ref_at(
            file(&kmp, "/androidMain/kotlin/client/Client.kt"),
            android,
            "intermediateEntry()",
            0,
        )["resolution"]["status"],
        "exact",
        "a quoted or commented dependsOn must not connect source sets"
    );
}

#[test]
fn local_qualified_prefix_shadows_a_package_and_imported_top_level_name() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let client = concat!(
        "package client\n",
        "import api.item\n",
        "fun call() {\n",
        "  val api = object {}\n",
        "  api.item()\n",
        "}\n",
    );
    write(
        &root.join("src/main/kotlin/api/Api.kt"),
        "package api\nfun item() = 1\n",
    );
    write(&root.join("src/main/kotlin/client/Client.kt"), client);

    let project = report(&run(&root, "repo", None));
    let reference = ref_at(file(&project, "/client/Client.kt"), client, "item()", 0);
    assert_ne!(
        reference["resolution"]["status"], "exact",
        "the local receiver `api` must prevent treating `api.item` as a package path"
    );
}

#[test]
fn typed_receiver_identity_prefers_members_and_imported_extensions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let left_client = concat!(
        "package client\n",
        "import left.Model\n",
        "import left.render\n",
        "fun memberWins(value: Model) = value.render()\n",
        "fun hiddenExtension(value: Model) = value.extra()\n",
    );
    let right_client = concat!(
        "package client\n",
        "import right.Model\n",
        "import right.render\n",
        "fun rightReceiver(value: Model) = value.render()\n",
    );
    write(
        &root.join("src/main/kotlin/left/Model.kt"),
        "package left\nclass Model { fun render() = 1 }\n",
    );
    write(
        &root.join("src/main/kotlin/left/Extensions.kt"),
        "package left\nfun Model.render() = 2\nfun Model.extra() = 3\n",
    );
    write(
        &root.join("src/main/kotlin/right/Model.kt"),
        "package right\nclass Model\n",
    );
    write(
        &root.join("src/main/kotlin/right/Extensions.kt"),
        "package right\nfun Model.render() = 4\n",
    );
    write(&root.join("src/main/kotlin/client/Left.kt"), left_client);
    write(&root.join("src/main/kotlin/client/Right.kt"), right_client);

    let project = report(&run(&root, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/client/Left.kt"),
            left_client,
            "render()",
            0,
        ),
        "/left/Model.kt",
        "render",
    );
    assert_ne!(
        ref_at(file(&project, "/client/Left.kt"), left_client, "extra()", 0,)["resolution"]["status"],
        "exact",
        "extensions without an import must not be considered"
    );
    exact_target(
        ref_at(
            file(&project, "/client/Right.kt"),
            right_client,
            "render()",
            0,
        ),
        "/right/Extensions.kt",
        "render",
    );
}

#[test]
fn instance_extension_and_class_qualified_companion_calls_select_distinct_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package precedence\n",
        "class C { companion object { fun work() = 1 } }\n",
        "fun C.work() = 2\n",
        "fun use(value: C) = value.work()\n",
        "fun classUse() = C.work()\n",
    );
    write(&root.join("src/main/kotlin/Selection.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Selection.kt");
    let companion_start = source.find("fun work() = 1").unwrap();
    let extension_start = source.find("fun C.work()").unwrap();
    let instance_call = ref_at(file, source, "work()", 2);
    exact_target(instance_call, "/Selection.kt", "work");
    assert_eq!(
        instance_call["resolution"]["symbols"]["start_byte"], extension_start,
        "instance syntax must select the extension, not a companion member"
    );
    let class_call = ref_at(file, source, "work()", 3);
    exact_target(class_call, "/Selection.kt", "work");
    assert_eq!(
        class_call["resolution"]["symbols"]["start_byte"], companion_start,
        "class-qualified syntax must select the companion member"
    );
}

#[test]
fn inherited_any_overload_never_beats_a_more_specific_interface_string_overload() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package precedence\n",
        "interface Textual { fun work(value: String) = 1 }\n",
        "open class General { fun work(value: Any) = 2 }\n",
        "class Combined : General(), Textual\n",
        "fun use(value: Combined) = value.work(\"hello\")\n",
    );
    write(&root.join("src/main/kotlin/Inherited.kt"), source);

    let project = report(&run(&root, "repo", None));
    let reference = ref_at(
        file(&project, "/Inherited.kt"),
        source,
        "work(\"hello\")",
        0,
    );
    match reference["resolution"]["status"].as_str() {
        Some("exact") => assert_eq!(
            reference["resolution"]["symbols"]["start_byte"],
            source.find("fun work(value: String)").unwrap(),
            "the literal String argument must never resolve to inherited work(Any)"
        ),
        Some("ambiguous") => {
            let candidates = reference["resolution"]["symbols"]
                .as_array()
                .expect("ambiguous candidate list");
            assert!(
                candidates.iter().any(|candidate| {
                    candidate["start_byte"] == source.find("fun work(value: String)").unwrap()
                }) && candidates.iter().any(|candidate| {
                    candidate["start_byte"] == source.find("fun work(value: Any)").unwrap()
                }),
                "conservative ambiguity should retain both applicable inherited candidates: {reference:#}"
            );
        }
        status => panic!(
            "expected exact String target or conservative ambiguity, got {status:?}: {reference:#}"
        ),
    }
}

#[test]
fn generic_receiver_lambdas_resolve_nested_members_and_keep_plain_lambdas_lexical() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let client = concat!(
        "package client\n",
        "import api.Builder\n",
        "import api.build\n",
        "import api.consume\n",
        "import api.lexicalHelper\n",
        "import api.viaAlias\n",
        "fun use() {\n",
        "  build { child() }\n",
        "  viaAlias { child() }\n",
        "  consume { lexicalHelper() }\n",
        "}\n",
    );
    write(
        &root.join("src/main/kotlin/api/Builder.kt"),
        concat!(
            "package api\n",
            "class Builder { fun child() = 1 }\n",
            "typealias BuilderAction = Builder.() -> Unit\n",
            "fun build(block: Builder.() -> Unit) {}\n",
            "fun viaAlias(block: BuilderAction) {}\n",
            "fun consume(block: () -> Unit) {}\n",
            "fun lexicalHelper() = 1\n",
        ),
    );
    write(&root.join("src/main/kotlin/client/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/client/Use.kt");
    let cases = [
        ("child() }", 0, "child"),
        ("child() }", 1, "child"),
        ("lexicalHelper()", 0, "lexicalHelper"),
    ];
    let failures = cases
        .into_iter()
        .filter_map(|(needle, occurrence, expected_name)| {
            let reference = ref_at(use_file, client, needle, occurrence);
            let target = &reference["resolution"]["symbols"];
            (reference["resolution"]["status"] != "exact"
                || target["name"] != expected_name
                || !target["file"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("/api/Builder.kt")))
            .then(|| format!("{needle} occurrence {occurrence}: {reference:#}"))
        })
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "receiver or lexical resolution failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn generic_calls_substitute_type_parameters_from_arguments_and_lambda_signatures() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let api = concat!(
        "package api\n",
        "fun <T> identity(value: T): T = value\n",
        "class Box<T>(val value: T)\n",
        "class Builder { fun child() = 1 }\n",
        "fun analyzedBuilder(): Builder = Builder()\n",
        "fun <T> withValue(value: T, block: T.() -> Unit) {}\n",
        "fun <T> callback(value: T, block: (T) -> Unit) {}\n",
        "fun <T : Number> bounded(value: T) = value\n",
        "fun <T : CharSequence> bounded(value: T) = value\n",
        "fun <T> boundedWhere(value: T): T where T : Number = value\n",
    );
    let client = concat!(
        "package client\n",
        "import api.Box\n",
        "import api.Builder\n",
        "import api.analyzedBuilder\n",
        "import api.bounded\n",
        "import api.boundedWhere\n",
        "import api.callback\n",
        "import api.identity\n",
        "import api.withValue\n",
        "fun use() {\n",
        "  identity(1)\n",
        "  Box(1)\n",
        "  withValue(analyzedBuilder()) { child() }\n",
        "  callback(analyzedBuilder()) { builder -> builder.child() }\n",
        "  callback(analyzedBuilder()) { it.child() }\n",
        "  val invalidBound = bounded(true)\n",
        "  val invalidWhereBound = boundedWhere(true)\n",
        "}\n",
    );
    write(&root.join("src/main/kotlin/api/Generic.kt"), api);
    write(&root.join("src/main/kotlin/client/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/client/Use.kt");
    let mut failures = Vec::new();
    for needle in ["bounded(true)", "boundedWhere(true)"] {
        let bounded = ref_at(use_file, client, needle, 0);
        if bounded["resolution"]["status"] == "exact" {
            failures.push(format!(
                "{needle} violates its declared upper bounds: {bounded:#}"
            ));
        }
    }
    let exact_cases = [
        ("identity(1)", 0, "identity"),
        ("Box(1)", 0, "Box"),
        ("withValue(", 0, "withValue"),
        ("callback(", 0, "callback"),
        ("callback(", 1, "callback"),
        ("child()", 0, "child"),
        ("child()", 1, "child"),
        ("child()", 2, "child"),
    ];
    failures.extend(
        exact_cases
            .into_iter()
            .filter_map(|(needle, occurrence, expected_name)| {
                let reference = ref_at(use_file, client, needle, occurrence);
                let target = &reference["resolution"]["symbols"];
                (reference["resolution"]["status"] != "exact"
                    || target["name"] != expected_name
                    || !target["file"]
                        .as_str()
                        .is_some_and(|path| path.ends_with("/api/Generic.kt")))
                .then(|| format!("{needle} occurrence {occurrence}: {reference:#}"))
            }),
    );
    assert!(
        failures.is_empty(),
        "generic signature/callback resolution failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn receiver_lambda_parameter_mapping_respects_position_names_and_trailing_syntax() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let api = concat!(
        "package api\n",
        "class A { fun work() = 1 }\n",
        "class B { fun work() = 2 }\n",
        "fun both(first: A.() -> Unit, second: B.() -> Unit) {}\n",
    );
    let client = concat!(
        "package client\n",
        "import api.A\n",
        "import api.B\n",
        "import api.both\n",
        "fun use() {\n",
        "  both({ work() }, { work() })\n",
        "  both(second = { work() }, first = { work() })\n",
        "  both({ work() }) { work() }\n",
        "}\n",
    );
    write(&root.join("src/main/kotlin/api/ReceiverBlocks.kt"), api);
    write(&root.join("src/main/kotlin/client/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/client/Use.kt");
    let a_work = api.find("fun work").unwrap();
    let b_work = api.match_indices("fun work").nth(1).unwrap().0;
    for (occurrence, expected_start) in [
        (0, a_work),
        (1, b_work),
        (2, b_work),
        (3, a_work),
        (4, a_work),
        (5, b_work),
    ] {
        let reference = ref_at(use_file, client, "work()", occurrence);
        exact_target(reference, "/api/ReceiverBlocks.kt", "work");
        assert_eq!(
            reference["resolution"]["symbols"]["start_byte"], expected_start,
            "receiver lambda occurrence {occurrence} must use its declared parameter type"
        );
    }
}

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

#[test]
fn numeric_literal_suffixes_select_only_their_matching_overloads() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package literals\n",
        "fun real(value: Float) = 1\n",
        "fun real(value: Double) = 2\n",
        "fun signed(value: Int) = 1\n",
        "fun signed(value: Long) = 2\n",
        "fun unsigned(value: UInt) = 1\n",
        "fun unsigned(value: ULong) = 2\n",
        "fun use() {\n",
        "  real(1.0f)\n",
        "  real(1.0F)\n",
        "  real(1.0)\n",
        "  signed(1L)\n",
        "  signed(1)\n",
        "  unsigned(1u)\n",
        "  unsigned(1uL)\n",
        "}\n",
    );
    write(&root.join("src/main/kotlin/Numeric.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Numeric.kt");
    let float_target = source.find("fun real(value: Float)").unwrap();
    let double_target = source.find("fun real(value: Double)").unwrap();
    let long_target = source.find("fun signed(value: Long)").unwrap();
    let int_target = source.find("fun signed(value: Int)").unwrap();
    let ulong_target = source.find("fun unsigned(value: ULong)").unwrap();
    let uint_target = source.find("fun unsigned(value: UInt)").unwrap();
    for (needle, occurrence, name, target) in [
        ("real(1.0f)", 0, "real", float_target),
        ("real(1.0F)", 0, "real", float_target),
        ("real(1.0)", 0, "real", double_target),
        ("signed(1L)", 0, "signed", long_target),
        ("signed(1)", 0, "signed", int_target),
        ("unsigned(1u)", 0, "unsigned", uint_target),
        ("unsigned(1uL)", 0, "unsigned", ulong_target),
    ] {
        let reference = ref_at(file, source, needle, occurrence);
        exact_target(reference, "/Numeric.kt", name);
        assert_eq!(
            reference["resolution"]["symbols"]["start_byte"], target,
            "literal {needle} must select its statically compatible overload"
        );
    }
}

#[test]
fn child_argument_never_selects_an_any_overload_when_base_is_more_specific() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package overloads\n",
        "open class Base\n",
        "class Child : Base()\n",
        "fun pick(value: Base) = 1\n",
        "fun pick(value: Any) = 2\n",
        "fun use(child: Child) = pick(child)\n",
    );
    write(&root.join("src/main/kotlin/Overloads.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Overloads.kt");
    let reference = ref_at(file, source, "pick(child)", 0);
    if reference["resolution"]["status"] == "exact" {
        assert_eq!(
            reference["resolution"]["symbols"]["start_byte"],
            source.find("fun pick(value: Base)").unwrap(),
            "the derived Child argument is assignable to Base and should never choose Any"
        );
    }
}

#[test]
fn dsl_markers_hide_outer_implicit_receivers_but_explicit_labels_can_select_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let client = concat!(
        "package client\n",
        "import api.outerDsl\n",
        "fun use() {\n",
        "  outerDsl outer@ {\n",
        "    inner {\n",
        "      hiddenOuter()\n",
        "      this@outer.hiddenOuter()\n",
        "      innerOnly()\n",
        "    }\n",
        "  }\n",
        "  outerDsl customOuter@ {\n",
        "    inner { this@customOuter.hiddenOuter() }\n",
        "  }\n",
        "}\n",
    );
    write(
        &root.join("src/main/kotlin/api/Markup.kt"),
        concat!(
            "package api\n",
            "@DslMarker annotation class Markup\n",
            "@Markup class Outer {\n",
            "  fun inner(block: Inner.() -> Unit) {}\n",
            "  fun hiddenOuter() = 1\n",
            "}\n",
            "@Markup class Inner{fun innerOnly()=1}\n",
            "fun outerDsl(block: Outer.() -> Unit) {}\n",
        ),
    );
    write(&root.join("src/main/kotlin/client/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/client/Use.kt");
    assert_ne!(
        ref_at(use_file, client, "hiddenOuter()", 0)["resolution"]["status"],
        "exact",
        "the @DslMarker on Inner must hide Outer from implicit receiver lookup"
    );
    exact_target(
        ref_at(use_file, client, "hiddenOuter()", 1),
        "/api/Markup.kt",
        "hiddenOuter",
    );
    exact_target(
        ref_at(use_file, client, "hiddenOuter()", 2),
        "/api/Markup.kt",
        "hiddenOuter",
    );
    exact_target(
        ref_at(use_file, client, "innerOnly()", 0),
        "/api/Markup.kt",
        "innerOnly",
    );
}

#[test]
fn nested_lambda_it_inference_selects_members_of_the_inner_parameter_type() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let client = concat!(
        "package client\n",
        "import api.InnerToken\n",
        "import api.OuterToken\n",
        "import api.consumeInner\n",
        "import api.consumeOuter\n",
        "fun use() {\n",
        "  consumeOuter { outer ->\n",
        "    consumeInner { it.innerCall() }\n",
        "    outer.outerCall()\n",
        "  }\n",
        "}\n",
    );
    write(
        &root.join("src/main/kotlin/api/Tokens.kt"),
        concat!(
            "package api\n",
            "class OuterToken { fun outerCall() = 1 }\n",
            "class InnerToken { fun innerCall() = 2 }\n",
            "fun consumeOuter(block: (OuterToken) -> Unit) {}\n",
            "fun consumeInner(block: (InnerToken) -> Unit) {}\n",
        ),
    );
    write(&root.join("src/main/kotlin/client/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/client/Use.kt");
    exact_target(
        ref_at(use_file, client, "innerCall()", 0),
        "/api/Tokens.kt",
        "innerCall",
    );
    exact_target(
        ref_at(use_file, client, "outerCall()", 0),
        "/api/Tokens.kt",
        "outerCall",
    );
}

#[test]
fn extension_properties_and_named_context_parameters_keep_distinct_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let client = concat!(
        "package client\n",
        "import api.Logger\n",
        "import api.code\n",
        "import api.work\n",
        "context(logger: Logger)\n",
        "fun use(text: String) {\n",
        "  logger.work()\n",
        "  work()\n",
        "  val size = text.code\n",
        "}\n",
    );
    let api = "package api\nclass Logger { fun work() = 1 }\nfun work() = 2\n";
    write(&root.join("src/main/kotlin/api/Logger.kt"), api);
    write(
        &root.join("src/main/kotlin/api/Extensions.kt"),
        "package api\nval String.code: Int get() = length\n",
    );
    write(&root.join("src/main/kotlin/client/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/client/Use.kt");
    let member = ref_at(use_file, client, "work()", 0);
    exact_target(member, "/api/Logger.kt", "work");
    assert_eq!(
        member["resolution"]["symbols"]["start_byte"],
        api.find("fun work").unwrap()
    );
    let top_level = ref_at(use_file, client, "work()", 1);
    exact_target(top_level, "/api/Logger.kt", "work");
    assert_eq!(
        top_level["resolution"]["symbols"]["start_byte"],
        api.match_indices("fun work").nth(1).unwrap().0
    );
    exact_target(
        ref_at(use_file, client, "code", 1),
        "/api/Extensions.kt",
        "code",
    );
}

#[test]
fn operator_and_infix_syntax_requires_matching_local_modifiers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package conventions\n",
        "class Addable { operator fun plus(other: Addable) = this }\n",
        "class NotAddable { fun plus(other: NotAddable) = this }\n",
        "class Ordered { operator fun compareTo(other: Ordered): Int = if (this < other) -1 else 0 }\n",
        "class Mergeable { infix fun merge(other: Mergeable) = this }\n",
        "class NotInfix { fun merge(other: NotInfix) = this }\n",
        "fun use(a: Addable, n: NotAddable, ordered: Ordered, m: Mergeable, x: NotInfix, i: Int, unknown: Any) {\n",
        "  val operatorMatch = a + a\n",
        "  val missingOperator = n + n\n",
        "  val orderedMatch = ordered < ordered\n",
        "  val identity = a === a\n",
        "  val infixMatch = m merge m\n",
        "  val missingInfix = x merge x\n",
        "  val stdlibOperator = i + 1\n",
        "  val unknownOperator = unknown + unknown\n",
        "}\n",
    );
    write(
        &root.join("src/main/kotlin/conventions/Operators.kt"),
        source,
    );

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Operators.kt");
    let addable = ref_at(file, source, "+ a", 0);
    exact_target(addable, "/Operators.kt", "plus");
    assert_eq!(
        addable["resolution"]["symbols"]["start_byte"],
        source.find("operator fun plus").unwrap()
    );
    let ordered = ref_at(file, source, "< other", 0);
    exact_target(ordered, "/Operators.kt", "compareTo");
    assert_eq!(
        ordered["resolution"]["symbols"]["start_byte"],
        source.find("operator fun compareTo").unwrap()
    );
    let compare_to = function(file, "compareTo");
    assert!(
        compare_to["cognitive_contributions"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion")),
        "relational syntax in compareTo must produce the direct recursive edge"
    );
    let identity_operator = source.find("===").unwrap();
    assert!(
        !file["resolution"].as_array().unwrap().iter().any(|item| {
            item["start_byte"] == identity_operator && item["resolution"]["status"] == "exact"
        }),
        "identity equality has no source-level convention target"
    );
    let mergeable = ref_at(file, source, "merge m", 0);
    exact_target(mergeable, "/Operators.kt", "merge");
    assert_eq!(
        mergeable["resolution"]["symbols"]["start_byte"],
        source.find("infix fun merge").unwrap()
    );
    for (needle, message) in [
        (
            "+ n",
            "a plain fun plus is not eligible for operator syntax",
        ),
        (
            "merge x",
            "a plain fun merge is not eligible for infix syntax",
        ),
        (
            "+ 1",
            "stdlib operators without local source declarations stay non-exact",
        ),
        (
            "+ unknown",
            "an unknown receiver cannot prove an operator target",
        ),
    ] {
        assert_ne!(
            ref_at(file, source, needle, 0)["resolution"]["status"],
            "exact",
            "{message}"
        );
    }
}

#[test]
fn local_callable_alias_companion_and_super_calls_preserve_static_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package sample\n",
        "fun target() = 1\n",
        "open class Base { fun inherited() = 2 }\n",
        "class Derived : Base() { fun viaSuper() = super.inherited() }\n",
        "class Registry{companion object{fun create()=Registry()}}\n",
        "fun viaCompanion() = Registry.create()\n",
        "fun viaAlias() { val alias = ::target; alias() }\n",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\n",
    );
    write(&root.join("src/main/kotlin/Sample.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Sample.kt");
    exact_target(
        ref_at(file, source, "inherited()", 1),
        "/Sample.kt",
        "inherited",
    );
    exact_target(ref_at(file, source, "create()", 1), "/Sample.kt", "create");
    exact_target(ref_at(file, source, "alias()", 0), "/Sample.kt", "target");
}

#[test]
fn compact_companion_string_interpolation_preserves_the_enclosing_type() {
    let temp = tempfile::tempdir().unwrap();
    let source = concat!(
        "package recovered\n",
        "class Messages{companion object{fun render()=\"value ${if (true) { 1 } else { 2 }}\"}}\n",
        "fun use()=Messages.render()\n",
    );
    write(&temp.path().join("Messages.kt"), source);
    let project = report(&run(temp.path(), "repo", None));
    let file = file(&project, "/Messages.kt");
    exact_target(
        ref_at(file, source, "render()", 1),
        "/Messages.kt",
        "render",
    );
    assert!(
        file["functions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|function| { !function["name"].as_str().unwrap().starts_with("script@") })
    );
}
