use super::*;

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
