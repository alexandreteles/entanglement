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
        "enum class Color { red, blue = red }; enum Plain { green };\nColor read(){return Color::red;}\nPlain a(){return Plain::green;}\nPlain b(){return green;}\n",
    );
    let report = report(&run("file", &path, None, None));
    let file = only_file(&report);
    for name in ["red", "green"] {
        let uses = references_to(file, name);
        assert_eq!(uses.len(), 2);
        assert!(
            uses.iter()
                .all(|item| item["resolution"]["symbols"]["kind"] == "constant")
        );
        assert_eq!(uses[0]["resolution"], uses[1]["resolution"]);
    }
}
