use super::unquote;

#[test]
fn go_string_decoding_covers_byte_rune_and_raw_forms() {
    for (raw, expected) in [
        (r#""example.com/\x70""#, "example.com/p"),
        (r#""\160\141\164\150""#, "path"),
        (r#""\u00e9\U0001f600""#, "é😀"),
        (r#""\xc3\xa9""#, "é"),
        ("`raw\r\ntext`", "raw\ntext"),
        (r#""\\\"""#, "\\\""),
    ] {
        assert_eq!(unquote(raw).as_deref(), Some(expected), "{raw}");
    }
}

#[test]
fn invalid_escapes_are_not_silently_reinterpreted() {
    for raw in [
        r#""\q""#,
        r#""\x0""#,
        r#""\400""#,
        r#""\uD800""#,
        r#""\U00110000""#,
        r#""\xff""#,
        "\"line\nfeed\"",
    ] {
        assert!(unquote(raw).is_none(), "{raw}");
    }
}
