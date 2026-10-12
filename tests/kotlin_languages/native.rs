use super::*;

#[test]
fn kotlin_captures_native_executable_scopes_decisions_and_tokens() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.kt");
    let source_text = r#"
package sample

fun topLevel(flag: Boolean): Int = if (flag) 1 else 0

fun String.extension(flag: Boolean): String {
    val tokenSentinel = "KotlinTokenSentinel"
    if (flag && isNotEmpty() || !flag) return this
    return tokenSentinel
}

class Worker(private val enabled: Boolean) {
    val initialized = if (enabled) 1 else 0

    init {
        if (enabled) register()
    }

    val computed: Int
        get() {
            if (enabled) return 1
            return 0
        }

    constructor() : this(false) {
        if (enabled) register()
    }

    fun method(limit: Int): Int {
        fun local(value: Int) = if (enabled && value > 0) value else 0
        val callback: (Int) -> Int = { value -> if (value < limit && value != 0) value else limit }
        val predicate: (Int) -> Boolean = { value -> value > 0 && value < limit }
        val anonymous = fun(value: Int): Int { return if (value == 0) 1 else value }
        return callback(local(anonymous(limit)))
    }
}

object Registry {
    fun lookup(value: Int?): Int = when (value) {
        0 -> 0
        1, 2 -> 1
        else -> -1
    }
}

fun oneBranchWhen(value: Int) {
    when (value) {
        1 -> println(value)
    }
}

fun loopDecisions(flag: Boolean, limit: Int) {
    for (index in 0 until limit) {
        if (index > 3) break
    }
    while (flag) {
        do {
            if (limit < 0) break
        } while (flag)
        break
    }
}

fun labeledBreak(flag: Boolean) {
    outer@ while (flag) {
        while (flag) {
            break@outer
        }
    }
}

fun labeledContinue(values: List<Int>) {
    outer@ for (value in values) {
        for (inner in values) {
            if (inner < value) continue@outer
        }
    }
}

fun exceptionAndElvis(value: Int?) = try {
    value ?: if (value == null) 0 else value
} catch (_: IllegalStateException) {
    1
} catch (_: RuntimeException) {
    2
}

context(String)
fun legacyReceiver(flag: Boolean) = if (flag) length else 0

context(value: String)
fun contextParameter(flag: Boolean) = if (flag) value.length else 0

val topInitialized = if (true) 1 else 0

fun withDefault(callback: () -> Unit = { if (true) topLevel(true) }) {}

fun contextCall(flag: Boolean) {
    // Braces and context(value: Type) text in comments must remain ordinary comments.
    val text = "context(fake) { keep these braces }"
    context(flag) {
        if (it) println(text)
    }
}
"#;
    write(&path, source_text);

    let output = report(&run("file", &path, None, Some("cc,cogc,halstead")));
    let source = &output["files"][0];
    assert_eq!(source["language"], "kotlin");
    for name in [
        "topLevel",
        "extension",
        "initialized",
        "init",
        "get",
        "Worker",
        "method",
        "local",
        "callback",
        "anonymous",
        "lookup",
        "oneBranchWhen",
        "loopDecisions",
        "labeledBreak",
        "labeledContinue",
        "exceptionAndElvis",
        "legacyReceiver",
        "contextParameter",
        "topInitialized",
        "withDefault",
        "contextCall",
        "predicate",
    ] {
        function(source, name);
    }

    let names: Vec<_> = source["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert!(names.len() >= 13, "missing executable scopes: {names:?}");

    let extension = function(source, "extension");
    assert!(extension["cyclomatic_complexity"].as_u64().unwrap() >= 4);
    let contribution_kinds: Vec<_> = extension["contributions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect();
    assert!(contribution_kinds.contains(&"condition"));
    assert!(contribution_kinds.contains(&"logical_condition"));
    assert_eq!(contribution_count(extension, "logical_condition"), 2);

    let when = function(source, "lookup");
    assert!(when["cyclomatic_complexity"].as_u64().unwrap() >= 3);
    assert!(
        when["contributions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "multiway")
    );
    assert_eq!(contribution_count(when, "multiway"), 1);
    assert!(
        function(source, "loopDecisions")["cyclomatic_complexity"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert_eq!(function(source, "extension")["cyclomatic_complexity"], 4);
    assert_eq!(function(source, "lookup")["cyclomatic_complexity"], 3);
    assert_eq!(
        function(source, "oneBranchWhen")["cyclomatic_complexity"],
        2
    );
    assert_eq!(
        function(source, "loopDecisions")["cyclomatic_complexity"],
        6
    );
    assert_eq!(
        contribution_count(function(source, "loopDecisions"), "condition"),
        5
    );
    assert_eq!(function(source, "labeledBreak")["cyclomatic_complexity"], 3);
    assert_eq!(function(source, "labeledBreak")["cognitive_complexity"], 4);
    assert_eq!(
        cognitive_contribution_count(function(source, "labeledBreak"), "labeled_jump"),
        1
    );
    assert_eq!(
        function(source, "labeledContinue")["cyclomatic_complexity"],
        4
    );
    assert_eq!(
        function(source, "labeledContinue")["cognitive_complexity"],
        7
    );
    assert_eq!(
        cognitive_contribution_count(function(source, "labeledContinue"), "labeled_jump"),
        1
    );
    assert_eq!(
        function(source, "exceptionAndElvis")["cyclomatic_complexity"],
        5
    );
    assert_eq!(
        contribution_count(function(source, "exceptionAndElvis"), "condition"),
        4
    );
    assert_eq!(
        function(source, "legacyReceiver")["cyclomatic_complexity"],
        2
    );
    assert_eq!(
        function(source, "contextParameter")["cyclomatic_complexity"],
        2
    );
    assert_eq!(function(source, "contextCall")["cyclomatic_complexity"], 1);
    assert_eq!(
        function(source, "topInitialized")["cyclomatic_complexity"],
        2
    );
    assert_eq!(function(source, "withDefault")["cyclomatic_complexity"], 1);
    let default_lambda_start = source_text.find("{ if (true) topLevel(true) }").unwrap();
    let default_lambda = source["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
                && item["start_byte"].as_u64() == Some(default_lambda_start as u64)
        })
        .unwrap_or_else(|| {
            panic!("missing default-argument lambda scope at {default_lambda_start}")
        });
    assert_eq!(default_lambda["cyclomatic_complexity"], 2);
    let legacy_type_start = source_text.find("context(String)").unwrap() + "context(".len();
    let context_value_start =
        source_text.find("context(value: String)").unwrap() + "context(".len();
    let context_type_start =
        source_text.find("context(value: String)").unwrap() + "context(value: ".len();
    assert!(has_operand(
        &function(source, "legacyReceiver")["halstead"],
        "String"
    ));
    assert!(has_operand_at(
        &function(source, "legacyReceiver")["halstead"],
        "String",
        legacy_type_start
    ));
    assert!(has_operand(
        &function(source, "contextParameter")["halstead"],
        "value"
    ));
    assert!(has_operand(
        &function(source, "contextParameter")["halstead"],
        "String"
    ));
    assert!(has_operand_at(
        &function(source, "contextParameter")["halstead"],
        "value",
        context_value_start
    ));
    assert!(has_operand_at(
        &function(source, "contextParameter")["halstead"],
        "String",
        context_type_start
    ));
    assert_eq!(function(source, "method")["cyclomatic_complexity"], 1);
    assert_eq!(function(source, "method")["cognitive_complexity"], 0);
    assert_eq!(function(source, "callback")["cyclomatic_complexity"], 3);
    assert_eq!(function(source, "callback")["cognitive_complexity"], 4);
    assert_eq!(function(source, "predicate")["cyclomatic_complexity"], 2);
    assert_eq!(function(source, "predicate")["cognitive_complexity"], 1);
    assert!(has_operand(
        &function(source, "contextCall")["halstead"],
        "context"
    ));
    let context_callbacks: Vec<_> = source["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
                && item["start_byte"].as_u64().unwrap()
                    > function(source, "contextCall")["start_byte"]
                        .as_u64()
                        .unwrap()
        })
        .collect();
    assert_eq!(context_callbacks.len(), 1);
    assert_eq!(context_callbacks[0]["cyclomatic_complexity"], 2);
    assert!(has_operand(&source["halstead"], "<string>"));
    assert!(has_operand(&extension["halstead"], "tokenSentinel"));
}

#[test]
fn opaque_dsl_names_do_not_change_scope_or_decision_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.kt");
    let second = temp.path().join("second.kt");
    write(
        &first,
        concat!(
            "class Node { fun first(block: Node.() -> Unit) {} fun second(block: Node.() -> Unit) {} fun text(value: String) {} }\n",
            "fun render(node: Node, flag: Boolean) { node.first { second { if (flag) text(\"x\") } } }\n",
        ),
    );
    write(
        &second,
        "fun render(node: Any, flag: Boolean) { node.ExternalColumn { ExternalRow { if (flag) ExternalText(\"x\") } } }\n",
    );
    let report_a = report(&run("file", &first, None, Some("all")));
    let report_b = report(&run("file", &second, None, Some("all")));
    let a = &report_a["files"][0];
    let b = &report_b["files"][0];
    let callbacks_a: Vec<_> = a["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous"))
        })
        .collect();
    let callbacks_b: Vec<_> = b["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous"))
        })
        .collect();
    assert_eq!(callbacks_a.len(), callbacks_b.len());
    assert!(callbacks_a.len() >= 2, "nested DSL lambdas need scopes");
    assert_eq!(
        function(a, "render")["cyclomatic_complexity"],
        function(b, "render")["cyclomatic_complexity"]
    );
    for metric in [
        "nloc",
        "cyclomatic_complexity",
        "cyclomatic_density",
        "cognitive_complexity",
    ] {
        assert_eq!(function(a, "render")[metric], function(b, "render")[metric]);
    }
    for metric in [
        "distinct_operators",
        "distinct_operands",
        "total_operators",
        "total_operands",
        "vocabulary",
        "length",
        "volume",
    ] {
        assert_eq!(
            function(a, "render")["halstead"][metric],
            function(b, "render")["halstead"][metric],
            "opaque API names should not change the {metric} metric"
        );
    }
    assert_eq!(
        function(a, "render")["maintainability_index"]["score"],
        function(b, "render")["maintainability_index"]["score"]
    );
    let callback_scores_a: Vec<_> = callbacks_a
        .iter()
        .map(|item| item["cyclomatic_complexity"].clone())
        .collect();
    let callback_scores_b: Vec<_> = callbacks_b
        .iter()
        .map(|item| item["cyclomatic_complexity"].clone())
        .collect();
    assert_eq!(callback_scores_a, callback_scores_b);
    assert_eq!(
        callbacks_a[0]["cyclomatic_complexity"],
        callbacks_b[0]["cyclomatic_complexity"]
    );
    assert!(
        callbacks_a
            .iter()
            .any(|item| item["cyclomatic_complexity"].as_u64().unwrap() > 1)
    );
}

#[test]
fn kotlin_multidollar_raw_string_interpolation_keeps_only_interpolated_names_as_operands() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("interpolation.kt");
    let source_text = r#"fun render(name: String) = $$"""price: $$name; literal: $name; value: $$${name.length}"""
"#;
    write(&path, source_text);

    let output = report(&run("file", &path, None, Some("halstead")));
    let file = &output["files"][0];
    assert_eq!(file["language"], "kotlin");
    let render = function(file, "render");
    let direct_interpolation = source_text.find("$$name").unwrap();
    let literal_dollar_name = source_text.find("literal: $name").unwrap() + "literal: ".len();
    let expression_interpolation = source_text.rfind("name.length").unwrap();

    assert!(has_operand_at(
        &render["halstead"],
        "name",
        direct_interpolation + 2
    ));
    assert!(has_operand_at(
        &render["halstead"],
        "name",
        expression_interpolation
    ));
    assert!(
        !has_operand_at(&render["halstead"], "name", literal_dollar_name + 1),
        "single-dollar text is literal with a two-dollar interpolation prefix"
    );
}

#[test]
fn declaration_only_primary_constructors_have_no_executable_metric_scope() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("declarations.kt");
    write(
        &path,
        concat!(
            "expect class ExpectedApi()\n",
            "external class ExternalApi()\n",
            "abstract class AbstractBase()\n",
        ),
    );

    let output = report(&run("file", &path, None, Some("cc")));
    let file = &output["files"][0];
    let functions = file["functions"].as_array().unwrap();
    assert!(!functions.iter().any(|item| item["name"] == "ExpectedApi"));
    assert!(!functions.iter().any(|item| item["name"] == "ExternalApi"));
    assert!(
        functions.iter().any(|item| item["name"] == "AbstractBase"),
        "an abstract class still has an executable primary constructor scope"
    );
}

#[test]
fn compact_class_companion_and_generic_dsl_scopes_match_multiline_source() {
    let temp = tempfile::tempdir().unwrap();
    let compact_path = temp.path().join("compact.kt");
    let multiline_path = temp.path().join("multiline.kt");
    write(
        &compact_path,
        concat!(
            "class C(flag:Boolean){/* } fun hidden() = if (true) 1 else 0; */",
            "val note=\"brace } { fun ghost() = if (true) 1 else 0\";",
            "fun direct(flag:Boolean)=if(flag)1 else 0;",
            "companion object{fun companion(flag:Boolean)=if(flag)1 else 0}}\n",
            "class Builder{fun child()=1}\n",
            "fun <T> withValue(value:T,block:T.()->Unit){}\n",
            "fun use(builder:Builder,flag:Boolean){withValue(builder){if(flag)child()}}\n",
        ),
    );
    write(
        &multiline_path,
        concat!(
            "class C(flag: Boolean) {\n",
            "    /* } fun hidden() = if (true) 1 else 0; */\n",
            "    val note = \"brace } { fun ghost() = if (true) 1 else 0\"\n",
            "    fun direct(flag: Boolean) = if (flag) 1 else 0\n",
            "    companion object {\n",
            "        fun companion(flag: Boolean) = if (flag) 1 else 0\n",
            "    }\n",
            "}\n",
            "class Builder { fun child() = 1 }\n",
            "fun <T> withValue(value: T, block: T.() -> Unit) {}\n",
            "fun use(builder: Builder, flag: Boolean) {\n",
            "    withValue(builder) {\n",
            "        if (flag) child()\n",
            "    }\n",
            "}\n",
        ),
    );

    let compact_report = report(&run("file", &compact_path, None, Some("all")));
    let multiline_report = report(&run("file", &multiline_path, None, Some("all")));
    let compact = &compact_report["files"][0];
    let multiline = &multiline_report["files"][0];
    let compact_scope_labels = compact["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}@{}..{}",
                item["name"].as_str().unwrap_or("<unnamed>"),
                item["start_byte"],
                item["end_byte"]
            )
        })
        .collect::<Vec<_>>();
    assert!(
        compact_scope_labels
            .iter()
            .any(|label| label.starts_with("C@")),
        "the compact primary constructor should be reported; scopes: {compact_scope_labels:?}"
    );
    for name in ["C", "direct", "companion", "child", "withValue", "use"] {
        let compact_scope = function(compact, name);
        let multiline_scope = function(multiline, name);
        for metric in ["cyclomatic_complexity", "cognitive_complexity"] {
            assert_eq!(
                compact_scope[metric], multiline_scope[metric],
                "compact owner layout changed {name}.{metric}"
            );
        }
        for metric in [
            "distinct_operators",
            "distinct_operands",
            "total_operators",
            "total_operands",
            "vocabulary",
            "length",
            "volume",
        ] {
            assert_eq!(
                compact_scope["halstead"][metric], multiline_scope["halstead"][metric],
                "compact owner layout changed {name} Halstead {metric}"
            );
        }
    }
    for source in [compact, multiline] {
        let functions = source["functions"].as_array().unwrap();
        assert!(
            !functions
                .iter()
                .any(|item| { matches!(item["name"].as_str(), Some("hidden" | "ghost")) })
        );
        let callbacks: Vec<_> = functions
            .iter()
            .filter(|item| {
                item["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("anonymous@"))
            })
            .collect();
        assert_eq!(callbacks.len(), 1, "one generic DSL callback scope");
        assert_eq!(callbacks[0]["cyclomatic_complexity"], 2);
        assert_eq!(callbacks[0]["cognitive_complexity"], 2);
        assert!(!has_operand(&callbacks[0]["halstead"], "ghost"));
        assert!(!has_operand(&callbacks[0]["halstead"], "hidden"));
    }
    let compact_callbacks: Vec<_> = compact["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    let multiline_callbacks: Vec<_> = multiline["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    for metric in [
        "distinct_operators",
        "distinct_operands",
        "total_operators",
        "total_operands",
        "vocabulary",
        "length",
        "volume",
    ] {
        assert_eq!(
            compact_callbacks[0]["halstead"][metric], multiline_callbacks[0]["halstead"][metric],
            "compact generic callback changed Halstead {metric}"
        );
    }
}
