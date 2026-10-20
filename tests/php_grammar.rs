use tree_sitter::{Language, Parser};

/// Parse the source with both PHP grammars and check the complete syntax tree.
fn accepts(source: &str, expected: bool) {
    for (language, prefix) in [
        (tree_sitter_php::LANGUAGE_PHP, "<?php\n"),
        (tree_sitter_php::LANGUAGE_PHP_ONLY, ""),
    ] {
        let mut parser = Parser::new();
        parser.set_language(&Language::from(language)).unwrap();
        let source = format!("{prefix}{source}");
        let tree = parser.parse(&source, None).unwrap();
        assert_eq!(
            !tree.root_node().has_error(),
            expected,
            "{source}\n{}",
            tree.root_node().to_sexp()
        );
    }
}

#[test]
fn php_83_final_trait_adaptations_parse() {
    for adaptation in ["final", "final alias", "private", "private alias"] {
        accepts(
            &format!(
                "trait Calls {{ function run() {{}} }} class Record {{ use Calls {{ run as {adaptation}; }} }}"
            ),
            true,
        );
    }
    for adaptation in [
        "private final",
        "final private",
        "public final alias",
        "final public alias",
    ] {
        accepts(
            &format!(
                "trait Calls {{ function run() {{}} }} class Record {{ use Calls {{ run as {adaptation}; }} }}"
            ),
            false,
        );
    }
}

#[test]
fn php_85_final_promoted_properties_parse() {
    for modifiers in [
        "final public",
        "public final",
        "final protected readonly",
        "final readonly public",
        "readonly final public",
    ] {
        accepts(
            &format!(
                "class Record {{ public function __construct({modifiers} string $name) {{}} }}"
            ),
            true,
        );
    }
}

#[test]
fn php_85_void_discards_parse_in_statement_positions() {
    for source in [
        "(void) sample();",
        "(VOID) ($flag ? first() : second());",
        "for ((void) first(), $i = 0; (void) check(), $i < 2; (void) next(), $i++) {}",
        "if ($flag) (void) sample();",
    ] {
        accepts(source, true);
    }
}

#[test]
fn php_85_void_discards_do_not_produce_values() {
    for source in [
        "$result = (void) sample();",
        "consume((void) sample());",
        "return (void) sample();",
        "if ((void) sample()) {}",
        "for (; (void) sample(); ) {}",
        "for (; $flag, (void) sample(); ) {}",
    ] {
        accepts(source, false);
    }
}

#[test]
fn php_82_through_85_valid_syntax_parses_with_both_grammars() {
    for source in [
        "readonly class Record { public function __construct(public (A&B)|null $value) {} }\nfunction flag(false|null $value): true { return true; }",
        "trait Codes { public const string NAME = 'record'; }\nclass Record { public const int CODE = 1; }\n$name = 'CODE'; $code = Record::{$name};",
        "class Record { public private(set) string $name; public string $label { get => $this->name; set { $this->name = $value; } } }\n$result = new Record()->label;",
        "#[Example(static function () { return 1; })] function sample() {}\nconst CALLBACK = strlen(...); const INTEGER = (int) 1.5;\n#[Example] const CODE = 1;\nclass Record { public private(set) static string $label; }\n$result = 'abc' |> strlen(...);\n$copy = clone($record, ['label' => 'copy']); $callback = clone(...);",
        "function clone(object $object): object { return $object; }\nfunction following(): int { return 1; }",
    ] {
        accepts(source, true);
    }
    accepts("function class() {}", false);
}
