use super::*;

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
