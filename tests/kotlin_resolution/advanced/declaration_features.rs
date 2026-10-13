use super::*;

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
