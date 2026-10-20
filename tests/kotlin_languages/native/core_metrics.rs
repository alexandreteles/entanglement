use super::*;

fn assert_reported_scopes(source: &Value) {
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
}

fn assert_decision_metrics(source_text: &str, source: &Value) {
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
    assert_eq!(extension["cyclomatic_complexity"], 4);
    assert_eq!(when["cyclomatic_complexity"], 3);
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
    assert_eq!(function(source, "method")["cyclomatic_complexity"], 1);
    assert_eq!(function(source, "method")["cognitive_complexity"], 0);
    assert_eq!(function(source, "callback")["cyclomatic_complexity"], 3);
    assert_eq!(function(source, "callback")["cognitive_complexity"], 4);
    assert_eq!(function(source, "predicate")["cyclomatic_complexity"], 2);
    assert_eq!(function(source, "predicate")["cognitive_complexity"], 1);
}

fn assert_context_and_token_operands(source_text: &str, source: &Value) {
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
    assert!(has_operand(
        &function(source, "extension")["halstead"],
        "tokenSentinel"
    ));
}

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

context(`context`: suspend (Int, String) -> Unit)
@Deprecated("old")
fun contextFunctionType() { `context`(1, "a") }

context() fun emptyContext() = 1
"#;
    write(&path, source_text);

    let output = report(&run("file", &path, None, Some("cc,cogc,halstead")));
    let source = &output["files"][0];
    assert_reported_scopes(source);
    assert_decision_metrics(source_text, source);
    assert_context_and_token_operands(source_text, source);
    for (name, prefix) in [
        ("contextFunctionType", "context(`context`"),
        ("emptyContext", "context()"),
    ] {
        let scope = function(source, name);
        assert_eq!(scope["start_byte"], source_text.find(prefix).unwrap());
        assert!(
            scope["halstead"]["operators"]
                .as_array()
                .unwrap()
                .iter()
                .any(|token| token["text"] == "context")
        );
    }
    assert!(
        source["functions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|scope| !scope["name"].as_str().unwrap().starts_with("script@"))
    );
    assert!(
        source["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .all(|reference| reference["path"] != serde_json::json!(["invoke"]))
    );
}
