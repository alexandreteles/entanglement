mod support;
use support::{only_file, references_to, report, run, write};

#[test]
fn c_enumerators_follow_declaration_order_and_block_scope() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("state.c");
    let source = "enum State { idle = 1, ready = idle };\nint before(void){return ready;}\nint local(void){enum { ready = 2 }; return ready;}\nint after(void){return ready;}\n";
    write(&path, source);
    let report = report(&run("file", &path, None, None));
    let file = only_file(&report);
    let global = source.find("ready = idle").unwrap();
    let local = source.find("ready = 2").unwrap();
    let uses = references_to(file, "ready");
    assert_eq!(uses.len(), 3);
    for (reference, start) in uses.iter().zip([global, local, global]) {
        assert_eq!(reference["resolution"]["symbols"]["start_byte"], start);
        assert_eq!(reference["resolution"]["symbols"]["kind"], "constant");
    }
    assert_eq!(
        references_to(file, "idle")[0]["resolution"]["status"],
        "exact"
    );
}

#[test]
fn cpp_scoped_and_unscoped_enumerators_keep_their_qualified_names() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("state.cpp");
    write(
        &path,
        r#"enum class Color { red, blue = red };
enum Plain { green };
Color read() { return Color::red; }
Plain a() { return Plain::green; }
Plain b() { return green; }
void local() {
    enum class State { off, on = off };
    auto value = State::off;
}
struct Group {
    int read() { return first + static_cast<int>(Mode::cold); }
    enum { first };
    enum class Mode { cold };
};
"#,
    );
    let report = report(&run("file", &path, None, None));
    let file = only_file(&report);
    for name in ["red", "green", "off"] {
        let uses = references_to(file, name);
        assert_eq!(uses.len(), 2);
        assert!(
            uses.iter()
                .all(|item| item["resolution"]["symbols"]["kind"] == "constant")
        );
        assert_eq!(uses[0]["resolution"], uses[1]["resolution"]);
    }
    for name in ["first", "cold"] {
        let uses = references_to(file, name);
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0]["resolution"]["symbols"]["kind"], "constant",
            "{name}"
        );
    }
}
