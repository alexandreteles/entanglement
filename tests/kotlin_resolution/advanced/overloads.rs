use super::*;

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
